"""Comparison math, capture boundaries/integrity, and real six-configuration orchestration."""

import hashlib
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import unittest

from test_demo import BINARY, HERE
from benchmark import inputs
from comparison_stats import export, percentile, stream_statistics, summarize, write_csv


class StatisticsTests(unittest.TestCase):
    def test_percentiles_and_stream_boundaries(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "empty.csv"
            write_csv(path, [{"old": "result"}])
            write_csv(path, [])
            self.assertEqual(path.read_text(), "")
        self.assertIsNone(percentile([], 0.5))
        self.assertEqual(percentile([7], 0.99), 7)
        self.assertAlmostEqual(percentile([10, 0, 30, 20], 0.95), 28.5)

        def event(stamp, outcome, **extra):
            return stamp, dict(outcome=outcome, kind=21, boot_id="a", **extra)

        events = [
            event(-100, "accepted_publication"),
            event(-10, "stale"),
            event(100, "recovered"),
            event(100, "accepted_publication"),
            event(200, "gap_pending", sequences=[2, 3], count=2),
            event(300, "reordered_publication", message=dict(sequence=2)),
            event(400, "duplicate_publication"),
            event(500, "stale"),
            event(1000, "recovered"),
            event(1000, "accepted_publication"),
        ]
        result = stream_statistics(events, 0, 1000, 21)
        self.assertEqual(
            result["unique_received"], 2
        )  # excludes duplicate and end boundary
        self.assertEqual(result["gap_unresolved_at_end"], 1)
        self.assertEqual(result["duplicate_publications"], 1)
        self.assertAlmostEqual(
            result["stale_seconds"], 0.6
        )  # includes stale at window start
        self.assertAlmostEqual(result["fresh_seconds"], 0.4)
        unseen = stream_statistics([], 0, 1000, 22)
        self.assertEqual(unseen["not_seen_seconds"], 1)

    def fixture(self, directory):
        manifest = dict(
            comparison_id="fixture",
            mode="board",
            format="json",
            crc="on",
            instrumented=False,
            measurement_start_ns=1_000_000_000,
            measurement_end_ns=3_000_000_000,
            command_period_seconds=1,
            clock_anchor=dict(monotonic_ns=0, unix_ms=10000),
            complete=True,
            logs={"client": "client.jsonl"},
            log_sha256={},
        )

        def event(stamp, outcome, request, **extra):
            return dict(
                format="json",
                crc="on",
                run_id="r",
                outcome=outcome,
                host_monotonic_ns=int(stamp * 1e6),
                monotonic_ms=stamp,
                unix_ms=10000 + stamp,
                raw_length=50,
                message=dict(client_session="s", request_id=request, kind=1),
                **extra,
            )

        # Warmup and exactly-end sends are excluded. A response after the window
        # still belongs to the measured request. Duplicate diagnostics are not successes.
        events = [
            event(500, "sent_request", 1),
            event(600, "accepted_response", 1, rtt_ms=100),
            event(1000, "sent_request", 2),
            event(1010, "accepted_response", 2, rtt_ms=10),
            event(1100, "duplicate_response", 2),
            event(2000, "sent_request", 3),
            event(3000, "timeout", 3),
            event(3000, "sent_request", 4),
        ]
        self.save(directory, manifest, events)
        return manifest, events

    def save(self, directory, manifest, events):
        data = "".join(json.dumps(e) + "\n" for e in events).encode()
        (directory / "client.jsonl").write_bytes(data)
        manifest["log_sha256"]["client"] = hashlib.sha256(data).hexdigest()
        (directory / "manifest.json").write_text(json.dumps(manifest))

    def test_request_cohort_timeout_missing_and_log_integrity(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            manifest, events = self.fixture(directory)
            report = summarize(directory)
            summary = report["summary"]
            self.assertEqual(
                (summary["requests"], summary["successes"], summary["timeouts"]),
                (2, 1, 1),
            )
            self.assertEqual(summary["rtt_p99_ms"], 10)
            self.assertTrue(summary["data_valid"])
            self.assertFalse(summary["all_commands_succeeded"])
            events = [e for e in events if e["outcome"] != "timeout"]
            self.save(directory, manifest, events)
            summary = summarize(directory)["summary"]
            self.assertFalse(summary["data_valid"])
            self.assertEqual(summary["timeouts"], 0)
            self.assertEqual(summary["missing_terminal_events"], 1)
            with (directory / "client.jsonl").open("a") as output:
                output.write(json.dumps(events[-1]) + "\n")
            self.assertTrue(
                any(
                    "SHA-256" in error
                    for error in summarize(directory)["integrity_errors"]
                )
            )

    def test_clock_jump_and_mixed_configuration_invalidate_data(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            manifest, events = self.fixture(directory)
            events[-1]["unix_ms"] += 200
            events[-1]["crc"] = "off"
            self.save(directory, manifest, events)
            errors = summarize(directory)["integrity_errors"]
            self.assertTrue(any("drift" in error for error in errors))
            self.assertTrue(any("mismatch" in error for error in errors))


class ComparisonProcesses(unittest.TestCase):
    def test_benchmark_measures_real_six_configuration_fixtures(self):
        fixtures = inputs()
        result = subprocess.run(
            [BINARY, "bench", "2", "2"],
            input="".join(json.dumps(f) + "\n" for f in fixtures),
            text=True,
            capture_output=True,
            check=True,
            timeout=20,
        )
        rows = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(rows), len(fixtures) * 4)
        # CSV/JSON float spelling can differ between languages. Compare benchmark
        # lengths with the Rust fixture RPC's actual encoded datagrams.
        encoded = subprocess.run(
            [BINARY, "codec"],
            input="".join(
                json.dumps(dict(f, operation="encode")) + "\n" for f in fixtures
            ),
            text=True,
            capture_output=True,
            check=True,
            timeout=20,
        )
        expected = {
            (f["format"], f["crc"], f["fixture"]): len(json.loads(line)["hex"]) // 2
            for f, line in zip(fixtures, encoded.stdout.splitlines(), strict=True)
        }
        for row in rows:
            self.assertEqual(
                row["datagram_bytes"],
                expected[row["format"], row["crc"], row["fixture"]],
            )
            self.assertEqual(
                row["datagram_bytes"] - row["body_bytes"],
                8 if row["crc"] == "on" else 0,
            )
            self.assertGreater(row["elapsed_ns"], 0)
        invalid = subprocess.run([BINARY, "bench", "0", "2"], capture_output=True)
        self.assertNotEqual(invalid.returncode, 0)

    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux comparison harness")
    def test_matrix_two_listeners_and_reanalysis(self):
        # Bind three sockets concurrently while choosing distinct free ports.
        sockets = [socket.socket(socket.AF_INET, socket.SOCK_DGRAM) for _ in range(3)]
        try:
            for sock in sockets:
                sock.bind(("127.0.0.1", 0))
            ports = [sock.getsockname()[1] for sock in sockets]
        finally:
            for sock in sockets:
                sock.close()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "matrix"
            subprocess.run(
                [
                    sys.executable,
                    HERE / "python/compare.py",
                    "run",
                    "--binary",
                    BINARY,
                    "--interface",
                    "127.0.0.1",
                    "--duration",
                    "2.2",
                    "--warmup",
                    ".2",
                    "--command-period",
                    ".2",
                    "--port",
                    str(ports[0]),
                    "--status-port",
                    str(ports[1]),
                    "--health-port",
                    str(ports[2]),
                    "--output",
                    root,
                    "--notes",
                    "CI loopback functional smoke; not a performance measurement",
                ],
                check=True,
                capture_output=True,
                text=True,
                timeout=60,
            )
            original = (root / "summary.csv").read_bytes()
            reports = export(root)
            self.assertEqual((root / "summary.csv").read_bytes(), original)
            self.assertEqual(len(reports), 6)
            self.assertEqual(
                {(r["summary"]["format"], r["summary"]["crc"]) for r in reports},
                {(f, c) for f in ("csv", "json", "protobuf") for c in ("off", "on")},
            )
            for report in reports:
                self.assertTrue(
                    report["summary"]["data_valid"], report["integrity_errors"]
                )
                self.assertTrue(report["summary"]["all_commands_succeeded"])
                self.assertEqual(len(report["streams"]), 4)
                self.assertTrue(all(s["unique_received"] for s in report["streams"]))
                self.assertTrue(
                    all(s["logged_sent_publications"] for s in report["streams"])
                )
                self.assertFalse(
                    any(s["duplicate_publications"] for s in report["streams"])
                )
            # Exports do not overwrite raw capture logs, and edited logs are flagged.
            manifest_path = next(root.glob("*/manifest.json"))
            manifest = json.loads(manifest_path.read_text())
            manifest["log_sha256"]["client"] = "changed"
            manifest_path.write_text(json.dumps(manifest))
            self.assertFalse(summarize(manifest_path.parent)["summary"]["data_valid"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
