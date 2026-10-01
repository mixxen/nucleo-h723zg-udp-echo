"""Send one bounded fault case to a read-only messaging responder, then probe recovery.

Mutations happen before sendto, so the OS computes a normal UDP checksum over
intentionally damaged application bytes. Sender silence does not identify cause;
correlate this log with responder/receiver diagnostics.
"""

import argparse
import copy
import json
import socket
import sys
from codec import CodecError, FORMATS, encode as encode_body
from framing import CRC_MODES, decode, seal
from client import Client, EventLog

CASES = ("corrupt", "malformed", "invalid", "version", "oversize", "truncated")


def packet(format, crc, case, device="board-01", session="0123456789abcdef"):
    request = dict(
        protocol_version=1,
        kind=2,
        device_id=device,
        client_session=session,
        request_id=1,
        get_status={},
    )
    original = encode_body(format, request)
    if case == "corrupt":
        # Change the correlation ID while retaining valid syntax and semantics.
        changed = copy.deepcopy(request)
        changed["request_id"] = 2
        altered = encode_body(format, changed)
        assert len(altered) == len(original)
        positions = [i for i, (a, b) in enumerate(zip(original, altered)) if a != b]
        assert len(positions) == 1
        wire = bytearray(seal(original, crc))
        wire[positions[0]] = altered[positions[0]]  # AFTER checksum calculation
        return bytes(wire), "crc_error" if crc == "on" else "accepted_request"
    if case == "malformed":
        return seal(b"\xff", crc), "decode_error"
    if case in ("invalid", "version"):
        # These scalar changes remain structurally decodable in all three formats.
        modified = dict(request)
        modified["request_id" if case == "invalid" else "protocol_version"] = (
            0 if case == "invalid" else 2
        )
        if format == "json":
            body = json.dumps(modified, separators=(",", ":")).encode()
        elif format == "protobuf":
            from codec import to_proto, messaging_pb2

            body = to_proto(modified, messaging_pb2.Envelope()).SerializeToString()
        else:
            body = (
                original.replace(b",1,,", b",0,,", 1)
                if case == "invalid"
                else b"2" + original[1:]
            )
        return seal(body, crc), (
            "validation_error" if case == "invalid" else "version_error"
        )
    if case == "oversize":
        return b"X" * 1025, "size_error"
    if case == "truncated":
        # A truncated application datagram; OS socket truncation is tested separately.
        return seal(original, crc)[:-1], "crc_error" if crc == "on" else "decode_error"
    raise ValueError("unknown fault case")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=FORMATS, required=True)
    parser.add_argument("--crc", choices=CRC_MODES, default="off")
    parser.add_argument("--host", required=True)
    parser.add_argument("--bind", required=True)
    parser.add_argument("--port", type=int, default=42000)
    parser.add_argument("--device", default="board-01")
    parser.add_argument("--case", choices=CASES, required=True)
    parser.add_argument("--log")
    args = parser.parse_args()
    output = open(args.log, "w", encoding="utf-8") if args.log else sys.stdout
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.bind((args.bind, 0))
            endpoint = (socket.gethostbyname(args.host), args.port)
            log = EventLog(
                output,
                role="fault_sender",
                format=args.format,
                crc=args.crc,
                interface=args.bind,
            )
            client = Client(sock, endpoint, args.format, args.device, 1, log, args.crc)
            if client.request("info") is None:
                log.event("preflight_failed")
                return 1
            # Use a separate session for the injected traffic, keeping the recovery probe distinct.
            wire, expected = packet(args.format, args.crc, args.case, args.device)
            log.event(
                "injected_fault",
                endpoint,
                len(wire),
                case=args.case,
                expected_receiver_outcome=expected,
                raw_prefix_hex=wire[:64].hex(),
                mutation_before_udp_checksum=True,
            )
            sock.sendto(wire, endpoint)
            sock.settimeout(0.25)
            try:
                data, peer = sock.recvfrom(65535)
                try:
                    reply = decode(args.format, data, args.crc)
                    log.event("fault_response_observed", peer, len(data), reply)
                except CodecError as error:
                    log.event(
                        error.outcome, peer, len(data), raw_prefix_hex=data[:64].hex()
                    )
            except socket.timeout:
                log.event("no_response_observed", endpoint, observation_window_ms=250)
            recovered = client.request("health")
            log.event(
                "recovery_passed"
                if recovered and recovered["kind"] == 13
                else "recovery_failed"
            )
            return 0 if recovered and recovered["kind"] == 13 else 1
    finally:
        if args.log:
            output.close()


if __name__ == "__main__":
    sys.exit(main())
