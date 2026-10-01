# Phase 5: CRC-32C and fault demonstrations

**Implementation and local host/build checks complete; board execution pending.**
Base: merged Phase 4, `deb811c` (PR #4). Its two CI workflows passed. No board was
attached or flashed in this phase; the physical Phase 3/4 gates remain open.

## What changed

Commands, responses, status publications and health publications can now carry
CRC-32C. The Rust host, Python client/listener and full NUCLEO application use the
same framing rules. The message schema and CRC-off body bytes are unchanged.
Each endpoint selects one encoding and one CRC mode for its entire run. Defaults
remain CRC off; there is no negotiation, format sniffing or retry in another mode.

| Setting | CRC off | CRC on |
|---|---|---|
| Host responder/client/listener | `--crc off` or omit | `--crc on` |
| Full board feature | `messaging-protobuf` | `messaging-protobuf,messaging-crc` |
| Signing/factory-flash script | `-Messaging protobuf` | `-Messaging protobuf -Crc` |
| Wire datagram | Existing encoded body | Same body plus eight uppercase hex characters |
| Maximum body / datagram | 1016 / 1016 bytes | 1016 / 1024 bytes |

Replace `protobuf` with `csv` or `json` as needed. The Phase 3 publisher-only smoke
image remains CRC off. The new `-Crc` switch requires a full `-Messaging` image.

CRC covers **every exact encoded body byte**, including envelope metadata, JSON
whitespace and CSV's terminating CRLF. It excludes the trailer itself. The trailer
has no separator or following newline. For CSV, it follows the record's CRLF;
strip and verify framing before passing the body to the CSV parser. UDP still
provides message boundaries, so no sync word was added. UDP checksums remain
unchanged and enabled in the existing networking paths.

The existing contract selects CRC-32C/Castagnoli: polynomial `0x1EDC6F41`, reflected
form `0x82F63B78`, initial value and final XOR `0xFFFFFFFF`, reflected input/output.
The conventional result for `123456789` is `E3069283`; empty input is `00000000`.
CRC detects accidental changes. It is not authentication and does not prevent
someone from changing data and recalculating the CRC.

Rust uses pinned [`crc` 3.3.0](https://docs.rs/crc/3.3.0/crc/), `no_std`, with its
`CRC_32_ISCSI` parameters and a table-based checksum. Python uses an independent
bitwise Castagnoli implementation, avoiding a new Python runtime dependency.
Both are checked against published vectors, not just against each other:

| Input | Conventional numeric result / our ASCII trailer |
|---|---|
| 32 zero bytes | `8A9136AA` |
| 32 `FF` bytes | `62A8AB43` |
| Bytes `00` through `1F` | `46DD794E` |
| Bytes `1F` through `00` | `113FDB5C` |

Source: [RFC 3720 Appendix B.4](https://www.rfc-editor.org/rfc/rfc3720.html#appendix-B.4).
That RFC displays digest bytes least-significant first. Our frozen contract prints
the numeric value most-significant hex digit first; the table accounts for this.

## Rejection order and counters

Transport peer/destination checks still apply first. For a selected endpoint,
framing rejects over-limit datagrams before doing CRC work. With CRC enabled,
missing/short trailers, non-uppercase-hex trailers and checksum mismatches produce
`crc_error`. The body is decoded only after CRC passes. An empty body with a valid
zero CRC passes integrity, then produces `decode_error`.

| Injected condition | Receiver outcome |
|---|---|
| One changed body byte, old CRC | `crc_error`; no decoded message or error reply |
| Malformed serialization with a freshly computed CRC | `decode_error` |
| Decodable request with request ID zero | `validation_error`; uncorrelatable, no reply |
| Decodable request with version 2 | `version_error`; no reply |
| Datagram above the selected mode's limit | `size_error`, before CRC |
| Truncated CRC trailer | `crc_error` |
| Delayed/duplicate response with valid CRC | Existing late/duplicate correlation outcome |
| Bad-CRC publications during an outage | CRC rejections; freshness still expires |

CRC failures increase the existing health `rejected_datagrams` total. They also
increment a saturating `crc_rejections` diagnostic count. The health schema and
CSV column mapping were kept intact, so this specific count is **not a new wire
health field**:

- Rust responder JSONL includes the cumulative count. It increments before the
  bounded log queue, so discarded log records do not discard the count.
- Python client/listener JSONL includes their own receiver-local count. Rejected
  byte samples remain capped at 64 bytes; bad CRC never supplies trusted message
  fields or updates the listener's sequence/session/freshness state.
- The MCU keeps the count in fixed storage and reports changed values over RTT,
  at most once per second from the health task, including while the link is down.
  Messaging images force nonblocking RTT mode so a detached/slow debugger cannot
  stall command or publisher tasks. RTT may discard diagnostics when full.

Counters only describe datagrams delivered to the application. NIC/IP/checksum
or full-queue drops remain invisible. A client timeout means it observed no valid
response before its deadline, not proof that CRC caused a rejection. Correlate
sender logs with receiver logs or MCU counters. Different CRC settings are a
configuration error; CRC-off parsers simply see all received bytes as a body.
Do not rely on every possible mode mismatch being detectable by body parsing.

## Run the host demo with CRC

From the repository root, using the [Phase 2 environment](PHASE2.md):

```sh
cargo +1.90.0 run --locked --manifest-path Rust/tools/message-demo/Cargo.toml \
  --target x86_64-unknown-linux-gnu -- serve --format protobuf --crc on \
  --bind 127.0.0.1:42000 --multicast-interface 127.0.0.1 \
  --log server-crc.jsonl
```

Use your actual host Rust target on Windows/macOS. On two additional terminals:

```sh
python Rust/tools/message-demo/python/listener.py --format protobuf --crc on \
  --interface 127.0.0.1 --log listener-crc-a.jsonl
```

Run the second listener with a different log filename. While both receive:

```sh
python Rust/tools/message-demo/python/client.py --format protobuf --crc on \
  --host 127.0.0.1 --bind 127.0.0.1 --repeat 30 --interval 1 \
  --log commands-crc.jsonl
```

Repeat with CSV and JSON, and again with CRC off at **every** endpoint. The
existing six-way process test demonstrates two listeners and continuing commands,
then stale indication, restart and a new boot ID. JSONL stays JSON regardless of
the selected wire encoding.

## Demonstrate request faults

Keep the responder running. Send a valid preflight command, one intentionally
changed datagram, and a valid health command afterward:

```sh
python Rust/tools/message-demo/python/faults.py --format protobuf --crc on \
  --host 127.0.0.1 --bind 127.0.0.1 --case corrupt --log corrupt-request.jsonl
```

`--case` also accepts `malformed`, `invalid`, `version`, `oversize`, and `truncated`.
Use the server's JSONL to identify the receiver's actual outcome. A successful
fault tool exit means its recovery health request succeeded; it does not treat
silence as proof of the expected fault classification.

`corrupt` changes request ID 1 to 2 **after** the CRC was computed while keeping
valid serialization. With CRC on, the responder rejects it. With CRC off, the
changed read request can be accepted and answered as request ID 2. This makes the
limit of parser-only checking visible. The regular client still protects its own
outstanding request through correlation.

For malformed and invalid cases, the tool computes a valid CRC over the deliberately
bad body. Mutations happen before `sendto`, so the OS creates a normal UDP checksum
over the already damaged application payload. This tests application integrity
without depending on a NIC accepting a bad UDP checksum. None of the tools disable
UDP checksum handling or transmit actuator commands.

## Demonstrate stream gaps, reordering and freshness

Stop the normal publisher first. Start a listener for the synthetic fault device:

```sh
python Rust/tools/message-demo/python/listener.py --format protobuf --crc on \
  --interface 127.0.0.1 --device fault-01 --duration 10 --log stream-faults.jsonl
```

Then run this finite publisher:

```sh
python Rust/tools/message-demo/python/stream_faults.py --format protobuf --crc on \
  --interface 127.0.0.1 --log fault-publisher.jsonl
```

It sends both streams. Status sequence `0, 2, 1, 2, 4` demonstrates a provisional
gap, reordering, a duplicate and another gap. It then sends a damaged CRC and a
malformed body with a good CRC, pauses four seconds so both streams go stale,
resumes with valid data, and changes boot ID. Pending gaps are finalized at the
boot transition. Sequence values and timing are intentional test traffic, not a
throughput baseline or a model of the real network's loss rate. The publisher
requires explicit interface selection and sends no commands or ACKs.

## Board build and physical procedure

From `Rust/`:

```sh
cargo +stable build --locked --release --no-default-features \
  --features messaging-protobuf,messaging-crc \
  --bin nucleo-h723zg-native-rmii-messaging
```

On the configured Windows bench:

```powershell
.\tools\build-signed.ps1 -Messaging protobuf -Crc -Version 0.1.0
.\tools\flash.ps1 -Messaging protobuf -Crc -Version 0.1.0
```

The artifact is `firmware-messaging-protobuf-crc-signed.bin`. Optional profiling
and rollback suffixes follow `-crc`, for example `-crc-profiling-signed.bin`.
Omitting `-Crc` retains the existing Phase 4 filenames. The factory-flash command
still **mass-erases internal flash**; use the established board/key setup. The
[Phase 4 trial/rollback procedure](PHASE4.md#mcuboot-checkpoint-and-rollback) still
applies, with the CRC-enabled artifact selected explicitly.

Use the board's observed IPv4 address and each host's actual NIC address in the
client/listener/fault commands. Repeat all six configurations with two physical
listeners. Verify CRC rejection count changes over RTT and the aggregate rejected
count in health, then valid commands/publications afterward. Run malformed,
invalid, wrong-version and oversized cases too. Repeat reconnect/reset, signed
boot and stack high-water checks from Phase 4. Preserve logs and packet captures.

The MCU has the same 10,240-byte fixed application payload storage. Framing
appends the trailer in place and borrows the verified body slice on receive.
Nothing is decoded from a truncated socket read; the receive error is rejected.
Very large datagrams can be dropped below the application before any counter is
updated. Physical CRC behavior and recovery remain **unverified** here.

## Local evidence and CI

```sh
python Rust/tools/message-demo/verify.py
python Rust/tools/message-demo/verify_board.py
```

The portable gate passes eight Rust tests and 27 Python test groups. Coverage
includes published CRC vectors, exact body preservation, cross-language fixtures,
body/trailer mutation, size and rejection precedence, all six command/multicast
configurations, valid traffic after faults, CRC-protected error replies,
duplicate/late response correlation, stale/recovered behavior during bad CRC
traffic, and both fault command-line tools. Existing bounded logging and
sequence-history tests still pass. Host multicast execution is Linux loopback;
other desktop OS and physical multicast results remain separate.

All six ARM images pass build, Clippy and disposable-key signing/verification.
The no-allocator ARM link now keeps both CRC modes and all codecs reachable.
The board gate verifies one codec feature per image, and builds CRC-enabled
profiling/rollback variants. CI runs these same gates and retains PowerShell
syntax checking. No dependency revisions for Embassy or the network stack changed.

Local Rust 1.98.1 release measurements for this Phase 5 tree:

| Encoding | CRC | Signed image | Static RAM |
|---|---|---:|---:|
| CSV | Off | 116,784 B | 28,368 B |
| CSV | On | 118,032 B | 28,368 B |
| JSON | Off | 120,144 B | 28,368 B |
| JSON | On | 121,368 B | 28,368 B |
| Protobuf | Off | 78,544 B | 28,368 B |
| Protobuf | On | 79,792 B | 28,368 B |

All fit the 262,144-byte signed-image limit. Static RAM includes task/DMA/network
storage and RTT; runtime stack high-water is still pending. Reports and separate
ELFs are written under `Rust/artifacts/messaging/` with `csv-off`, `csv-on`, etc.
CI uploads resource reports, not private keys or flashable images. These numbers
are build evidence only; Phase 6 owns the controlled runtime comparison.
