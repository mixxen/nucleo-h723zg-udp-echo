# Protobuf review and follow-up

Review date: 2026-10-02. Source baseline: `0ea9a63` plus the uncommitted
wire-type fix described below. This is a prototype review, not qualification.

## Architecture

`protocol/messaging.proto` defines the envelope and bodies. The descriptor and
`profile.json` generate bounded Rust models, micropb types, independent Python
Protobuf types, and reference documentation. CSV, JSON and Protobuf share
semantic validation and command policy; JSON is the prototype mapping, not
ProtoJSON. The portable Rust codec is `no_std` and requires no heap allocator.

Native-RMII firmware runs command, status and health tasks. Commands are
read-only device-info/status/health requests on UDP 42000. Status is synthetic
temperature, published at 10 Hz; health is published at 1 Hz. Both use multicast
239.255.42.1, ports 42001/42002 respectively. An optional CRC-32C trailer detects
accidental corruption, not authentication or replay. Local send completion is
not remote delivery; clients track timeouts, sequence gaps and freshness.

## Decoder correction

The micropb 0.6 generator matches known field numbers without checking their
wire types. Changing the golden request's first tag from `08` (varint) to `0A`
(length-delimited), with the remaining bytes unchanged, was accepted by Rust
but rejected by Python with `decode_error`.

`generate.py` now derives wire-type guards from the descriptor and adds them
to the generated decoder arms. Wrong-typed known fields reach the existing
unknown-field skip path, matching standard Protobuf handling. A well-formed
wrong-type field is not necessarily a malformed message: it must be skipped,
and the remaining message still undergoes semantic validation. Unknown fields,
duplicate scalars and oneof/message merge behavior remain supported.

There is no extra runtime parsing pass, dependency update or wire/schema change.
Generated code is not hand-edited. Generation fails if the expected upstream
decoder/arm shape changes. This intentionally narrow compatibility shim must be
reviewed when upgrading micropb; remove it if the upstream generator supplies
equivalent guards. Regressions exercise the original failure and wrong-type
fields throughout the envelope and each nested message type.

Generated text now uses canonical LF endings, with scoped `.gitattributes`
rules preserving them on checkout. This keeps byte-for-byte generation checks
consistent between Windows and Linux.

## Verification of the correction

Windows, Rust 1.90.0, Python virtual environment with the pinned protocol
requirements:

```powershell
& Rust/artifacts/board-test-venv/Scripts/python.exe Rust/tools/message-demo/verify.py
```

Generation/reproducibility, formatting, host/ARM lint and ARM no-allocator link
passed. Eight Rust unit tests and 26 Python test groups passed; seven
Linux-specific Python groups were skipped. The new wrong-wire-type group passed.

The same `verify.py` gate then passed in a disposable Linux Docker container
based on `rust:1.97.1`, with Rust 1.90.0 and the pinned Python dependencies:
eight Rust tests and all 33 Python groups passed, with no skips. This included
the six-configuration/two-listener loopback comparison and deterministic
re-analysis. A read-only source mount was copied into an isolated temporary Git
snapshot for the comparison's provenance checks; this was functional test
evidence, not a new physical-network performance report. Generation, formatting,
host/ARM lint and the ARM no-allocator link also passed there.

The full Protobuf/CRC-off firmware passed release build and lint with the
installed stable compiler. The Protobuf/CRC-on/profiling combination also passed
release lint. Commands below run from `Rust/`:

```powershell
cargo build --locked --release --no-default-features --features messaging-protobuf --bin nucleo-h723zg-native-rmii-messaging
cargo clippy --locked --release --no-default-features --features messaging-protobuf --bin nucleo-h723zg-native-rmii-messaging -- -D warnings
cargo clippy --locked --release --no-default-features --features messaging-protobuf,messaging-crc,profiling --bin nucleo-h723zg-native-rmii-messaging -- -D warnings
```

No post-fix board flash or physical-network retest is claimed here.

## Physical evidence and limits

The initial Windows/native-RMII Protobuf/CRC-off smoke test at 192.168.68.66
completed 30/30 commands. A roughly 20-second listener observed 201 status and
20 health publications without reported sequence gaps, duplicates or stale
events. Three additional commands succeeded after reset. Startup publication
skips were visible in cumulative health counters. These observations used the
pre-fix image; they are not a retest of the decoder correction.

Raw logs remain ignored development artifacts under `Rust/artifacts/messaging/`
with the `board-2026-10-02-protobuf-` prefix. This smoke test does not complete
the six-configuration, ten-minute physical comparison, another-host multicast,
fault injection, signed-trial rollback, cable/DHCP recovery or load acceptance.
The historical comparison tables are not regenerated by this correction.

## Next measurement work

1. Build and test the corrected image on the board, including malformed tags
   and recovery with a valid command. Preserve image/source hashes and raw logs.
2. Measure encode and decode cycles separately using DWT on a dedicated
   instrumented release image. Use fixed fixtures, warm-up, repeated batches,
   a recorded CPU clock and compiler flags, and no logging inside timed regions.
   Keep validation/conversion/framing included and state the exact boundaries.
   Reject failing operations rather than counting them as fast successes.
3. Use the existing `-Profiling` image and `python/profile.py` endpoint for
   executor busy time and painted stack high-water during sustained traffic.
   Preserve before/after snapshots and elapsed intervals. Neither metric is
   total MCU CPU utilization or isolated codec cost. Keep baseline and
   instrumented image results separate; do not silently replace the baseline.
4. Run the documented Linux physical comparison for all six format/CRC
   combinations with an isolated network and recorded switch/IGMP/firewall
   configuration. Retain receiver errors, command cohorts, health deltas,
   rates and provenance, not only successful RTTs.
5. Only then evaluate the encoder's model clone/conversion and approximately
   1 KB scratch buffer/copy. Consider encoding into the caller's buffer if
   measured cycles or stack margin justify it. Keep semantic and wire fixtures
   unchanged and benchmark before/after with the same setup.

See [Phase 6](PHASE6.md) for existing bench commands and
[comparison](COMPARISON.md) for historical results and measurement caveats.
