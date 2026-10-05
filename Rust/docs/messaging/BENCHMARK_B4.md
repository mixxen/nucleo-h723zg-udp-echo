# B4: stress, soak and acceptance reporting

B4 adds host orchestration on top of the B2 runner and merged B3 firmware
(PR #11, `ed72ae8`). It does not change the wire protocol, firmware, signing or
flash procedure. Local synthetic/loopback checks are not physical performance
measurements. B5 board characterization remains open.

## Run a campaign

Build the host library and set `BENCHMARK_CODEC_LIB` using the [B2 instructions](BENCHMARK_B2_LIVE.md).
The checked-in examples use the Rust host responder on loopback:

```sh
python Rust/tools/message-demo/python/benchmark_live.py responder \
  --format protobuf --crc on --interface 127.0.0.1 --bind 127.0.0.1
```

In another terminal, from the repository root:

```sh
python Rust/tools/message-demo/python/benchmark_campaign.py run \
  Rust/tools/message-demo/examples/benchmark-stress.json \
  --output benchmark-results/stress-protobuf-on
python Rust/tools/message-demo/python/benchmark_campaign.py assess \
  benchmark-results/stress-protobuf-on
```

Use a new output directory each time. The examples contain **illustrative
thresholds**, not established NUCLEO limits. A failed host example can reveal host
pacing/FFI limits and must not be reported as board saturation. Repeat with
matching CSV/JSON/Protobuf and CRC settings on both ends for six-mode comparisons.
No automatic format detection, image flashing or rebooting is performed.

For a board run, edit the example's `base` object: set `target_kind` to `board`,
use the board and host NIC IPv4 addresses, and supply `image_sha256` for the exact
B3 benchmark image. See [B3](BENCHMARK_B3.md) for building/installing it. Preserve
build/boot logs and network configuration with the evidence. Normal board
commands and normal 10 Hz status / 1 Hz health continue during benchmark traffic;
the host responder does not reproduce that background load.

The JSON file configures experiments only. The `.proto` remains the authoritative
wire schema; this adds no YAML or additional message-definition layer.

## Stress behavior

`benchmark-stress.json` explicitly lists payload sizes and increasing rate steps.
It runs an initial recovery workload as a baseline, then a load/recovery pair for
each payload/rate combination. Every stage has a fresh leased session and epoch.
The example has 12 load steps, 12 recovery stages and one initial baseline.

| Workload | Configure in every rate step and recovery |
|---|---|
| Command only | Nonzero `command_hz`; both stream periods zero |
| Stream only | `command_hz: 0`; one or both stream periods nonzero |
| Combined | Nonzero command rate and both stream periods |

Stream rates are `1,000,000 / period_us`; zero disables a stream. Each rate
dimension must be nondecreasing through the sweep. Recovery rates/payload must
not exceed the first load. Use separate campaigns to compare different workload
types. The greatest listed rates are the maximum offered load; there is no
unbounded adaptive ramp. Existing B1 bounds remain in force.

With `stop_on_failure: true`, cumulative thresholds are checked at report
boundaries after `live_min_seconds`. A breach stops further probes, attempts
normal stop, records unresolved probes separately and proceeds to the recovery
workload. After recovery it stops the ramp, even when recovery passes. A failed
initial baseline or recovery always stops the campaign. Invalid/incomplete runs
also stop immediately, without pretending recovery succeeded.

`stop_on_failure: false` disables early threshold stops and permits later load
steps after failed loads, provided each recovery succeeds. Final acceptance still
applies. This option never bypasses a failed baseline/recovery or invalid evidence.
Checks occur at reporting boundaries and are not hard real-time safety limits.

## Continuous soak

```sh
python Rust/tools/message-demo/python/benchmark_campaign.py run \
  Rust/tools/message-demo/examples/benchmark-soak.json \
  --output benchmark-results/soak-smoke-protobuf-on
```

The example is a 60-second smoke run. After inspecting it, make separate config
copies with `duration_seconds` set to `3600` and then `28800` for one and eight
hours. Each invocation keeps **one configuration, one session and a continuously
renewed lease** for the whole soak. Reports do not restart the workload. Duration
can be configured up to seven days; preflight rejects command counts that would
exhaust the u32 probe ID space. Warm-up and deadline drain are additional time.

Ctrl-C and the campaign CLI's SIGTERM handler attempt stop and preserve incomplete
results. SIGKILL, power failure and unavailable storage cannot guarantee a final
record; lease expiry is the fallback. Output flush is not an fsync guarantee.
Do not resume a partial directory as though it were a continuous run.

## Thresholds and evidence meaning

Every resolved threshold is saved in the campaign and individual run manifests.
Acceptance uses these definitions:

| Threshold | Measurement |
|---|---|
| `max_p99_ms` | Upper-bound p99 of validated timely application RTTs; response decoding/FFI included |
| `max_deadline_fraction` | Deadline misses divided by sent probes; late replies remain deadline misses |
| `min_send_ratio` | Sent probes divided by requested Hz × actual measured seconds, excluding warm-up/drain |
| `max_stale_fraction` | Per-stream time stale **or never seen**, divided by that stream's observed duration |
| `max_gap_fraction` | Finalized missing sequence positions divided by missing + unique observed positions |
| `live_min_seconds` | Grace period before cumulative online acceptance checks begin |

Pending reordering gaps are not counted as finalized loss during a live run.
At normal completion, the tracker finalizes its retained positions. Neither
initial absence nor an unobservable trailing sequence is fabricated into a gap
count; stale/not-seen time covers missing forward progress. No useful command
RTT samples, or no samples from an enabled stream, cannot pass final acceptance.
Thresholds include small initial stream acquisition delays, so choose grace and
duration suitable for the slowest stream. Interval records remain available to
inspect short excursions that a cumulative threshold might dilute.

The report exposes scheduling skips, outstanding-window skips, encoding-boundary
skips and local send errors under `host_limits`. These explain why requested load
may not have been offered. Window pressure may also reflect slow board responses;
these counters do not identify a unique bottleneck or prove MCU saturation.
Boot-ID changes observed from the expected device/endpoint are counted, and a
changed boot between stages prevents recovery from being declared successful.
Boot observations are not authenticated reboot attestations.

| Outcome / exit code | Meaning |
|---|---|
| `pass` / 0 | Complete evidence, confirmed stop, and all applicable criteria met |
| `fail` / 1 | Threshold, cleanup or boot-continuity failure; includes deliberate threshold stops |
| `incomplete` / 2 | Interrupted run, missing/truncated final record or unfinished work |
| `invalid` / 3 | Malformed/inconsistent evidence, mismatched stage parameters or execution/storage error |

Invalid and incomplete evidence take precedence over a performance failure in the
campaign summary. A low-load recovery pass never erases the preceding failure.

## Saved reports and offline assessment

`campaign.jsonl` contains resolved configuration, orchestrator source hash,
stage summaries and the campaign outcome. Each `stage-NNN/` has the existing
`events.jsonl` and `intervals.csv`, including host/library provenance, parameters,
identities, cumulative/interval metrics and cleanup result. Interval health
snapshots preserve counter trends; raw per-packet retention is not enabled.

Assessment streams through the files, verifies child parameters against the
campaign, checks measurement/drain boundaries and reconciles probe/histogram
counts. It recomputes results rather than trusting cached stage outcomes. It is
an integrity check, not a signature or protection against deliberate forgery.

```sh
python Rust/tools/message-demo/python/benchmark_campaign.py assess \
  benchmark-results/stress-protobuf-on/stage-001/events.jsonl
python Rust/tools/message-demo/python/benchmark_campaign.py assess \
  benchmark-results/stress-protobuf-on --thresholds alternate-thresholds.json
```

The alternate file is a JSON object of threshold names/values. Omitted keys use
the documented code defaults, not the original policy. Output labels the override;
original evidence is untouched. An alternate policy cannot turn a prematurely
stopped run into proof that its planned duration passed.

At most 128 load steps (257 stages including recovery) are allowed. The campaign
stores only bounded stage outcomes; metrics retain the existing fixed histograms
and bounded 4,096-entry windows. Offline assessment retains one record and small
summaries, not every interval. Disk usage still grows with duration/report rate;
provision storage before long soaks.

## Optional resource trends

For a separately built profiling board image, set `base.instrumented: true`, its
`image_sha256`, and `base.profile_interval: 5`. Zero, the default, disables polling;
enabled intervals must be at least one second. Sampling adds explicit UDP 5001
traffic and is excluded from baseline comparisons.

The sampler has one outstanding request, reads nonblockingly and never resets
device counters. JSONL diagnostics retain raw counters, stack high-water and
interval executor busy percentage. Timeouts, invalid samples and counter resets
are explicit; missing samples do not become zero usage. Samples have host receipt
times but the existing profiling protocol has no request IDs, so late replies
cannot be precisely correlated. Offline assessment summarizes sample availability,
maximum observed stack usage and peak sampled executor busy time. The stack
watermark is since boot and is not reset at stage boundaries. These resource
metrics are informational, not additional pass/fail thresholds. Executor busy
time is not total MCU utilization, and measured stack high-water is not a proof
of worst-case safety. Physical profiling remains a B5 gate.

## Verification

`verify.py` includes the B4 suite. Ten test methods cover explicit/bounded
configuration, threshold boundaries, missing observations, successful and failed
recovery, early stops, interruption cleanup, alternate-policy reassessment,
damaged/mismatched evidence and nonblocking profiler behavior. A real Rust host
responder runs a continuous soak across multiple renewals, confirming only one
configure/stop pair. A 4,000-interval synthetic report verifies bounded offline
memory (under 8 MiB traced peak). These checks supplement the existing six-mode
socket/fault tests and portable gates. No hour/eight-hour physical soak, board
capacity or physical profiling result is claimed.
