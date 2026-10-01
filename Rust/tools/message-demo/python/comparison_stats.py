"""Windowed statistics from saved comparison logs; never infer a network loss cause."""

from collections import Counter, defaultdict
import csv
import hashlib
import json
import math
from pathlib import Path


def percentile(values, fraction):
    """Linear interpolation at (n - 1) * fraction; missing samples stay missing."""
    if not values:
        return None
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def write_csv(path, rows):
    if not rows:
        Path(path).write_text("", encoding="utf-8")
        return
    fields = list(dict.fromkeys(key for row in rows for key in row))
    with Path(path).open("w", newline="", encoding="utf-8") as output:
        writer = csv.DictWriter(output, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def read_log(path):
    # Incomplete records fail analysis rather than quietly improving the result.
    with Path(path).open(encoding="utf-8") as source:
        return [json.loads(line) for line in source if line.strip()]


def timed(records, manifest):
    """Python uses the runner's host monotonic clock; Rust is anchored once via UTC.

    Rust's Instant has a private origin. Its first event's UTC millisecond stamp
    maps that origin to the runner's clock (about 1 ms quantization). Later events
    use only elapsed monotonic time. UTC discontinuities are reported separately.
    """
    if not records:
        return []
    first = records[0]
    anchor = manifest["clock_anchor"]
    origin = anchor["monotonic_ns"] / 1e6 + first["unix_ms"] - anchor["unix_ms"]
    return [
        (
            (
                event["host_monotonic_ns"] / 1e6
                if "host_monotonic_ns" in event
                else origin + event["monotonic_ms"] - first["monotonic_ms"]
            ),
            event,
        )
        for event in records
    ]


def stream_statistics(events, start, end, kind):
    selected = [
        (stamp, event)
        for stamp, event in events
        if (event.get("kind") or (event.get("message") or {}).get("kind")) == kind
        and stamp < end
    ]
    measured = [event for stamp, event in selected if stamp >= start]
    counts = Counter(event["outcome"] for event in measured)
    state, cursor = "not_seen", start
    seconds = Counter()
    intervals = Counter()
    pending = defaultdict(set)
    for stamp, event in selected:
        stop = max(start, min(end, stamp))
        if stop > cursor:
            seconds[state] += (stop - cursor) / 1000
            cursor = stop
        outcome = event["outcome"]
        next_state = state
        if outcome == "not_seen":
            next_state = "not_seen"
        elif outcome == "stale":
            next_state = "stale"
        elif outcome in ("accepted_publication", "recovered"):
            next_state = "fresh"
        if next_state != state and stamp >= start:
            intervals[next_state] += 1
        state = next_state
        boot = event.get("boot_id") or (event.get("message") or {}).get("boot_id")
        if outcome == "gap_pending":
            pending[boot].update(event["sequences"])
        elif outcome == "gap_final":
            pending[boot].difference_update(event["sequences"])
        elif outcome == "reordered_publication":
            pending[boot].discard(event["message"]["sequence"])
    seconds[state] += (end - cursor) / 1000
    forward = counts["accepted_publication"]
    reordered = counts["reordered_publication"]
    return dict(
        kind=kind,
        forward_publications=forward,
        reordered_publications=reordered,
        unique_received=forward + reordered,
        unique_receive_hz=(forward + reordered) * 1000 / (end - start),
        duplicate_publications=counts["duplicate_publication"],
        late_publications=counts["late_publication"],
        discontinuities=counts["discontinuity"],
        gap_final_observed=sum(
            e.get("count", 0) for e in measured if e["outcome"] == "gap_final"
        ),
        gap_unresolved_at_end=sum(map(len, pending.values())),
        stale_transitions=intervals["stale"],
        stale_seconds=seconds["stale"],
        not_seen_seconds=seconds["not_seen"],
        fresh_seconds=seconds["fresh"],
    )


def summarize(directory):
    directory = Path(directory)
    manifest = json.loads((directory / "manifest.json").read_text())
    start, end = (
        manifest["measurement_start_ns"] / 1e6,
        manifest["measurement_end_ns"] / 1e6,
    )
    duration = (end - start) / 1000
    context = {
        key: manifest[key]
        for key in ("comparison_id", "mode", "format", "crc", "instrumented")
    }
    context["duration_seconds"] = duration
    logs = {role: read_log(directory / name) for role, name in manifest["logs"].items()}
    errors = []
    times = {}
    for role, records in logs.items():
        expected_hash = manifest.get("log_sha256", {}).get(role)
        actual_hash = hashlib.sha256(
            (directory / manifest["logs"][role]).read_bytes()
        ).hexdigest()
        if expected_hash != actual_hash:
            errors.append(f"{role}: missing or mismatched log SHA-256")
        if not records:
            errors.append(f"{role}: empty log")
            continue
        if any(
            e.get("format") != manifest["format"] or e.get("crc") != manifest["crc"]
            for e in records
        ):
            errors.append(f"{role}: format/CRC metadata mismatch")
        if len({e["run_id"] for e in records}) != 1:
            errors.append(f"{role}: mixed process runs")
        if role != "client" and not any(e["outcome"] == "ready" for e in records):
            errors.append(f"{role}: missing ready record")
        stamps = timed(records, manifest)
        if any(b[0] < a[0] for a, b in zip(stamps, stamps[1:])):
            # Publisher threads can enqueue out of timestamp order; sort them.
            stamps.sort(key=lambda pair: pair[0])
        times[role] = stamps
        drift = max(
            abs(
                (e["unix_ms"] - records[0]["unix_ms"])
                - (e["monotonic_ms"] - records[0]["monotonic_ms"])
            )
            for e in records
        )
        if drift > 100:
            errors.append(f"{role}: wall/monotonic drift exceeds 100 ms ({drift:.1f})")
        if max(e.get("log_dropped", 0) for e in records):
            errors.append(f"{role}: responder dropped diagnostic records")
    client = times.get("client", [])
    requests = {}
    for stamp, event in client:
        message = event.get("message") or {}
        token = (message.get("client_session"), message.get("request_id"))
        if event["outcome"] == "sent_request" and start <= stamp < end:
            requests[token] = dict(
                context,
                client_session=token[0],
                request_id=token[1],
                kind=message["kind"],
                sent_offset_seconds=(stamp - start) / 1000,
                request_datagram_bytes=event["raw_length"],
                outcome="missing_terminal_event",
                rtt_ms=None,
                response_datagram_bytes=None,
            )
        elif token in requests and event["outcome"] in (
            "accepted_response",
            "error_response",
            "timeout",
        ):
            if requests[token]["outcome"] != "missing_terminal_event":
                errors.append("client: multiple terminal outcomes for a request")
            requests[token].update(
                outcome=event["outcome"],
                rtt_ms=event.get("rtt_ms"),
                response_datagram_bytes=event["raw_length"] or None,
            )
    samples = list(requests.values())
    outcomes = Counter(row["outcome"] for row in samples)
    if outcomes["missing_terminal_event"]:
        errors.append("client: request lacks a terminal event")
    rtts = [row["rtt_ms"] for row in samples if row["outcome"] == "accepted_response"]
    measured_client = [e for stamp, e in client if start <= stamp < end]
    summary = dict(
        context,
        requests=len(samples),
        successes=len(rtts),
        timeouts=outcomes["timeout"],
        error_responses=outcomes["error_response"],
        missing_terminal_events=outcomes["missing_terminal_event"],
        success_percent=100 * len(rtts) / len(samples) if samples else None,
        attempted_command_hz=len(samples) / duration,
        planned_command_slots=math.ceil(
            duration / manifest["command_period_seconds"] - 1e-9
        ),
        unattempted_command_slots=max(
            0,
            math.ceil(duration / manifest["command_period_seconds"] - 1e-9)
            - len(samples),
        ),
        scheduled_slots_missed=sum(
            e.get("count", 0)
            for e in measured_client
            if e["outcome"] == "command_slots_skipped"
        ),
        rtt_p50_ms=percentile(rtts, 0.5),
        rtt_p95_ms=percentile(rtts, 0.95),
        rtt_p99_ms=percentile(rtts, 0.99),
        rtt_max_ms=max(rtts, default=None),
    )
    streams, sizes, diagnostics, health = [], [], [], []
    for role, records in times.items():
        window = [e for stamp, e in records if start <= stamp < end]
        counts = Counter(e["outcome"] for e in window)
        snapshots = defaultdict(list)
        for stamp, event in records:
            message = event.get("message") or {}
            if (
                start <= stamp < end
                and event["outcome"] in ("accepted_response", "accepted_publication")
                and "health" in message
            ):
                snapshots[message["boot_id"]].append((stamp, message["health"]))
        for boot, values in snapshots.items():
            first_time, first = values[0]
            last_time, last = values[-1]
            for counter in (
                "received_requests",
                "rejected_datagrams",
                "send_errors",
                "skipped_publications",
            ):
                health.append(
                    dict(
                        context,
                        role=role,
                        boot_id=boot,
                        counter=counter,
                        samples=len(values),
                        observed_interval_seconds=(last_time - first_time) / 1000,
                        first=first[counter],
                        last=last[counter],
                        delta=(
                            last[counter] - first[counter]
                            if last[counter] >= first[counter]
                            else None
                        ),
                        saturated=last[counter] == 2**32 - 1,
                    )
                )
        for outcome, count in sorted(counts.items()):
            diagnostics.append(dict(context, role=role, outcome=outcome, count=count))
        groups = defaultdict(list)
        for event in window:
            if event["outcome"] in (
                "sent_request",
                "accepted_response",
                "error_response",
                "sent_publication",
                "publication_after_skip",
                "accepted_publication",
                "reordered_publication",
            ):
                groups[event["message"]["kind"]].append(event["raw_length"])
        for kind, lengths in sorted(groups.items()):
            trailer = 8 if manifest["crc"] == "on" else 0
            sizes.append(
                dict(
                    context,
                    role=role,
                    kind=kind,
                    count=len(lengths),
                    body_min_bytes=min(lengths) - trailer,
                    body_max_bytes=max(lengths) - trailer,
                    body_mean_bytes=sum(lengths) / len(lengths) - trailer,
                    datagram_min_bytes=min(lengths),
                    datagram_max_bytes=max(lengths),
                    datagram_mean_bytes=sum(lengths) / len(lengths),
                )
            )
        if role.startswith("listener"):
            for kind in (21, 22):
                row = dict(
                    context, role=role, **stream_statistics(records, start, end, kind)
                )
                sent = None
                if "responder" in times:
                    sent = sum(
                        start <= stamp < end
                        and e["outcome"]
                        in ("sent_publication", "publication_after_skip")
                        and (e.get("message") or {}).get("kind") == kind
                        for stamp, e in times["responder"]
                    )
                row.update(
                    logged_sent_publications=sent,
                    logged_send_hz=sent / duration if sent is not None else None,
                )
                streams.append(row)
    # Quality and success are separate: a valid experiment can expose failures.
    summary["data_valid"] = bool(manifest.get("complete")) and not errors
    summary["all_commands_succeeded"] = bool(samples) and len(rtts) == len(samples)
    summary["both_listeners_received_both_streams"] = len(streams) == 4 and all(
        s["unique_received"] for s in streams
    )
    report = dict(
        summary=summary,
        streams=streams,
        sizes=sizes,
        commands=samples,
        diagnostics=diagnostics,
        health=health,
        integrity_errors=errors,
    )
    (directory / "summary.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def export(root):
    root = Path(root)
    manifests = sorted(root.glob("*/manifest.json"))
    if not manifests:
        raise ValueError("no per-configuration manifests found")
    reports = [summarize(path.parent) for path in manifests]
    write_csv(root / "summary.csv", [r["summary"] for r in reports])
    for key in ("streams", "sizes", "commands", "diagnostics", "health"):
        write_csv(root / (key + ".csv"), [row for r in reports for row in r[key]])
    return reports
