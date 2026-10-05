"""Read the existing profiling endpoint of an instrumented NUCLEO image."""

import argparse
import json
import socket
from benchmark_profile import decode_snapshot


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
    print(json.dumps(decode_snapshot(data), indent=2))


if __name__ == "__main__":
    main()
