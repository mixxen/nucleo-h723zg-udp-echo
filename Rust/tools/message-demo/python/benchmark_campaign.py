"""B4 bounded stress/soak campaigns and streaming, reproducible assessment."""

import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
import signal
from types import SimpleNamespace

from benchmark_live import parser as live_parser, run, validate_args
from load_report import inspect_report, strict_json
from load_metrics import Histogram

POLICY = dict(
    max_p99_ms=100.0,
    max_deadline_fraction=0.01,
    min_send_ratio=0.95,
    max_stale_fraction=0.05,
    max_gap_fraction=0.01,
    live_min_seconds=2.0,
)
RATE_KEYS = {"command_hz", "status_period_us", "health_period_us"}


def number(value, low, high, name):
    if (
        type(value) not in (int, float)
        or not math.isfinite(value)
        or not low <= value <= high
    ):
        raise ValueError(f"{name} must be finite in {low}..{high}")
    return value


def policy(values):
    if not isinstance(values, dict) or set(values) - POLICY.keys():
        raise ValueError("unknown threshold name")
    result = dict(POLICY, **values)
    for name, value in result.items():
        number(value, 0, 1 if name.endswith(("fraction", "ratio")) else 86400000, name)
    return result


def normalize(config):
    """Resolve every default before any output directory or socket is created."""
    allowed = {
        "mode",
        "base",
        "thresholds",
        "stop_on_failure",
        "payloads",
        "rates",
        "dwell_seconds",
        "recovery",
        "duration_seconds",
    }
    if not isinstance(config, dict) or set(config) - allowed:
        raise ValueError("unknown campaign option")
    c = copy.deepcopy(config)
    if c.get("mode") not in ("stress", "soak"):
        raise ValueError("mode must be stress or soak")
    c["thresholds"] = policy(c.get("thresholds", {}))
    c.setdefault("stop_on_failure", True)
    if type(c["stop_on_failure"]) is not bool:
        raise ValueError("stop_on_failure must be boolean")
    base = c.get("base", {})
    required = {"target", "target_kind", "interface", "format", "crc"}
    if not isinstance(base, dict) or not required <= base.keys():
        raise ValueError("base requires target, target_kind, interface, format and crc")
    defaults = vars(
        live_parser().parse_args(
            [
                "run",
                "--target",
                "127.0.0.1",
                "--target-kind",
                "host",
                "--interface",
                "127.0.0.1",
                "--format",
                "json",
                "--crc",
                "off",
                "--output",
                "unused",
            ]
        )
    )
    if set(base) - (defaults.keys() - {"mode", "output", "duration"}):
        raise ValueError("unknown base option, or stage-owned duration/output")
    defaults.update(base)
    c["base"] = {
        k: v for k, v in defaults.items() if k not in ("mode", "output", "duration")
    }
    number(c["base"]["warmup"], 0, 604800, "warmup")
    number(c["base"]["report_interval"], 0.001, 604800, "report_interval")
    number(c["base"]["profile_interval"], 0, 604800, "profile_interval")
    if c["base"]["format"] not in ("csv", "json", "protobuf") or c["base"][
        "crc"
    ] not in ("off", "on"):
        raise ValueError("invalid encoding/CRC")
    if (
        c["base"]["target_kind"] not in ("host", "board")
        or type(c["base"]["instrumented"]) is not bool
    ):
        raise ValueError("invalid target kind/instrumented flag")
    for k in (
        "command_hz",
        "payload_bytes",
        "pattern_seed",
        "outstanding",
        "deadline_ms",
        "lease_ms",
        "status_period_us",
        "health_period_us",
        "command_port",
        "status_port",
        "health_port",
    ):
        if type(c["base"][k]) is not int:
            raise ValueError(f"{k} must be an integer")
    if c["mode"] == "soak":
        if set(c) & {"payloads", "rates", "dwell_seconds", "recovery"}:
            raise ValueError(
                "soak has one continuous workload; no sweep/recovery options"
            )
        number(c.get("duration_seconds"), 0.001, 604800, "duration_seconds")
    else:
        if "duration_seconds" in c:
            raise ValueError("stress uses dwell_seconds")
        number(c.get("dwell_seconds"), 0.001, 604800, "dwell_seconds")
        if (
            not isinstance(c.get("payloads"), list)
            or not c["payloads"]
            or len(c["payloads"]) > 64
        ):
            raise ValueError("provide 1..64 payloads")
        for value in c["payloads"]:
            if type(value) is not int or not 0 <= value <= 512:
                raise ValueError("payloads must be integers in 0..512")
        if (
            not isinstance(c.get("rates"), list)
            or not c["rates"]
            or len(c["rates"]) > 64
        ):
            raise ValueError("provide 1..64 explicit rate steps")
        # Rates are explicit maxima, never an unbounded ramp. Each dimension must
        # increase monotonically; use separate campaigns for different workloads.
        previous = (0, 0, 0)
        for rate in c["rates"]:
            if (
                not isinstance(rate, dict)
                or set(rate) != RATE_KEYS
                or any(type(v) is not int for v in rate.values())
            ):
                raise ValueError(
                    "each rate requires three integer command/stream settings"
                )
            current = tuple(
                rate[k] if k == "command_hz" else (1e6 / rate[k] if rate[k] > 0 else 0)
                for k in ("command_hz", "status_period_us", "health_period_us")
            )
            if any(a < b for a, b in zip(current, previous)) or not any(current):
                raise ValueError("rates must be nondecreasing and enable some traffic")
            previous = current
        recovery = c.get("recovery")
        if not isinstance(recovery, dict) or set(recovery) != RATE_KEYS | {
            "payload_bytes",
            "duration_seconds",
        }:
            raise ValueError(
                "recovery requires payload_bytes, duration_seconds and all three rates"
            )
        number(recovery["duration_seconds"], 0.001, 604800, "recovery duration")
        if any(
            type(v) is not int for k, v in recovery.items() if k != "duration_seconds"
        ):
            raise ValueError("recovery payload/rates must be integers")
        first = c["rates"][0]
        if (
            recovery["payload_bytes"] > min(c["payloads"])
            or recovery["command_hz"] > first["command_hz"]
        ):
            raise ValueError("recovery must not exceed the first load/payload")
        for name in ("status_period_us", "health_period_us"):
            a, b = recovery[name], first[name]
            if a and (not b or a < b):
                raise ValueError("recovery stream rate must not exceed the first load")
        if 1 + 2 * len(c["rates"]) * len(c["payloads"]) > 257:
            raise ValueError("at most 128 load steps per campaign")
    for stage in stages(c):
        args = arguments(c, stage, Path("unused"))
        validate_args(args)
        if math.ceil((args.warmup + args.duration) * args.command_hz) >= 2**32:
            raise ValueError(
                "stage would exhaust probe request IDs; reduce rate/duration"
            )
        if not any((args.command_hz, args.status_period_us, args.health_period_us)):
            raise ValueError("a stage must enable some traffic")
        if args.report_interval > args.duration:
            raise ValueError("report_interval must not exceed stage duration")
    return c


def stages(c):
    if c["mode"] == "soak":
        yield dict(role="soak", duration=c["duration_seconds"])
        return
    baseline = dict(c["recovery"])
    baseline["duration"] = baseline.pop("duration_seconds")
    yield dict(baseline, role="baseline")
    for payload in c["payloads"]:
        for rate in c["rates"]:
            yield dict(
                rate, payload_bytes=payload, duration=c["dwell_seconds"], role="load"
            )
            yield dict(baseline, role="recovery")


def arguments(c, stage, path):
    values = dict(c["base"], **{k: v for k, v in stage.items() if k != "role"})
    return SimpleNamespace(
        **values,
        mode="run",
        output=str(path),
        acceptance=c["thresholds"],
        campaign_role=stage["role"],
    )


def judge(record, parameters, thresholds, seconds, final=False):
    """Cumulative metrics. No averages of interval percentiles or inferred loss causes."""
    counts = record["probes"]["counts"]
    sent = counts.get("sent", 0)
    failures = []
    metrics = {}
    if parameters["command_hz"]:
        metrics["requested_command_hz"] = parameters["command_hz"]
        metrics["achieved_command_hz"] = sent / seconds if seconds > 0 else 0
        metrics["mean_request_datagram_bytes"] = (
            counts.get("sent_bytes", 0) / sent if sent else None
        )
        metrics["send_ratio"] = (
            sent / (parameters["command_hz"] * seconds) if seconds > 0 else 0
        )
        metrics["deadline_fraction"] = (
            counts.get("deadline_misses", 0) / sent if sent else 0
        )
        p99 = record["probes"]["rtt"]["p99_upper_ns"]
        metrics["p99_ms"] = p99 / 1e6 if p99 is not None else None
        if metrics["send_ratio"] + 1e-12 < thresholds["min_send_ratio"]:
            failures.append("command_send_ratio")
        if metrics["deadline_fraction"] > thresholds["max_deadline_fraction"]:
            failures.append("command_deadline_fraction")
        if p99 is not None and p99 / 1e6 > thresholds["max_p99_ms"]:
            failures.append("command_p99")
        if final and (not sent or p99 is None):
            failures.append("no_usable_command_latency")
    for kind, period in (
        ("21", parameters["status_period_us"]),
        ("22", parameters["health_period_us"]),
    ):
        if not period:
            continue
        stream = record["streams"].get(kind)
        if stream is None:
            if final:
                failures.append(f"stream_{kind}_missing")
            continue
        times = stream["freshness_ns"]
        total = sum(times.values())
        stale = (
            (times.get("not_seen", 0) + times.get("stale", 0)) / total if total else 1
        )
        c = stream["counts"]
        unique = sum(
            c.get(k, 0) for k in ("baseline", "progress", "reordered_publication")
        )
        gaps = c.get("final_gaps", 0)  # Pending reorder positions are not losses.
        gap_fraction = gaps / (gaps + unique) if gaps + unique else 0
        metrics[kind] = dict(
            stale_fraction=stale,
            gap_fraction=gap_fraction,
            unique=unique,
            requested_hz=1e6 / period,
            observed_unique_hz=unique / seconds if seconds > 0 else 0,
        )
        if stale > thresholds["max_stale_fraction"]:
            failures.append(f"stream_{kind}_stale")
        if gap_fraction > thresholds["max_gap_fraction"]:
            failures.append(f"stream_{kind}_gaps")
        if final and not unique:
            failures.append(f"stream_{kind}_not_seen")
    if counts.get("boot_change_observations", 0):
        failures.append("boot_change_observed")
    host_limits = {
        k: v
        for k, v in {**record["pacing"], **counts}.items()
        if v
        and k
        in (
            "scheduling_skips",
            "window_skips",
            "local_send_errors",
            "encoding_end_skips",
        )
    }
    return dict(failures=failures, metrics=metrics, host_limits=host_limits)


def assess(path, override=None):
    """Read saved evidence with bounded memory; never convert missing output to pass."""
    try:
        result = inspect_report(path)
        if result["evidence_issue"]:
            return dict(outcome="incomplete", reason=result["evidence_issue"])
        metadata = result["manifest"]["metadata"]
        parameters = metadata["parameters"]
        thresholds = policy(
            override if override is not None else parameters["acceptance"]
        )
        final = result["final"]
        measurement = cleanup = identity = stopped = None
        profile = dict(
            requested=bool(parameters.get("profile_interval", 0)),
            samples=0,
            errors=0,
            counter_resets=0,
            max_stack_bytes=None,
            peak_interval_executor_busy_percent=None,
            last=None,
        )
        with Path(path).open(encoding="utf-8") as source:
            for line in source:
                r = strict_json(line)
                if r["type"] == "diagnostic":
                    v = r["values"]
                    if "measured_start_ns" in v:
                        measurement = v
                        identity = v["run_identity"]
                    if "cleanup" in v:
                        cleanup = v["cleanup"]
                    if "threshold_stop" in v:
                        stopped = v["threshold_stop"]
                    if "profiling" in v:
                        sample = v["profiling"]
                        if sample["status"] == "sample":
                            profile["samples"] += 1
                            profile["counter_resets"] += bool(
                                sample["counter_reset_observed"]
                            )
                            profile["last"] = sample["metrics"]
                            stack = sample["metrics"]["stack_high_water_bytes"]
                            profile["max_stack_bytes"] = max(
                                profile["max_stack_bytes"] or 0, stack
                            )
                            busy = sample["interval_executor_busy_percent"]
                            if busy is not None:
                                profile["peak_interval_executor_busy_percent"] = max(
                                    profile["peak_interval_executor_busy_percent"] or 0,
                                    busy,
                                )
                        else:
                            profile["errors"] += 1
        if not measurement:
            return dict(outcome="incomplete", reason=final["reason"], cleanup=cleanup)
        if measurement["measured_end_ns"] - measurement["measured_start_ns"] != int(
            parameters["duration"] * 1e9
        ):
            raise ValueError("measurement window differs from configured duration")
        if (
            result["complete"]
            and result["manifest"]["start_monotonic_ns"] + final["elapsed_ns"]
            < measurement["measured_end_ns"] + parameters["deadline_ms"] * 1_000_000
        ):
            raise ValueError("complete run lacks full measurement/deadline drain")
        seconds = (
            min(
                measurement["measured_end_ns"],
                result["manifest"]["start_monotonic_ns"] + final["elapsed_ns"],
            )
            - measurement["measured_start_ns"]
        )
        seconds /= 1e9
        if seconds <= 0:
            return dict(
                outcome="incomplete", reason="no_measured_window", cleanup=cleanup
            )
        counts = final["probes"]["counts"]
        if any(type(v) is not int or v < 0 for v in counts.values()):
            raise ValueError("invalid counts")
        if result["complete"] and counts.get("sent", 0) != counts.get(
            "timely_reply", 0
        ) + counts.get("deadline_misses", 0):
            raise ValueError("probe counts do not reconcile")
        if final["probes"]["rtt"]["count"] != counts.get("timely_reply", 0):
            raise ValueError("RTT count does not reconcile")
        histogram = final["probes"]["rtt"]
        if any(
            type(histogram[k]) is not int or histogram[k] < 0
            for k in ("count", "overflow")
        ):
            raise ValueError("invalid histogram totals")
        if result["complete"] and (
            final["probes"]["outstanding"] != 0 or final["probes"]["closed"] is not True
        ):
            raise ValueError("complete run has unfinished probes")
        for stream in final["streams"].values():
            if any(
                type(value) is not int or value < 0
                for group in (stream["counts"], stream["freshness_ns"])
                for value in group.values()
            ):
                raise ValueError("invalid stream accounting")
            if stream["closed"] is not True or stream["pending_gaps"] != 0:
                raise ValueError("final stream accounting is not closed")
        bins = histogram["bins"]
        if any(
            not k.isdigit()
            or not 0 <= int(k) < len(Histogram.edges)
            or type(v) is not int
            or v < 0
            for k, v in bins.items()
        ):
            raise ValueError("invalid histogram bins")
        if sum(bins.values()) + histogram["overflow"] != histogram["count"]:
            raise ValueError("histogram counts do not reconcile")
        reconstructed = Histogram()
        reconstructed.buckets = [
            bins.get(str(i), 0) for i in range(len(Histogram.edges))
        ]
        reconstructed.count = histogram["count"]
        reconstructed.maximum = histogram["max_ns"]
        if reconstructed.quantile(0.99) != histogram["p99_upper_ns"]:
            raise ValueError("histogram p99 does not reconcile")
        assessment = judge(final, parameters, thresholds, seconds, final=True)
        if not result["complete"]:
            outcome = (
                "fail"
                if final["reason"] == "threshold_stop" and stopped
                else "incomplete"
            )
        elif cleanup != "stop_acknowledged":
            outcome = "fail"
            assessment["failures"].append("cleanup_unconfirmed")
        else:
            outcome = "fail" if assessment["failures"] else "pass"
        return dict(
            assessment,
            outcome=outcome,
            reason=final["reason"],
            cleanup=cleanup,
            identity=identity,
            measured_seconds=seconds,
            thresholds=thresholds,
            override=override is not None,
            live_stop=stopped,
            profiling=profile,
        )
    except (OSError, ValueError, TypeError, KeyError, OverflowError) as error:
        return dict(outcome="invalid", reason=f"{type(error).__name__}: {error}")


def write(file, value):
    file.write(json.dumps(value, allow_nan=False, separators=(",", ":")) + "\n")
    file.flush()


def combined(outcomes, finished):
    if "invalid" in outcomes:
        return "invalid"
    if "incomplete" in outcomes:
        return "incomplete"
    if "fail" in outcomes:
        return "fail"
    return "pass" if finished and outcomes else "incomplete"


def execute(config, directory, runner=run):
    c = normalize(config)
    root = Path(directory)
    root.mkdir(parents=True, exist_ok=False)
    outcomes = []  # At most 257 stage labels, independent of soak duration.
    baseline_boot = None
    halt_after_recovery = False
    finished = False
    terminal_outcome = None
    reason = "interrupted"
    with (root / "campaign.jsonl").open("x", encoding="utf-8") as log:
        write(
            log,
            dict(
                type="manifest",
                schema_version=1,
                configuration=c,
                source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            ),
        )
        try:
            for index, stage in enumerate(stages(c)):
                args = arguments(c, stage, root / f"stage-{index:03d}")

                def observe(record, seconds):
                    if (
                        not c["stop_on_failure"]
                        or seconds < c["thresholds"]["live_min_seconds"]
                    ):
                        return None
                    failures = judge(record, vars(args), c["thresholds"], seconds)[
                        "failures"
                    ]
                    return failures or None

                runner(args, observer=observe)
                result = assess(Path(args.output) / "events.jsonl")
                boot = result.get("identity", {}).get("boot_id")
                if baseline_boot is None:
                    baseline_boot = boot
                elif boot is not None and boot != baseline_boot:
                    if result["outcome"] not in ("invalid", "incomplete"):
                        result["outcome"] = "fail"
                    result.setdefault("failures", []).append(
                        "boot_changed_between_stages"
                    )
                outcomes.append(result["outcome"])
                write(
                    log, dict(type="stage", index=index, stage=stage, assessment=result)
                )
                print(
                    json.dumps(
                        dict(event="stage", index=index, role=stage["role"], **result)
                    ),
                    flush=True,
                )
                if result["outcome"] in ("invalid", "incomplete"):
                    reason = "stage_" + result["outcome"]
                    break
                if stage["role"] == "baseline" and result["outcome"] != "pass":
                    reason = "baseline_failed"
                    break
                if stage["role"] == "load" and result["outcome"] != "pass":
                    halt_after_recovery = c["stop_on_failure"]
                if stage["role"] == "recovery" and (
                    result["outcome"] != "pass" or halt_after_recovery
                ):
                    reason = (
                        "recovery_failed"
                        if result["outcome"] != "pass"
                        else "load_failed_recovery_passed"
                    )
                    break
            else:
                finished = True
                reason = "all_stages_finished"
        except KeyboardInterrupt:
            reason = "keyboard_interrupt"
            terminal_outcome = "incomplete"
        except (OSError, ValueError, RuntimeError) as error:
            reason = f"{type(error).__name__}: {error}"
            terminal_outcome = "invalid"
        final = dict(
            type="final",
            outcome=combined(
                outcomes + ([terminal_outcome] if terminal_outcome else []), finished
            ),
            reason=reason,
            stages=len(outcomes),
            all_stages_finished=finished,
            terminal_outcome=terminal_outcome,
        )
        write(log, final)
        return final


def reassess(directory, override=None):
    """Recompute each child result, including recovery and boot continuity."""
    root = Path(directory)
    outcomes = []
    summaries = []  # Bounded by the same 257-stage configuration limit.
    first_boot = None
    try:
        with (root / "campaign.jsonl").open(encoding="utf-8") as source:
            manifest = strict_json(next(source))
            if (
                manifest.get("type") != "manifest"
                or manifest.get("schema_version") != 1
            ):
                raise ValueError("unsupported campaign manifest")
            c = normalize(manifest["configuration"])
            expected = iter(stages(c))
            final = None
            for line in source:
                if not line.endswith("\n"):
                    return dict(outcome="incomplete", reason="truncated_campaign")
                r = strict_json(line)
                if final is not None:
                    raise ValueError("record after final")
                if r["type"] == "stage":
                    if r["index"] != len(outcomes) or r["stage"] != next(expected):
                        raise ValueError("stage order/configuration mismatch")
                    child = root / f"stage-{len(outcomes):03d}" / "events.jsonl"
                    with child.open(encoding="utf-8") as data:
                        saved = strict_json(next(data))["metadata"]["parameters"]
                    intended = vars(arguments(c, r["stage"], child.parent))
                    if {k: v for k, v in saved.items() if k != "output"} != {
                        k: v for k, v in intended.items() if k != "output"
                    }:
                        raise ValueError("child parameters differ from campaign")
                    result = assess(child, override)
                    boot = result.get("identity", {}).get("boot_id")
                    if first_boot is None:
                        first_boot = boot
                    elif boot is not None and boot != first_boot:
                        if result["outcome"] not in ("invalid", "incomplete"):
                            result["outcome"] = "fail"
                        result.setdefault("failures", []).append(
                            "boot_changed_between_stages"
                        )
                    outcomes.append(result["outcome"])
                    summaries.append(
                        dict(
                            index=r["index"],
                            role=r["stage"]["role"],
                            payload_bytes=intended["payload_bytes"],
                            assessment=result,
                        )
                    )
                elif r["type"] == "final":
                    final = r
                else:
                    raise ValueError("unknown campaign record")
            if final is None:
                return dict(outcome="incomplete", reason="missing_campaign_final")
            if type(final["all_stages_finished"]) is not bool:
                raise ValueError("invalid completion flag")
            finished = next(expected, None) is None
            if final["all_stages_finished"] and not finished:
                raise ValueError("missing stage evidence")
            if final["stages"] != len(outcomes) or final.get(
                "terminal_outcome"
            ) not in (None, "invalid", "incomplete"):
                raise ValueError("invalid campaign final")
            terminal = (
                [final["terminal_outcome"]] if final.get("terminal_outcome") else []
            )
            return dict(
                outcome=combined(
                    outcomes + terminal, finished and final["all_stages_finished"]
                ),
                stages=len(outcomes),
                stage_outcomes=outcomes,
                results=summaries,
                encoding=c["base"]["format"],
                crc=c["base"]["crc"],
                override=override is not None,
                reason=final["reason"],
            )
    except (OSError, ValueError, KeyError, TypeError, StopIteration) as error:
        return dict(outcome="invalid", reason=f"{type(error).__name__}: {error}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    start = sub.add_parser("run")
    start.add_argument("config", type=Path)
    start.add_argument("--output", required=True)
    inspect = sub.add_parser("assess")
    inspect.add_argument("path", type=Path)
    inspect.add_argument(
        "--thresholds",
        type=Path,
        help="explicit alternate policy; original evidence stays unchanged",
    )
    args = parser.parse_args()

    # SIGTERM uses the same cleanup/final-record path as Ctrl-C. SIGKILL still
    # relies on the board lease and leaves explicitly incomplete evidence.
    def interrupt(*_):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupt)
    try:
        if args.mode == "run":
            result = execute(strict_json(args.config.read_text()), args.output)
        else:
            override = (
                policy(strict_json(args.thresholds.read_text()))
                if args.thresholds
                else None
            )
            result = (
                reassess(args.path, override)
                if args.path.is_dir()
                else assess(args.path, override)
            )
    except KeyboardInterrupt:
        result = dict(outcome="incomplete", reason="keyboard_interrupt")
    except (OSError, ValueError, KeyError, TypeError) as error:
        result = dict(outcome="invalid", reason=f"{type(error).__name__}: {error}")
    print(json.dumps(result, indent=2))
    return {"pass": 0, "fail": 1, "incomplete": 2, "invalid": 3}[result["outcome"]]


if __name__ == "__main__":
    raise SystemExit(main())
