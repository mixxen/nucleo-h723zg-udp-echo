"""B4 policy, orchestration and actual Rust-service socket evidence."""

import copy
from contextlib import contextmanager
import json
from pathlib import Path
import socket
import struct
import sys
import tempfile
import threading
import tracemalloc
import unittest
from unittest.mock import patch

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE / "python"))
from benchmark_campaign import (
    assess,
    execute,
    judge,
    normalize,
    policy,
    reassess,
    stages,
)
from benchmark_live import parser, responder
from load_metrics import Histogram, Pacer, ProbeTracker, RunIdentity
from load_report import RunReport
from benchmark_profile import Sampler, decode_snapshot


def config(mode="soak"):
    c = dict(
        mode=mode,
        base=dict(
            target="127.0.0.1",
            target_kind="host",
            interface="127.0.0.1",
            format="json",
            crc="on",
            command_hz=100,
            payload_bytes=0,
            status_period_us=0,
            health_period_us=0,
            warmup=0,
            report_interval=0.05,
            deadline_ms=30,
            lease_ms=2000,
        ),
        thresholds=dict(
            max_p99_ms=1000,
            max_deadline_fraction=1,
            min_send_ratio=0,
            max_stale_fraction=1,
            max_gap_fraction=1,
            live_min_seconds=0.1,
        ),
    )
    if mode == "soak":
        c["duration_seconds"] = 0.25
    else:
        c.update(
            payloads=[64],
            rates=[dict(command_hz=100, status_period_us=0, health_period_us=0)],
            dwell_seconds=0.25,
            recovery=dict(
                payload_bytes=0,
                duration_seconds=0.2,
                command_hz=50,
                status_period_us=0,
                health_period_us=0,
            ),
        )
    return c


@contextmanager
def server(c, fault=None):
    sockets = [socket.socket(socket.AF_INET, socket.SOCK_DGRAM) for _ in range(3)]
    for sock in sockets:
        sock.bind(("127.0.0.1", 0))
    ports = [s.getsockname()[1] for s in sockets]
    for sock in sockets:
        sock.close()
    cli = [
        "responder",
        "--format",
        "json",
        "--crc",
        "on",
        "--interface",
        "127.0.0.1",
        "--bind",
        "127.0.0.1",
    ]
    for name, port in zip(("command_port", "status_port", "health_port"), ports):
        c["base"][name] = port
        cli += ["--" + name.replace("_", "-"), str(port)]
    stop, ready = threading.Event(), threading.Event()
    worker = threading.Thread(
        target=responder,
        args=(parser().parse_args(cli), stop, ready, fault),
        daemon=True,
    )
    worker.start()
    try:
        if not ready.wait(5):
            raise RuntimeError("responder did not start")
        yield
    finally:
        stop.set()
        worker.join(5)
        if worker.is_alive():
            raise RuntimeError("responder did not stop")


class PolicyTests(unittest.TestCase):
    def test_offline_assessment_memory_does_not_retain_intervals(self):
        parameters = dict(
            normalize(config())["base"], duration=4, acceptance=policy({})
        )
        identity = RunIdentity(
            "127.0.0.1", 42010, "board-01", "0123456789abcdef", "abcdef0123456789", 1
        )
        probes = ProbeTracker(identity)
        pacer = Pacer(0, 100)
        probes.sent(1, 0, 100)
        probes.reply(identity, 1, 15, 1, 100)
        with tempfile.TemporaryDirectory() as tmp:
            with RunReport(Path(tmp) / "run", dict(parameters=parameters), 0) as report:
                report.diagnostic(
                    0,
                    run_identity=identity.__dict__,
                    measured_start_ns=0,
                    measured_end_ns=4_000_000_000,
                )
                for i in range(1, 4001):
                    report.interval(i * 1_000_000, probes, pacer, [])
                report.diagnostic(4_030_000_000, cleanup="stop_acknowledged")
                report.finish(
                    4_030_000_000,
                    "duration_and_deadline_drain",
                    True,
                    probes,
                    pacer,
                    [],
                )
            tracemalloc.start()
            try:
                result = assess(Path(tmp) / "run/events.jsonl")
                _, peak = tracemalloc.get_traced_memory()
            finally:
                tracemalloc.stop()
            self.assertEqual(result["outcome"], "fail")  # Deliberately undersent.
            self.assertIn("command_send_ratio", result["failures"])
            self.assertLess(peak, 8 * 1024 * 1024)

    def test_optional_profiler_never_waits_and_reports_missing_samples(self):
        packet = struct.pack(
            "<8sIIQQQII",
            b"STRMPRF1",
            400_000_000,
            32768,
            200_000_000,
            32768,
            10,
            2048,
            90000,
        )
        self.assertEqual(decode_snapshot(packet)["executor_busy_percent"], 50)
        with self.assertRaises(ValueError):
            decode_snapshot(packet[:-1])
        with patch("benchmark_profile.socket.socket") as constructor:
            sock = constructor.return_value
            sock.recv.side_effect = [
                BlockingIOError(),
                packet,
                BlockingIOError(),
                BlockingIOError(),
            ]
            sampler = Sampler("127.0.0.1", "127.0.0.1", 1, 0)
            self.assertIsNone(sampler.tick(0))
            self.assertEqual(sampler.tick(1)["status"], "sample")
            self.assertIsNone(sampler.tick(1_000_000_000))
            self.assertEqual(sampler.tick(2_000_000_000)["status"], "timeout")
            sock.setblocking.assert_called_once_with(False)
            sampler.close()
            sock.close.assert_called_once()

    def test_configuration_is_bounded_and_explicit(self):
        for path in (HERE / "examples").glob("benchmark-*.json"):
            self.assertTrue(list(stages(normalize(json.loads(path.read_text())))))
        c = normalize(config("stress"))
        self.assertEqual(
            [s["role"] for s in stages(c)], ["baseline", "load", "recovery"]
        )
        self.assertEqual(normalize(c), c)
        bad = config("stress")
        bad["payloads"] = list(range(64))
        bad["rates"] *= 3
        with self.assertRaises(ValueError):
            normalize(bad)
        for mutation in (
            {"thresholds": {"max_p99_ms": float("nan")}},
            {"duration_seconds": float("inf")},
            {"stop_on_failure": "false"},
            {"typo": 1},
        ):
            with self.assertRaises(ValueError):
                normalize(dict(config(), **mutation))
        bad = config("stress")
        bad["rates"].append(dict(command_hz=99, status_period_us=0, health_period_us=0))
        with self.assertRaises(ValueError):
            normalize(bad)

    def test_threshold_boundaries_and_host_limits(self):
        h = Histogram()
        h.add(10_000_000)
        r = dict(
            probes=dict(
                counts=dict(sent=100, timely_reply=99, deadline_misses=1),
                rtt=h.snapshot(),
            ),
            pacing=dict(scheduling_skips=1, window_skips=2),
            streams={},
        )
        p = config()["base"]
        t = policy(dict(max_p99_ms=10, min_send_ratio=1, max_deadline_fraction=0.01))
        self.assertFalse(judge(r, p, t, 1, True)["failures"])
        r["probes"]["counts"]["deadline_misses"] = 2
        result = judge(r, p, t, 1, True)
        self.assertEqual(result["failures"], ["command_deadline_fraction"])
        self.assertEqual(
            result["host_limits"], dict(scheduling_skips=1, window_skips=2)
        )
        r["probes"]["counts"]["boot_change_observations"] = 1
        self.assertIn("boot_change_observed", judge(r, p, t, 1, True)["failures"])

    def test_no_traffic_and_pending_gaps_do_not_imply_pass_or_loss(self):
        p = dict(config()["base"], status_period_us=10000)
        r = dict(
            probes=dict(counts={}, rtt=Histogram().snapshot()),
            pacing={},
            streams={
                "21": dict(
                    counts={"baseline": 1},
                    freshness_ns={"not_seen": 50, "fresh": 50},
                    pending_gaps=100,
                )
            },
        )
        result = judge(r, p, policy({}), 1, True)
        self.assertIn("no_usable_command_latency", result["failures"])
        self.assertIn("stream_21_stale", result["failures"])
        self.assertEqual(result["metrics"]["21"]["gap_fraction"], 0)


class CampaignTests(unittest.TestCase):
    def test_soak_is_one_configuration_and_reanalysis_matches(self):
        c = config()
        c["duration_seconds"] = 2.2
        c["base"]["report_interval"] = 0.2
        c["base"].update(status_period_us=10000, health_period_us=10000)
        controls = []

        def record(request, packet):
            if request["kind"] != 5:
                controls.append(request["kind"])
            return [(0, packet)]

        with tempfile.TemporaryDirectory() as tmp, server(c, record):
            result = execute(c, tmp + "/soak")
            self.assertEqual(result["outcome"], "pass")
            self.assertEqual(controls.count(2), 1)
            self.assertEqual(controls.count(4), 1)
            self.assertGreaterEqual(controls.count(3), 2)
            self.assertEqual(reassess(tmp + "/soak")["outcome"], "pass")
            path = Path(tmp) / "soak/stage-000/events.jsonl"
            self.assertEqual(assess(path, dict(max_p99_ms=0))["outcome"], "fail")
            records = [json.loads(line) for line in path.read_text().splitlines()]
            self.assertGreater(sum(r["type"] == "interval" for r in records), 2)
            self.assertTrue(any(r.get("health") for r in records))
            final = records[-1]
            final["probes"]["counts"]["sent"] += 1
            path.write_text("".join(json.dumps(r) + "\n" for r in records))
            self.assertEqual(assess(path)["outcome"], "invalid")
            self.assertEqual(reassess(tmp + "/soak")["outcome"], "invalid")

    def test_load_failure_stops_ramp_but_checks_recovery(self):
        c = config("stress")
        c["thresholds"]["max_deadline_fraction"] = 0.1
        c["rates"].append(dict(command_hz=200, status_period_us=0, health_period_us=0))

        def fault(request, packet):
            return (
                []
                if request["kind"] == 5 and len(request["payload"]) == 64
                else [(0, packet)]
            )

        with tempfile.TemporaryDirectory() as tmp, server(c, fault):
            result = execute(c, tmp + "/stress")
            self.assertEqual(result["outcome"], "fail")
            self.assertEqual(result["reason"], "load_failed_recovery_passed")
            saved = reassess(tmp + "/stress")
            self.assertEqual(saved["stage_outcomes"], ["pass", "fail", "pass"])
            self.assertEqual(saved["outcome"], "fail")
            load = assess(Path(tmp) / "stress/stage-001/events.jsonl")
            self.assertEqual(load["reason"], "threshold_stop")
            self.assertEqual(load["cleanup"], "stop_acknowledged")
            recovery = Path(tmp) / "stress/stage-002/events.jsonl"
            records = [json.loads(line) for line in recovery.read_text().splitlines()]
            for record in records:
                if (
                    record["type"] == "diagnostic"
                    and "run_identity" in record["values"]
                ):
                    record["values"]["run_identity"]["boot_id"] = "ffffffffffffffff"
            recovery.write_text("".join(json.dumps(r) + "\n" for r in records))
            rebooted = reassess(tmp + "/stress")
            self.assertEqual(rebooted["stage_outcomes"], ["pass", "fail", "fail"])
            self.assertIn(
                "boot_changed_between_stages",
                rebooted["results"][2]["assessment"]["failures"],
            )

    def test_failed_recovery_prevents_further_load(self):
        c = config("stress")
        c["thresholds"]["max_deadline_fraction"] = 0.1
        c["rates"].append(dict(command_hz=200, status_period_us=0, health_period_us=0))
        failed = False

        def fault(request, packet):
            nonlocal failed
            if request["kind"] == 5:
                failed |= len(request["payload"]) == 64
                if failed:
                    return []
            return [(0, packet)]

        with tempfile.TemporaryDirectory() as tmp, server(c, fault):
            result = execute(c, tmp + "/stress")
            self.assertEqual(result["reason"], "recovery_failed")
            self.assertEqual(
                reassess(tmp + "/stress")["stage_outcomes"], ["pass", "fail", "fail"]
            )

    def test_interrupted_soak_stops_and_keeps_evidence(self):
        c = config()
        with tempfile.TemporaryDirectory() as tmp, server(c):
            import benchmark_live

            original = benchmark_live.RunReport.interval

            def interrupt(report, *args):
                record = original(report, *args)
                if not getattr(report, "interrupted_once", False):
                    report.interrupted_once = True
                    raise KeyboardInterrupt
                return record

            with patch.object(benchmark_live.RunReport, "interval", interrupt):
                result = execute(c, tmp + "/interrupted")
            self.assertEqual(result["outcome"], "incomplete")
            self.assertEqual(reassess(tmp + "/interrupted")["outcome"], "incomplete")
            path = Path(tmp) / "interrupted/stage-000/events.jsonl"
            self.assertEqual(assess(path)["cleanup"], "stop_acknowledged")
            path.write_text(path.read_text() + '{"type":')
            self.assertEqual(assess(path)["outcome"], "incomplete")

    def test_mismatched_stage_and_missing_final_cannot_pass(self):
        c = config()
        with tempfile.TemporaryDirectory() as tmp, server(c):
            execute(c, tmp + "/run")
            log = Path(tmp) / "run/campaign.jsonl"
            lines = log.read_text().splitlines()
            log.write_text("\n".join(lines[:-1]) + "\n")
            self.assertEqual(reassess(tmp + "/run")["outcome"], "incomplete")
            r = json.loads(lines[1])
            r["stage"]["duration"] += 1
            lines[1] = json.dumps(r)
            log.write_text("\n".join(lines) + "\n")
            self.assertEqual(reassess(tmp + "/run")["outcome"], "invalid")


if __name__ == "__main__":
    unittest.main()
