"""Deterministic B2 accounting tests. No board or network timing assumptions."""

import csv
from dataclasses import replace
import json
from pathlib import Path
import random
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parent / "python"))
from load_metrics import Histogram, Pacer, ProbeTracker, RunIdentity, StreamTracker
from load_report import RunReport, inspect_report

IDENTITY = RunIdentity(
    "192.0.2.1", 42010, "board-01", "0123456789abcdef", "abcdef0123456789", 1
)
MS = 1_000_000


class HistogramTests(unittest.TestCase):
    def test_exact_nearest_rank_with_documented_error(self):
        rng = random.Random(723)
        samples = [0, 1, 999, 1000, 1001, 60_000_000_000]
        samples += [rng.randrange(60_000_000_001) for _ in range(20000)]
        whole, left, right = Histogram(), Histogram(), Histogram()
        for index, value in enumerate(samples):
            whole.add(value)
            (left if index % 2 else right).add(value)
        left.merge(right)
        self.assertEqual(whole.snapshot(), left.snapshot())
        samples.sort()
        for fraction in (0.5, 0.95, 0.99, 1):
            import math

            actual = samples[math.ceil(len(samples) * fraction) - 1]
            bound = whole.quantile(fraction)
            self.assertGreaterEqual(bound, actual)
            self.assertLessEqual(bound, max(1000, actual * 1.01 + 1))
        self.assertEqual(whole.minimum, 0)
        self.assertEqual(whole.maximum, 60_000_000_000)
        self.assertLess(len(whole.buckets), 1900)

    def test_overflow_and_empty(self):
        histogram = Histogram()
        self.assertIsNone(histogram.quantile(0.99))
        histogram.add(60_000_000_001)
        self.assertEqual(histogram.overflow, 1)
        self.assertIsNone(histogram.quantile(0.99))
        self.assertEqual(histogram.maximum, 60_000_000_001)


class ProbeTests(unittest.TestCase):
    def test_deadline_duplicate_late_and_invalid(self):
        tracker = ProbeTracker(IDENTITY, deadline_ns=10 * MS)
        tracker.sent(1, 0, 100)
        tracker.sent(2, MS, 100)
        self.assertEqual(
            tracker.reply(IDENTITY, 1, 15, 2 * MS, 120, False), "invalid_reply"
        )
        self.assertEqual(len(tracker.pending), 2)
        self.assertEqual(tracker.reply(IDENTITY, 1, 15, 9 * MS, 120), "timely_reply")
        self.assertEqual(tracker.reply(IDENTITY, 1, 15, 9 * MS, 120), "duplicate_reply")
        self.assertEqual(tracker.reply(IDENTITY, 2, 15, 11 * MS, 120), "late_reply")
        self.assertEqual(
            tracker.reply(IDENTITY, 2, 15, 12 * MS, 120), "duplicate_reply"
        )
        self.assertEqual(tracker.counts["deadline_misses"], 1)
        self.assertEqual(tracker.rtt.count, 1)
        self.assertEqual(tracker.rtt.maximum, 9 * MS)
        tracker.finish(12 * MS)

    def test_every_identity_component_and_kind_is_required(self):
        tracker = ProbeTracker(IDENTITY)
        tracker.sent(1, 0, 1)
        for field, value in dict(
            peer_ip="192.0.2.2",
            peer_port=42000,
            device_id="other",
            boot_id="fedcba9876543210",
            session_id="1111111111111111",
            run_epoch=2,
        ).items():
            self.assertEqual(
                tracker.reply(replace(IDENTITY, **{field: value}), 1, 15, MS, 1),
                "foreign_reply",
            )
        self.assertEqual(tracker.reply(IDENTITY, 1, 12, MS, 1), "foreign_reply")
        self.assertEqual(
            tracker.reply(IDENTITY, 99, 15, MS, 1), "outside_history_or_unsent"
        )
        self.assertEqual(len(tracker.pending), 1)

    def test_window_drain_and_interrupted_accounting(self):
        tracker = ProbeTracker(IDENTITY, limit=1)
        tracker.sent(1, 0, 100)
        with self.assertRaises(ValueError):
            tracker.sent(2, MS, 100)
        with self.assertRaises(ValueError):
            tracker.finish(MS)
        tracker.finish(MS, interrupted=True)
        self.assertEqual(tracker.counts["unresolved_at_stop"], 1)
        self.assertEqual(tracker.counts["deadline_misses"], 0)
        self.assertEqual(tracker.reply(IDENTITY, 1, 15, MS, 100), "after_close")

    def test_bounded_history_and_id_exhaustion(self):
        tracker = ProbeTracker(IDENTITY, history=4)
        for request_id in range(1, 10001):
            now = request_id * MS
            tracker.sent(request_id, now, 100)
            tracker.reply(IDENTITY, request_id, 15, now, 100)
            self.assertLessEqual(len(tracker.recent), 4)
        self.assertEqual(
            tracker.reply(IDENTITY, 1, 15, now, 100), "outside_history_or_unsent"
        )
        self.assertEqual(tracker.counts["timely_reply"], 10000)
        tracker.sent(2**32 - 1, now, 100)
        with self.assertRaises(ValueError):
            tracker.sent(1, now, 100)
        with self.assertRaises(ValueError):
            tracker.sent(2**32, now, 100)

    def test_missing_replies_and_clock_validation(self):
        tracker = ProbeTracker(IDENTITY, deadline_ns=MS)
        tracker.sent(1, 0, 100)
        tracker.tick(MS)
        self.assertEqual(tracker.counts["deadline_misses"], 1)
        with self.assertRaises(ValueError):
            tracker.tick(MS - 1)
        with self.assertRaises(ValueError):
            ProbeTracker(IDENTITY, limit=4097)


class PacerTests(unittest.TestCase):
    def test_no_burst_after_stall_and_full_window(self):
        pacer = Pacer(0, 1000)
        self.assertTrue(pacer.due(0))
        self.assertFalse(pacer.due(0))
        self.assertFalse(pacer.due(5 * MS, window_available=False))
        self.assertTrue(pacer.due(10 * MS))
        self.assertEqual(
            dict(pacer.counts),
            dict(
                planned_slots=11,
                scheduling_skips=8,
                window_skips=1,
                send_opportunities=2,
            ),
        )
        self.assertEqual(pacer.next_due_ns(), 11 * MS)

    def test_fractional_period_has_no_accumulating_roundoff(self):
        pacer = Pacer(100, 3)
        for slot in range(10000):
            now = pacer.next_due_ns()
            self.assertEqual(now, 100 + (slot * 1_000_000_000 + 2) // 3)
            self.assertTrue(pacer.due(now))
        self.assertEqual(pacer.counts["scheduling_skips"], 0)
        self.assertFalse(Pacer(0, 0).due(10**12))
        with self.assertRaises(ValueError):
            Pacer(0, float("nan"))


class StreamTests(unittest.TestCase):
    def tracker(self, window=4):
        return StreamTracker(IDENTITY, 21, 10 * MS, window)

    def observe(self, tracker, sequence, now=0):
        return tracker.observe(IDENTITY, 21, sequence, now, 100)

    def test_reorder_duplicate_gap_and_freshness(self):
        tracker = self.tracker()
        self.assertEqual(tracker.snapshot(0)["freshness"], "not_seen")
        self.observe(tracker, 0)
        self.observe(tracker, 2, MS)
        self.assertEqual(tracker.snapshot(MS)["pending_gaps"], 1)
        self.assertEqual(self.observe(tracker, 1, 2 * MS), "reordered_publication")
        self.assertEqual(self.observe(tracker, 1, 3 * MS), "duplicate_publication")
        self.assertEqual(tracker.snapshot(11 * MS)["freshness"], "stale")
        self.observe(tracker, 4, 12 * MS)
        self.assertEqual(tracker.snapshot(12 * MS)["freshness"], "fresh")
        tracker.finish(12 * MS)
        self.assertEqual(tracker.counts["final_gaps"], 1)
        self.assertEqual(
            tracker.snapshot(12 * MS)["freshness_ns"],
            dict(not_seen=0, fresh=11 * MS, stale=MS),
        )
        tracker.finish(12 * MS)
        self.assertEqual(tracker.counts["final_gaps"], 1)

    def test_wrap_large_jump_and_half_range(self):
        tracker = self.tracker()
        self.observe(tracker, 2**32 - 2)
        self.observe(tracker, 1)
        self.assertEqual(tracker.snapshot(0)["pending_gaps"], 2)
        self.assertEqual(self.observe(tracker, 2**32 - 1), "reordered_publication")
        self.assertEqual(self.observe(tracker, 0), "reordered_publication")
        self.assertEqual(self.observe(tracker, 1 + 2**31), "ambiguous_sequence")
        self.observe(tracker, 10001)
        self.assertEqual(tracker.counts["final_gaps"], 9996)
        self.assertEqual(tracker.snapshot(0)["pending_gaps"], 3)
        self.assertLessEqual(tracker.bits.bit_length(), 4)
        self.assertEqual(self.observe(tracker, 1), "outside_history")

    def test_bitmap_matches_independent_set_model(self):
        rng = random.Random(42)
        tracker = self.tracker(32)
        highest, seen, final = 0, {0}, 0
        self.observe(tracker, 0)
        for _ in range(5000):
            sequence = max(0, highest + rng.randrange(-50, 80))
            if sequence > highest:
                old_base, new_base = max(0, highest - 31), max(0, sequence - 31)
                final += (
                    new_base
                    - old_base
                    - sum(old_base <= value < new_base for value in seen)
                )
                highest = sequence
                seen = {value for value in seen if value >= new_base}
                seen.add(sequence)
            elif sequence >= max(0, highest - 31):
                seen.add(sequence)
            self.observe(tracker, sequence)
            self.assertEqual(tracker.counts["final_gaps"], final)
            self.assertEqual(
                tracker.snapshot(0)["pending_gaps"], min(32, highest + 1) - len(seen)
            )

    def test_foreign_invalid_do_not_advance(self):
        tracker = self.tracker()
        tracker.observe(replace(IDENTITY, run_epoch=2), 21, 100, 0, 100)
        tracker.observe(IDENTITY, 22, 100, 0, 100)
        tracker.observe(IDENTITY, 21, 100, 0, 100, False)
        self.assertIsNone(tracker.highest)


class ReportTests(unittest.TestCase):
    def test_incremental_intervals_final_and_reanalysis(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "run"
            tracker, pacer = ProbeTracker(IDENTITY), Pacer(0, 10)
            with RunReport(directory, {"evidence": "synthetic_test"}, 0) as report:
                tracker.sent(1, 0, 100)
                tracker.reply(IDENTITY, 1, 15, MS, 100)
                first = report.interval(10 * MS, tracker, pacer, [])
                second = report.interval(20 * MS, tracker, pacer, [])
                self.assertEqual(first["send_hz"], 100)
                self.assertEqual(second["send_hz"], 0)
                self.assertEqual(first["probes"]["interval_rtt"]["count"], 1)
                self.assertEqual(second["probes"]["interval_rtt"]["count"], 0)
                report.finish(20 * MS, "duration", True, tracker, pacer, [])
            result = inspect_report(directory / "events.jsonl")
            self.assertTrue(result["complete"])
            self.assertEqual(result["intervals"], 2)
            with (directory / "intervals.csv").open() as source:
                rows = list(csv.DictReader(source))
            self.assertEqual([row["sent"] for row in rows], ["1", "0"])
            with self.assertRaises(FileExistsError):
                RunReport(directory, {}, 0)

    def test_interruption_and_truncation_never_imply_completion(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "run"
            tracker, pacer = ProbeTracker(IDENTITY), Pacer(0, 10)
            with RunReport(directory, {}, 0) as report:
                tracker.sent(1, 0, 100)
                report.interval(MS, tracker, pacer, [])
            path = directory / "events.jsonl"
            self.assertEqual(inspect_report(path)["evidence_issue"], "missing_final")
            with path.open("a") as output:
                output.write('{"type":"final"')
            result = inspect_report(path)
            self.assertFalse(result["complete"])
            self.assertEqual(result["intervals"], 1)
            self.assertEqual(result["evidence_issue"], "truncated_record")

    def test_explicit_interrupted_final(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / "run"
            tracker, pacer = ProbeTracker(IDENTITY), Pacer(0, 10)
            with RunReport(directory, {}, 0) as report:
                tracker.sent(1, 0, 100)
                report.finish(MS, "keyboard_interrupt", False, tracker, pacer, [])
            final = inspect_report(directory / "events.jsonl")
            self.assertFalse(final["complete"])
            self.assertEqual(
                final["final"]["probes"]["counts"]["unresolved_at_stop"], 1
            )


if __name__ == "__main__":
    unittest.main()
