# B2 live host benchmark

The host benchmark now sends command probes while receiving multicast status and
health, using the opt-in portable Rust benchmark codec. It supports CSV, JSON and
Protobuf, each with explicitly selected CRC off/on. [B3 firmware](BENCHMARK_B3.md)
now implements the benchmark service; physical board acceptance remains pending.

## Build and run a local demo

The following commands are for Linux, from the repository root. Use Python 3.12
and the portable project's Rust 1.90.0 toolchain. Select the appropriate host
target and shared-library extension on another platform; those platforms have
not been exercised by these loopback tests.

```sh
python -m pip install -r Rust/protocol/requirements-phase1.txt
cargo +1.90.0 build --locked --release \
  --manifest-path Rust/tools/message-demo/Cargo.toml \
  --target x86_64-unknown-linux-gnu --lib
export BENCHMARK_CODEC_LIB="$PWD/Rust/tools/message-demo/target/x86_64-unknown-linux-gnu/release/libbenchmark_codec.so"
```

In terminal 1, start the host responder:

```sh
python Rust/tools/message-demo/python/benchmark_live.py responder \
  --format protobuf --crc on --interface 127.0.0.1 --bind 127.0.0.1
```

In terminal 2, start a run. The output directory must not already exist:

```sh
python Rust/tools/message-demo/python/benchmark_live.py run \
  --format protobuf --crc on --interface 127.0.0.1 \
  --target 127.0.0.1 --target-kind host \
  --payload-bytes 512 --command-hz 100 --outstanding 64 \
  --status-period-us 10000 --health-period-us 100000 \
  --lease-ms 10000 --deadline-ms 1000 \
  --warmup 1 --duration 10 --report-interval 1 \
  --output benchmark-results/b2-protobuf-on
```

This requests 100 command probes/s, 100 status publications/s and 10 health
publications/s, all with a 512-byte application payload. Encoded wire sizes differ
by format. Use `--payload-bytes 0` for empty payloads, `--command-hz 0` for
stream-only operation, or zero stream periods for command-only operation.
The periods are integer microseconds; effective Hz is `1,000,000 / period` and is
saved in the run diagnostics. Change the format/CRC flags on both processes for
the other five configurations. No automatic format/CRC detection is attempted.

For another host, replace the target and interface with actual IPv4 addresses.
The interface is always explicit. Multicast uses group 239.255.42.1, TTL 1, and
the B1 ports 42010/42011/42012 by default. Alternate ports are for isolated host
tests; initial board firmware will use the fixed contract ports.

`--target-kind board` requires an image SHA-256 and a B3 image built with
`messaging-benchmark`. Do not interpret timeout as proof that the
device is offline or that a particular firmware version is installed.

## Runtime and measurement behavior

- Discovery verifies format/CRC capabilities. Configure acquires a single
  peer/socket and session, bound to the current boot and epoch. Ordinary command
  and publication formats are not modified.
- Control retries reuse the exact ID and contents. Renewal timing uses the
  conservative remaining lease budget, including after lost acknowledgments.
  Probes never renew a lease. Stop is attempted on completion, interruption or
  error, even if configure's acknowledgment was lost. Lease expiry is the fallback.
- Absolute pacing offers at most the newest due slot. Scheduling/window skips
  and local send failures are recorded separately. A target rate is not a claim
  that the host or responder sustained it.
- At most 64 received datagrams are processed before returning to timers. Receive
  work is divided among ready sockets. Reporting, renewals and deadlines are
  serviced in the same loop; no unbounded per-packet history is accumulated.
- Warm-up uses the same active run. Measurement resets host histories while
  retaining increasing probe IDs; warm-up replies are excluded. Stream gaps
  begin at the first accepted measured publication, not sequence zero.
- Sending stops at the measured-window boundary. A full response-deadline drain
  follows. Stream observations/freshness durations stop at the measurement end;
  probe replies can still complete during drain. Reports spanning the boundary
  use actual interval time, so their send rate includes the no-send drain period.
- RTT includes response decoding, pattern validation, the host Python/Rust FFI
  and JSON conversion overhead. Request encoding precedes the send timestamp.
  This is application RTT, not an isolated network delay or MCU codec time.
- Multicast reports sequence gaps/reordering/duplicates, time since forward
  progress, forward inter-arrival histograms and durations fresh/stale/not-seen.
  Reordered/duplicate samples do not refresh old status. Initial absence is
  reported as not-seen; it is not fabricated into a packet-loss count.
- Health diagnostics preserve first/last accepted measured samples. Differences
  describe their sample interval, not necessarily the entire run. Counters can
  saturate; unavailable final health is left unavailable. The host service uses
  active send-failure state for OK/DEGRADED and clears that state after success.

The host responder emits benchmark traffic only. It does not reproduce the
ordinary board application's background 10 Hz status / 1 Hz health streams.
The manifest labels this difference, so host results must not be presented as
equivalent board throughput measurements. B3 firmware retains the normal streams.

## Saved evidence

`events.jsonl` is authoritative and flushed incrementally. It contains the
manifest, interval records, diagnostics and an explicit final outcome.
`intervals.csv` contains convenient interval counts/rates/RTT quantiles. Console
JSON summaries provide live visibility. No per-packet capture is enabled.

The manifest records parameters, codec shared-library SHA-256, source commit,
dirty-worktree state, selected Python source hashes, platform and Python version.
Run diagnostics record boot/session/epoch, effective stream rates and measurement
boundaries. A supplied board image hash identifies the intended image; the host
does not independently attest what image is flashed. Preserve the build command,
compiler version, shared library and any board image alongside physical results.

Inspect a run without loading its entire log into memory:

```sh
python Rust/tools/message-demo/python/benchmark_live.py inspect \
  benchmark-results/b2-protobuf-on/events.jsonl
```

Complete means the run and deadline drain finished; it does not mean it met
performance thresholds. B4 adds sweep/soak orchestration, acceptance thresholds
and richer offline comparison. Missing/truncated final output remains incomplete.
Output failures still trigger cleanup, but no file API can promise a final record
when its disk is unavailable. Abrupt process termination relies on lease expiry.

## Implementation and verification

The `benchmark` feature in `messaging-codec` is disabled by default. The existing
host crate enables it and exports a small host-only shared-library binding.
Encoding/decoding uses the same no-allocator Rust codec as firmware. Since B3,
the responder also uses the portable Rust lease/scheduling service by default;
Python owns host sockets, clocks and statistics. `responder --service python`
selects the independent B2 state-machine reference for comparison. Timing includes
the host-only FFI/JSON bridge and is not an isolated MCU service measurement.

`generate_benchmark.py` compiles the authoritative descriptor and uses the shared
micropb generator. CSV input borrows up to 20 fields from 1,016-byte scratch;
output streams validated scalars and the payload into the bounded output buffer.
Normal 128-byte cells are not enlarged. CRC uses the existing framing module.
Status/health semantics call the existing application validator.

The generated serde adapter retains flat optional body fields, matching the
existing adapter, rather than adding a separate enum conversion layer in B2.
Exactly one body is enforced by validation and the payload is stored only once.
An ARM compile-time gate proves the entire model fits the B1 1,536-byte budget.
This refines B1's proposed enum representation; B3 may reduce it further if its
measured task/stack budget requires that optimization. B3 records signed-image
and static RAM measurements; runtime stack evidence still requires a board.

Validation includes all 53 B1 fixtures through six Rust codec modes, independent
CSV/JSON and protoc Protobuf interoperability, a manual golden request, long-f32
and all status-validity cases, strict JSON/CSV failures and CRC corruption.
Host service tests cover ownership, duplicate/conflicting/stale IDs, invalid
atomic configuration, exact expiry, delayed controls, epoch/ID exhaustion,
renewal, reply suppression and absolute publication scheduling.

Real loopback tests run all six modes with command and both multicast streams.
An additional fault run drops replies/ACKs, delays and duplicates replies,
corrupts CRC and injects invalid-datagram bursts while renewing a two-second lease.
An interrupted run checks the cleanup/evidence path. Deterministic metrics tests
remain the exact-count oracle; OS scheduling is not asserted to be lossless.
The full portable gate and ARM no-allocator link include the new codec, and the
single-format ARM configurations compile. These are host/build results, not
physical stress, soak, MCU timing or network-switch acceptance.
