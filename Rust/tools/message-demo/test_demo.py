"""Independent fixtures, cross-language codecs, and actual loopback UDP exchanges."""

import contextlib
import copy
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE / "python"))
import codec
from client import Client, EventLog

BINARY = Path(
    os.environ.get(
        "MESSAGE_DEMO_BIN", HERE / "target/x86_64-unknown-linux-gnu/debug/message-demo"
    )
)
CASES = json.loads((ROOT / "protocol/fixtures/cases.json").read_text())
GOLDEN = json.loads((ROOT / "protocol/fixtures/golden_request.json").read_text())


def adapted(message, format):
    message = copy.deepcopy(message)
    if "device_info" in message:
        message["device_info"]["encoding"] = codec.FORMATS[format]
    return message


def unchecked(format, message):
    """Bypass validation to send malformed semantic fixtures with real serializers."""
    if format == "json":
        return json.dumps(message, separators=(",", ":")).encode()
    if format == "protobuf":
        return codec.to_proto(
            message, codec.messaging_pb2.Envelope()
        ).SerializeToString()
    # CSV cannot represent a conflicting body: the kind determines its columns.
    import csv

    body = codec.KIND_BODY[message["kind"]]
    values = [message.get(k, "") for k in codec.PROFILE["common_csv_columns"]]
    values += [
        message.get(body, {}).get(k, "")
        for k in codec.PROFILE["body_csv_columns"][body]
    ]
    values = [str(v).lower() if type(v) is bool else v for v in values]
    result = io.StringIO(newline="")
    csv.writer(result, lineterminator="\r\n").writerow(values)
    return result.getvalue().encode()


class CodecTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rust = subprocess.Popen(
            [BINARY, "codec"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True
        )

    @classmethod
    def tearDownClass(cls):
        cls.rust.stdin.close()
        cls.rust.wait(timeout=5)
        cls.rust.stdout.close()

    def rpc(self, operation, format, **args):
        self.rust.stdin.write(
            json.dumps(dict(operation=operation, format=format, **args)) + "\n"
        )
        self.rust.stdin.flush()
        return json.loads(self.rust.stdout.readline())

    def check_wire(self, format, wire, expected):
        self.assertEqual(
            self.rpc("decode", format, hex=wire.hex()), {"error": expected}
        )
        with self.assertRaises(codec.CodecError) as caught:
            codec.decode(format, wire)
        self.assertEqual(caught.exception.outcome, expected)

    def test_authored_golden_bytes(self):
        expected = CASES["valid"][0]["message"]
        for format in codec.FORMATS:
            wire = (
                bytes.fromhex(GOLDEN["protobuf_hex"])
                if format == "protobuf"
                else GOLDEN[format].encode()
            )
            with self.subTest(format=format):
                self.assertEqual(codec.decode(format, wire), expected)
                self.assertEqual(
                    self.rpc("decode", format, hex=wire.hex()), {"message": expected}
                )
                self.assertEqual(codec.encode(format, expected), wire)
                self.assertEqual(
                    bytes.fromhex(self.rpc("encode", format, message=expected)["hex"]),
                    wire,
                )

    def test_all_positive_fixtures_both_directions(self):
        for case in CASES["valid"]:
            for format in codec.FORMATS:
                message = adapted(case["message"], format)
                with self.subTest(case=case["name"], format=format):
                    self.assertEqual(
                        self.rpc(
                            "decode", format, hex=codec.encode(format, message).hex()
                        ),
                        {"message": message},
                    )
                    rust = bytes.fromhex(
                        self.rpc("encode", format, message=message)["hex"]
                    )
                    self.assertEqual(codec.decode(format, rust), message)

    def test_negative_semantics(self):
        for case in CASES["invalid"]:
            for format in codec.FORMATS:
                message = adapted(case["message"], format)
                with self.subTest(case=case["name"], format=format):
                    # Fixed-capacity construction rejects the overlong field before semantic validation.
                    expected = (
                        "decode_error"
                        if case["name"] == "info_oversized_version"
                        else case["expected"].split(":")[0]
                    )
                    result = self.rpc("encode", format, message=message)
                    self.assertEqual(result, {"error": expected})
                    with self.assertRaises(codec.CodecError) as caught:
                        codec.encode(format, message)
                    self.assertEqual(caught.exception.outcome, expected)
                    if format == "csv" and case["name"] == "wrong_body":
                        continue
                    wire = unchecked(format, message)
                    self.check_wire(format, wire, expected)

    def test_wire_rejections(self):
        for case in CASES["wire_rejections"][:3]:
            self.check_wire(
                case["format"],
                bytes.fromhex(case["hex"]) if "hex" in case else case["body"].encode(),
                "decode_error",
            )
        request = GOLDEN["json"].encode()
        json_bad = [
            b"",
            b"{}{}",
            request[:-1],
            request + b"x",
            request.replace(b'"request_id":1', b'"request_id":null'),
            request.replace(b'"request_id":1', b'"request_id":true'),
            request.replace(b'"request_id":1', b'"request_id":1.0'),
            request.replace(b'"request_id":1', b'"request_id":4294967296'),
            request.replace(b'"request_id":1', b'"request_id":-1'),
            request.replace(
                b'"get_device_info":{}', b'"get_device_info":{"surprise":1}'
            ),
            request[:-1] + b',"unknown":1}',
            request[:-1] + b',"request_id":1}',
            request.replace(b"board-01", b"bad\x01id"),
        ]
        for wire in json_bad:
            with self.subTest(wire=wire):
                self.check_wire("json", wire, "decode_error")
        csv_bad = [
            GOLDEN["csv"].replace("\r\n", "\n"),
            GOLDEN["csv"] + "x\r\n",
            GOLDEN["csv"].replace("board-01", '"board-01'),
            GOLDEN["csv"].replace("board-01", 'bo"ard-01'),
            GOLDEN["csv"].replace("board-01", '"board-01"x'),
            GOLDEN["csv"].replace(",1,,", ",01,,"),
            GOLDEN["csv"].replace(",1,,", ",4294967296,,"),
            GOLDEN["csv"].replace("\r\n", ",\r\n"),
        ]
        for wire in csv_bad:
            with self.subTest(wire=wire):
                self.check_wire("csv", wire.encode(), "decode_error")
        for format in codec.FORMATS:
            for length in (1017, 1024, 1025, 60000):
                self.check_wire(format, b"x" * length, "size_error")

    def test_float32_and_absence(self):
        for value in (-100, 200, -0.0, 0.1, 1.17549435e-38):
            for format in codec.FORMATS:
                message = copy.deepcopy(CASES["valid"][7]["message"])
                message["status"]["temperature_celsius"] = value
                expected = codec.validate(message, format)
                wire = codec.encode(format, message)
                rust = self.rpc("decode", format, hex=wire.hex())
                self.assertEqual(codec.shape(rust["message"]), expected)
                wire = bytes.fromhex(self.rpc("encode", format, message=message)["hex"])
                self.assertEqual(codec.decode(format, wire), expected)
        for value in (float("nan"), float("inf"), -float("inf")):
            message = copy.deepcopy(CASES["valid"][7]["message"])
            message["status"]["temperature_celsius"] = value
            for format in ("csv", "protobuf"):
                self.check_wire(format, unchecked(format, message), "validation_error")

    def test_standard_protobuf_unknown_and_duplicate_fields(self):
        expected = CASES["valid"][0]["message"]
        wire = bytes.fromhex(GOLDEN["protobuf_hex"])
        # Unknown tag 100; duplicate scalar request_id, last value wins.
        for suffix, request_id in ((b"\xa0\x06\x01", 1), (b"\x30\x02", 2)):
            message = dict(expected, request_id=request_id)
            self.assertEqual(
                self.rpc("decode", "protobuf", hex=(wire + suffix).hex()),
                {"message": message},
            )
            self.assertEqual(codec.decode("protobuf", wire + suffix), message)

    def test_protobuf_wrong_wire_types(self):
        wire = bytes.fromhex(GOLDEN["protobuf_hex"])
        # Regression: micropb previously treated 0A as protocol_version's 08.
        self.check_wire("protobuf", b"\x0a" + wire[1:], "decode_error")
        # A well-formed wrong-typed known field must be skipped as unknown, just
        # like Google's decoder. Exercise every supported type and nested body.
        wire_types = {2: 5, 8: 0, 9: 2, 11: 2, 13: 0, 14: 0}
        def varint(value):
            result = bytearray()
            while value > 127:
                result.append((value & 127) | 128)
                value >>= 7
            result.append(value)
            return bytes(result)

        for case in CASES["valid"]:
            expected = adapted(case["message"], "protobuf")
            proto = codec.to_proto(expected, codec.messaging_pb2.Envelope())
            for descriptor in (proto.DESCRIPTOR, *[
                value.DESCRIPTOR for entry, value in proto.ListFields()
                if entry.message_type is not None
            ]):
                for entry in descriptor.fields:
                    wrong_type = 2 if wire_types[entry.type] != 2 else 0
                    extra = varint((entry.number << 3) | wrong_type) + b"\x00"
                    if descriptor is proto.DESCRIPTOR:
                        malformed = proto.SerializeToString() + extra
                    else:
                        body_entry = next(e for e, v in proto.ListFields()
                                          if e.message_type is descriptor)
                        body = getattr(proto, body_entry.name).SerializeToString() + extra
                        malformed = (proto.SerializeToString()
                                     + varint((body_entry.number << 3) | 2)
                                     + varint(len(body)) + body)
                    with self.subTest(case=case["name"], message=descriptor.name, field=entry.name):
                        self.assertEqual(codec.decode("protobuf", malformed), expected)
                        self.assertEqual(self.rpc("decode", "protobuf", hex=malformed.hex()),
                                         {"message": expected})

    def test_protobuf_message_merge_and_oneof(self):
        expected = copy.deepcopy(CASES["valid"][7]["message"])
        first = copy.deepcopy(expected)
        del first["status"]["temperature_celsius"]
        wire = unchecked("protobuf", first) + unchecked(
            "protobuf", {"status": {"temperature_celsius": 25.5}}
        )
        self.assertEqual(codec.decode("protobuf", wire), expected)
        self.assertEqual(
            self.rpc("decode", "protobuf", hex=wire.hex()), {"message": expected}
        )
        # Switching the oneof discards the previous body; final kind/body still must agree.
        wire = unchecked("protobuf", CASES["valid"][0]["message"]) + unchecked(
            "protobuf", {"kind": 2, "get_status": {}}
        )
        expected = CASES["valid"][1]["message"]
        self.assertEqual(codec.decode("protobuf", wire), expected)
        self.assertEqual(
            self.rpc("decode", "protobuf", hex=wire.hex()), {"message": expected}
        )


@contextlib.contextmanager
def server(format, sample="valid", extra=()):
    with tempfile.TemporaryDirectory() as temp:
        path = Path(temp) / "server.jsonl"
        with open(Path(temp) / "stderr", "w+") as stderr:
            process = subprocess.Popen(
                [
                    BINARY,
                    "serve",
                    "--format",
                    format,
                    "--bind",
                    "127.0.0.1:0",
                    "--sample",
                    sample,
                    "--log",
                    str(path),
                    *extra,
                ],
                stdout=subprocess.DEVNULL,
                stderr=stderr,
            )
            try:
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        stderr.seek(0)
                        raise AssertionError(stderr.read())
                    lines = path.read_text().split("\n")[:-1] if path.exists() else []
                    ready = next(
                        (
                            event
                            for event in map(json.loads, lines)
                            if event["outcome"] == "ready"
                        ),
                        None,
                    )
                    if ready:
                        break
                    time.sleep(0.01)
                else:
                    raise AssertionError("responder did not become ready")
                ip, port = ready["local_endpoint"].rsplit(":", 1)
                yield (ip, int(port)), path
            finally:
                process.terminate()
                process.wait(timeout=5)


class NetworkTests(unittest.TestCase):
    def test_commands_rejections_and_recovery(self):
        for format in codec.FORMATS:
            with self.subTest(format=format), server(format) as (
                endpoint,
                path,
            ), socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                sock.bind(("127.0.0.1", 0))
                log = io.StringIO()
                client = Client(
                    sock, endpoint, format, "board-01", 1, EventLog(log, format=format)
                )
                info = client.request("info")
                self.assertEqual(
                    info["device_info"],
                    dict(
                        firmware_version="host-demo-0.1.0",
                        encoding=codec.FORMATS[format],
                    ),
                )
                status = client.request("status")
                self.assertEqual(
                    status["status"],
                    dict(temperature_celsius=25.5, validity=1, synthetic=True),
                )
                health = client.request("health")
                self.assertEqual(
                    health["health"],
                    dict(
                        state=1,
                        received_requests=3,
                        rejected_datagrams=0,
                        send_errors=0,
                        skipped_publications=0,
                    ),
                )
                self.assertEqual(info["boot_id"], status["boot_id"])
                for data in (b"broken", b"x" * 1025):
                    sock.sendto(data, endpoint)
                # Correlatable malformed request gets INVALID_REQUEST, then valid work continues.
                bad = copy.deepcopy(CASES["valid"][0]["message"])
                bad["boot_id"] = "fedcba9876543210"
                sock.sendto(unchecked(format, bad), endpoint)
                sock.settimeout(1)
                reply = codec.decode(format, sock.recvfrom(65535)[0])
                self.assertEqual(reply["error"], {"code": 2})
                self.assertEqual(reply["client_session"], bad["client_session"])
                health = client.request("health")
                self.assertEqual(health["health"]["rejected_datagrams"], 3)
                self.assertEqual(health["health"]["received_requests"], 4)
                deadline = time.monotonic() + 2
                while True:
                    events = [
                        json.loads(line) for line in path.read_text().split("\n")[:-1]
                    ]
                    if any(event["outcome"] == "size_error" for event in events):
                        break
                    self.assertLess(
                        time.monotonic(), deadline, "queued log was not written"
                    )
                    time.sleep(0.01)
                self.assertIn("size_error", [event["outcome"] for event in events])
                rejected = next(
                    event for event in events if event["outcome"] == "size_error"
                )
                self.assertEqual(rejected["raw_length"], 1025)
                self.assertEqual(len(bytes.fromhex(rejected["raw_prefix_hex"])), 64)
                self.assertTrue(
                    all(event["crc"] == "off" and event["revision"] for event in events)
                )

    def test_drop_rules_and_error_budget(self):
        for format in codec.FORMATS:
            with self.subTest(format=format), server(format) as (
                endpoint,
                _,
            ), socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                sock.bind(("127.0.0.1", 0))
                sock.settimeout(0.05)
                request = copy.deepcopy(CASES["valid"][0]["message"])
                dropped = [
                    dict(request, protocol_version=2),
                    dict(request, device_id="other"),
                    dict(request, request_id=0),
                    CASES["valid"][-1]["message"],
                    CASES["valid"][11]["message"],
                ]
                for message in dropped:
                    sock.sendto(unchecked(format, message), endpoint)
                    with self.assertRaises(socket.timeout):
                        sock.recvfrom(65535)
                if format != "csv":
                    sock.settimeout(1)
                    sock.sendto(unchecked(format, dict(request, kind=99)), endpoint)
                    reply = codec.decode(format, sock.recvfrom(65535)[0])
                    self.assertEqual(reply["error"], {"code": 1})
        # A burst of malformed but correlatable messages receives at most the initial budget
        # plus elapsed refills. Send and count on loopback; no retries or response loops.
        with server("json") as (endpoint, path), socket.socket(
            socket.AF_INET, socket.SOCK_DGRAM
        ) as sock:
            sock.bind(("127.0.0.1", 0))
            sock.settimeout(0.08)
            bad = dict(CASES["valid"][0]["message"], boot_id="fedcba9876543210")
            started = time.monotonic()
            for _ in range(30):
                sock.sendto(unchecked("json", bad), endpoint)
            replies = 0
            while True:
                try:
                    codec.decode("json", sock.recvfrom(65535)[0])
                    replies += 1
                except socket.timeout:
                    break
            self.assertGreaterEqual(replies, 1)
            self.assertLessEqual(replies, 10 + int((time.monotonic() - started) * 10))
            self.assertLess(replies, 30)
            client = Client(
                sock,
                endpoint,
                "json",
                "board-01",
                1,
                EventLog(io.StringIO(), format="json"),
            )
            health = client.request("health")
            self.assertEqual(health["health"]["rejected_datagrams"], 30)
            deadline = time.monotonic() + 2
            while '"error_rate_limited"' not in path.read_text():
                self.assertLess(
                    time.monotonic(), deadline, "queued log was not written"
                )
                time.sleep(0.01)

    def test_python_cli_all_formats_and_unavailable(self):
        for format in codec.FORMATS:
            with self.subTest(format=format), server(format, "unavailable") as (
                endpoint,
                _,
            ):
                result = subprocess.run(
                    [
                        sys.executable,
                        str(HERE / "python/client.py"),
                        "--format",
                        format,
                        "--port",
                        str(endpoint[1]),
                    ],
                    capture_output=True,
                    text=True,
                    timeout=5,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                responses = [
                    e["message"]
                    for e in map(json.loads, result.stdout.splitlines())
                    if e["outcome"] == "accepted_response"
                ]
                self.assertEqual(len(responses), 3)
                self.assertEqual(
                    responses[1]["status"], {"validity": 2, "synthetic": True}
                )
                self.assertEqual(responses[2]["health"]["state"], 1)

    def test_timeout_and_correlation(self):
        # Deliberately scripted peer: independent response construction, bad session/device/kind,
        # timeout, late response, then duplicate response during a later request.
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as peer, socket.socket(
            socket.AF_INET, socket.SOCK_DGRAM
        ) as sock:
            peer.bind(("127.0.0.1", 0))
            peer.settimeout(2)
            sock.bind(("127.0.0.1", 0))
            errors = []

            def worker():
                try:
                    first, source = peer.recvfrom(65535)
                    first = codec.decode("json", first)
                    second, source = peer.recvfrom(65535)
                    second = codec.decode("json", second)

                    def response(request):
                        return dict(
                            protocol_version=1,
                            kind=12,
                            device_id="board-01",
                            boot_id="fedcba9876543210",
                            uptime_ms=1,
                            client_session=request["client_session"],
                            request_id=request["request_id"],
                            status=dict(validity=2, synthetic=True),
                        )

                    late, valid = response(first), response(second)
                    for message in (
                        late,
                        dict(valid, device_id="other"),
                        dict(valid, client_session="1234567890abcdef"),
                        valid,
                    ):
                        peer.sendto(codec.encode("json", message), source)
                    third, source = peer.recvfrom(65535)
                    third = codec.decode("json", third)
                    peer.sendto(codec.encode("json", valid), source)
                    peer.sendto(codec.encode("json", response(third)), source)
                except BaseException as error:
                    errors.append(error)

            thread = threading.Thread(target=worker)
            thread.start()
            output = io.StringIO()
            client = Client(
                sock,
                peer.getsockname(),
                "json",
                "board-01",
                0.08,
                EventLog(output, format="json"),
            )
            try:
                self.assertIsNone(client.request("status"))
                client.timeout = 1
                self.assertIsNotNone(client.request("status"))
                self.assertIsNotNone(client.request("status"))
            finally:
                thread.join(timeout=3)
            self.assertFalse(thread.is_alive())
            self.assertFalse(errors, errors)
            outcomes = [
                e["outcome"] for e in map(json.loads, output.getvalue().splitlines())
            ]
            for outcome in (
                "timeout",
                "late_response",
                "unmatched_response",
                "duplicate_response",
                "accepted_response",
            ):
                self.assertIn(outcome, outcomes)
            self.assertEqual(client.history.maxlen, 64)


if __name__ == "__main__":
    unittest.main(verbosity=2)
