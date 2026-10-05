# B2 host load generator: implementation progress

Date: 2026-10-05 UTC. B1 merged as PR #8 (`60a7605`).

**B2 host implementation is delivered for review.** This document records the
measurement core merged in PR #9. The subsequent
[live integration and walkthrough](BENCHMARK_B2_LIVE.md) implements codecs,
controls, probes and listeners. Firmware integration remains B3 and has not started.

## Why split B2

B1 exposed a separate bounded-codec integration task: 512-byte payloads and
20-column CSV capabilities do not fit the normal adapter's cells. B2 therefore
has two review units:

1. **Measurement core (this increment):** independently testable pacing,
   correlation/accounting, bounded latency/sequence history, incremental reports.
2. **Codec and live runtime (now delivered for review):** generated benchmark Rust adapters,
   host responder/control state machine, concurrent probe generator and multicast
   receivers using this accounting core, then six-configuration integration tests.

This split keeps measurement semantics reviewable before networking and codec
work is added. Completion of this increment does not satisfy the full B2 exit gate.

## Delivered modules

Files live under `Rust/tools/message-demo/python/`:

- `load_metrics.py`: pacing, bounded probe tracking, latency histograms and
  multicast sequence/freshness tracking.
- `load_report.py`: append-only JSONL evidence, interval CSV, and streaming
  evidence-integrity inspection. Output directories must be new to avoid
  accidentally replacing earlier evidence.

The existing Phase 6 `comparison_stats.py` analyzes retained per-event logs and
uses exact sample percentiles; its whole-log/sample retention is unsuitable for
the planned long-running generator. The new module preserves those tools and
uses bounded state with a separately documented quantile definition. The normal
listener's `stream_state.py` likewise remains unchanged.

Run the independent tests from the repository root:

```sh
python Rust/tools/message-demo/test_load_metrics.py
```

The same tests are included in `verify.py`. No new Python dependency is required.

## Frozen accounting choices

| Topic | Behavior |
|---|---|
| Clock | Integer host monotonic nanoseconds; backwards time rejected |
| RTT | Caller timestamps immediately before send and after valid response decoding/pattern verification; request encoding excluded |
| Deadline | Reply must arrive strictly before `sent + deadline`; equality is late |
| Timely reply | First valid matching reply before deadline; contributes one RTT sample |
| Deadline miss | No valid reply before deadline; never decremented by a later reply |
| Late reply | First matching reply after timeout, while identity remains in recent history |
| Duplicate | Additional reply for a retained timely/late identity; never another success |
| Evicted/unknown ID | `outside_history_or_unsent`; never inferred to be a fresh success |
| Correlation | Exact peer IP/port, device, boot, session, epoch, request ID and response kind |
| Pending probes | 1–4,096; nonwrapping, strictly increasing u32 request IDs |
| Recent replies/timeouts | At most 4,096 IDs in completion/timeout insertion order; duplicates do not extend retention |
| Pacing | Integer 0–10,000 Hz, absolute rational schedule; zero disables |
| Scheduling pressure | At most the newest due slot is offered; older due slots counted as scheduling skips |
| Window pressure | Newest due slot counted as a window skip if unavailable; no catch-up burst |
| Send opportunities | Attempt opportunities, not successful sends; runtime must separately record local send failures |
| Multicast gaps | Provisional until they leave the reorder window or the listener finalizes |
| Stream history | At most 4,096 sequence positions in a bitmap; large jumps use bounded work/storage |
| Sequence wrap | Modular u32 ordering; exact half-range difference is ambiguous and rejected |
| Freshness | Time since last forward progress; duplicates/reordered packets do not make old data fresh |
| Stream boundaries | First observed sequence is a baseline; no inferred gaps before first or after last observed sequence |
| Completion | Normal probe completion requires deadline drain; interrupted outstanding requests become unresolved, not invented timeouts |

The live decoder must establish structural validity and payload correctness
before calling the tracker with `valid=True`. `RunIdentity` is a correlation key,
not an authentication mechanism. Controls have their own ID namespace and never
enter probe RTT samples. Stream identity includes its publication source port.

## Histogram and report interpretation

Histograms retain a fixed array of geometric bins, not all RTT samples. Above
1 microsecond, each reported nearest-rank quantile is an upper bound with at most
1% plus 1 ns binning error. Positive values below 1 microsecond share a 1 us bin;
zero has its own bin. Min, max, count and sum are exact. Values above 60 seconds
increment overflow. A quantile whose rank falls in overflow is unknown, not
clamped. Empty histograms also return unknown quantiles.

Merge bucket counts to combine distributions. Do not average p99 values.
The manifest records all bin edges; snapshots store nonempty bin counts. Interval
histograms cover replies observed in that reporting interval, and may include a
probe sent in the preceding interval. Cumulative counts reconcile the whole run;
interval send/reply counts are not request cohorts and must not be divided to
claim interval delivery probability.

Each report interval writes cumulative and interval probe counts, current
outstanding/high-water state, interval/cumulative histograms, cumulative pacing
counters and stream observations. Rates use actual elapsed interval time. CSV is
an interval convenience export; JSONL retains the full evidence. Reporting resets
only the interval histogram, not outstanding probes, cumulative state or schedules.

A final record includes an explicit completion flag and stopping reason.
Completion is not threshold acceptance (`acceptance=not_evaluated`). Missing final
records and truncated final physical lines are incomplete evidence. Complete
malformed JSON records fail inspection. Files are flushed per record, but this
does not promise fsync-level power-loss durability. Failure between JSONL and CSV
writes can leave the CSV behind; JSONL is the authoritative evidence stream.

## Verification and remaining B2 gates

Sixteen deterministic tests cover known delay, missing/invalid/foreign replies,
exact deadline equality, late/duplicate replies, ID exhaustion, window pressure,
clock rollback, rational pacing, bitmap accounting against an independent set
model, wrap/large jumps, stale recovery, histogram merging/error/overflow,
history bounds and interrupted output. These tests inject observations into the
accounting core. They do not demonstrate UDP fault injection or device behavior.
The complete portable verification gate also passes locally with Rust 1.90.0,
including existing regression tests, generation/lint and the ARM no-allocator link.

The following integration gates identified by the first increment are now covered
by the [live runtime and its tests](BENCHMARK_B2_LIVE.md). Physical behavior remains
unverified; stress/soak orchestration belongs to B4.

Original integration checklist:

- Generate and verify real bounded Rust benchmark codecs across all six
  format/CRC modes, including cross-language fixtures and strict wire rejection.
- Implement host capabilities/configure/renew/stop and the owner/lease state
  machine with all B1 state scenarios as executable tests.
- Connect paced probes, bounded receive batches, deadline drain, multicast
  listeners, report timers and live terminal summaries in one responsive runtime.
- Add parameter validation, effective rate/size metadata, source/binary/image
  provenance, warm-up isolation, health-counter and freshness-duration reporting.
- Run delay/drop/reorder/duplicate/corruption and receive-flood tests through
  actual sockets; verify scheduling fairness and that renewal remains responsive.
- Validate interruption cleanup and saved-report reanalysis with the integrated
  runtime. Stress/soak orchestration and thresholds remain B4, hardware results B5.
