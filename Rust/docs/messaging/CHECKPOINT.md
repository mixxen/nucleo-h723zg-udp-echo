# Phase 2 checkpoint

Date: 2026-09-30. Base: merged Phase 1, `338a1cb` (PR #1).

## Delivered

- Reproducible `.proto` descriptor generation for micropb, bounded Rust models,
  CSV mappings, Python Protobuf/metadata, and the [message reference](REFERENCE.md).
- A portable `no_std` codec crate with CSV, custom prototype JSON, Protobuf,
  fixed capacities, presence preservation, and shared semantic validation.
- Rust host responder and Python client for device-info, status, and health reads.
  Synthetic samples can be valid, unavailable, or faulty.
- Request correlation, bounded completed/expired history, one outstanding request,
  no retries, saturating health counters, and rate-limited error responses.
- Human terminal output and JSONL diagnostics with capped rejected-byte samples.
- [Runnable walkthrough](PHASE2.md), automated verification, and a dedicated CI job.

## Verification performed locally

`python Rust/tools/message-demo/verify.py` passed with Rust 1.90.0, Python 3.12,
grpcio-tools 1.76.0, protobuf 6.33.6, and micropb/micropb-gen 0.6.0:

- 18 authored positive fixtures across all three formats, Python-to-Rust and
  Rust-to-Python; independent golden request bytes match both encoders/decoders.
- All 20 authored negative cases rejected by both encoders; receiver checks also
  exercise each representable wire case. CSV cannot represent conflicting body
  tags independently of its kind. A capacity overflow is a `decode_error`, now
  clarified in the fixture.
- Malformed quoting/JSON/Protobuf, wrong types/versions, null, duplicate and unknown
  JSON keys, size bounds, missing fields, escaping, binary32 boundaries, absent
  versus zero, and nonfinite measurements.
- Standard Protobuf unknown fields, duplicate scalar replacement, submessage
  merging, and oneof replacement agree with Python's Protobuf implementation.
- Live loopback exchanges for all three commands/formats; unavailable samples;
  counters, recovery after rejection, wrong target/version, no response loops,
  unsupported kinds, and error-reply limiting.
- Client timeout, late/duplicate and unmatched replies, with bounded history.
- 11 test groups passed. Generation drift and isolated schema-change
  reproducibility passed. Formatting and host/ARM Clippy passed.
- A release ELF links for `thumbv7em-none-eabihf` with all three codecs reachable,
  `no_std`, and no global allocator. This checks more than type-checking alone.

## Remaining gates

No NUCLEO is connected here. No physical Ethernet, multicast, DHCP recovery,
signed image, stack high-water, or MCU performance result is claimed. The link
probe is not a flashable application. Existing firmware and signing behavior
are unchanged. GitHub CI results are separate from the local checks above.

CRC remains off; the Phase 1 CRC rejection expectation waits for Phase 5. The
host service is synchronous for this command-only phase; Phase 3 must separate
publication schedules from request arrival and add two-listener/staleness checks.

## Next: Phase 3

Add independent multicast status/health publishers and a Python listener using
the existing codecs. Track per-stream sequences, duplicates/reordering,
provisional gaps, boot changes and stale/recovered state. Demonstrate two
listeners while commands remain responsive, then perform the physical NUCLEO
multicast smoke test when bench access is available. Record that hardware gate
as pending until actually tested.
