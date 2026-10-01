"""Read the existing profiling endpoint of an instrumented NUCLEO image."""

import argparse
import json
import socket
import struct


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--bind", required=True)
    parser.add_argument("--reset", action="store_true")
    args = parser.parse_args()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind((args.bind, 0))
        sock.connect((args.host, 5001))
        sock.settimeout(1)
        sock.send(b"STRMPRF1" + (b"R" if args.reset else b"S"))
        data = sock.recv(49)
    if len(data) != 48 or data[:8] != b"STRMPRF1":
        raise SystemExit("invalid profiling response")
    _, cpu_hz, ticks_hz, busy, ticks, polls, high_water, capacity = struct.unpack(
        "<8sIIQQQII", data
    )
    if not cpu_hz or not ticks_hz or not high_water <= capacity <= 131072:
        raise SystemExit("invalid profiling metrics")
    print(
        json.dumps(
            {
                "cpu_hz": cpu_hz,
                "elapsed_seconds": ticks / ticks_hz,
                "executor_busy_percent": (
                    busy * ticks_hz * 100 / (cpu_hz * ticks) if ticks else None
                ),
                "executor_polls": polls,
                "stack_high_water_bytes": high_water,
                "stack_capacity_bytes": capacity,
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
