"""B2 real-codec/state/UDP tests. Loopback evidence is not a physical board result."""

import copy
import csv
import importlib.util
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
from contextlib import nullcontext

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE / "python"))
from benchmark_live import parser, responder, run
from benchmark_service import Service, valid_configuration
from benchmark_wire import Wire, pattern
from codec import CodecError
from load_report import inspect_report
from framing import seal, open_frame
from google.protobuf import json_format

BASE = dict(
    protocol_version=1,
    device_id="board-01",
    boot_id="0123456789abcdef",
    session_id="abcdef0123456789",
    run_epoch=1,
)
CONFIG = dict(
    payload_bytes=512,
    pattern_seed=723,
    status_period_us=10000,
    health_period_us=10000,
    lease_ms=2000,
)
PEER = ("127.0.0.1", 50000)


def control(kind, request=1, **values):
    return dict(
        BASE,
        kind=kind,
        request_id=request,
        **{
            {2: "configure", 3: "renew", 4: "stop"}[kind]: (
                copy.deepcopy(CONFIG) if kind == 2 else {}
            )
        },
        **values,
    )


class CodecTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        protocol = HERE.parents[1] / "protocol"
        subprocess.run(
            [
                sys.executable,
                "-m",
                "grpc_tools.protoc",
                f"-I{protocol}",
                f"--python_out={cls.temp.name}",
                str(protocol / "messaging.proto"),
                str(protocol / "benchmark/benchmark.proto"),
            ],
            check=True,
        )
        sys.path.insert(0, cls.temp.name)
        spec = importlib.util.spec_from_file_location(
            "b2_reference_pb", Path(cls.temp.name) / "benchmark/benchmark_pb2.py"
        )
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

        cls.proto = module.Envelope

    @classmethod
    def tearDownClass(cls):
        sys.path.remove(cls.temp.name)
        cls.temp.cleanup()

    def test_all_reference_fixtures_and_manual_golden(self):
        spec = importlib.util.spec_from_file_location(
            "b1_checker", HERE.parents[1] / "protocol/benchmark/check_contract.py"
        )
        reference = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(reference)
        cases, _ = reference.expanded()
        for format in ("csv", "json", "protobuf"):
            for crc in ("off", "on"):
                wire = Wire(format, crc)
                for case in cases:
                    message = copy.deepcopy(case["message"])
                    if "limits" in message:
                        message["limits"].update(
                            encoding={"csv": 1, "json": 2, "protobuf": 3}[format],
                            crc_enabled=crc == "on",
                        )
                    encoded = wire.encode(message)
                    self.assertLessEqual(len(encoded), 1024)
                    self.assertEqual(
                        wire.decode(encoded), message, (format, crc, case["name"])
                    )
                    if format == "csv":
                        independent = reference.csv_encode(message)
                    elif format == "json":
                        independent = json.dumps(
                            message, separators=(",", ":")
                        ).encode()
                    else:
                        independent = json_format.ParseDict(
                            message, self.proto()
                        ).SerializeToString()
                        restored = json_format.MessageToDict(
                            self.proto.FromString(open_frame(encoded, crc)),
                            preserving_proto_field_name=True,
                            use_integers_for_enums=True,
                        )
                        self.assertEqual(restored, message)
                    self.assertEqual(wire.decode(seal(independent, crc)), message)
                golden = reference.CASES["golden_capabilities"]
                original = next(
                    c["message"] for c in cases if c["name"] == "capabilities"
                )
                if crc == "off":
                    expected = (
                        bytes.fromhex(golden["protobuf_hex"])
                        if format == "protobuf"
                        else golden[format].encode()
                    )
                    self.assertEqual(wire.encode(original), expected)

    def test_strict_rejections_presence_crc_and_status(self):
        for format in ("csv", "json", "protobuf"):
            for crc in ("off", "on"):
                wire = Wire(format, crc)
                message = dict(BASE, kind=5, request_id=1, payload="", probe={})
                self.assertEqual(wire.decode(wire.encode(message))["payload"], "")
                for bad in (
                    dict(message, payload="x" * 513),
                    dict(message, payload="!"),
                    dict(message, protocol_version=2),
                    dict(message, run_epoch=0),
                ):
                    with self.assertRaises(CodecError):
                        wire.encode(bad)
                del message["payload"]
                with self.assertRaises(CodecError):
                    wire.encode(message)
                for validity in (1, 2, 3):
                    status = dict(validity=validity, synthetic=True)
                    if validity == 1:
                        status["temperature_celsius"] = -99.99999237060547
                    m = dict(
                        BASE,
                        kind=21,
                        sequence=2**32 - 1,
                        uptime_ms=2**32 - 1,
                        payload="x" * 512,
                        status=status,
                    )
                    self.assertEqual(wire.decode(wire.encode(m)), m)
                packet = wire.encode(
                    dict(BASE, kind=5, request_id=1, payload="x", probe={})
                )
                if crc == "on":
                    with self.assertRaisesRegex(CodecError, "crc_error"):
                        wire.decode(bytes([packet[0] ^ 1]) + packet[1:])
        wire = Wire("json", "off")
        good = wire.encode(dict(BASE, kind=5, request_id=1, payload="", probe={}))
        for bad in (
            good.replace(b'"kind":5', b'"kind":5,"kind":5'),
            good.replace(b'"payload":""', b'"payload":null'),
            good[:-1] + b',"unknown":0}',
        ):
            with self.assertRaises(CodecError):
                wire.decode(bad)
        wire = Wire("csv", "off")
        good = wire.encode(dict(BASE, kind=5, request_id=1, payload="", probe={}))
        for bad in (
            good[:-2] + b"\n",
            good.replace(b"1,5", b"01,5"),
            good.replace(b"board-01", b'bo"ard-01'),
        ):
            with self.assertRaises(CodecError):
                wire.decode(bad)


class ServiceTests(unittest.TestCase):
    def service(self):
        return Service("board-01", BASE["boot_id"], "json", "off", 0)

    def code(self, s, m, now=0, peer=PEER):
        reply = s.handle(m, peer, now)
        self.assertIsNotNone(reply)
        return reply["result"]["code"]

    def test_acquire_duplicate_conflict_stale_and_owner(self):
        s = self.service()
        self.assertEqual(self.code(s, control(2)), 1)
        expiry = s.expiry
        self.assertEqual(self.code(s, control(2), 1), 2)
        self.assertEqual(s.expiry, expiry)
        bad = control(2)
        bad["configure"]["payload_bytes"] = 1
        self.assertEqual(self.code(s, bad, 2), 8)
        self.assertEqual(self.code(s, control(3, 2), 3), 1)
        self.assertEqual(self.code(s, control(3, 1), 4), 7)
        self.assertEqual(self.code(s, control(3, 3), 5, ("127.0.0.2", 50000)), 5)
        self.assertEqual(self.code(s, control(2, 3), 6), 5)
        self.assertEqual(s.counters["received_requests"], 2)

    def test_exact_expiry_old_messages_and_link_loss(self):
        s = self.service()
        self.code(s, control(2))
        self.assertEqual(self.code(s, control(3, 2), 2_000_000_000), 6)
        self.assertIsNone(s.owner)
        self.assertEqual(s.epoch, 2)
        probe = dict(BASE, kind=5, request_id=1, payload=pattern(512, 723, 1), probe={})
        self.assertIsNone(s.handle(probe, PEER, 2_000_000_000))
        new = control(2)
        new["run_epoch"] = 2
        self.assertEqual(self.code(s, new, 2_000_000_000), 1)
        self.assertEqual(self.code(s, control(4, 3), 2_000_000_000), 6)
        s.end()  # Host shutdown/link-loss hook, also required by B3 board integration.
        self.assertEqual(s.epoch, 3)
        self.assertFalse(s.schedules)

    def test_stop_duplicate_stop_delayed_config_and_idle(self):
        s = self.service()
        self.code(s, control(2))
        self.assertEqual(self.code(s, control(4, 2)), 3)
        self.assertEqual(self.code(s, control(4, 2)), 6)
        self.assertEqual(self.code(s, control(2)), 6)
        idle = control(3, 3)
        idle["run_epoch"] = 2
        self.assertEqual(self.code(s, idle), 9)

    def test_invalid_config_is_atomic_and_no_id_consumption(self):
        s = self.service()
        bad = control(2)
        bad["configure"]["status_period_us"] = 999
        self.assertEqual(self.code(s, bad), 4)
        self.assertIsNone(s.owner)
        self.assertEqual(self.code(s, control(2)), 1)
        self.assertTrue(valid_configuration(s.config))

    def test_final_epoch_and_control_id(self):
        s = self.service()
        s.epoch = 2**32 - 1
        m = control(2, 2**32 - 1)
        m["run_epoch"] = s.epoch
        self.assertEqual(self.code(s, m), 1)
        renew = control(3, 1)
        renew["run_epoch"] = s.epoch
        self.assertEqual(self.code(s, renew), 7)
        s.tick(2_000_000_000)
        self.assertTrue(s.exhausted)
        self.assertEqual(self.code(s, m, 2_000_000_000), 10)

    def test_duplicate_renew_no_extension_and_bucket_does_not_block_actions(self):
        s = self.service()
        self.code(s, control(2))
        self.assertEqual(self.code(s, control(3, 2), 1_000_000_000), 1)
        self.assertEqual(self.code(s, control(3, 2), 1_100_000_000), 2)
        self.assertEqual(s.expiry, 3_000_000_000)
        s.tokens = 0
        self.assertIsNone(s.handle(control(3, 3), PEER, 1_100_000_000))
        self.assertEqual(s.expiry, 3_100_000_000)
        self.assertIsNone(s.handle(control(4, 4), PEER, 1_100_000_000))
        self.assertIsNone(s.owner)

    def test_probe_pattern_schedule_and_no_catchup(self):
        s = self.service()
        self.code(s, control(2))
        bad = dict(BASE, kind=5, request_id=1, payload="x", probe={})
        self.assertIsNone(s.handle(bad, PEER, 0))
        good = dict(bad, payload=pattern(512, 723, 1))
        self.assertEqual(s.handle(good, PEER, 0)["payload"], good["payload"])
        self.assertFalse(s.publications(9_999_999))
        output = s.publications(35_000_000)
        self.assertEqual([m["sequence"] for m in output], [2, 2])
        self.assertEqual(s.counters["skipped_publications"], 4)
        self.assertFalse(s.publications(35_000_000))


class SocketTests(unittest.TestCase):
    def test_six_configurations_real_command_and_multicast(self):
        # Allocate ports without relying on fixed CI machine availability.
        for format in ("csv", "json", "protobuf"):
            for selected_crc in (
                ("off", "on", "fault", "interrupt")
                if format == "json"
                else ("off", "on")
            ):
                injecting = selected_crc == "fault"
                interrupting = selected_crc == "interrupt"
                crc = "on" if injecting or interrupting else selected_crc
                with self.subTest(
                    format=format, crc=crc
                ), tempfile.TemporaryDirectory() as tmp:
                    ports = []
                    sockets = []
                    for _ in range(3):
                        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                        sock.bind(("127.0.0.1", 0))
                        sockets.append(sock)
                        ports.append(sock.getsockname()[1])
                    for sock in sockets:
                        sock.close()
                    common = [
                        "--format",
                        format,
                        "--crc",
                        crc,
                        "--interface",
                        "127.0.0.1",
                        "--command-port",
                        str(ports[0]),
                        "--status-port",
                        str(ports[1]),
                        "--health-port",
                        str(ports[2]),
                    ]
                    server = parser().parse_args(
                        ["responder", *common, "--bind", "127.0.0.1"]
                    )
                    stop, ready = threading.Event(), threading.Event()
                    errors = []
                    controls = set()

                    def fault(request, packet):
                        kind = request["kind"]
                        if kind in (2, 3) and kind not in controls:
                            controls.add(kind)
                            return (
                                []
                            )  # Lost ACK: retry must not restart/extend the run.
                        if kind == 5:
                            residue = request["request_id"] % 5
                            if residue == 0:
                                return []
                            if residue == 1:
                                return [(90_000_000, packet)]
                            if residue == 2:
                                return [(0, packet), (0, packet)]
                            if residue == 3:
                                return [(0, bytes([packet[0] ^ 1]) + packet[1:])]
                            if residue == 4:
                                return [(0, packet)] + [(0, b"invalid")] * 16
                        return [(0, packet)]

                    def serve():
                        try:
                            responder(server, stop, ready, fault if injecting else None)
                        except BaseException as error:
                            errors.append(error)
                            ready.set()

                    thread = threading.Thread(target=serve, daemon=True)
                    thread.start()
                    self.assertTrue(ready.wait(5))
                    self.assertFalse(errors)
                    try:
                        client = parser().parse_args(
                            [
                                "run",
                                *common,
                                "--target",
                                "127.0.0.1",
                                "--target-kind",
                                "host",
                                "--output",
                                str(Path(tmp) / "run"),
                                "--duration",
                                "2.2" if injecting else ".18",
                                "--warmup",
                                ".03",
                                "--report-interval",
                                ".5" if injecting else ".05",
                                "--command-hz",
                                "100",
                                "--deadline-ms",
                                "50",
                                "--payload-bytes",
                                "512",
                                "--status-period-us",
                                "10000",
                                "--lease-ms",
                                "2000",
                                "--health-period-us",
                                "10000",
                            ]
                        )
                        with (
                            patch(
                                "benchmark_live.Pacer.due",
                                side_effect=KeyboardInterrupt,
                            )
                            if interrupting
                            else nullcontext()
                        ):
                            self.assertEqual(run(client), not interrupting)
                        evidence = inspect_report(Path(tmp) / "run/events.jsonl")
                        self.assertEqual(evidence["complete"], not interrupting)
                        if interrupting:
                            self.assertEqual(
                                evidence["final"]["reason"], "keyboard_interrupt"
                            )
                            continue
                        counts = evidence["final"]["probes"]["counts"]
                        with (Path(tmp) / "run/intervals.csv").open() as source:
                            rows = list(csv.DictReader(source))
                        self.assertEqual(
                            sum(int(row["sent"]) for row in rows), counts.get("sent", 0)
                        )
                        self.assertEqual(
                            sum(int(row["timely_reply"]) for row in rows),
                            counts.get("timely_reply", 0),
                        )
                        self.assertGreater(counts.get("timely_reply", 0), 0)
                        self.assertEqual(
                            counts.get("sent", 0),
                            counts.get("timely_reply", 0)
                            + counts.get("deadline_misses", 0),
                        )
                        for stream in evidence["final"]["streams"].values():
                            self.assertGreater(stream["counts"].get("progress", 0), 0)
                        if injecting:
                            for name in (
                                "deadline_misses",
                                "late_reply",
                                "duplicate_reply",
                                "crc_error",
                            ):
                                self.assertGreater(counts.get(name, 0), 0, name)
                            self.assertIn(
                                3, controls
                            )  # Duration exceeds the initial lease.
                    finally:
                        stop.set()
                        thread.join(5)
                    self.assertFalse(thread.is_alive())
                    self.assertFalse(errors)


if __name__ == "__main__":
    unittest.main()
