# Messaging Benchmark: Phased Implementation Plan

Date: 2026-10-04 HST (2026-10-05 UTC)  
Status: B1 contract and reference checks delivered for review; B2–B5 planned.
Repository baseline: `0ea9a631917e0809ec11619115a18e3f71a301c0` (Phase 6 merged).

This is the follow-on to the [original messaging plan](PLAN.md). It covers the
actual CSV/JSON/Protobuf application, including command/response and multicast
status/health. The separate [UDP echo benchmark](../../BENCHMARK_PLAN.md) remains
the transport baseline.

## Goal and current evidence

Build a repeatable benchmark that varies message size and frequency, monitors
latency and delivery, finds overload behavior, and supports long-running soaks.
Preserve existing commands and normal firmware behavior. Add opt-in,
benchmark-specific messages and publisher configuration where necessary.

Alex reported successfully loading and running the program on a NUCLEO. Treat
this as an initial hardware smoke-test report. The exact image/configuration and
measurements still need to be recorded; this does not close all physical gates
listed in the [Phase 6 checkpoint](CHECKPOINT.md).

Reuse the [Phase 6 tools and measurement rules](PHASE6.md) and
[comparison evidence](COMPARISON.md). Extend or factor existing functionality
before creating parallel tools. The current messaging benchmark has fixed message
shapes, one outstanding command, and compile-time board publication rates. The
existing raw-UDP tool has size/rate sweeps and soak modes, but it does not measure
the real messaging codec/application path.

## Controls and scope

| Parameter | Meaning |
|---|---|
| Encoding and CRC | CSV, JSON, or Protobuf; CRC off/on; explicit per run with no automatic detection |
| Synthetic payload bytes | Equivalent application data across formats; actual encoded body/datagram sizes reported separately |
| Command rate | Requested benchmark probes per second |
| Outstanding-request limit | Bounded concurrency, independent of offered rate |
| Status and health rates | Independently configured multicast publication frequencies |
| Warm-up and duration | Exclude startup effects; support short runs and multi-hour soaks |
| Response deadline | Separate timely replies, late replies, and no valid reply observed by the deadline |
| Reporting interval | Live terminal summaries and incremental CSV/JSONL output |
| Acceptance thresholds | Configurable limits for latency, delivery, freshness, and achieved rate |

Propose synthetic payloads of **0–512 bytes**, subject to B1 worst-case encoding
and memory checks. Retain the existing 1,024-byte maximum application datagram
initially, including the CRC trailer when enabled. Reject oversized combinations
before traffic begins. B1 will freeze rate, concurrency, lease and payload limits;
these are experimental controls, not production requirements.

Out of scope: actuation, production authentication/control, automatic flashing,
automatic reboot to conceal failures, synchronized-clock infrastructure,
unbounded traffic generation, a GUI, and an automatic production format choice.

## Stages and progress

Aim for one reviewable PR per stage; split a stage if its scope warrants it.
Implementation stages can proceed without a board, but physical gates stay open
until real results are available.

| Stage | Deliverable | Exit criteria | Status |
|---|---|---|---|
| B1 | Benchmark contract and resource budget | Equivalent messages, limits, compatibility rules and fixed-memory/socket budget | Delivered for review; runtime validation pending |
| B2 | Host load generator and live metrics | Fault-controlled host tests prove pacing, correlation, bounded tracking and correct metrics | Planned |
| B3 | NUCLEO benchmark support | All six format/CRC configurations build and interoperate; physical operation recorded separately | Planned |
| B4 | Stress and soak orchestration | Sweeps, long-run reporting, thresholds, recovery checks and interruption handling verified | Planned |
| B5 | Physical characterization and report | Reproducible board results establish a tested operating range and disclose unresolved failures | Planned |

### B1: Benchmark contract and resource budget

- Define benchmark probe/response messages, deterministic synthetic payloads,
  session and sequence identity, and publisher configuration/control responses.
- Preserve existing command semantics and default publication behavior. Decide
  how benchmark extensions are identified, feature-gated and versioned, including
  behavior with older firmware. Keep `.proto` authoritative; do not add YAML.
- Define precise payload-size semantics. Equal application data is the primary
  cross-format comparison; never pretend equal data implies equal wire lengths.
- Define configuration validation, one controlling session, lease renewal,
  explicit stop, expiry, and restoration of defaults. Specify which counters
  describe local application actions versus received observations.
- Check worst-case encoded sizes, generated string capacities, CSV cell storage,
  task storage, stack needs and socket counts before freezing limits.
- Provide reviewed normal, boundary and invalid fixtures for all three formats.

Exit evidence: contract, resource table, fixtures, and explicit compatibility and
state-transition rules. No claims of performance from design limits alone.

B1 deliverables: [contract](BENCHMARK_CONTRACT.md),
[resource budget and reference size measurements](BENCHMARK_RESOURCES.md), and
[schema/fixtures/checker](../../protocol/benchmark/). The checker covers 53 valid
fixtures across six configurations and 15 semantic rejection cases. Lease state
scenarios are specifications, not executed runtime tests. See the current
[checkpoint](CHECKPOINT.md) for verification and remaining gates.

### B2: Host load generator and live metrics

- Add paced probes with multiple outstanding requests, bounded tracking and
  strong run/request correlation. Keep the ordinary read-command client usable.
- Record requested versus achieved send rate, skipped host send slots and window
  saturation. Do not silently throttle and call the requested load achieved.
- Use monotonic clocks. Define whether RTT includes encoding and decoding, and
  preserve that definition across formats and reports.
- Monitor multicast sequences, duplicates, reordering, gaps and freshness while
  command traffic runs. Set stale thresholds explicitly for the selected rates.
- Report interval and cumulative counters, rates and latency distributions.
  Use bounded histograms with documented precision and overflow accounting;
  merge histograms rather than averaging interval percentiles.
- Save metadata and results incrementally. Make per-packet capture optional and
  identify its overhead. Check existing echo/Phase 6 tools for unbounded sample
  retention before reusing them for long soaks.

Exit evidence: host responder tests deliberately inject delay, missing replies,
duplicates, reordering, corruption and scheduling pressure. Tests verify exact
accounting, deadline boundaries, bounded memory and incomplete-run reporting.

### B3: NUCLEO benchmark support

- Add an opt-in firmware benchmark feature and bounded synthetic payload handling
  through the actual application codec path.
- Support validated run configuration for payloads and independent publication
  rates. A rejected request must leave the active configuration unchanged.
- Allow one controlling benchmark session with a bounded lease. Explicit stop
  restores defaults; expiry also restores them after a host crash/disconnection.
  Specify whether reconnecting requires a new run to prevent accidental load
  resumption. This mechanism is not authentication; use an isolated bench network.
- Keep command processing, publication and lease expiry independently responsive
  under load. Avoid catch-up bursts and per-packet firmware logging.
- Expose useful application counters and boot/session changes. Integrate optional
  existing profiling without introducing profiler traffic into baseline runs.
- Build and check all six configurations against signed-image and resource
  limits. Preserve signing, boot-confirmation and recovery behavior.

Exit evidence: host/ARM checks, firmware size and memory budgets, interoperability,
and lease/configuration failure tests. Physical probe, stream and lease-expiry
checks require the board and are recorded separately from build success.

### B4: Stress and soak orchestration

**Stress:** Sweep selected payload sizes and progressively increase rates with
explicit dwell times and maximum load. Support command-only, stream-only, and
combined workloads to isolate bottlenecks. After each high-load stage, return to
a low baseline and verify recovery. Distinguish host-limited runs from evidence
of board saturation. Configure stop thresholds; do not reboot away a failure.

**Soak:** Hold a selected workload continuously. Start with a short smoke run,
then one hour, then eight hours; longer runs remain configurable. Record interval
and cumulative results, stale intervals, boot changes and resource trends without
growing in-memory histories for the full run. Reporting intervals must not add
traffic pauses or repeatedly restart the workload.

On completion, interruption or error, retain partial results and the stopping
reason. Attempt normal cleanup; lease expiry is the fallback. Separate
pass/fail, invalid evidence and incomplete runs. Keep acceptance thresholds in
the run manifest so a reader can reproduce the judgment.

Exit evidence: reproducible sweep/soak examples, bounded-state checks, controlled
failure/recovery tests, and successful re-analysis of saved interval data.

### B5: Physical characterization and report

- Record the exact signed image, source revision, compiler/flags, board clocks,
  interfaces, switch/IGMP setup, logging policy and all effective parameters.
- Begin with low-rate smoke tests and confirm equivalent workloads across all
  formats. Run the six-configuration comparison using a recorded order.
- Measure rate/size sweeps, identify the first threshold violation, and verify
  post-stress recovery. Report an observed operating range with chosen headroom,
  not an unsupported universal maximum.
- Run one-hour and eight-hour soaks at selected sustained workloads. Include two
  multicast listeners and a separate physical receiver where practical.
- Run optional profiling separately. Perform deliberate cable/reset/recovery
  exercises as labeled fault tests, not silently inside clean baseline runs.
- Retain manifests, summaries, histogram data, selected packet traces and failure
  evidence. Preserve host-versus-board and baseline-versus-instrumented labels.

Exit evidence: physical report, repeatable commands, disclosed limitations and
open gates. If bench access is unavailable, deliver the procedure and tools with
the physical results explicitly pending.

## Measurement interpretation

| Measurement | What it establishes |
|---|---|
| Command RTT p50/p95/p99/max | Application round-trip timing under the documented timer boundaries |
| Missing response | No valid correlated reply observed within the chosen accounting/deadline rules; not proof of network loss |
| Host-skipped slot | Requested send opportunity the host did not execute |
| Multicast gap | Missing sequence observation, provisional until the reorder/finalization rules apply |
| Duplicate/reordered/corrupt | Distinct observations; counters may overlap and must not be summed as unique lost packets |
| Inter-arrival timing/freshness | Receiver-observed timing; not multicast one-way latency without clock synchronization |
| Local send/skip/reject counters | Application actions, not all lower-layer drops or confirmed physical transmissions |
| Executor utilization | Existing profiler's task-polling measurement, not total MCU CPU utilization |
| Stack high-water | Measured usage for that instrumented run, not a proof of worst-case safe margin |

Keep late valid replies separate from timely successes and never double-count
duplicates. Define drain/finalization rules for requests sent near the end of a
window. Record histogram precision, sample counts and evidence-quality failures.
Do not claim precise tail guarantees from small samples or infer MCU codec cycles
from host timings.

## Execution and stopping point

Before each stage, read this plan and the current checkpoint, verify the merged
baseline, and select the stage's acceptance criteria. Afterward, update the
progress table with the PR/commit, verification performed and remaining gates.
Preserve unrelated repository changes and keep documentation readable for someone
learning Rust.

B1 began after plan PR #7 merged (`a90c7c7`). B2 has not started. Completion means a
reproducible benchmark toolset and physical evidence for the tested operating
range. Production requirements, final format/CRC policy and deployment acceptance
remain separate decisions.
