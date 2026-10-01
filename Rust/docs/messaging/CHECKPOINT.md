# Phase 5 checkpoint

Date: 2026-10-01 UTC. Base: merged Phase 4, `deb811c` (PR #4).
Phase 4's Messaging prototype and Rust CI workflows passed before this work.

**Implementation and local verification complete. Physical Phase 3/4/5 gates pending.**

## Delivered

- Optional CRC-32C framing for all three formats across Rust host, Python
  client/listener, and full NUCLEO command/stream application. Exactly eight
  uppercase hex characters follow the unchanged encoded body; modes are fixed
  per run with no auto-detection or fallback.
- Size checks before CRC, CRC before decoding, and common decode/validation/
  version outcomes. No CRC error replies and no freshness/history updates from
  rejected publications. Existing correlation, schedules and bounds remain.
- Saturating CRC rejection diagnostics in receiver logs and rate-limited MCU RTT;
  CRC failures also contribute to the existing aggregate rejected count in health.
  The schema, CSV columns and CRC-off bodies are unchanged.
- Messaging images force nonblocking RTT to keep diagnostics from stalling tasks.
- `--crc off|on` host options, `messaging-crc` firmware feature and `-Crc` signing/
  factory-flash selectors with distinct artifacts.
- Finite request/stream fault tools, independent published CRC vectors,
  cross-language/framing/failure tests, and a six-configuration board package gate.
- [Phase 5 walkthrough](PHASE5.md) with host demos, precise diagnostic meaning,
  board procedure, local measurements, and pending acceptance work.

## Verification performed locally

- Rust 1.90.0 portable gates: reproducible generation, formatting, host/ARM lint,
  eight Rust tests, and ARM linking without an allocator while both CRC modes and
  all three codecs remain reachable.
- All 27 Python test groups passed: the earlier 18 codec/command/multicast groups
  plus nine CRC/fault groups. The two-listener command/restart test now runs all
  six format/CRC configurations. Fault tests verify receiver outcomes and valid
  traffic afterward, not silence alone.
- CRC tests check published vectors independently in Rust and Python, mutation of
  body/trailer bytes, boundary lengths, exact body preservation, malformed input
  with a valid CRC, semantic/version failures, CRC-protected error replies,
  delayed/duplicate response correlation, and stale/recovered stream behavior.
- Rust 1.98.1: all six ARM release images built and passed Clippy. Disposable-key
  imgtool 2.4.0 signing/verification passed; signed sizes range from 78,544 bytes
  (Protobuf/off) to 121,368 bytes (JSON/on), below 262,144 bytes. Static RAM is
  28,368 bytes per baseline image; runtime stack high-water is not measured.
- CRC-enabled profiling and rollback-test variants built/linted. Exact single
  codec selection and invalid multi-format rejection are still checked.

Rust adds pinned `crc` 3.3.0 and locked `crc-catalog` 2.5.0. Python CRC uses an
independent bitwise implementation. Embassy/network revisions are unchanged.
PowerShell execution and GitHub CI are separate from these local Linux checks.

## Remaining gates

No board was attached or flashed. Physical command/multicast delivery in six
configurations, CRC counter behavior, UDP checksum/TTL/IGMP observations, signed
boot/trial confirmation/rollback, hardware RNG, cable/DHCP recovery and stack
high-water remain pending. Existing Phase 3/4 acceptance checklists still apply.
Host multicast evidence is Linux loopback only; Windows/macOS remain unverified.

CRC is accidental-error detection, not authentication. Application counters do
not observe lower-layer drops. The separate CRC count is diagnostic, not a new
health wire field. RTT can discard records; host logging queues can drop records
while retaining cumulative counts. Fault tools' recovery success does not by
itself prove which rejection occurred at the receiver.

## Next: Phase 6

Create the controlled comparison runner, statistical exports, and walkthrough for
three encodings by two CRC modes. Measure like-for-like packet sizes, command
success/RTT, publication rates, gaps/freshness, encode/decode cost, flash/static RAM
and available stack high-water. Keep host versus MCU and instrumented versus
baseline results distinct. No format recommendation or physical success claim
follows from build sizes alone. Bench-dependent rows stay pending until real
measurements are supplied or a board becomes available.
