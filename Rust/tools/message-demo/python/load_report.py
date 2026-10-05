"""Incremental B2 evidence output. No per-packet or whole-run sample retention."""

import csv
import json
from pathlib import Path

from load_metrics import Histogram, stamp


def strict_json(text):
    def reject(value):
        raise ValueError(f"nonfinite JSON number: {value}")

    return json.loads(text, parse_constant=reject)


class RunReport:
    """Exclusive new directory; every complete JSONL line survives interruption.

    Missing final records mean incomplete evidence. Flush is not an fsync/power
    loss guarantee. Explicit finish is required, so exceptions cannot imply pass.
    """

    columns = (
        "elapsed_ns",
        "interval_ns",
        "sent",
        "timely_reply",
        "deadline_misses",
        "late_reply",
        "outstanding",
        "send_hz",
        "timely_reply_hz",
        "p99_upper_ns",
    )

    def __init__(self, directory, metadata, start_ns):
        self.start = self.previous = stamp(start_ns)
        self.previous_counts = {}
        self.finished = False
        directory = Path(directory)
        directory.mkdir(parents=True, exist_ok=False)
        self.jsonl = self.csv_file = None
        try:
            self.jsonl = (directory / "events.jsonl").open("x", encoding="utf-8")
            self.csv_file = (directory / "intervals.csv").open(
                "x", newline="", encoding="utf-8"
            )
            self.writer = csv.DictWriter(self.csv_file, fieldnames=self.columns)
            self.writer.writeheader()
            self._write(
                dict(
                    type="manifest",
                    schema_version=1,
                    metadata=metadata,
                    start_monotonic_ns=start_ns,
                    histogram=dict(
                        edges_ns=Histogram.edges,
                        quantile="nearest-rank upper bound",
                        overflow="unknown quantile",
                        error="1% + 1 ns above 1 us",
                    ),
                )
            )
            self.csv_file.flush()
        except BaseException:
            self.close()
            raise

    def _write(self, record):
        self.jsonl.write(
            json.dumps(record, separators=(",", ":"), allow_nan=False) + "\n"
        )
        self.jsonl.flush()

    def interval(self, now, probes, pacing, streams):
        stamp(now)
        if self.finished or now <= self.previous:
            raise ValueError("report interval must advance time in an unfinished run")
        probes.tick(now)
        snapshot = probes.snapshot()
        counts = snapshot["counts"]
        delta = {
            name: value - self.previous_counts.get(name, 0)
            for name, value in counts.items()
        }
        duration = now - self.previous
        record = dict(
            type="interval",
            elapsed_ns=now - self.start,
            interval_ns=duration,
            probes=snapshot,
            interval_counts=delta,
            pacing=dict(pacing.counts),
            health=getattr(probes, "last_health", None),
            streams={str(stream.kind): stream.snapshot(now) for stream in streams},
            send_hz=delta.get("sent", 0) * 1e9 / duration,
            timely_reply_hz=delta.get("timely_reply", 0) * 1e9 / duration,
        )
        self._write(record)
        row = {
            key: record[key]
            for key in ("elapsed_ns", "interval_ns", "send_hz", "timely_reply_hz")
        }
        row.update(
            {
                key: delta.get(key, 0)
                for key in ("sent", "timely_reply", "deadline_misses", "late_reply")
            }
        )
        row.update(
            outstanding=snapshot["outstanding"],
            p99_upper_ns=snapshot["interval_rtt"]["p99_upper_ns"],
        )
        self.writer.writerow(row)
        self.csv_file.flush()
        self.previous, self.previous_counts = now, dict(counts)
        probes.interval_rtt = Histogram()
        return record

    def finish(self, now, reason, complete, probes, pacing, streams):
        if type(complete) is not bool or not isinstance(reason, str) or not reason:
            raise ValueError("finish requires an explicit reason and completion state")
        stamp(now)
        if self.finished or now < self.previous:
            raise ValueError("already finished or backwards final time")
        probes.finish(now, interrupted=not complete)
        for stream in streams:
            stream.finish(now)
        self._write(
            dict(
                type="final",
                elapsed_ns=now - self.start,
                reason=reason,
                complete=complete,
                acceptance="not_evaluated",
                probes=probes.snapshot(),
                pacing=dict(pacing.counts),
                streams={str(stream.kind): stream.snapshot(now) for stream in streams},
            )
        )
        self.finished = True

    def close(self):
        for file in (self.jsonl, self.csv_file):
            if file is not None:
                file.close()

    def diagnostic(self, now, **values):
        stamp(now)
        if self.finished or now < self.previous:
            raise ValueError("diagnostic outside active reporting window")
        self._write(dict(type="diagnostic", elapsed_ns=now - self.start, values=values))

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def inspect_report(path):
    """Stream through saved records; tolerate only a truncated final physical line.

    Preserve prior complete records, but never accept a damaged/truncated run as
    complete. This is an evidence-integrity summary, not threshold assessment.
    """
    intervals = 0
    final = None
    previous = -1
    damage = None
    manifest = None
    with Path(path).open(encoding="utf-8") as source:
        for index, line in enumerate(source):
            if not line.endswith("\n"):
                damage = "truncated_record"
                break
            record = strict_json(line)
            if index == 0:
                if (
                    record.get("type") != "manifest"
                    or record.get("schema_version") != 1
                ):
                    raise ValueError("missing or unsupported manifest")
                manifest = record
                continue
            if final is not None:
                raise ValueError("record after final")
            elapsed = stamp(record["elapsed_ns"])
            if elapsed < previous:
                raise ValueError("backwards report time")
            if record["type"] == "interval":
                if elapsed <= previous or record["interval_ns"] <= 0:
                    raise ValueError("invalid interval")
                intervals += 1
            elif record["type"] == "final":
                final = record
            elif record["type"] == "diagnostic":
                pass
            else:
                raise ValueError("unexpected record type")
            previous = elapsed
    return dict(
        intervals=intervals,
        manifest=manifest,
        final=final,
        complete=bool(
            manifest and final and final.get("complete") is True and not damage
        ),
        evidence_issue=damage or (None if final else "missing_final"),
    )
