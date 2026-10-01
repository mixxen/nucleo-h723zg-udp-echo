# Phase 4 checkpoint

Date: 2026-10-01 UTC. Base: merged Phase 3, `4a431b8` (PR #3).
Phase 3's Messaging prototype and Rust CI workflows both passed before this work.

**Implementation and local verification complete. Physical Phase 3/4 gates pending.**

## Delivered

- Dedicated native-RMII messaging image with read commands and independent status
  and health publishers. Fixed buffers, short counter critical sections, explicit
  receive-loop yielding, bounded local sends, and queue clearing on observed
  link/configuration loss or local timeout.
- One CSV, JSON or Protobuf codec enabled per image. Default host builds still
  contain all three. Generated conversions honor those features reproducibly.
- Portable command acceptance/error/drop policy shared by the host and MCU,
  including the bounded error-reply budget and independent saturating health state.
- Hardware boot nonce, local codec/task/timer checkpoint, verified MCUboot trial
  confirmation, deliberate rollback-test variant, and optional existing profiling.
- Network supervisor recovery when a cable is removed during DHCP or an IPv4
  configuration disappears while link remains up. Application readiness checks
  use a 10 ms timer so tasks do not contend for the pinned stack's single state
  wake slot; optional profiling uses this approach at startup too.
- `-Messaging csv|json|protobuf` signing/factory-flash selectors; separate artifact
  names for formats, profiling and rollback-test images.
- Paced Python command demo, profiling snapshot tool, firmware verification script,
  CI build/package/resource gates, and [complete Phase 4 walkthrough](PHASE4.md).

## Verification performed locally

- Portable `verify.py`: generation/reproducibility, formatting, host and ARM lint,
  link without an allocator, five Rust tests, and all 18 Python command/codec/
  multicast groups passed with Rust 1.90.0. Existing two-listener tests still pass
  in every format while commands continue.
- `verify_board.py`: all three release images built and passed Clippy with
  warnings denied on Rust 1.98.1. Cargo metadata confirms exactly one codec feature
  per image. Protobuf profiling and rollback variants built/linted; missing or
  multiple firmware formats were rejected.
- Disposable-key imgtool 2.4.0 signing and verification passed for every format.
  Signed bytes: CSV 116,712; JSON 120,224; Protobuf 78,712. All fit the 262,144-byte
  limit. Linked static RAM is 28,348 bytes in each baseline image. Section/symbol
  reports distinguish task/DMA/network storage from unmeasured runtime stack use.
- Existing native UDP echo release build and Protobuf smoke Clippy passed after
  the supervisor/feature changes. No upstream dependency revision changed.

PowerShell runtime and GitHub CI results are separate from local Linux checks.
The workflow retains PowerShell syntax checking and builds the existing smoke image.

## Pending physical gates

No board was attached or flashed. Signed factory/trial boot, actual MCU command
responses and two physical multicast receivers in each format, UDP checksum/TTL/
IGMP behavior, DHCP fallback and reconnect, hardware RNG, reset identities, trial
confirmation/rollback, and stack high-water still require bench evidence.

The [Phase 4 acceptance checklist](PHASE4.md#physical-acceptance-record) carries
those gates; [Phase 3's physical multicast checklist](SMOKE_BENCH.md) remains open.
Local queue draining is not receiver acknowledgment, and application counters do
not see packets dropped below the socket. Host multicast validation remains Linux
loopback only. No throughput or format recommendation follows from image sizes.

## Next: Phase 5, preserving the pending bench gates

Add fixed-per-run CRC-32C with independent vectors and corruption/failure demos.
Keep the exact existing wire body and bounded transport rules, verify CRC before
decoding, and extend shared host/board paths and diagnostics. CRC remains off in
this phase. Run the physical Phase 3/4 acceptance procedure when bench access is
available; continuing implementation does not waive it.
