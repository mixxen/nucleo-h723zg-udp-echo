"""Finite multicast fault demo using synthetic device fault-01; never sends commands.

Use on a test segment with the normal publisher stopped. Join with listener.py
--device fault-01 and the same format/CRC/interface configuration.
"""

import argparse
import contextlib
import ipaddress
import socket
import sys
import time
from codec import FORMATS
from framing import CRC_MODES, encode, seal
from client import EventLog, nonce


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=FORMATS, required=True)
    parser.add_argument("--crc", choices=CRC_MODES, default="on")
    parser.add_argument("--interface", required=True)
    parser.add_argument("--group", default="239.255.42.1")
    parser.add_argument("--status-port", type=int, default=42001)
    parser.add_argument("--health-port", type=int, default=42002)
    parser.add_argument(
        "--pause",
        type=float,
        default=4,
        help="silence interval; default exceeds both listener stale thresholds",
    )
    parser.add_argument("--log")
    args = parser.parse_args()
    try:
        group, interface = ipaddress.IPv4Address(args.group), ipaddress.IPv4Address(
            args.interface
        )
        if (
            not group.is_multicast
            or interface.is_unspecified
            or interface.is_multicast
            or int(interface) == 2**32 - 1
        ):
            parser.error("choose a multicast group and a concrete IPv4 interface")
    except ipaddress.AddressValueError as error:
        parser.error(str(error))
    if (
        not 0 < args.pause < float("inf")
        or args.status_port == args.health_port
        or not all(0 < p < 65536 for p in (args.status_port, args.health_port))
    ):
        parser.error("use a positive finite pause and distinct nonzero ports")
    output = open(args.log, "w", encoding="utf-8") if args.log else sys.stdout
    try:
        with contextlib.ExitStack() as cleanup:
            senders = {}
            for kind, port in ((21, args.status_port), (22, args.health_port)):
                sock = cleanup.enter_context(
                    socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                )
                # Exclusive source ports help catch an accidentally running normal publisher.
                sock.bind((args.interface, port))
                sock.setsockopt(
                    socket.IPPROTO_IP,
                    socket.IP_MULTICAST_IF,
                    socket.inet_aton(args.interface),
                )
                sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
                sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 1)
                senders[kind] = (sock, port)
            log = EventLog(
                output,
                role="fault_publisher",
                format=args.format,
                crc=args.crc,
                interface=args.interface,
                device_id="fault-01",
            )
            started = time.monotonic()
            boot = nonce()

            def send(kind, sequence, mutation=None):
                message = dict(
                    protocol_version=1,
                    kind=kind,
                    device_id="fault-01",
                    boot_id=boot,
                    sequence=sequence,
                    uptime_ms=int((time.monotonic() - started) * 1000) & 0xFFFFFFFF,
                )
                if kind == 21:
                    message["status"] = dict(
                        temperature_celsius=25.5, validity=1, synthetic=True
                    )
                else:
                    message["health"] = dict(
                        state=1,
                        received_requests=0,
                        rejected_datagrams=0,
                        send_errors=0,
                        skipped_publications=0,
                    )
                wire = encode(args.format, message, args.crc)
                if mutation == "corrupt":
                    # Frame first, then change one body byte before the OS constructs UDP.
                    wire = bytes([wire[0] ^ 1]) + wire[1:]
                elif mutation == "malformed":
                    wire = seal(b"\xff", args.crc)
                sock, port = senders[kind]
                sock.sendto(wire, (args.group, port))
                log.event(
                    "injected_publication" if mutation else "sent_publication",
                    (args.group, port),
                    len(wire),
                    message,
                    injected_fault=mutation,
                    mutation_before_udp_checksum=bool(mutation),
                )

            # Status: provisional gap 1, reordered 1, duplicate 2, then gap 3.
            for health_sequence, status_sequence in enumerate((0, 2, 1, 2, 4)):
                send(21, status_sequence)
                send(22, health_sequence)
                time.sleep(0.1)
            send(21, 5, "corrupt")
            send(21, 6, "malformed")
            log.event("publisher_pause", duration_seconds=args.pause)
            time.sleep(args.pause)
            send(21, 7)
            send(22, 5)
            time.sleep(0.1)
            boot = nonce()
            send(21, 0)
            send(22, 0)
            log.event("demo_complete")
        return 0
    finally:
        if args.log:
            output.close()


if __name__ == "__main__":
    sys.exit(main())
