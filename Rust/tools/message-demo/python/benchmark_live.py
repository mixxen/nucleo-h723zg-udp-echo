"""B2 host benchmark: actual Rust codecs, UDP probes, multicast and leased controls.

Examples and measurement boundaries: docs/messaging/BENCHMARK_B2_LIVE.md.
"""

import argparse
from contextlib import ExitStack
import hashlib
import json
import math
from pathlib import Path
import secrets
import select
import socket
import subprocess
import sys
import time

from benchmark_service import Service, valid_configuration
from benchmark_rust_service import RustService
from benchmark_wire import Wire, PROFILE, ROOT, pattern
from codec import CodecError
from load_metrics import Pacer, ProbeTracker, RunIdentity, StreamTracker
from load_report import RunReport, inspect_report


def nonce():
    value = secrets.token_hex(8)
    return value if int(value, 16) else "0000000000000001"


def endpoint(sock):
    sock.setblocking(False)
    return sock


def identity(message, peer):
    return RunIdentity(
        peer[0],
        peer[1],
        message["device_id"],
        message["boot_id"],
        message["session_id"],
        message["run_epoch"],
    )


def listener(group, port, interface):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("", port))
        sock.setsockopt(
            socket.IPPROTO_IP,
            socket.IP_ADD_MEMBERSHIP,
            socket.inet_aton(group) + socket.inet_aton(interface),
        )
        return endpoint(sock)
    except BaseException:
        sock.close()
        raise


def responder(args, stop=None, ready=None, fault_hook=None):
    wire = Wire(args.format, args.crc)
    service_class = (
        RustService if getattr(args, "service", "rust") == "rust" else Service
    )
    service = service_class(
        args.device, nonce(), args.format, args.crc, time.monotonic_ns()
    )
    delayed = []  # Test-only fault injection, bounded to 64 queued datagrams.
    previous_epoch = service.epoch
    with ExitStack() as resources:
        command = resources.enter_context(
            socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        )
        command.bind((args.bind, args.command_port))
        endpoint(command)
        publishers = {}
        for kind, port in ((21, args.status_port), (22, args.health_port)):
            sock = resources.enter_context(
                socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            )
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            sock.bind((args.bind, port))
            sock.setsockopt(
                socket.IPPROTO_IP,
                socket.IP_MULTICAST_IF,
                socket.inet_aton(args.interface),
            )
            sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
            sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 1)
            publishers[kind] = (endpoint(sock), port)
        if ready is not None:
            ready.set()
        print(
            json.dumps(
                dict(event="ready", boot=service.boot, format=args.format, crc=args.crc)
            ),
            flush=True,
        )
        try:
            while stop is None or not stop.is_set():
                service.tick(time.monotonic_ns())
                if service.epoch != previous_epoch:
                    delayed.clear()
                    previous_epoch = service.epoch
                select.select([command], [], [], 0.001)
                for _ in range(64):
                    try:
                        data, peer = command.recvfrom(65535)
                    except BlockingIOError:
                        break
                    now = time.monotonic_ns()
                    try:
                        request = wire.decode(data)
                    except CodecError:
                        service.tick(now)
                        service.reject()
                        continue
                    reply = service.handle(request, peer, time.monotonic_ns())
                    if service.epoch != previous_epoch:
                        delayed.clear()
                        previous_epoch = service.epoch
                    if reply is not None:
                        try:
                            packet = wire.encode(reply)
                            deliveries = (
                                [(0, packet)]
                                if fault_hook is None
                                else fault_hook(request, packet)
                            )
                            for delay, packet in deliveries:
                                if not 0 <= delay <= 60_000_000_000:
                                    raise ValueError("invalid test delay")
                                if delay:
                                    if len(delayed) < 64:
                                        delayed.append(
                                            (time.monotonic_ns() + delay, packet, peer)
                                        )
                                else:
                                    command.sendto(packet, peer)
                                    service.sent(0, True)
                        except (OSError, CodecError):
                            service.sent(0, False)
                remaining = []
                for due, packet, peer in delayed:
                    if due > time.monotonic_ns():
                        remaining.append((due, packet, peer))
                    else:
                        try:
                            command.sendto(packet, peer)
                            service.sent(0, True)
                        except OSError:
                            service.sent(0, False)
                delayed = remaining
                for message in service.publications(time.monotonic_ns()):
                    sock, port = publishers[message["kind"]]
                    try:
                        sock.sendto(wire.encode(message), (args.group, port))
                        service.sent(message["kind"], True)
                    except (OSError, CodecError):
                        service.sent(message["kind"], False)
                        service.count("skipped_publications")
        finally:
            service.end()
            if isinstance(service, RustService):
                service.close()


def exchange(sock, target, wire, message, expected, timeout=1.5):
    """Bounded initial/final control exchange; retries preserve ID and contents."""
    start = time.monotonic_ns()
    deadline = start + int(timeout * 1e9)
    next_send = 0
    packet = wire.encode(message)
    while time.monotonic_ns() < deadline:
        now = time.monotonic_ns()
        if now >= next_send:
            sock.sendto(packet, target)
            next_send = now + 500_000_000
        select.select([sock], [], [], 0.01)
        for _ in range(64):
            try:
                data, peer = sock.recvfrom(65535)
            except BlockingIOError:
                break
            try:
                reply = wire.decode(data)
            except CodecError:
                continue
            if (
                peer == target
                and reply["kind"] == expected
                and reply["device_id"] == message["device_id"]
                and reply["session_id"] == message["session_id"]
                and reply.get("request_id") == message["request_id"]
                and ("boot_id" not in message or reply["boot_id"] == message["boot_id"])
                and (
                    "run_epoch" not in message
                    or reply["run_epoch"] == message["run_epoch"]
                )
            ):
                return reply, start
    raise TimeoutError("no valid correlated control reply")


def provenance(wire):
    try:
        repo = ROOT.parent
        commit = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=repo, text=True
        ).strip()
        dirty = bool(
            subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)
        )
    except (OSError, subprocess.CalledProcessError):
        commit, dirty = None, None
    return dict(
        source_commit=commit,
        worktree_dirty=dirty,
        codec_library=str(wire.path),
        codec_sha256=wire.sha256,
        python=sys.version,
        platform=sys.platform,
        sources_sha256={
            p.name: hashlib.sha256(p.read_bytes()).hexdigest()
            for p in (
                Path(__file__),
                Path(__file__).with_name("benchmark_service.py"),
                Path(__file__).with_name("benchmark_rust_service.py"),
                Path(__file__).with_name("benchmark_wire.py"),
                Path(__file__).with_name("load_metrics.py"),
                Path(__file__).with_name("load_report.py"),
            )
        },
    )


def validate_args(args):
    for name in ("duration", "report_interval"):
        value = getattr(args, name)
        if not math.isfinite(value) or value <= 0:
            raise ValueError(f"{name} must be positive and finite")
    if not math.isfinite(args.warmup) or args.warmup < 0:
        raise ValueError("warmup must be finite and nonnegative")
    if (
        not 0 <= args.command_hz <= 10000
        or not 1 <= args.outstanding <= 4096
        or not 1 <= args.deadline_ms <= 60000
    ):
        raise ValueError("rate/window/deadline outside profile bounds")
    if not math.isfinite(args.stale_multiplier) or args.stale_multiplier < 1:
        raise ValueError("stale multiplier must be finite and >=1")
    config = {
        name: getattr(args, name)
        for name in (
            "payload_bytes",
            "pattern_seed",
            "status_period_us",
            "health_period_us",
            "lease_ms",
        )
    }
    if not valid_configuration(config):
        raise ValueError("configuration outside advertised benchmark profile")
    for name in ("target", "interface", "group"):
        socket.inet_aton(getattr(args, name))
    if args.interface == "0.0.0.0":
        raise ValueError("select an explicit interface IPv4 address")
    if args.target_kind == "board" and not args.image_sha256:
        raise ValueError("board runs require --image-sha256")
    if args.image_sha256 and (
        len(args.image_sha256) != 64
        or any(c not in "0123456789abcdef" for c in args.image_sha256)
    ):
        raise ValueError("image sha256 must be 64 lowercase hex digits")
    if len({args.command_port, args.status_port, args.health_port}) != 3 or any(
        not 1 <= p <= 65535
        for p in (args.command_port, args.status_port, args.health_port)
    ):
        raise ValueError("ports must be distinct and in 1..65535")
    return config


def run(args):
    config = validate_args(args)
    wire = Wire(args.format, args.crc)
    target = (args.target, args.command_port)
    session = nonce()
    base = dict(protocol_version=1, device_id=args.device, session_id=session)
    control_id = 1
    start = time.monotonic_ns()
    # Create evidence before network setup so failed preflight leaves a record.
    metadata = dict(
        parameters=vars(args),
        configuration=config,
        provenance=provenance(wire),
        evidence="host_socket_run" if args.target_kind == "host" else "board_run",
        normal_background_streams=args.target_kind == "board",
        rtt_boundary="pre-send to validated reply, including FFI/JSON bridge and response decoding",
        request_encoding_in_rtt=False,
    )
    with RunReport(args.output, metadata, start) as report, ExitStack() as resources:
        probes = ProbeTracker(
            RunIdentity(args.target, args.command_port, args.device, "", session, 0),
            args.outstanding,
            args.deadline_ms * 1_000_000,
        )
        pacer = Pacer(start, 0)
        streams = []
        command = None
        acquired = False
        reason = "preflight_failure"
        complete = False
        cleanup = "not_acquired"
        measuring = False
        try:
            command = resources.enter_context(
                socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            )
            command.bind((args.interface, 0))
            endpoint(command)
            receivers = {}
            for kind, port, period in (
                (21, args.status_port, args.status_period_us),
                (22, args.health_port, args.health_period_us),
            ):
                if period:
                    receivers[
                        resources.enter_context(
                            listener(args.group, port, args.interface)
                        )
                    ] = kind
            capabilities, _ = exchange(
                command,
                target,
                wire,
                dict(base, kind=1, request_id=1, capabilities={}),
                11,
            )
            if not capabilities["limits"]["available"]:
                raise RuntimeError("benchmark service busy or exhausted")
            base.update(
                boot_id=capabilities["boot_id"], run_epoch=capabilities["run_epoch"]
            )
            # Cleanup must be attempted even when configure's ACK is lost.
            acquired = True
            reply, control_sent = exchange(
                command,
                target,
                wire,
                dict(base, kind=2, request_id=control_id, configure=config),
                12,
            )
            if reply["result"]["code"] not in (1, 2):
                raise RuntimeError(f"configure rejected: {reply['result']['code']}")
            lease_expiry = (
                control_sent + reply["result"]["lease_remaining_ms"] * 1_000_000
            )
            confirmed_at = time.monotonic_ns()
            renew_at = confirmed_at + max(0, lease_expiry - confirmed_at) // 3
            origin = time.monotonic_ns()
            measured_start = origin + int(args.warmup * 1e9)
            measured_end = measured_start + int(args.duration * 1e9)
            finish_at = measured_end + args.deadline_ms * 1_000_000
            run_id = RunIdentity(
                args.target,
                args.command_port,
                args.device,
                base["boot_id"],
                session,
                base["run_epoch"],
            )
            probes = ProbeTracker(
                run_id, args.outstanding, args.deadline_ms * 1_000_000
            )
            pacer = Pacer(origin, args.command_hz)
            request_id = 0
            first_measured_id = 1
            measuring = False
            report.diagnostic(
                origin,
                run_identity=run_id.__dict__,
                measured_start_ns=measured_start,
                measured_end_ns=measured_end,
                effective_status_hz=(
                    1e6 / args.status_period_us if args.status_period_us else 0
                ),
                effective_health_hz=(
                    1e6 / args.health_period_us if args.health_period_us else 0
                ),
            )
            pending_control = None
            next_report = measured_start + int(args.report_interval * 1e9)
            while time.monotonic_ns() < finish_at:
                now = time.monotonic_ns()
                if now >= lease_expiry:
                    raise RuntimeError("lease confirmation expired")
                if not measuring and now >= measured_start:
                    measuring = True
                    probes = ProbeTracker(
                        run_id, args.outstanding, args.deadline_ms * 1_000_000
                    )
                    pacer = Pacer(measured_start, args.command_hz)
                    first_measured_id = request_id + 1
                    report.previous = measured_start
                    streams = [
                        StreamTracker(
                            RunIdentity(
                                args.target,
                                port,
                                args.device,
                                base["boot_id"],
                                session,
                                base["run_epoch"],
                            ),
                            kind,
                            int(period * 1000 * args.stale_multiplier),
                            start_ns=measured_start,
                        )
                        for kind, port, period in (
                            (21, args.status_port, args.status_period_us),
                            (22, args.health_port, args.health_period_us),
                        )
                        if period
                    ]
                probes.tick(now)
                if pending_control is None and now >= renew_at:
                    if control_id >= 2**32 - 2:
                        raise RuntimeError("control ID space exhausted")
                    control_id += 1
                    message = dict(base, kind=3, request_id=control_id, renew={})
                    pending_control = [wire.encode(message), now, 0]
                if pending_control is not None and now >= pending_control[2]:
                    command.sendto(pending_control[0], target)
                    pending_control[2] = now + 500_000_000
                if now < measured_end and pacer.due(
                    now, len(probes.pending) < args.outstanding
                ):
                    request_id += 1
                    if request_id >= 2**32:
                        raise RuntimeError("probe ID space exhausted")
                    packet = wire.encode(
                        dict(
                            base,
                            kind=5,
                            request_id=request_id,
                            payload=pattern(
                                args.payload_bytes, args.pattern_seed, request_id
                            ),
                            probe={},
                        )
                    )
                    sent = time.monotonic_ns()
                    try:
                        if sent >= measured_end:
                            probes.counts["encoding_end_skips"] += 1
                        else:
                            command.sendto(packet, target)
                            probes.sent(request_id, sent, len(packet))
                    except OSError:
                        probes.counts["local_send_errors"] += 1
                next_due = pacer.next_due_ns() if now < measured_end else None
                timeout = (
                    min(0.001, max(0, (next_due - time.monotonic_ns()) / 1e9))
                    if next_due is not None
                    else 0.001
                )
                ready, _, _ = select.select([command, *receivers], [], [], timeout)
                # Total receive work is bounded, with per-socket rotation by select.
                for sock in ready:
                    for _ in range(max(1, 64 // len(ready))):
                        try:
                            data, peer = sock.recvfrom(65535)
                        except BlockingIOError:
                            break
                        try:
                            message = wire.decode(data)
                        except CodecError as error:
                            probes.counts[error.outcome] += 1
                            continue
                        observed = time.monotonic_ns()
                        if sock is command and message["kind"] == 12:
                            if (
                                pending_control is not None
                                and peer == target
                                and identity(message, peer) == run_id
                                and message.get("request_id") == control_id
                            ):
                                if message["result"]["code"] not in (1, 2):
                                    raise RuntimeError(
                                        f"renew rejected: {message['result']['code']}"
                                    )
                                lease_expiry = (
                                    pending_control[1]
                                    + message["result"]["lease_remaining_ms"]
                                    * 1_000_000
                                )
                                pending_control = None
                                renew_at = (
                                    observed + max(0, lease_expiry - observed) // 3
                                )
                            continue
                        if sock is command:
                            if message["kind"] != 15:
                                probes.counts["unexpected_command_kind"] += 1
                                continue
                            if measuring and message["request_id"] < first_measured_id:
                                continue
                            valid = message["payload"] == pattern(
                                args.payload_bytes,
                                args.pattern_seed,
                                message["request_id"],
                            )
                            probes.reply(
                                identity(message, peer),
                                message["request_id"],
                                message["kind"],
                                observed,
                                len(data),
                                valid,
                            )
                        elif measuring and observed < measured_end:
                            stream = next(
                                s for s in streams if s.kind == receivers[sock]
                            )
                            if message["kind"] not in (21, 22):
                                probes.counts["unexpected_stream_kind"] += 1
                                continue
                            valid = message["payload"] == pattern(
                                args.payload_bytes,
                                args.pattern_seed,
                                message["sequence"],
                            )
                            outcome = stream.observe(
                                identity(message, peer),
                                message["kind"],
                                message["sequence"],
                                observed,
                                len(data),
                                valid,
                            )
                            if message["kind"] == 22 and outcome in (
                                "baseline",
                                "progress",
                            ):
                                probes.last_health = message["health"]
                                if not hasattr(probes, "first_health"):
                                    probes.first_health = message["health"]
                now = time.monotonic_ns()
                if measuring and now >= measured_end:
                    for stream in streams:
                        if not stream.closed:
                            stream.finish(measured_end)
                if measuring and now >= next_report:
                    record = report.interval(now, probes, pacer, streams)
                    print(
                        json.dumps(
                            dict(
                                event="interval",
                                phase="load" if now < measured_end else "drain",
                                elapsed_s=(now - measured_start) / 1e9,
                                send_hz=record["send_hz"],
                                outstanding=len(probes.pending),
                                counts=dict(probes.counts),
                                p99_upper_ns=probes.rtt.quantile(0.99),
                            )
                        ),
                        flush=True,
                    )
                    next_report = now + int(args.report_interval * 1e9)
            complete = True
            reason = "duration_and_deadline_drain"
        except KeyboardInterrupt:
            reason = "keyboard_interrupt"
        except (OSError, ValueError, RuntimeError, TimeoutError) as error:
            reason = f"{type(error).__name__}: {error}"
        finally:
            now = time.monotonic_ns()
            if acquired and command is not None:
                try:
                    stopped, _ = exchange(
                        command,
                        target,
                        wire,
                        dict(base, kind=4, request_id=control_id + 1, stop={}),
                        12,
                    )
                    cleanup = (
                        "stop_acknowledged"
                        if stopped["result"]["code"] == 3
                        else (
                            "run_obsolete"
                            if stopped["result"]["code"] == 6
                            else "lease_fallback"
                        )
                    )
                except (OSError, ValueError, TimeoutError):
                    cleanup = "lease_fallback"
            # Preserve the measurement end timestamp, excluding stop exchange time.
            # Cleanup precedes output so disk failure cannot prevent stop.
            if measuring and now > report.previous:
                report.interval(now, probes, pacer, streams)
            report.diagnostic(
                now,
                cleanup=cleanup,
                first_health=getattr(probes, "first_health", None),
                last_health=getattr(probes, "last_health", None),
            )
            report.finish(now, reason, complete, probes, pacer, streams)
            print(
                json.dumps(
                    dict(
                        event="final",
                        complete=complete,
                        reason=reason,
                        cleanup=cleanup,
                        output=args.output,
                    )
                ),
                flush=True,
            )
        return complete


def parser():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest="mode", required=True)
    server = sub.add_parser("responder")
    client = sub.add_parser("run")
    inspect = sub.add_parser("inspect")
    inspect.add_argument("events")
    for command in (server, client):
        command.add_argument(
            "--format", choices=("csv", "json", "protobuf"), required=True
        )
        command.add_argument("--crc", choices=("off", "on"), required=True)
        command.add_argument("--interface", required=True)
        command.add_argument("--device", default="board-01")
        command.add_argument("--group", default="239.255.42.1")
        for name, default in (
            ("command-port", 42010),
            ("status-port", 42011),
            ("health-port", 42012),
        ):
            command.add_argument("--" + name, type=int, default=default)
    server.add_argument("--bind", default="0.0.0.0")
    server.add_argument(
        "--service",
        choices=("rust", "python"),
        default="rust",
        help="portable firmware service, or independent Python reference",
    )
    client.add_argument("--target", required=True)
    client.add_argument("--target-kind", choices=("host", "board"), required=True)
    client.add_argument("--image-sha256")
    client.add_argument(
        "--instrumented",
        action="store_true",
        help="label a separately built profiling image in the manifest",
    )
    client.add_argument("--output", required=True)
    for name, default in (
        ("payload-bytes", 64),
        ("pattern-seed", 723),
        ("status-period-us", 100000),
        ("health-period-us", 1000000),
        ("lease-ms", 10000),
        ("command-hz", 10),
        ("outstanding", 64),
        ("deadline-ms", 1000),
    ):
        client.add_argument("--" + name, type=int, default=default)
    for name, default in (
        ("duration", 10.0),
        ("warmup", 1.0),
        ("report-interval", 1.0),
        ("stale-multiplier", 3.0),
    ):
        client.add_argument("--" + name, type=float, default=default)
    return p


if __name__ == "__main__":
    args = parser().parse_args()
    if args.mode == "responder":
        responder(args)
    elif args.mode == "inspect":
        print(json.dumps(inspect_report(args.events), indent=2))
    else:
        raise SystemExit(0 if run(args) else 2)
