"""Capture repeatable command/two-listener comparisons, or re-export saved logs.

Linux orchestration only. Host mode starts a responder; board mode observes one
already-flashed configuration. Never flashes, resets, or changes a device.
"""

import argparse
from contextlib import ExitStack
import hashlib
import ipaddress
import json
import math
import os
from pathlib import Path
import platform
import random
import signal
import socket
import subprocess
import sys
import time

from client import Client, EventLog, nonce
from comparison_stats import export, read_log

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def provenance():
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=REPO, text=True).strip()

    paths = git(
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
        "Rust/messaging",
        "Rust/protocol",
        "Rust/tools/message-demo",
        "Rust/Cargo.toml",
        "Rust/Cargo.lock",
        "Rust/src",
        "Rust/memory.x",
        "Rust/.cargo/config.toml",
        "Rust/rust-toolchain.toml",
    ).splitlines()
    files = {
        path: digest(REPO / path)
        for path in sorted(set(paths))
        if (REPO / path).is_file()
    }
    return dict(
        revision=git("rev-parse", "HEAD"),
        dirty=bool(git("status", "--porcelain")),
        source_sha256=hashlib.sha256(
            json.dumps(files, sort_keys=True).encode()
        ).hexdigest(),
        source_files=files,
        platform=platform.platform(),
        machine=platform.machine(),
        python=sys.version,
        cpu_count=os.cpu_count(),
        clock=time.get_clock_info("monotonic").implementation,
    )


def wait_ready(process, path):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(
                f"{path.name}: process exited {process.returncode}; inspect stderr"
            )
        try:
            if any(event["outcome"] == "ready" for event in read_log(path)):
                return
        except (FileNotFoundError, json.JSONDecodeError):
            pass  # The writer may be between its JSON and newline writes.
        time.sleep(0.02)
    raise RuntimeError(f"{path.name}: no ready record within 10 seconds")


def stop(process, interrupt=False):
    if process.poll() is None:
        if interrupt:
            process.send_signal(
                signal.SIGINT
            )  # Listener finalizes gaps and closes membership.
        else:
            process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
            raise RuntimeError("comparison process required forced termination")


def capture(args, root, configuration, order, environment):
    format, crc = configuration
    directory = root / f"{order:02d}-{format}-{crc}"
    directory.mkdir()
    comparison_id = nonce()
    manifest = dict(
        comparison_id=comparison_id,
        mode=args.mode,
        format=format,
        crc=crc,
        instrumented=args.instrumented,
        order=order,
        environment=environment,
        warmup_seconds=args.warmup,
        requested_duration_seconds=args.duration,
        command_period_seconds=args.command_period,
        command_timeout_seconds=args.timeout,
        stream_status_hz=10,
        stream_health_hz=1,
        listeners=2,
        listener_placement="two processes on the capture host",
        interface=args.interface,
        host=args.host,
        group=args.group,
        command_port=args.port,
        status_port=args.status_port,
        health_port=args.health_port,
        device=args.device,
        notes=args.notes,
        complete=False,
        firmware_revision=args.firmware_revision,
        image_sha256=digest(args.firmware_image) if args.firmware_image else None,
        binary_sha256=digest(args.binary) if args.mode == "host" else None,
        logging_policy="responder bounded queue 256; Python synchronous JSONL; human output to files",
        schema_sha256=digest(REPO / "Rust/protocol/messaging.proto"),
        profile_sha256=digest(REPO / "Rust/protocol/profile.json"),
        protocol_version=1,
        logs={
            "client": "client.jsonl",
            "listener1": "listener1.jsonl",
            "listener2": "listener2.jsonl",
        },
    )
    if args.mode == "host":
        manifest["logs"]["responder"] = "responder.jsonl"
    manifest_path = directory / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    print(
        f"{order}: {format}/{crc}: warm-up {args.warmup:g}s, measure {args.duration:g}s",
        flush=True,
    )
    processes = []
    listeners = []
    try:
        with ExitStack() as stack:
            # Map Rust Instant to the same host clock once; RTT never uses UTC.
            manifest["clock_anchor"] = dict(
                monotonic_ns=time.monotonic_ns(), unix_ms=time.time_ns() / 1e6
            )
            for index in (1, 2):
                role = f"listener{index}"
                stderr = stack.enter_context((directory / (role + ".stderr")).open("w"))
                command = [
                    sys.executable,
                    str(HERE / "listener.py"),
                    "--format",
                    format,
                    "--crc",
                    crc,
                    "--interface",
                    args.interface,
                    "--source",
                    args.host,
                    "--device",
                    args.device,
                    "--group",
                    args.group,
                    "--status-port",
                    str(args.status_port),
                    "--health-port",
                    str(args.health_port),
                    "--log",
                    str(directory / (role + ".jsonl")),
                ]
                process = subprocess.Popen(
                    command, cwd=REPO, stdout=subprocess.DEVNULL, stderr=stderr
                )
                processes.append(process)
                listeners.append(process)
                stack.callback(stop, process, True)
                wait_ready(process, directory / (role + ".jsonl"))
            if args.mode == "host":
                stderr = stack.enter_context((directory / "responder.stderr").open("w"))
                process = subprocess.Popen(
                    [
                        str(args.binary),
                        "serve",
                        "--format",
                        format,
                        "--crc",
                        crc,
                        "--bind",
                        f"{args.host}:{args.port}",
                        "--device",
                        args.device,
                        "--multicast-interface",
                        args.interface,
                        "--group",
                        args.group,
                        "--status-port",
                        str(args.status_port),
                        "--health-port",
                        str(args.health_port),
                        "--status-hz",
                        "10",
                        "--health-hz",
                        "1",
                        "--log",
                        str(directory / "responder.jsonl"),
                    ],
                    cwd=REPO,
                    stdout=subprocess.DEVNULL,
                    stderr=stderr,
                )
                processes.append(process)
                stack.callback(stop, process)
                wait_ready(process, directory / "responder.jsonl")
            sock = stack.enter_context(socket.socket(socket.AF_INET, socket.SOCK_DGRAM))
            sock.bind((args.interface, 0))
            output = stack.enter_context((directory / "client.jsonl").open("w"))
            log = EventLog(
                output,
                format=format,
                crc=crc,
                comparison_id=comparison_id,
                local_endpoint=sock.getsockname(),
                interface=args.interface,
                device_id=args.device,
            )
            client = Client(
                sock,
                (args.host, args.port),
                format,
                args.device,
                args.timeout,
                log,
                crc,
            )

            def drive(start_ns, end_ns):
                period_ns = round(args.command_period * 1e9)
                slot = 0
                commands = ("info", "status", "health")
                while True:
                    due = start_ns + slot * period_ns
                    if due >= end_ns:
                        break
                    delay = (due - time.monotonic_ns()) / 1e9
                    if delay > 0:
                        time.sleep(delay)
                    now = time.monotonic_ns()
                    if now >= end_ns:
                        break
                    latest = (now - start_ns) // period_ns
                    if latest > slot:
                        log.event("command_slots_skipped", count=latest - slot)
                        slot = latest  # One newest attempt; never a catch-up burst.
                    if any(p.poll() is not None for p in processes):
                        raise RuntimeError("a comparison process exited during capture")
                    client.request(commands[slot % len(commands)])
                    slot += 1
                remaining = (end_ns - time.monotonic_ns()) / 1e9
                if remaining > 0:
                    time.sleep(remaining)

            warmup = time.monotonic_ns()
            drive(warmup, warmup + round(args.warmup * 1e9))
            manifest["measurement_start_ns"] = time.monotonic_ns()
            manifest["measurement_end_ns"] = manifest["measurement_start_ns"] + round(
                args.duration * 1e9
            )
            log.event("measurement_start", duration_seconds=args.duration)
            drive(manifest["measurement_start_ns"], manifest["measurement_end_ns"])
            log.event("measurement_end")
            # Keep the publisher alive while listeners drain. Cooldown is excluded.
            time.sleep(0.2)
            for listener in listeners:
                stop(listener, True)
                if listener.returncode:
                    raise RuntimeError(
                        f"listener exited {listener.returncode}; inspect logs"
                    )
            manifest["complete"] = True
    except BaseException as error:
        manifest["complete"] = False
        manifest["failure"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        manifest["log_sha256"] = {
            role: digest(directory / name)
            for role, name in manifest["logs"].items()
            if (directory / name).exists()
        }
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    return directory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    analyze = sub.add_parser("analyze", help="recompute CSVs from saved logs/manifests")
    analyze.add_argument("directory", type=Path)
    run = sub.add_parser("run")
    run.add_argument("--mode", choices=("host", "board"), default="host")
    run.add_argument("--binary", type=Path)
    run.add_argument(
        "--output",
        type=Path,
        required=True,
        help="new directory; existing paths are refused",
    )
    run.add_argument(
        "--format", choices=("all", "csv", "json", "protobuf"), default="all"
    )
    run.add_argument("--crc", choices=("both", "off", "on"), default="both")
    run.add_argument(
        "--interface", required=True, help="concrete capture-host IPv4 address"
    )
    run.add_argument(
        "--host", help="responder IPv4; defaults to interface in host mode"
    )
    run.add_argument("--duration", type=float, default=600)
    run.add_argument("--warmup", type=float, default=10)
    run.add_argument("--command-period", type=float, default=1)
    run.add_argument("--timeout", type=float, default=1)
    run.add_argument(
        "--seed",
        type=int,
        default=723,
        help="reproducible shuffle of configuration order",
    )
    run.add_argument("--group", default="239.255.42.1")
    run.add_argument("--port", type=int, default=42000)
    run.add_argument("--status-port", type=int, default=42001)
    run.add_argument("--health-port", type=int, default=42002)
    run.add_argument("--device", default="board-01")
    run.add_argument("--firmware-revision", help="exact flashed board source revision")
    run.add_argument(
        "--firmware-image",
        type=Path,
        help="flashed signed image, hashed for provenance",
    )
    run.add_argument(
        "--instrumented", action="store_true", help="label a separate profiling run"
    )
    run.add_argument(
        "--notes",
        required=True,
        help="network, clocks, compiler/build command, logging setup",
    )
    args = parser.parse_args()
    if args.operation == "analyze":
        reports = export(args.directory)
    else:
        if not sys.platform.startswith("linux"):
            parser.error("comparison orchestration is currently verified only on Linux")
        if (
            not all(
                math.isfinite(v) and v > 0
                for v in (args.duration, args.command_period, args.timeout)
            )
            or not math.isfinite(args.warmup)
            or args.warmup < 0
        ):
            parser.error(
                "duration, command-period, timeout must be positive finite; warmup nonnegative finite"
            )
        if args.command_period < 0.001:
            parser.error("command-period must be at least .001 seconds")
        try:
            interface = ipaddress.IPv4Address(args.interface)
            if (
                interface.is_unspecified
                or interface.is_multicast
                or int(interface) == 2**32 - 1
            ):
                raise ValueError("choose a concrete local interface")
            if not ipaddress.IPv4Address(args.group).is_multicast:
                raise ValueError("choose a multicast group")
            args.host = args.host or (args.interface if args.mode == "host" else None)
            host = ipaddress.IPv4Address(args.host)
            if host.is_multicast or host.is_unspecified or int(host) == 2**32 - 1:
                raise ValueError("choose a unicast responder address")
        except (ValueError, ipaddress.AddressValueError) as error:
            parser.error(str(error))
        ports = (args.port, args.status_port, args.health_port)
        if len(set(ports)) != 3 or not all(0 < port < 65536 for port in ports):
            parser.error("command/status/health ports must be distinct and nonzero")
        if args.mode == "host":
            if (
                args.binary is None
                or not args.binary.is_file()
                or args.host != args.interface
            ):
                parser.error(
                    "host mode needs an existing --binary and --host equal to --interface"
                )
            args.binary = args.binary.resolve()
            if args.instrumented or args.firmware_image or args.firmware_revision:
                parser.error("firmware/profiling selectors belong to board mode")
        elif (
            args.format == "all"
            or args.crc == "both"
            or not args.firmware_revision
            or not args.firmware_image
            or not args.firmware_image.is_file()
        ):
            parser.error(
                "board mode needs explicit format, CRC, host, firmware revision and existing firmware image"
            )
        root = args.output.resolve()
        root.mkdir(parents=True, exist_ok=False)
        environment = provenance()
        configurations = [
            (f, c)
            for f in (
                ("csv", "json", "protobuf") if args.format == "all" else (args.format,)
            )
            for c in (("off", "on") if args.crc == "both" else (args.crc,))
        ]
        random.Random(args.seed).shuffle(configurations)
        (root / "matrix.json").write_text(
            json.dumps(
                dict(
                    seed=args.seed,
                    configurations=configurations,
                    environment=environment,
                    notes=args.notes,
                ),
                indent=2,
            )
            + "\n"
        )
        for order, configuration in enumerate(configurations, 1):
            capture(args, root, configuration, order, environment)
        reports = export(root)
    for report in reports:
        print(json.dumps(report["summary"], sort_keys=True))
    return (
        0
        if all(
            r["summary"]["data_valid"]
            and r["summary"]["all_commands_succeeded"]
            and r["summary"]["both_listeners_received_both_streams"]
            for r in reports
        )
        else 1
    )


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(f"comparison: {error}")
