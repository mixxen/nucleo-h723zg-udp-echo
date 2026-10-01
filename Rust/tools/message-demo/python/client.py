"""Read-only UDP client: one request outstanding, bounded history, no retries."""

import argparse
from collections import deque
import json
import secrets
import socket
import subprocess
import sys
import time
from codec import CodecError, FORMATS
from framing import CRC_MODES, decode, encode

COMMANDS = {
    "info": (1, "get_device_info"),
    "status": (2, "get_status"),
    "health": (3, "get_health"),
}


def nonce():
    while True:
        value = secrets.token_hex(8)
        if value != "0" * 16:
            return value


def revision():
    try:
        return subprocess.check_output(
            ["git", "describe", "--always", "--dirty"],
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


class EventLog:
    def __init__(self, output, **metadata):
        self.output = output
        self.started = time.monotonic()
        self.crc_rejections = 0
        self.metadata = dict(
            run_id=nonce(),
            revision=revision(),
            protocol_version=1,
            crc=metadata.pop("crc", "off"),
            role=metadata.pop("role", "client"),
            **metadata,
        )

    def event(self, outcome, peer=None, raw_length=0, message=None, **extra):
        if outcome == "crc_error":
            self.crc_rejections = min(2**32 - 1, self.crc_rejections + 1)
        record = dict(
            self.metadata,
            outcome=outcome,
            crc_rejections=self.crc_rejections,
            peer=peer,
            raw_length=raw_length,
            message=message,
            monotonic_ms=(time.monotonic() - self.started) * 1000,
            host_monotonic_ns=time.monotonic_ns(),
            unix_ms=time.time_ns() // 1_000_000,
            **extra,
        )
        print(
            json.dumps(record, separators=(",", ":"), allow_nan=False),
            file=self.output,
            flush=True,
        )


class Client:
    def __init__(self, sock, endpoint, format, device, timeout, log, crc="off"):
        self.sock, self.endpoint, self.format = sock, endpoint, format
        if crc not in CRC_MODES:
            raise ValueError("CRC mode must be off or on")
        self.crc = crc
        log.metadata["crc"] = crc
        self.device, self.timeout, self.log = device, timeout, log
        self.session, self.request_id = nonce(), 0
        self.history = deque(maxlen=64)

    def request(self, command):
        if self.request_id == 2**32 - 1:
            self.session, self.request_id = nonce(), 0
        self.request_id += 1
        kind, body = COMMANDS[command]
        message = dict(
            protocol_version=1,
            kind=kind,
            device_id=self.device,
            client_session=self.session,
            request_id=self.request_id,
            **{body: {}},
        )
        wire = encode(self.format, message, self.crc)
        started = time.monotonic()
        try:
            self.sock.sendto(wire, self.endpoint)
        except OSError as error:
            self.log.event(
                "local_io_error", self.endpoint, len(wire), message, error=str(error)
            )
            raise
        self.log.event("sent_request", self.endpoint, len(wire), message)
        deadline = started + self.timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                self.history.append(
                    (self.session, self.request_id, "late_response", kind + 10)
                )
                self.log.event("timeout", self.endpoint, message=message)
                return None
            self.sock.settimeout(remaining)
            try:
                data, peer = self.sock.recvfrom(65535)
            except socket.timeout:
                continue
            except OSError as error:
                self.log.event("local_io_error", self.endpoint, error=str(error))
                raise
            # The deadline still applies if the receive completes after the scheduling deadline.
            if time.monotonic() > deadline:
                self.log.event("after_deadline", peer, len(data))
                continue
            if peer != self.endpoint:
                self.log.event("wrong_peer", peer, len(data))
                continue
            try:
                response = decode(self.format, data, self.crc)
            except CodecError as error:
                self.log.event(
                    error.outcome, peer, len(data), raw_prefix_hex=data[:64].hex()
                )
                continue
            token = (response.get("client_session"), response.get("request_id"))
            matches = (
                response.get("device_id") == self.device
                and token == (self.session, self.request_id)
                and response.get("kind") in (kind + 10, 14)
            )
            if matches:
                self.history.append((*token, "duplicate_response", kind + 10))
                self.log.event(
                    "error_response" if response["kind"] == 14 else "accepted_response",
                    peer,
                    len(data),
                    response,
                    rtt_ms=(time.monotonic() - started) * 1000,
                )
                return response
            previous = next(
                (
                    entry
                    for entry in self.history
                    if entry[:2] == token
                    and response.get("device_id") == self.device
                    and response.get("kind") in (entry[3], 14)
                ),
                None,
            )
            self.log.event(
                previous[2] if previous else "unmatched_response",
                peer,
                len(data),
                response,
            )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=FORMATS, required=True)
    parser.add_argument("--crc", choices=CRC_MODES, default="off")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=42000)
    parser.add_argument(
        "--bind",
        default="127.0.0.1",
        help="local IPv4 interface address; use its LAN address for remote hosts",
    )
    parser.add_argument("--device", default="board-01")
    parser.add_argument(
        "--commands", nargs="+", choices=COMMANDS, default=list(COMMANDS)
    )
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument(
        "--interval",
        type=float,
        default=0,
        help="seconds between completed command attempts",
    )
    parser.add_argument("--timeout", type=float, default=1.0)
    parser.add_argument("--log", help="JSONL path (default: stdout)")
    args = parser.parse_args()
    if (
        args.repeat < 1
        or not 0 < args.timeout < float("inf")
        or not 0 <= args.interval < float("inf")
    ):
        parser.error(
            "repeat/timeout must be positive; interval must be nonnegative and finite"
        )
    endpoint = (socket.gethostbyname(args.host), args.port)
    output = open(args.log, "w", encoding="utf-8") if args.log else sys.stdout
    succeeded = True
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.bind((args.bind, 0))
            log = EventLog(
                output,
                format=args.format,
                crc=args.crc,
                local_endpoint=sock.getsockname(),
                interface=args.bind,
                device_id=args.device,
            )
            client = Client(
                sock, endpoint, args.format, args.device, args.timeout, log, args.crc
            )
            remaining = args.repeat * len(args.commands)
            for _ in range(args.repeat):
                for command in args.commands:
                    response = client.request(command)
                    if response is None:
                        print(
                            f"{command}: timeout (no valid reply before deadline)",
                            file=sys.stderr,
                        )
                        succeeded = False
                    else:
                        print(
                            f"{command}: {json.dumps(response, sort_keys=True)}",
                            file=sys.stderr,
                        )
                        succeeded &= response["kind"] != 14
                    remaining -= 1
                    if remaining and args.interval:
                        time.sleep(args.interval)
    except (OSError, CodecError) as error:
        print(f"client: {error}", file=sys.stderr)
        succeeded = False
    finally:
        if args.log:
            output.close()
    return 0 if succeeded else 1


if __name__ == "__main__":
    sys.exit(main())
