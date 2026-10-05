# Messaging comparison: local evidence and pending board work

Follow-up: [Protobuf review](PROTOBUF_REVIEW.md) records the 2026-10-02 board
smoke test and decoder correction. The tables here retain their original
provenance; they have not been regenerated for the corrected codec.

Date: 2026-10-01 UTC. Base: merged Phase 5, `60e0b253fd09b0d821db00f72f60c63fab0836ad`.
These results characterize this implementation, fixed fixtures, and a shared Linux
host. They do not complete the planned native-Ethernet comparison. Reproduction
commands and measurement definitions are in [Phase 6](PHASE6.md).

**Protobuf is the leading candidate for the next board trial**, based on its lower
compiled footprint and the host codec measurements below. CSV remains competitive
for small packet sizes. No production format or CRC policy is selected: physical
reliability, MCU timing and stack measurements are still missing.

## Fixed-message sizes

Bytes from the actual Rust codec using the reviewed fixture values. Each body is
the complete application envelope, including identity/correlation/sequence fields.
CRC-off datagram size equals body size; CRC-on adds exactly eight bytes to every
entry. UDP/IP/Ethernet headers are excluded.

| Fixture | CSV body | JSON body | Protobuf body |
|---|---:|---:|---:|
| Device-info request | 36 | 126 | 37 |
| Device-info response | 71 | 214 | 72 |
| Valid status publication | 54 | 183 | 49 |
| Health publication | 52 | 222 | 50 |
| Device-info response with escaping | 84 | 226 | 81 |

For the fixed status/health examples at 10 Hz + 1 Hz, application traffic is
592 B/s (CSV), 2,052 B/s (JSON), or 540 B/s (Protobuf) before CRC. CRC adds 88 B/s
to these eleven publications per second. These are fixture-based calculations,
not measured link bandwidth; actual sequence/uptime/counter widths vary. All
three fit the prototype's datagram budget comfortably for these examples.

The JSON representation is the explicitly defined `prototype-json-v1`, not
ProtoJSON. Compact binary encoding does not make Protobuf smallest for every
message: the short request and ordinary device-info response are one byte smaller
in CSV here. See [all fixture sizes](results/phase6/fixture_sizes.csv).

## Host codec cost

Release Rust 1.90.0, x86_64-unknown-linux-gnu, default Cargo release optimization,
no LTO and no RUSTFLAGS. All codecs are enabled in this host executable. Each
fixture/configuration receives nine batches of 10,000 operations after warm-up;
inputs are shuffled with seed 723. The environment is shared, with no CPU affinity
or governor control, so these are descriptive measurements, not stable deadlines.

The table uses the valid status-publication fixture. Units are **nanoseconds per
operation, median of batch means**, rounded to the nearest nanosecond. These are
not individual-call latency percentiles and must not be read as MCU cycle costs.

| Encoding | CRC | Encode | Decode |
|---|---|---:|---:|
| CSV | off | 430 | 16,121 |
| CSV | on | 560 | 15,805 |
| JSON | off | 351 | 740 |
| JSON | on | 750 | 1,139 |
| Protobuf | off | 87 | 164 |
| Protobuf | on | 188 | 263 |

CSV decoding in this implementation includes constructing a `csv-core` reader
for each record, strict quote checks, typed conversion and shared validation.
The observed cost is a property of these paths and build settings, not proof that
CSV must intrinsically be slower. The slightly lower CSV/CRC-on decode median is
ordinary run variation; it does not show that CRC makes decoding faster. Do not
subtract noisy independent medians to claim an exact CRC cost.

The timed Rust loop excludes subprocess, JSONL and UDP overhead; it includes
validation, normalization, bounded copies/conversions and framing. Protobuf is
fastest in this selected fixture on this host. All fixtures, summary statistics,
and batch means remain available in [codec.csv](results/phase6/codec.csv),
[codec_batches.csv](results/phase6/codec_batches.csv), and
[codec provenance](results/phase6/codec-provenance.json).

## Host command and multicast preview

Six sequential configurations, each with ten seconds of warm-up and **60 measured
seconds**, using the release responder on Linux loopback. Two listener processes
shared the same host/NIC. Configuration order was JSON/off, CSV/off, JSON/on,
Protobuf/on, CSV/on, Protobuf/off (seed 723). The codec benchmark completed first;
this was still a shared development host without CPU isolation or governor control.

All **360/360 measured commands succeeded**, 60 per configuration, with no missed
command slots, timeouts or error responses. Each listener received 600 status
and 60 health publications per configuration: observed rates were 10 Hz and 1 Hz.
The responder logged the same successful local send counts. No measured gaps,
duplicates, reordering, stale/not-seen time or rejection outcomes were observed.
Observed health send-error/rejection/skip counters stayed at zero. Log hashes,
clock checks and terminal-result checks passed; re-export reproduced the CSVs.

| Encoding | CRC | Success | RTT p50 ms | p95 ms | p99 ms | Max ms |
|---|---|---:|---:|---:|---:|---:|
| CSV | off | 60/60 | 0.273 | 0.457 | 0.541 | 0.579 |
| CSV | on | 60/60 | 0.406 | 0.563 | 0.618 | 0.648 |
| JSON | off | 60/60 | 0.275 | 0.468 | 0.571 | 0.589 |
| JSON | on | 60/60 | 0.454 | 0.672 | 1.897 | 3.411 |
| Protobuf | off | 60/60 | 0.249 | 0.430 | 1.011 | 1.650 |
| Protobuf | on | 60/60 | 0.339 | 0.617 | 1.156 | 1.307 |

These are end-to-end application RTTs including Python response decoding/CRC and
host logging/scheduling effects. Request encoding occurs before the timer. One
minute and 60 RTT samples per configuration cannot establish rare failures or
stable tail behavior. The different p99/max ordering does not contradict the
codec microbenchmark: it measures a different path with scheduling noise.

This preview validates the runner and produces an interpretable example report.
It is **not** the planned ten-minute-per-configuration native-Ethernet experiment,
a second-machine multicast test, a zero-loss guarantee, or evidence that CRC can
be omitted. No physical board was involved.

Retained evidence: [summary](results/phase6/traffic_summary.csv),
[command samples](results/phase6/traffic_commands.csv),
[stream observations](results/phase6/traffic_streams.csv),
[live sizes](results/phase6/traffic_sizes.csv),
[health counters](results/phase6/traffic_health.csv),
[diagnostics](results/phase6/traffic_diagnostics.csv), and
[traffic provenance](results/phase6/traffic-provenance.json). Full raw logs are
local development artifacts, not committed here; preserve whole output
directories when collecting physical bench results.

## MCU build footprint, inherited from Phase 5

These are the prior local Phase 5 baseline builds, retained with their original
metadata in [phase5_resources.json](results/phase6/phase5_resources.json). They
are not new Phase 6 hardware measurements. Phase 6 changes host tooling/docs only;
firmware, portable wire codecs, schema and dependencies are unchanged.

Rust 1.98.1, thumbv7em-none-eabihf, size-optimized release with fat LTO, exactly
one encoding per image, no profiling. Signed size includes the existing header
and signature packaging with imgtool 2.4.0.

| Encoding | CRC | Signed bytes | Static RAM bytes |
|---|---|---:|---:|
| CSV | off | 116,784 | 28,368 |
| CSV | on | 118,032 | 28,368 |
| JSON | off | 120,144 | 28,368 |
| JSON | on | 121,368 | 28,368 |
| Protobuf | off | 78,544 | 28,368 |
| Protobuf | on | 79,792 | 28,368 |

All are below the 262,144-byte signed-image limit. Protobuf/off is 38,240 bytes
smaller than CSV/off and 41,600 smaller than JSON/off. The linker leaves 102,704
bytes in the configured RAM region beyond static allocations, but that is **not
measured safe stack margin**. Runtime stack high-water remains pending.

## Dependencies and maintaining the schema

No Phase 6 dependencies were added. Existing direct codec-specific dependencies:

| Encoding | Rust dependencies | Additional maintained behavior |
|---|---|---|
| CSV | `csv-core` 0.1.13, `ryu` 1.0.23 | Strict record/quote rules, typed cells, frozen column mapping |
| JSON | `serde-json-core` 0.6.0 | Custom mapping, strict null/unknown/duplicate/control handling |
| Protobuf | `micropb` 0.6.0 | Generated bounded messages and conversion to the shared model |

All share `heapless` 0.9.3, `serde` 1.0.228, CRC 3.3.0, common contract validation,
and the envelope/model. Generator dependencies are separate: `micropb-gen` 0.6.0,
`serde_json` 1.0.145 and host protoc/Python tooling. The existing requirements pin
grpcio-tools 1.76.0 and protobuf 6.33.6. Python CSV/JSON use the standard library;
Python Protobuf uses generated code/runtime. Python CRC is the independent bitwise
implementation used in earlier correctness tests.

The [source/dependency inventory](results/phase6/inventory.csv) records direct
Cargo dependencies and physical source-line counts, including comments/blanks.
CSV's `cells.rs` and `csv.rs` account for 160 handwritten lines. The shared
`lib.rs` has 177 lines containing dispatch, JSON and Protobuf adapters; validation
has 127 and framing 173. The 548-line generated shared model contains mappings
for multiple formats, and generated Protobuf has 2,004 lines. These are not
comparable measures of developer effort or flash use, and shared lines are not
charged independently to every encoding. Host tooling is listed separately by
path; this inventory is not a repository-wide count of firmware/generator code.

The isolated schema-change gate passed again: adding one enum value to `.proto`
reproducibly changes generated Rust/Python Protobuf, schema metadata and reference
docs. CSV column order remains explicitly frozen in the supplemental profile.
Behavioral validation and tests still need review when the meaning changes. This
supports continuing with `.proto` plus bounded profile metadata; it provides no
reason to introduce a YAML schema layer. No timed human-effort study was performed.

## Physical evidence still required

| Measurement/gate | Status |
|---|---|
| Ten measured minutes per format/CRC on native Ethernet | Pending |
| Physical command delivery and multicast to two listeners | Pending |
| Another physical host, switch/IGMP, TTL and UDP-checksum observations | Pending |
| MCU encode/decode timing and actual board publication rate | Pending |
| Runtime stack high-water and executor utilization in a separate instrumented run | Pending |
| Signed boot, trial confirmation/rollback, RNG, reset and cable/DHCP recovery | Pending |
| Board fault counters and valid traffic after CRC/other injected failures | Pending |

The next decision should use the same six configurations on the board and retain
all raw logs, manifests, image hashes and boot/network evidence. Favor Protobuf
if the current footprint advantage persists and MCU timing/stack/maintenance
results meet the application needs. Keep CSV or JSON available for diagnostic
work if their readability materially helps. An all-success loopback run cannot
establish whether application CRC is needed on the physical system; that decision
requires the application's error-detection requirements and fault evidence.
