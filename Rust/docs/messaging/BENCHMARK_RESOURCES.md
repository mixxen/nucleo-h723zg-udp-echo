# B1 resource budget and integration decisions

These are B1 design limits and reference encoding measurements, not measured
firmware RAM/stack or throughput for an implemented benchmark feature.

## Wire budget

`python Rust/protocol/benchmark/check_contract.py` compiles the proposed schema
and exercises 53 explicit/expanded valid fixtures in six format/CRC configurations.
It uses protoc Protobuf and descriptor-driven reference CSV/JSON, not the future
Rust benchmark codecs. Metadata reaches maximum allowed lengths/u32 values and
health counters saturate. Payload lengths include 0, 1, 128, 129 and 512 bytes.

| Encoding | Largest tested body | With CRC | Remaining to 1,024 bytes |
|---|---:|---:|---:|
| CSV | 666 | 674 | 350 |
| JSON | 876 | 884 | 140 |
| Protobuf | 636 | 644 | 380 |

The maximum is the 512-byte health publication with maximum metadata/counters.
Payload and identifiers contain no quote/escape characters. These are bounds
for the reviewed canonical fixture encodings, not a claim that arbitrary JSON
whitespace/number spellings fit. Every actual encoder must still enforce the
1,016-byte body limit. B2 will compare real Rust bytes/sizes to these examples,
including long finite-f32 spellings and all status validity cases.

## MCU storage and sockets

Add sockets only with the opt-in benchmark feature. Preserve the normal image's
five-slot allocation. Benchmark images use `StackResources<8>`: DHCP, three
normal messaging sockets, three benchmark sockets and optional profiling.
Without profiling, leave the reserved slot unused. No SSH/update/echo service is
added to the messaging image.

| Additional storage | Planned bytes | Reason |
|---|---:|---|
| Benchmark command RX, two datagrams | 2,048 | Bounded queue for control/probes |
| Benchmark command TX, two datagrams | 2,048 | Bounded response queue |
| Command input and output work buffers | 2,048 | One 1,024-byte buffer each |
| Benchmark status TX and output buffer | 2,048 | One queued datagram plus encoding buffer |
| Benchmark health TX and output buffer | 2,048 | Independent stream |
| **Additional fixed packet storage** | **10,240** | Excludes metadata, model and task storage |

The previous uninstrumented messaging baseline used 28,368 static RAM bytes.
Adding packet storage alone would reach 38,608 bytes; this is arithmetic, not a
new linker result. B3 must measure network/socket metadata, async task storage,
codec/model temporaries, scheduling/lease state, stacks and profiling overhead.
Use these initial implementation gates:

- Total benchmark-image static RAM at most 65,536 bytes in the existing 128 KiB
  RAM region. Remaining RAM is not proven safe stack headroom.
- Single decoded benchmark envelope budget at most 1,536 bytes on ARM, enforced
  when the generated Rust representation exists. Store body as an enum and one
  `heapless::String<512>` payload; do not embed a payload in every body variant.
- One owner/configuration/control watermark and last-control record, with an
  initial combined state budget of 256 bytes excluding counters and task storage.
  No board-side per-probe history: the host owns outstanding-response tracking.
- Signed image at most 262,144 bytes, unchanged. Measure each single-encoding
  CRC-off/on image and separate profiling variants before accepting B3.
- No allocator in MCU codecs/tasks. Measure runtime stack high-water during the
  later physical runs; do not substitute host `size_of` or linker free space.

## CSV and code generation constraint

The current normal adapter uses `String<128>` cells and a 16-cell vector.
Benchmark payloads exceed 128 bytes, and the capabilities response has 20 cells
(10 common plus 10 body). Increasing every normal cell to 512 bytes would waste
memory and change baseline behavior.

For the benchmark path, use bounded decoded byte scratch (up to 1,016 bytes) and
up to 20 offset pairs/borrowed field slices; stream output fields into the fixed
encoding buffer. Only the payload model owns its 512-byte string. On 32-bit ARM,
20 pairs of u32 offsets are 160 bytes, before parser-specific state. Keep strict
CSV quoting and type checks shared with normal codecs without expanding normal
cell capacity. B2 must implement and cross-check this path; B1 does not pretend
the existing generator already supports this model or these larger messages.

## Host bounds and accounting

Limit outstanding probes and retained reorder positions to 4,096 each, with
explicit bounds on late/duplicate history per session/stream. On eviction,
classify observations as outside the retained history; do not silently call them
fresh successes or proven losses. No list of every RTT for a multi-hour run.
Histograms and interval accumulation must have fixed memory bounds and documented
quantile precision. Saving optional raw packet records is an explicit disk/I/O
choice; interval summaries remain the default. The old echo soak's `all_samples`
retention cannot be reused unchanged for this guarantee.

Expose host scheduling/window limitations alongside target and achieved rates.
Check timers after bounded receive batches so an inbound flood cannot indefinitely
postpone pacing, lease renewal or reports. Controls and load probes use the same
owner socket, with reserved control-processing opportunities on the host. A
missed renewal under overload can end the run and must appear in its outcome.

## Evidence and next gates

The B1 checker also checks a manually authored three-format capabilities request,
15 semantic rejection fixtures and deterministic pattern vectors. CRC-on sizes
add the already frozen eight-byte trailer; CRC correctness remains the Phase 5
gate. The 15 lease/ownership scenarios in fixtures are acceptance specifications,
not executable state-machine tests yet. B2/B3 must implement those tests, compare
actual Rust encodings, measure generated ARM types, and build the opt-in images.

Normal schema/generated sources, firmware buffers, socket allocations, ports and
features are unchanged by B1. No board was used for these checks. Alex's initial
successful NUCLEO run remains valuable smoke evidence, not benchmark measurements.
