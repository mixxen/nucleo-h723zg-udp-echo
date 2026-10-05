# Messaging checkpoint

## Benchmark B1: contract and resource budget

Date: 2026-10-05 UTC. Base: merged benchmark plan PR #7, `a90c7c7`.
B1 delivered for review on `messaging/benchmark-b1`; B2–B5 are not implemented.
See the [benchmark plan](BENCHMARK_PLAN.md), [contract](BENCHMARK_CONTRACT.md)
and [resource budget](BENCHMARK_RESOURCES.md).

- Added an isolated benchmark Protobuf schema/profile with equivalent CSV/JSON
  mappings, 0–512-byte deterministic payloads, discovery, probes, configuration,
  lease renewal/stop, and multicast status/health.
- Specified boot/session/epoch correlation, ownership, idempotency, expiry,
  bounded rates/storage, socket allocation and future firmware resource gates.
- Reference checker passes 53 fixtures across all six format/CRC configurations,
  a manual three-format golden request, and 15 semantic rejection cases.
  Largest tested datagram is 884 bytes (JSON with CRC). Fifteen state scenarios
  are specified for later executable host/firmware tests.
- Existing Phase 1 schema/fixture checker passes. The B1 checker is wired into
  the portable verification entry point. The complete local gate could not run
  because this workspace has no Rust toolchain; CI must supply that evidence.
- Normal schema, generated Rust, firmware and existing runtime behavior are
  unchanged. No benchmark throughput, ARM memory or lease behavior is measured.

Alex reported successfully loading and running the program on a NUCLEO. This is
initial hardware smoke evidence; the image/configuration and measurements remain
unrecorded. Physical acceptance gates below remain open.

## Previous Phase 6 checkpoint (retained evidence)

Date: 2026-10-01 UTC. Base: merged Phase 5, `60e0b25` (PR #5).
Phase 5's Rust and Messaging prototype CI workflows passed before this work.

**Phase 6 implementation and host evidence delivered. Physical Phase 3–6 gates
remain pending.** See the [walkthrough](PHASE6.md) and [comparison report](COMPARISON.md).

## Delivered

- Linux runner for CSV/JSON/Protobuf with CRC off/on, two listener processes,
  warm-up and measured windows, paced read commands, shuffled configuration
  order, process cleanup, retained JSONL, source/binary/image hashes and manifests.
- Host mode starts a responder per configuration. Board mode observes one
  already-flashed configuration with explicit firmware metadata; no automated
  flashing, reset or device configuration.
- Offline CSV exports for command cohorts, RTT percentiles, sizes, stream rates,
  gaps/freshness, health-counter observations and every diagnostic outcome.
  Checks distinguish invalid/missing logs, request timeouts, and real successes.
- In-process Rust benchmark of the existing portable codec, excluding transport
  and JSONL I/O. Six configurations, 16 fixed fixtures, validation/conversion/CRC
  included; individual samples are batch means, not per-call latency percentiles.
- Retained compact traffic/codec exports, batch samples and provenance, plus the
  original Phase 5 MCU build resource report clearly labeled as inherited evidence.
- Dependency/source inventory and reproducible schema-change evidence. No YAML
  layer, new dependency, wire/schema change or firmware behavior change.

## Verification

The complete portable gate passed locally with Rust 1.90.0: generation checks,
formatting, host/ARM lint, eight Rust tests, all 32 Python groups (27 existing plus
five comparison groups), and an ARM link without an allocator. The comparison
suite includes actual six-configuration/two-listener captures and byte-identical
CSV re-analysis, fixture sizing against encoded Rust output, boundary/stale math,
cohort timeouts versus missing results, and clock/configuration/hash failures.
Final comparison checks cover empty re-exports clearing previous results.

The fixed-fixture host benchmark completed 1,728 timed batches. The six host
preview configurations each ran for ten warm-up seconds and 60 measured seconds:
360/360 commands succeeded; each listener observed 600 status and 60 health
publications per configuration, with no measured gaps, duplicates or stale time.
Saved-log integrity checks passed and CSV re-export reproduced the results; the
planned ten-minute native-Ethernet runs have not been performed. The runtime
codec and firmware were not modified, so no new local six-image firmware build
was needed in this phase. The existing CI board matrix remains enabled.

The host benchmark and capture identify their own pre-commit source snapshots.
Later comparison-only error/export handling changes do not change the measured
Rust codec or executable. Do not replace the retained original hashes with the
future commit SHA or imply the base commit alone contains the new runner.

## Remaining work: bench acceptance and selection

No board was attached or flashed. Complete all six physical command/multicast
runs, another-host/switch/IGMP/TTL/checksum observations, signed boot/trial
confirmation/rollback, hardware RNG, reset/cable/DHCP recovery, board fault
counters/recovery, MCU encode/decode timing and runtime stack high-water.

Keep baseline and profiling images/results separate. Executor utilization is
not total MCU utilization; host codec nanoseconds are not MCU cycles. Board TX
rate remains unavailable without additional sender evidence. Health deltas span
sample intervals and never prove a lower-layer loss cause.

Protobuf is the leading candidate for the next board trial from the currently
observed footprint and host codec cost. CSV remains close for small packet sizes.
No production protocol or CRC policy has been selected. The six-phase tooling
plan is implemented; final physical acceptance and the format decision require
the documented bench evidence.
