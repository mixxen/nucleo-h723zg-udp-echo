"""Receive multicast status/health on an explicit IPv4 interface. Sends no ACKs."""

import argparse
import ipaddress
import json
import selectors
import re
import socket
import struct
import sys
import time
from codec import CodecError, FORMATS, decode
from client import EventLog
from stream_state import DeviceStreams


def join_group(group, port, interface):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 128 * 1024)
        # Unix group binding excludes ordinary unicast packets on the same port.
        # Windows multicast sockets conventionally bind the wildcard address.
        sock.bind(("" if sys.platform == "win32" else group, port))
        if sys.platform.startswith("linux"):
            # Linux UAPI IP_MULTICAST_ALL=49: receive only this socket's memberships.
            sock.setsockopt(socket.IPPROTO_IP, 49, 0)
            # Linux UAPI IP_RECVTTL=12, IP_PKTINFO=8; report observed TTL/destination.
            sock.setsockopt(socket.IPPROTO_IP, 12, 1)
            sock.setsockopt(socket.IPPROTO_IP, 8, 1)
        sock.setsockopt(
            socket.IPPROTO_IP,
            socket.IP_ADD_MEMBERSHIP,
            socket.inet_aton(group) + socket.inet_aton(interface),
        )
        sock.setblocking(False)
        return sock
    except BaseException:
        sock.close()
        raise


def receive(sock):
    if sys.platform.startswith("linux"):
        data, control, flags, peer = sock.recvmsg(65535, 128)
        details = {}
        for level, kind, value in control:
            if level == socket.IPPROTO_IP and kind == socket.IP_TTL:
                details["observed_ttl"] = struct.unpack("=i", value)[0]
            elif level == socket.IPPROTO_IP and kind == 8:
                details["destination"] = socket.inet_ntoa(value[8:12])
        if flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC):
            raise CodecError("size_error")
        return data, peer, details
    data, peer = sock.recvfrom(65535)
    return data, peer, {}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=FORMATS, required=True)
    parser.add_argument(
        "--interface",
        required=True,
        help="concrete local IPv4 address, e.g. 127.0.0.1 for loopback",
    )
    parser.add_argument("--group", default="239.255.42.1")
    parser.add_argument("--status-port", type=int, default=42001)
    parser.add_argument("--health-port", type=int, default=42002)
    parser.add_argument(
        "--device",
        action="append",
        help="expected device ID; repeat for up to 32 devices (default board-01)",
    )
    parser.add_argument("--source", help="optional producer IPv4 address filter")
    parser.add_argument(
        "--status-stale",
        type=float,
        default=0.5,
        help="seconds without forward status progress",
    )
    parser.add_argument("--health-stale", type=float, default=3.0)
    parser.add_argument(
        "--duration",
        type=float,
        help="stop after this many seconds; default until Ctrl+C",
    )
    parser.add_argument("--log", help="JSONL path; default stdout")
    args = parser.parse_args()
    devices = list(dict.fromkeys(args.device or ["board-01"]))
    try:
        group, interface = ipaddress.IPv4Address(args.group), ipaddress.IPv4Address(
            args.interface
        )
        if args.source:
            ipaddress.IPv4Address(args.source)
        if (
            not group.is_multicast
            or interface.is_multicast
            or interface.is_unspecified
            or int(interface) == 2**32 - 1
        ):
            parser.error("choose a multicast group and a concrete local IPv4 interface")
    except ipaddress.AddressValueError as error:
        parser.error(str(error))
    if len(devices) > 32:
        parser.error("at most 32 devices can be tracked")
    if not all(re.fullmatch(r"[a-zA-Z0-9_.-]{1,32}", device) for device in devices):
        parser.error(
            "device IDs must be 1..32 ASCII letters, digits, underscores, dots or hyphens"
        )
    if (
        not all(
            0 < value < float("inf")
            for value in (args.status_stale, args.health_stale, args.duration or 1)
        )
        or args.duration == 0
    ):
        parser.error("thresholds and duration must be positive and finite")
    if args.status_port == args.health_port or not all(
        0 < port < 65536 for port in (args.status_port, args.health_port)
    ):
        parser.error("use distinct nonzero status/health ports")
    streams = {
        device: DeviceStreams(args.status_stale, args.health_stale)
        for device in devices
    }
    output = open(args.log, "w", encoding="utf-8") if args.log else sys.stdout
    log = EventLog(
        output,
        role="listener",
        format=args.format,
        interface=args.interface,
        group=args.group,
        status_port=args.status_port,
        health_port=args.health_port,
    )
    selector = selectors.DefaultSelector()
    sockets = []
    seen = set()
    started = time.monotonic()

    def record(events, device, peer=None, data=b"", message=None, **details):
        for event in events:
            fields = dict(event)
            outcome = fields.pop("outcome")
            log.event(
                outcome, peer, len(data), message, device_id=device, **fields, **details
            )
            if outcome == "accepted_publication":
                seen.add((device, fields["kind"]))
                body = message["status"] if fields["kind"] == 21 else message["health"]
                print(
                    f'{device} {"status" if fields["kind"]==21 else "health"} seq={message["sequence"]}: {json.dumps(body)}',
                    file=sys.stderr,
                )
            elif outcome not in ("baseline", "gap_pending"):
                print(f"{device}: {outcome} {fields}", file=sys.stderr)

    try:
        for port, kind in ((args.status_port, 21), (args.health_port, 22)):
            sock = join_group(args.group, port, args.interface)
            sockets.append(sock)
            selector.register(sock, selectors.EVENT_READ, (port, kind))
        log.event("ready")
        print(
            f"Listening: {args.group} on {args.interface} / {args.format} / TTL expected 1",
            file=sys.stderr,
        )
        for device in devices:
            record(
                [dict(outcome="not_seen", kind=21), dict(outcome="not_seen", kind=22)],
                device,
            )
        while args.duration is None or time.monotonic() - started < args.duration:
            now = time.monotonic()
            for device, tracker in streams.items():
                record(tracker.tick(now), device)
            remaining = (
                max(0, args.duration - (now - started))
                if args.duration is not None
                else 0.05
            )
            for key, _ in selector.select(min(0.05, remaining)):
                port, kind = key.data
                # One datagram per ready socket per pass keeps timers/other stream fair.
                try:
                    data, peer, details = receive(key.fileobj)
                except BlockingIOError:
                    continue
                except CodecError as error:
                    log.event(error.outcome)
                    continue
                if (
                    peer[1] != port
                    or (args.source and peer[0] != args.source)
                    or details.get("destination", args.group) != args.group
                ):
                    log.event("wrong_peer_or_destination", peer, len(data), **details)
                    continue
                try:
                    message = decode(args.format, data)
                except CodecError as error:
                    log.event(
                        error.outcome,
                        peer,
                        len(data),
                        raw_prefix_hex=data[:64].hex(),
                        **details,
                    )
                    continue
                device = message["device_id"]
                if message["kind"] != kind or device not in streams:
                    log.event(
                        "unexpected_publication", peer, len(data), message, **details
                    )
                    continue
                now = time.monotonic()
                record(streams[device].tick(now), device)
                record(
                    streams[device].observe(message, now),
                    device,
                    peer,
                    data,
                    message,
                    **details,
                )
    except KeyboardInterrupt:
        pass
    except OSError as error:
        log.event("local_io_error", error=str(error))
        print(f"listener: {error}", file=sys.stderr)
        return 1
    finally:
        for device, tracker in streams.items():
            record(tracker.finish(), device)
        for sock in sockets:
            try:
                sock.setsockopt(
                    socket.IPPROTO_IP,
                    socket.IP_DROP_MEMBERSHIP,
                    socket.inet_aton(args.group) + socket.inet_aton(args.interface),
                )
            except OSError:
                pass  # Closing the socket also releases its membership after an interface failure.
            finally:
                sock.close()
        selector.close()
        if args.log:
            output.close()
    return (
        0
        if all((device, kind) in seen for device in devices for kind in (21, 22))
        else 1
    )


if __name__ == "__main__":
    sys.exit(main())
