"""Independent CRC vectors, failure precedence, and actual UDP fault/recovery tests."""

import copy
import io
import itertools
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

import test_demo
from test_demo import HERE, BINARY, CASES, adapted, server, unchecked
from test_multicast import events, wait_until, listener_process, ports, GROUP, INTERFACE
import codec
import framing
from client import Client, EventLog
from faults import CASES as FAULT_CASES, packet


class FramingTests(unittest.TestCase):
    setUpClass = classmethod(test_demo.CodecTests.setUpClass.__func__)
    tearDownClass = classmethod(test_demo.CodecTests.tearDownClass.__func__)
    rpc = test_demo.CodecTests.rpc

    def rejection(self, format, wire, expected, crc="on"):
        self.assertEqual(
            self.rpc("decode", format, crc=crc, hex=wire.hex()), {"error": expected}
        )
        with self.assertRaises(codec.CodecError) as caught:
            framing.decode(format, wire, crc)
        self.assertEqual(caught.exception.outcome, expected)

    def test_published_vectors(self):
        # RFC 3720 B.4: digest byte order is reversed from our numeric hex trailer.
        for data, value in (
            (b"", 0),
            (b"123456789", 0xE3069283),
            (bytes(32), 0x8A9136AA),
            (b"\xff" * 32, 0x62A8AB43),
            (bytes(range(32)), 0x46DD794E),
            (bytes(range(31, -1, -1)), 0x113FDB5C),
        ):
            self.assertEqual(framing.checksum(data), value)
            self.assertEqual(framing.seal(data, "on"), data + f"{value:08X}".encode())
        self.assertEqual(framing.open_frame(b"123456789E3069283", "on"), b"123456789")

    def test_fixture_matrix_and_exact_body_preservation(self):
        for format, crc in itertools.product(codec.FORMATS, framing.CRC_MODES):
            for case in CASES["valid"]:
                message = adapted(case["message"], format)
                wire = framing.encode(format, message, crc)
                self.assertEqual(
                    framing.open_frame(wire, crc), codec.encode(format, message)
                )
                self.assertEqual(
                    self.rpc("decode", format, crc=crc, hex=wire.hex()),
                    {"message": message},
                )
                rust = bytes.fromhex(
                    self.rpc("encode", format, crc=crc, message=message)["hex"]
                )
                self.assertEqual(framing.decode(format, rust, crc), message)
                self.assertEqual(
                    len(rust),
                    len(
                        bytes.fromhex(
                            self.rpc("encode", format, message=message)["hex"]
                        )
                    )
                    + (8 if crc == "on" else 0),
                )

    def test_mutation_and_failure_precedence(self):
        for format in codec.FORMATS:
            for case in FAULT_CASES:
                wire, expected = packet(format, "on", case)
                self.rejection(format, wire, expected)
            original = framing.encode(format, CASES["valid"][0]["message"], "on")
            for index in range(len(original)):
                damaged = bytearray(original)
                damaged[index] ^= 1
                self.rejection(format, bytes(damaged), "crc_error")
            # Recalculate CRC for invalid contents: integrity succeeds, parsing still fails.
            self.rejection(format, framing.seal(b"\xff", "on"), "decode_error")
            mutated, expected = packet(format, "off", "corrupt")
            self.assertEqual(expected, "accepted_request")
            self.assertEqual(
                self.rpc("decode", format, hex=mutated.hex())["message"]["request_id"],
                2,
            )

    def test_trailer_boundaries_and_mode_mismatch(self):
        for invalid in (None, True, 1, "auto"):
            self.assertEqual(
                self.rpc("decode", "json", crc=invalid, hex=""),
                {"error": "validation_error"},
            )
        body = codec.encode("json", CASES["valid"][0]["message"])
        wire = framing.seal(body, "on")
        for damaged in (
            wire[:-1],
            wire + b"\n",
            wire[:-8] + b"e3069283",
            wire[:-8] + b"G0000000",
            body,
        ):
            self.rejection("json", damaged, "crc_error")
        for length in range(8):
            self.rejection("json", b"0" * length, "crc_error")
        self.rejection("json", b"00000000", "decode_error")
        padded = body + b" " * (1016 - len(body))
        boundary = framing.seal(padded, "on")
        self.assertEqual(len(boundary), 1024)
        self.assertIn(
            "message", self.rpc("decode", "json", crc="on", hex=boundary.hex())
        )
        self.rejection("json", boundary + b"X", "size_error")
        self.rejection("json", boundary, "size_error", "off")
        self.rejection("json", wire, "decode_error", "off")
        with self.assertRaises(codec.CodecError):
            framing.seal(padded + b" ", "on")


class CommandFaultTests(unittest.TestCase):
    def test_udp_faults_counters_and_recovery_all_six_configs(self):
        for format, crc in itertools.product(codec.FORMATS, framing.CRC_MODES):
            with self.subTest(format=format, crc=crc), server(
                format, extra=("--crc", crc)
            ) as (endpoint, path), socket.socket(
                socket.AF_INET, socket.SOCK_DGRAM
            ) as sock:
                sock.bind((INTERFACE, 0))
                output = io.StringIO()
                client = Client(
                    sock,
                    endpoint,
                    format,
                    "board-01",
                    1,
                    EventLog(output, format=format),
                    crc,
                )
                self.assertEqual(
                    client.request("health")["health"]["rejected_datagrams"], 0
                )
                rejected, crc_rejections = 0, 0
                for case in FAULT_CASES:
                    wire, expected = packet(format, crc, case)
                    before = len(events(path))
                    sock.sendto(wire, endpoint)
                    # Wait for evidence at the receiver; silence alone is not the verdict.
                    wait_until(
                        lambda: any(
                            e["outcome"] == expected for e in events(path)[before:]
                        )
                    )
                    if expected == "accepted_request":
                        sock.settimeout(1)
                        response = framing.decode(format, sock.recvfrom(65535)[0], crc)
                        self.assertEqual(response["request_id"], 2)
                    else:
                        rejected += 1
                        crc_rejections += expected == "crc_error"
                        sock.settimeout(0.02)
                        with self.assertRaises(socket.timeout):
                            sock.recvfrom(65535)
                    recovered = client.request("health")
                    self.assertEqual(
                        recovered["health"]["rejected_datagrams"], rejected
                    )
                    wait_until(
                        lambda: events(path)[-1]["crc_rejections"] == crc_rejections
                    )
                self.assertTrue(all(e["crc"] == crc for e in events(path)))
                # A decodable, correlated invalid request gets a CRC-protected error reply.
                bad = dict(CASES["valid"][0]["message"], boot_id="fedcba9876543210")
                sock.sendto(framing.seal(unchecked(format, bad), crc), endpoint)
                sock.settimeout(1)
                self.assertEqual(
                    framing.decode(format, sock.recvfrom(65535)[0], crc)["error"],
                    {"code": 2},
                )

    def test_corrupt_duplicate_late_responses_do_not_confuse_client(self):
        for format in codec.FORMATS:
            with socket.socket(
                socket.AF_INET, socket.SOCK_DGRAM
            ) as source, socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as target:
                source.bind((INTERFACE, 0))
                target.bind((INTERFACE, 0))
                source.settimeout(2)
                failures = []

                def responder():
                    try:
                        first = None
                        late = None
                        for index in range(3):
                            data, peer = source.recvfrom(65535)
                            request = framing.decode(format, data, "on")
                            reply = dict(
                                protocol_version=1,
                                kind=12,
                                device_id="board-01",
                                boot_id="1122334455667788",
                                uptime_ms=0,
                                client_session=request["client_session"],
                                request_id=request["request_id"],
                                status=dict(
                                    temperature_celsius=25.5, validity=1, synthetic=True
                                ),
                            )
                            wire = framing.encode(format, reply, "on")
                            if index == 0:
                                bad = bytearray(wire)
                                bad[0] ^= 1
                                source.sendto(bad, peer)
                                source.sendto(wire, peer)
                                first = wire
                            elif index == 1:
                                source.sendto(
                                    first, peer
                                )  # completed request, duplicate
                                late = wire  # withhold until request 3 is received
                            else:
                                source.sendto(late, peer)
                                source.sendto(wire, peer)
                    except BaseException as error:
                        failures.append(error)

                worker = threading.Thread(target=responder)
                worker.start()
                output = io.StringIO()
                client = Client(
                    target,
                    source.getsockname(),
                    format,
                    "board-01",
                    1,
                    EventLog(output, format=format),
                    "on",
                )
                try:
                    self.assertEqual(client.request("status")["request_id"], 1)
                    client.timeout = 0.1
                    self.assertIsNone(client.request("status"))
                    client.timeout = 1
                    self.assertEqual(client.request("status")["request_id"], 3)
                finally:
                    worker.join(3)
                self.assertFalse(worker.is_alive())
                self.assertFalse(failures)
                outcomes = {
                    e["outcome"]
                    for e in map(json.loads, output.getvalue().splitlines())
                }
                self.assertTrue(
                    {
                        "crc_error",
                        "duplicate_response",
                        "timeout",
                        "late_response",
                        "accepted_response",
                    }
                    <= outcomes
                )

    def test_fault_cli_is_runnable_and_observes_recovery(self):
        with server("protobuf", extra=("--crc", "on")) as (endpoint, path):
            result = subprocess.run(
                [
                    sys.executable,
                    HERE / "python/faults.py",
                    "--format",
                    "protobuf",
                    "--crc",
                    "on",
                    "--host",
                    endpoint[0],
                    "--port",
                    str(endpoint[1]),
                    "--bind",
                    INTERFACE,
                    "--case",
                    "corrupt",
                ],
                capture_output=True,
                text=True,
                timeout=5,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            records = list(map(json.loads, result.stdout.splitlines()))
            self.assertIn("recovery_passed", {e["outcome"] for e in records})
            self.assertIn("crc_error", {e["outcome"] for e in events(path)})


@unittest.skipUnless(
    sys.platform.startswith("linux"), "multicast metadata harness uses Linux"
)
class StreamFaultTests(unittest.TestCase):
    def test_finite_stream_fault_demo(self):
        status, health = ports()
        with tempfile.TemporaryDirectory() as temporary, listener_process(
            "protobuf",
            status,
            health,
            Path(temporary) / "stream.jsonl",
            "on",
            "fault-01",
        ):
            path = Path(temporary) / "stream.jsonl"
            result = subprocess.run(
                [
                    sys.executable,
                    HERE / "python/stream_faults.py",
                    "--format",
                    "protobuf",
                    "--crc",
                    "on",
                    "--interface",
                    INTERFACE,
                    "--status-port",
                    str(status),
                    "--health-port",
                    str(health),
                    "--pause",
                    "0.55",
                ],
                capture_output=True,
                text=True,
                timeout=5,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            expected = {
                "gap_pending",
                "reordered_publication",
                "duplicate_publication",
                "crc_error",
                "decode_error",
                "stale",
                "recovered",
                "session_change",
            }
            wait_until(lambda: expected <= {e["outcome"] for e in events(path)})
            self.assertTrue(
                any(
                    e["outcome"] == "session_change" and e.get("previous_boot_id")
                    for e in events(path)
                )
            )

    def test_bad_crc_does_not_refresh_freshness_and_valid_data_recovers(self):
        for format in codec.FORMATS:
            status, health = ports()
            with tempfile.TemporaryDirectory() as temporary, listener_process(
                format, status, health, Path(temporary) / "listen.jsonl", "on"
            ), socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
                path = Path(temporary) / "listen.jsonl"
                sender.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                sender.bind((INTERFACE, status))
                sender.setsockopt(
                    socket.IPPROTO_IP,
                    socket.IP_MULTICAST_IF,
                    socket.inet_aton(INTERFACE),
                )
                sender.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
                message = copy.deepcopy(CASES["valid"][11]["message"])
                message["kind"] = 21
                message["sequence"] = 0
                sender.sendto(framing.encode(format, message, "on"), (GROUP, status))
                wait_until(
                    lambda: any(
                        e["outcome"] == "accepted_publication" for e in events(path)
                    )
                )
                for sequence in range(1, 16):
                    message["sequence"] = sequence
                    damaged = bytearray(framing.encode(format, message, "on"))
                    damaged[-1] = ord("G")
                    sender.sendto(damaged, (GROUP, status))
                    time.sleep(0.02)
                wait_until(lambda: any(e["outcome"] == "stale" for e in events(path)))
                records = events(path)
                self.assertEqual(
                    sum(e["outcome"] == "accepted_publication" for e in records), 1
                )
                self.assertTrue(
                    any(
                        e["outcome"] == "crc_error" and e["crc_rejections"] > 0
                        for e in records
                    )
                )
                message["sequence"] = 16
                sender.sendto(framing.encode(format, message, "on"), (GROUP, status))
                wait_until(
                    lambda: any(e["outcome"] == "recovered" for e in events(path))
                )
                sender.settimeout(0.02)
                with self.assertRaises(socket.timeout):
                    sender.recvfrom(65535)  # no ACKs


if __name__ == "__main__":
    unittest.main(verbosity=2)
