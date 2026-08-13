# UDP benchmark plan

## Purpose

Measure application-level latency, bandwidth, and reliability for three
NUCLEO-H723ZG UDP echo implementations under the same host, network, payload,
and test procedure:

1. native STM32 Ethernet MAC with RMII PHY and Embassy;
2. W5500 MACRAW with Embassy; and
3. W5500 hardwired UDP offload.

The benchmark measures the complete round trip from the Windows computer,
through the network and board firmware, and back. It is not a PHY-only or
W5500-only measurement.

## Questions to answer

- What are the minimum, typical, and tail UDP echo latencies?
- What sustained payload bandwidth can each implementation deliver without
  packet loss?
- What peak rate can each implementation reach before loss grows materially?
- How do payload size and burst size affect latency and loss?
- Can each implementation run for an extended period without missing,
  duplicating, reordering, or corrupting datagrams?
- How quickly does service recover from a cable reconnect, board reset, or
  DHCP interruption?

## Test topology and comparison rules

```text
Windows benchmark client
          |
          | sequenced UDP request
          v
   switch/router ---------- NUCLEO-H723ZG UDP port 7
          ^                         |
          | identical UDP reply     |
          +-------------------------+
```

Both Ethernet cables may remain connected, but the current firmware images
activate only one interface at a time. Flash and benchmark each peer image
independently. Do not compare the managed SSH/update application with the
UDP-only images because its additional tasks and sockets make it a different
workload.

Keep these controls unchanged across variants:

- same computer, NIC, switch/router, and cabling;
- same release profile and firmware version;
- same payload sizes, durations, timeouts, and offered rates;
- same W5500 SPI frequency (currently 20 MHz);
- no debugger attached during recorded runs; and
- no unrelated high-volume traffic on the test network when practical.

Record the board IP and MAC for every run. Current DHCP observations are:

| Interface | MAC | Last observed IP |
|---|---|---|
| Native RMII | `02-00-00-00-00-00` | `192.168.68.66` |
| W5500 | `02-00-00-00-55-00` | `192.168.68.74` |

Addresses are DHCP-assigned and must not be assumed to remain constant.

## Host benchmark tool

Create a standalone Rust command-line program in
`Rust/tools/udp-benchmark/`. Keep it separate from the embedded crate so it
can use the Windows standard library and host test harness normally.

The program will use one UDP socket with enlarged operating-system receive
and transmit buffers. A sender generates traffic while a receiver validates
replies. A monotonic high-resolution host clock measures round trips, so the
computer and board do not need synchronized clocks.

Each datagram will contain a small benchmark header followed by deterministic
payload data:

| Field | Purpose |
|---|---|
| magic and protocol version | reject unrelated UDP traffic |
| run identifier | reject delayed packets from an earlier run |
| sequence number | detect loss, duplication, and reordering |
| transmission timestamp | calculate round-trip time |
| requested payload length | validate framing and truncation |
| deterministic data pattern | detect payload corruption |

The embedded server remains a true echo server: it does not parse or modify
this header.

Packets smaller than the 32-byte benchmark header are supported only by the
stop-and-wait functional and latency paths, where the complete datagram is
compared byte-for-byte. Concurrent throughput, burst, and soak modes use
instrumented payloads of at least 32 bytes so every outstanding packet can be
accounted for independently.

### Command modes

Provide these initial modes:

```text
udp-benchmark latency
udp-benchmark throughput
udp-benchmark burst
udp-benchmark soak
```

Common options should include board IP, UDP port, payload sizes, timeout,
warm-up duration, measurement duration, random seed, and output path. Use
stable defaults but record every effective option in the result file.

## Firmware benchmark configuration

Per-packet `defmt` logging can dominate a high-rate test or fill the RTT
buffer when no debugger is reading it. Add a Cargo feature such as
`benchmark` that removes successful per-packet messages while retaining boot,
link, DHCP, and error diagnostics. Normal demo builds should retain their
current tutorial-friendly logging.

Do not change socket counts, buffer sizes, polling intervals, SPI frequency,
or protocol behavior merely to improve a result. If one is intentionally
tuned, record it as a separate configuration and rerun the baseline.

## Test phases

### 1. Functional gate

Before recording performance, use the existing acceptance client with
payloads `0, 1, 32, 256, 1472`. The offload variant is expected to omit the
unsupported zero-byte echo case.

A variant proceeds only if all supported payloads return byte-for-byte from
the expected IP address and UDP port.

### 2. Latency

Use stop-and-wait traffic: send one datagram and wait for its reply before
sending another. This avoids queueing delay and measures the normal
application round trip.

For each supported payload size `1, 32, 64, 256, 512, 1024, 1472`:

- warm up for 2 seconds;
- collect at least 10,000 successful samples;
- use a finite reply timeout and count timeouts rather than retrying them
  invisibly; and
- report minimum, mean, p50, p90, p95, p99, p99.9, maximum, and standard
  deviation.

Windows scheduling can create outliers, so report distributions rather than
only an average. These are end-to-end application RTTs, not guaranteed chip
latencies.

### 3. Sustained bandwidth

Use a bounded sliding window of outstanding sequence numbers so the host can
keep the device busy without confusing loss with an unbounded queue.

For each payload size `64, 256, 512, 1024, 1472`:

1. warm up for 2 seconds;
2. run each offered rate for 10 seconds;
3. stop transmitting and allow a 2-second drain period; and
4. increase the offered rate until the loss threshold is crossed.

Begin with a coarse rate sweep, then use a narrower search around the
transition. Report both:

- **zero-loss sustained goodput:** highest tested payload rate with no missing
  or corrupt replies; and
- **peak goodput:** highest successfully echoed payload rate even if the host
  offered more traffic.

Count only echoed UDP payload bytes as application goodput. Also report
packets per second and offered payload bandwidth. Do not label the
request-plus-reply byte total as one-way throughput.

### 4. Burst capacity

After an idle interval, send bursts of `1, 4, 8, 16, 32, 64, 128` datagrams
as quickly as the host can. Repeat each burst at least 100 times for payloads
`64`, `512`, and `1472`.

Report the probability of a completely returned burst, packets lost per
burst, and latency distribution within the burst. This exposes the effect of
Embassy packet queues, W5500 socket memory, and the firmware polling loop.

### 5. Reliability soak

Run mixed payload sizes with deterministic pseudorandom selection at 70% of
the measured zero-loss rate. Start with 15 minutes; use at least 8 hours for a
release-quality result.

Record periodic snapshots and final totals for:

- sent and validly echoed packets and bytes;
- missing and late packets;
- duplicates and reordered replies;
- truncated or corrupt payloads;
- timeouts and longest observed outage; and
- latency percentiles per time interval, so degradation is visible over time.

The random seed must be recorded so a failing payload sequence can be
reproduced.

### 6. Recovery tests

Keep recovery testing separate from steady-state reliability because packet
loss is expected during a deliberate outage. While paced traffic continues:

- unplug and reconnect the active Ethernet cable;
- reset the board;
- allow DHCP to renew; and
- optionally interrupt the switch/router port.

Measure the last valid reply before the event, first valid reply afterward,
total outage, packets lost, and whether service recovers without reflashing.
Manual events must be timestamped in the result log.

## Metrics and definitions

| Metric | Definition |
|---|---|
| RTT | host receive timestamp minus host send timestamp for one sequence |
| offered rate | UDP payload bits sent by the host per measurement second |
| goodput | payload bits returned correctly per measurement second |
| loss | sent sequences with no valid reply by the end of the drain period |
| late | valid reply received after its normal timeout but during drain |
| duplicate | more than one valid reply for the same run and sequence |
| reordered | reply sequence arrives behind a later sequence |
| corrupt | returned length, header, or deterministic payload differs |

Report loss as both a count and percentage. Preserve raw integer nanosecond
timestamps in machine-readable output; round only the human-readable report.

## Outputs

Each invocation should create:

- JSON containing metadata, configuration, summaries, and all counters;
- CSV containing per-sample or per-interval measurements; and
- an optional Markdown summary suitable for the trade study.

Metadata should include timestamp, operating system, host name, git commit,
firmware variant/version, IP/MAC, SPI frequency where applicable, payload
configuration, and benchmark-tool version.

Do not commit large raw soak-test files by default. Commit the tool, schemas,
small representative results, and reviewed summary tables.

## Validation of the benchmark itself

- Unit-test packet encoding, decoding, patterns, sequence accounting, and
  percentile calculations on the host.
- Test the tool against a local software echo server before using hardware.
- Inject synthetic loss, duplication, reordering, corruption, and delayed
  packets to prove that every counter changes as intended.
- Make rate pacing and timeouts use monotonic time.
- Run the existing acceptance script before and after benchmark development to
  ensure the firmware still provides ordinary UDP echo behavior.

Host-only unit tests should run in GitHub Actions. Hardware performance runs
remain manual because hosted CI has no board or controlled LAN.

## Initial acceptance criteria

These criteria validate the test setup without prejudging which architecture
should be fastest:

- all supported functional-gate datagrams echo byte-for-byte;
- a completed latency run reports the requested sample count and percentiles;
- the bandwidth search identifies both a zero-loss rate and a saturation
  region;
- a 15-minute baseline soak has no corruption or duplicate replies;
- every missing or late sequence is accounted for explicitly; and
- results for all three variants can be reproduced from recorded commands and
  metadata.

Performance targets should be established only after the first controlled
baseline. They can then become regression thresholds with an explicit
tolerance for Windows and LAN variability.

## Implementation sequence

1. Add the host crate, packet format, CLI, and local echo-server tests.
2. Implement stop-and-wait latency measurement and JSON/CSV output.
3. Add concurrent receive, bounded windows, pacing, and throughput search.
4. Add burst and soak modes with complete sequence accounting.
5. Add the firmware `benchmark` feature to suppress per-packet logging.
6. Run short validation tests against all three board images.
7. Run the controlled baseline and add its summary to `TRADE_STUDY.md`.
8. Add optional recovery experiments and long-duration soak runs.

This ordering validates measurement correctness before using the results to
compare architectures.

## Implementation and baseline status

Implemented on 2026-08-12:

- standalone Rust host tool with `latency`, `throughput`, `burst`, `soak`,
  `suite`, and `compare` commands;
- monotonic timing, bounded outstanding windows, paced offered rates, drain
  periods, sequence accounting, deterministic payload validation, and
  JSON/CSV/Markdown outputs;
- eleven host tests, including a localhost UDP echo server and injected loss,
  delay, duplication, reordering, and corruption;
- embedded `benchmark` feature that removes successful per-packet logging;
- signed benchmark image selection in the build and flash scripts;
- CI formatting, unit-test, Clippy, and benchmark-feature firmware checks;
- automated three-image Windows runner in
  `tools/run-benchmark-comparison.ps1`; and
- one controlled full baseline covering all three variants.

The reviewed results and conclusions are in
[BENCHMARK_REPORT.md](BENCHMARK_REPORT.md). The baseline used every initial
acceptance setting in this plan: 10,000 samples per latency size, saturation
for every throughput size, 100 burst repetitions, and a 15-minute soak. The
optional eight-hour release soak and manual recovery events remain future
experiments rather than requirements for this initial baseline.
