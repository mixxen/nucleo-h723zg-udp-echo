# Messaging prototype contract v1

Status: Phase 1 design baseline, 2026-09-30. No running service is delivered by
this document. [Plan](PLAN.md) · [Integration](INTEGRATION.md) ·
[Fixtures](../../protocol/fixtures/cases.json) · [Checkpoint](CHECKPOINT.md).

## Authoring decision

Maintain message structure in [messaging.proto](../../protocol/messaging.proto).
Use established Protobuf generation for Rust (`micropb`) and Python. A small
Phase 2 adapter will read a protoc descriptor to derive CSV/JSON types and
reference documentation for this limited schema. No YAML-to-Protobuf compiler.
[profile.json](../../protocol/profile.json) adds capacities, explicit CSV column
order and demo defaults; it does not redefine field types or numeric tags.
No lists, maps, recursive messages or runtime schema discovery in v1.

The JSON mapping below is **prototype-json-v1, not ProtoJSON**. This avoids
implying compatibility with ProtoJSON's enum, naming and presence conventions.
The descriptor-adapter feasibility remains a Phase 2 implementation gate; if
micropb accessors require adapters, keep those small rather than changing the
schema to suit one codec. No format has been selected for production.

## Transport and framing

One complete Envelope per IPv4 UDP datagram. No sync word, automatic codec
sniffing, application fragmentation or retransmission. Select exactly one codec
and CRC mode in both endpoints' run configuration. The device-info response
reports the active codec; it is not a format negotiation mechanism.

Maximum encoded body: 1016 bytes, including CSV CRLF. Maximum full datagram:
1024 bytes. These same body limits apply with CRC on or off. Receive buffers
must detect truncation, reject oversized input, and never parse a prefix as a
complete message. IPv4 with a 20-byte header requires MTU >=1052 for the largest
packet. Check the actual interface/path MTU; reduce the configured limit or fail
startup on a smaller path, rather than rely on fragmentation. Keep UDP checksum
generation and verification enabled and inspect their behavior on the real path.

Defaults: command UDP 42000; multicast group 239.255.42.1; status UDP 42001;
health UDP 42002; multicast TTL 1. These are configurable private-lab choices,
not globally assigned service ports. Source publication ports equal their stream
ports. Command replies use the command socket and the request source endpoint.
Listeners explicitly select a NIC and join the group. Both streams are full
snapshots. There are no subscriber registrations, multicast commands or ACKs.

## Common fields and presence

All fields are proto3 optional so absence is observable, not silently a zero.
Application validation enforces the following table in every codec.

| Field | Requests | Responses | Publications |
|---|---|---|---|
| protocol_version | Required, exactly 1 | Same | Same |
| kind | Required recognized request kind | Required response kind | Required publication kind |
| device_id | Required target ID | Required producer ID | Required producer ID |
| boot_id | Absent | Required | Required |
| client_session | Required | Echo request value | Absent |
| request_id | Required, 1..4294967295 | Echo request value | Absent |
| sequence | Absent | Absent | Required, 0..4294967295 |
| uptime_ms | Absent | Required, 0..4294967295 | Required, 0..4294967295 |

Device IDs: 1..32 ASCII characters from letters, digits, `_`, `-`, `.`. Default
bench ID is `board-01`; explicitly provision different IDs and MAC addresses
before using multiple boards. This does not create a discovery service.
Boot/client session IDs: exactly 16 lowercase hex digits, nonzero. Client chooses
a fresh random 64-bit session on process start and before request-ID exhaustion.
Firmware chooses a fresh boot nonce using a verified board RNG path at boot;
never use a fixed network-stack seed as a boot ID. RNG initialization failure
must stop the messaging service with a local diagnostic. Host responder uses
host randomness. No flash counter is needed. Nonces identify sessions, not users.

Requests increase request_id monotonically in a session. Phase 2 client has at
most one outstanding request initially, with a 1000 ms deadline and no retries.
Match source endpoint, device_id, client_session, request_id and expected kind
(or ERROR_RESPONSE). Reject wrong correlation; record duplicates and late replies
without crediting a later request. Retain the last 64 completed/expired IDs in a
bounded host history; older unmatched replies are `unmatched`, not claimed
precisely as duplicates. A changed boot ID is reported; a delayed response from
an earlier board boot can still match an outstanding read, so do not interpret
it as proof of current board liveness. Producer uptime is captured at snapshot
creation, not arrival, and wraps modulo 2^32 (~49.7 days).

Each stream starts sequence at zero per boot and advances modulo 2^32 for each
scheduled publication attempt, including a locally skipped/failed send. Use
serial-number comparisons only within a half-range (<2^31). A subscriber's first
packet establishes a baseline; joining late is not packet loss. A boot change
starts a new stream history. Track a bounded reorder window of 64 sequence IDs;
only finalize a gap after it leaves that window or the run ends. Far-ahead or
ambiguous half-range jumps produce a discontinuity event, not billions of gaps.
Keep recent retired boot IDs to avoid switching history back on a late packet.

## Message and body mapping

Exactly one body is present and must match kind. Reject mismatched or multiple
bodies. For Protobuf, apply this rule to the decoded message using standard Protobuf
merge/oneof semantics described below. Do not build a second Protobuf parser.

| Kind (numeric) | Body | Content |
|---|---|---|
| GET_DEVICE_INFO (1) | get_device_info | Empty |
| GET_STATUS (2) | get_status | Empty |
| GET_HEALTH (3) | get_health | Empty |
| DEVICE_INFO_RESPONSE (11) | device_info | firmware_version, encoding |
| STATUS_RESPONSE (12) | status | temperature_celsius, validity, synthetic |
| HEALTH_RESPONSE (13) | health | state and four counters |
| ERROR_RESPONSE (14) | error | code |
| STATUS_PUBLICATION (21) | status | Same status body |
| HEALTH_PUBLICATION (22) | health | Same health body |

DeviceInfo: both fields required. firmware_version is 1..32 printable ASCII bytes
(0x20..0x7e), supporting spaces, commas, quotes and backslashes for escaping tests.
encoding is CSV=1, JSON=2 or PROTOBUF=3 and must agree with the configured codec.

Status: validity and synthetic required; synthetic must be true in this demo.
VALID=1 requires a finite IEEE-754 binary32 temperature from -100 to 200 Celsius.
UNAVAILABLE=2 or SENSOR_FAULT=3 requires temperature to be absent. Zero is a valid
measurement, not a missing value. Text codecs must round-trip the binary32 value;
use shortest round-trip decimal representation. Negative zero normalizes to zero.
NaN and infinity are rejected. Tests include unavailable and fault samples without
claiming a physical sensor exists.

Health: all five fields required; state OK=1 or DEGRADED=2. Counters are uint32,
saturate at UINT32_MAX, and reset at boot. received_requests counts accepted valid
read requests (include the current get_health); rejected_datagrams counts input
rejections observed by the service; send_errors counts local send failures;
skipped_publications counts due slots not submitted. State describes current
local service health, not whether historical counters are nonzero. Use DEGRADED
for an active local publication/send problem, OK after recovery. Missing/invalid
measurement does not automatically mean the messaging service is unhealthy.
Never report unobservable network drops as measured firmware counters.

Error: code required, UNSUPPORTED_KIND=1 or INVALID_REQUEST=2. Only respond when
version, target and complete correlation are valid, and CRC has passed if enabled.
Recognizable unknown request kinds can get UNSUPPORTED_KIND. Other malformed
packets, wrong target/version, CRC errors, publications received on the command
port and uncorrelatable input are dropped and counted; avoid error loops. An
error response itself is never answered. Bound error responses to 10/s with a
burst of 10; suppressed replies remain locally counted. No authorization or
actuation is implied by this prototype.

## Encoding rules

### CSV

UTF-8, exactly one record with terminal CRLF, no header, no extra records.
Common columns are fixed in profile.json; append only the columns for the kind's
body. Empty columns mean absent; an empty present string is not valid for our
string fields. Enums are decimal numbers, Booleans `true`/`false`, unsigned integers
are canonical decimal without sign or leading zero (except zero), session IDs
are text. Quote fields containing commas or quotes, doubling embedded quotes.
CR/LF inside string values are excluded by the ASCII bounds. Reject wrong column
count, malformed quotes, invalid UTF-8 and trailing records. Column order never
follows incidental source declaration order.

### JSON

UTF-8 object with exact snake_case schema field names. Scalars use JSON numbers
for uint32/enums, Boolean for bool, and strings for IDs/version text. The active
body is a nested object named exactly as in the mapping table; empty requests
carry `{}`. Omit absent fields; explicit null is rejected. Reject unknown keys,
duplicate keys at any depth, multiple bodies, non-integral integer fields and
out-of-range values. Strings must escape quotes/backslashes correctly. Emit
compact JSON; accept JSON whitespace and arbitrary object-key order. Serializer
choice must pass escaping fixtures; do not assume a no-heap crate has full string
support. This is a deliberately narrow custom mapping, not ProtoJSON.

### Protobuf

Standard binary proto3 encoding of Envelope. Use the library decoder and enforce
presence, known enum values, configured capacities, version and the same semantic
rules on the decoded message. Decode must consume the supplied body or report an
error; bound input to 1016 bytes before parsing. Preserve standard Protobuf rules:
unknown tags may be skipped; duplicate singular fields and oneof occurrences use
the library's standard merge/last-value behavior. Do not claim these inputs are
rejected merely because JSON duplicates are rejected. Include these parser-policy
differences explicitly in Phase 2 tests and the final comparison. A valid version
is mandatory even if unknown tags were skipped. Accept arbitrary field order.
Empty requests still contain their zero-length body submessage. Version changes
are coordinated; unknown-field tolerance is not a backward-compatibility promise.
Do not reuse retired field numbers.

### CRC mode

Off: body only. On: body followed by exactly eight uppercase ASCII hex characters
of CRC-32C; no separator or final newline after the trailer. The body length is
UDP payload length minus eight. This same trailer keeps CSV/JSON captures textual
and avoids inserting a CRC inside the encoded object. Strip the trailer before
codec parsing. No auto-detection or fallback after failure.

CRC-32C/Castagnoli: polynomial 0x1EDC6F41 (reflected 0x82F63B78), init 0xFFFFFFFF,
refin/refout true, xorout 0xFFFFFFFF. Check `123456789` -> E3069283; empty -> 00000000.
The trailer is the conventional 32-bit result rendered most significant hex digit
first. Protect every body byte, including metadata and CSV CRLF; exclude trailer.
CRC is accidental-error detection, not authentication. Phase 5 implements it;
Phase 1 freezes the representation so size comparisons include its eight bytes.

## Scheduling, freshness and logging

Status period 100 ms, health 1000 ms. Defaults are experiment settings. Independent
bounded tasks; no catch-up burst after link loss or a stalled send. Each due slot
advances sequence; missed slots increment skipped_publications. Try-send or use a
bounded send deadline shorter than that stream's period. Resolve cancellation
semantics against the selected API rather than assuming a cancelled send did not
transmit. Command handling must remain responsive while publications continue.

Default stale thresholds: status 500 ms, health 3000 ms since the most recent
accepted forward-progress packet on the host monotonic clock. Duplicates/older
packets do not refresh freshness. Initial state is `not_seen`; unavailable data
is distinct from stale delivery. Keep UTC only as an optional log label; compute
RTT and deadlines with the host monotonic clock. No cross-machine one-way latency
claim without clock synchronization. Link-down health may only be visible locally
until network recovery; silence alone does not identify why a device disappeared.

Fixed-capacity string overflow is a `decode_error`: it prevents construction of
the bounded message before application validation. A decoded message with missing
required fields or invalid values produces `validation_error`.

JSON Lines records: run ID, revision, schema/version, codec, CRC mode, role,
endpoint, NIC, device/boot/client IDs when known, kind, request/sequence, local
monotonic time, raw length, decoded values and outcome. Outcomes distinguish
success, unavailable/fault data, crc_error, decode_error, validation_error,
version_error, local_io_error, timeout, duplicate, late/unmatched, gap,
reordered, session_change and stale/recovered. Preserve rejected-byte samples
with a fixed cap and counters; no unbounded MCU history. Export statistics as CSV.
