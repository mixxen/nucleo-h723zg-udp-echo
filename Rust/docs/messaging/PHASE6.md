# Phase 6: Controlled comparison and walkthrough

Phase 6 adds a Linux capture runner, offline statistics, a fixed-fixture Rust codec
benchmark, and the [comparison report](COMPARISON.md). The report separates local
host evidence, earlier MCU build evidence, and pending physical measurements.
There is no board attached to this development environment.

## Start with the host matrix

Run from the repository root. Use the Python environment installed for earlier
phases (`Rust/protocol/requirements-phase1.txt`). No new dependencies are needed.
The example target below is for an x86-64 Linux host; use the host triple printed
by `rustc +1.90.0 -vV` on another supported Linux machine.

```sh
cargo +1.90.0 build --locked --release \
  --manifest-path Rust/tools/message-demo/Cargo.toml \
  --target x86_64-unknown-linux-gnu

python Rust/tools/message-demo/python/compare.py run \
  --mode host \
  --binary Rust/tools/message-demo/target/x86_64-unknown-linux-gnu/release/message-demo \
  --interface 127.0.0.1 \
  --output Rust/artifacts/comparison-host \
  --notes 'Linux loopback; Rust 1.90.0, x86_64-unknown-linux-gnu, default release; normal file logging'
```

This takes about 61 minutes: six configurations, each with ten seconds of warm-up
and 600 measured seconds. Status is 10 Hz, health 1 Hz, and one read-only command
is scheduled each second, rotating info/status/health. The command timeout is one
second; there is one outstanding request and no retries. Two separate Python
listener processes run on the capture host. Host mode starts/stops its own Rust
responder for each configuration and never contacts a board.

Configuration order is shuffled with the recorded seed (default 723). For a short
functional run, add `--warmup 2 --duration 10`. The checked-in local report uses
60 measured seconds per configuration and is explicitly a preview. For repeated
measurements, use a new output directory and seed each time; preserve order and
compare repetitions rather than treating thousands of packets as independent
experimental runs.

Output directories must be new. Only the runner's child processes are stopped.
SIGINT stops listeners cleanly and finalizes their full-capture gap logs. Command
scheduling skips old slots after a delay, instead of sending a catch-up burst.
Unattempted slots remain visible even if a final timeout consumes the remaining
measurement window. A failed request does not prevent later configurations from
being captured; a child-process or local I/O failure aborts and leaves an
incomplete manifest. Inspect that manifest and stderr files before rerunning.

The runner exits successfully when the capture is complete and internally valid,
all attempted commands succeed, and both listeners observe both streams. This is
not an automatic throughput or reliability acceptance threshold. Inspect rates,
unattempted slots, gaps, stale time, counters, and diagnostics in the CSVs.

## Saved evidence and exports

Each configuration has a manifest, client/responder/listener JSONL files, stderr
files, and `summary.json`. The parent has `matrix.json` and these CSV exports:

| File | Meaning |
|---|---|
| `summary.csv` | Attempted/planned commands, unattempted slots, success, timeouts, RTT p50/p95/p99/max, and evidence quality |
| `commands.csv` | Each measured request and its terminal outcome, RTT, request/response datagram lengths |
| `streams.csv` | Per listener and stream: forward/reordered/duplicate/late observations, rate, gaps, freshness, and host logged send count |
| `sizes.csv` | Live body/datagram min, mean and max by role and message kind |
| `health.csv` | First/last received health counters within the window, same-boot deltas, observed interval and saturation |
| `diagnostics.csv` | Every observed outcome count per role during the window |

Regenerate exports without sending any network traffic:

```sh
python Rust/tools/message-demo/python/compare.py analyze Rust/artifacts/comparison-host
```

Raw logs remain authoritative. Archive the whole output directory when keeping a
bench result; CSVs alone cannot recreate every observation. Artifacts are ignored
by Git. The compact checked-in report retains exports and provenance, while the
full local capture is a development artifact. SHA-256 checks detect edited or
missing log content; malformed/truncated JSON fails analysis. A missing terminal
request event is an evidence error, not a timeout. Nonzero responder log-drop
counts invalidate the capture. Queue drops occurring after the last recorded
counter cannot be ruled out by logs alone.

Manifests record the exact Git revision, dirty state, hashes of relevant source
files, schema/profile hashes, executable/image hash, Python/platform/clock,
configuration, notes, process run IDs in the linked logs, and capture boundaries.
Source hashes identify a pre-commit working tree without pretending its base
revision contains the changes. The runner cannot infer a binary's compiler flags:
record the actual build command/toolchain in `--notes`. A board image hash records
the file supplied, not independent proof that those bytes are running on a board.
Record flashing and boot evidence alongside it.

## Measurement definitions

- Windows are half-open: start included, end excluded. Requests belong to the
  window in which their `sent_request` event occurs; their matching terminal
  result may occur afterward. Warm-up requests do not enter measured RTTs.
- RTT uses Python monotonic time from immediately before `sendto` through valid
  decoding/correlation. Request encoding precedes the timer. RTT includes Python
  response validation/CRC, host scheduling,
  synchronous request logging, transport, and responder work. It is not pure
  network latency or MCU processing time. Python uses a bitwise CRC while Rust
  uses a table implementation, so CRC-on RTT differences include host client cost.
- Successful RTTs alone enter percentiles. Timeouts, error replies and missing
  results are separate. Percentiles interpolate sorted samples at `(n-1)*p`;
  small-sample p99 values are descriptive, not tail guarantees. No one-way latency
  is inferred from unsynchronized host and board clocks.
- Python processes share the capture host's monotonic clock. Rust `Instant` has
  its own origin, mapped once through a UTC timestamp to the runner clock with
  approximately one millisecond timestamp quantization. Subsequent Rust event
  selection uses monotonic elapsed time. UTC/monotonic drift over 100 ms flags an
  invalid capture. Events close to the boundary can fall on opposite sides;
  subtracting send/receive counts is not a proven loss measurement.
- Stream receive rate counts forward publications plus received reordered gap
  fills. Duplicates and late packets are separate. Log outcome counts cannot be
  summed as datagram counts: one packet can generate multiple diagnostic events.
- `gap_final_observed` counts gaps finalized during the window, which can have
  begun during warm-up. `gap_unresolved_at_end` is pending state at the boundary,
  before any later arrival or shutdown finalization. Neither establishes a
  network cause; skips, buffering, logging, and scheduling matter too.
- Stale and not-seen time are clipped to the window, including a state carried in
  from warm-up. Only forward progress refreshes freshness. A never-seen stream
  reports not-seen time, not zero stale time as evidence of healthy reception.
- Host send rate comes from successful local `sendto` records, not link-layer
  transmission confirmation. Board send rate is blank because no equivalent
  per-publication sender log is captured. Receive rate and same-boot health
  counter observations are available; do not label these as actual board TX rate.
- Health deltas span the first/last health samples **inside** the window, not the
  exact full window. They never subtract across boot IDs. Saturated counters are
  marked; their delta undercounts further activity. An unexpected decrease yields
  no delta. Compare observed intervals before comparing totals.
- Body bytes are the encoded application envelope. Datagram bytes add eight
  ASCII CRC characters in CRC-on mode. Neither includes UDP/IP/Ethernet headers.
  Dynamic uptime/counters/sequence widths affect live sizes; fixed-fixture sizes
  below provide the like-for-like comparison.

## Fixed-fixture codec benchmark

Stop traffic captures and avoid other workloads while timing. Use a release build:

```sh
python Rust/tools/message-demo/python/benchmark.py \
  --binary Rust/tools/message-demo/target/x86_64-unknown-linux-gnu/release/message-demo \
  --output Rust/artifacts/comparison-codec \
  --iterations 10000 --batches 9 \
  --build-description 'rustc 1.90.0; x86_64-unknown-linux-gnu; default Cargo release, opt-level 3, no LTO; no RUSTFLAGS'
```

The 16 reviewed semantic fixtures include all message kinds, zero/valid/
unavailable/fault status, and escaping. Each runs in all six configurations;
device-info's encoding field is adapted to match the selected format. Exact
inputs, shuffled order, batch samples, source/executable hashes, `codec.csv`, and
`fixture_sizes.csv` are saved. Timing cannot establish general performance beyond
these bounded message shapes and this machine.

The new Rust `bench` command loads each fixture before timing and warms both
paths. It calls the existing portable `framing::encode` and `framing::decode`
functions repeatedly into fixed buffers. `std::hint::black_box` prevents the
compiler from replacing repeated known inputs with constants. Timing includes
validation, normalization, bounded copies/conversions and CRC where enabled.
JSONL parsing, transport and output happen outside timed loops. Operation order
alternates across batches. The harness checks the fixture round-trip before
starting; an invalid fixture fails the benchmark.

Each sample is elapsed batch time divided by iterations. Reported p50/p95 values
are percentiles of **batch means**, not per-call latency percentiles. The host
build contains all codecs and uses a different compiler/profile/architecture
from the size-optimized single-codec MCU images. Do not convert host nanoseconds
into MCU cycles or use these timings as MCU deadline evidence.

`inventory.csv` lists pinned direct dependencies and source physical line counts
including comments/blanks. Generated files are separated. Shared validation,
model conversion and infrastructure cannot fairly be charged to one encoding.
The earlier `check_generation.py` gate still verifies an isolated enum addition
regenerates Rust/Python Protobuf, metadata and reference docs reproducibly. This
is evidence of the change path, not a timed human-effort study or proof that every
new field type is supported.

## Native Ethernet bench run

Complete the physical checks in [Phase 4](PHASE4.md) and the corruption/recovery
checks in [Phase 5](PHASE5.md). Use an isolated bench network, unique device/MAC
identity, a recorded switch/IGMP setup and interface/firewall configuration, and
consistent board clocks and compiler flags. Keep any packet capture or profiling
policy identical across compared runs. There must be only one intended producer
for the selected device and stream ports.

Build/sign/flash each single-encoding image using the existing Phase 4/5 commands.
The comparison runner deliberately does not automate the mass-erasing factory
flash path. For each already-running image, record boot/confirmation evidence,
then run on a Linux capture host connected to the board's physical network:

```sh
python Rust/tools/message-demo/python/compare.py run \
  --mode board --format protobuf --crc on \
  --host 192.168.1.50 --interface 192.168.1.20 \
  --firmware-revision FULL_SOURCE_COMMIT \
  --firmware-image Rust/artifacts/firmware-messaging-protobuf-crc-signed.bin \
  --output Rust/artifacts/comparison-board-protobuf-on \
  --notes 'NUCLEO-H723ZG native RMII, 400 MHz; record compiler/flags, switch, IGMP, firewall, interface, boot and logging evidence here'
```

Replace example addresses and metadata. Board mode requires one explicit format
and CRC mode, revision and existing image file. Repeat for CSV/JSON/Protobuf with
CRC off/on in a recorded randomized order. Each invocation defaults to ten
warm-up seconds plus ten measured minutes. Two listeners run on the capture host;
use another physical host for the separate cross-machine multicast acceptance
check. The runner's own test on loopback cannot satisfy that gate.

Keep normal baseline images separate from profiling images. For a separate
instrumented run, use `-Profiling`, supply that image hash, and add `--instrumented`.
Save a profiling reset/snapshot with `python/profile.py` before/after the run and
record their actual interval. The endpoint reports executor busy time and painted
stack high-water, not total MCU utilization or per-codec encode/decode cycles.
The runner does not poll it, so baseline measurements incur no profiler traffic.
Direct MCU codec timing remains pending; do not fill it with host timings.

For fresh flash/static-RAM evidence, run `verify_board.py` with the recorded stable
compiler and retain `Rust/artifacts/messaging/resources.json` plus section/symbol
reports. It builds/signs/verifies all six images with a disposable key and also
checks profiling/rollback variants; it does not execute them. Runtime stack
high-water and reliable operating margin require the actual board run.

## Verification and completion

`verify.py` now includes five comparison test groups: statistics/boundaries,
request cohorts and log integrity, clock/configuration mismatch detection,
fixed-fixture benchmark sizing, and the real six-configuration/two-listener runner
with deterministic re-analysis. CI uses short functional captures, not timing
thresholds. Existing codec, CRC, fault, multicast and ARM no-allocator gates remain.

Phase 6 tooling and the local report are delivered. Physical six-configuration
measurements, signed boot/recovery checks, MCU encode/decode cost, observed board
TX rates, and runtime stack high-water remain pending. A final format decision is
not implied by finishing the tooling or by a smaller build alone.
