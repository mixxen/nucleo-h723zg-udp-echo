# NUCLEO-H723ZG UDP benchmark report

> Historical baseline: these tables intentionally preserve the original
> 20 MHz, polling-based W5500 comparison. Current interrupt-driven and retained
> performance results are in
> [STREAM_BENCHMARK_REPORT.md](STREAM_BENCHMARK_REPORT.md).

This report compares UDP-only release firmware using the same Windows host, LAN, packet format, and benchmark procedure. Latency is application round-trip time; throughput is correctly echoed UDP payload goodput.

## Executive summary

- **Native RMII was the clear steady-state performance winner.** At 1,472
  bytes it delivered 1.042 ms median RTT and 16.0 Mbit/s strict zero-loss
  goodput, compared with 4.438 ms / 2.0 Mbit/s for MACRAW and 4.163 ms /
  2.5 Mbit/s for W5500 offload.
- **W5500 offload improved typical and tail latency over MACRAW.** At 1,472
  bytes its median was about 6% lower and its p99 about 38% lower. Its
  large-packet strict zero-loss rate was 25% higher, although peak returned
  goodput was similar for the two W5500 implementations.
- **MACRAW absorbed some bursts better than the other implementations.** It
  completed every 64-byte burst through 32 packets and every 512-byte burst
  through 16 packets. Native RMII's four-entry packet queues accepted only
  four 64-byte packets or one large packet per unpaced burst in this test.
- **All implementations were reliable at a conservative paced load.** Across
  the three 15-minute soaks, 377,144 of 377,148 packets returned correctly.
  No payload corruption, duplicate reply, or reordered reply was observed.
  Offload returned every one of its 44,171 soak packets.
- These results characterize **this firmware configuration**, particularly
  the 20 MHz W5500 SPI clock and 1 ms polling. They are not the W5500 chip's
  theoretical maximum and should not be generalized to interrupt-driven or
  80 MHz designs.

## Method

The Windows Rust client sent self-validating UDP datagrams to port 7. Packets
of 32 bytes or more contained a run ID, sequence number, monotonic send
timestamp, and deterministic payload pattern. One-byte latency packets were
validated byte-for-byte using stop-and-wait operation. The firmware remained
a protocol-agnostic echo server.

Each firmware variant ran these same phases:

1. a five-repetition functional gate for supported payloads;
2. 10,000 successful stop-and-wait samples at each of seven payload sizes;
3. 10-second offered-rate trials, with two refinement trials after
   saturation;
4. 100 repetitions of every combination of three payload and seven burst
   sizes; and
5. a 900-second mixed-size soak at 70% of the variant's lowest strict
   zero-loss rate.

The client used one enlarged-buffer UDP socket, monotonic timing, a bounded
256-packet outstanding window, a 250 ms normal reply timeout, and a two-second
post-load drain. No debugger was attached. Benchmark firmware omitted only
successful per-packet RTT logging; startup, link, DHCP, and error diagnostics
remained compiled in. The complete automated run took 5,810 seconds.

The command used from the repository root was:

```powershell
powershell -ExecutionPolicy Bypass -File .\Rust\tools\run-benchmark-comparison.ps1 `
    -Version 0.4.0 `
    -SkipFirmwareBuild `
    -OutputRoot .\Rust\benchmark-results\baseline-20260812-v2
```

## Test subjects

| Variant | Firmware | Board IP | MAC | SPI | Host |
|---|---|---|---|---:|---|
| Native RMII + Embassy | 0.4.0 | 192.168.68.66 | `02-00-00-00-00-00` | n/a | UBUNTU-RTX |
| W5500 MACRAW + Embassy | 0.4.0 | 192.168.68.74 | `02-00-00-00-55-00` | 20 MHz | UBUNTU-RTX |
| W5500 hardware offload | 0.4.0 | 192.168.68.74 | `02-00-00-00-55-00` | 20 MHz | UBUNTU-RTX |

## Latency

| Variant | Bytes | Samples | Min ms | p50 ms | p95 ms | p99 ms | p99.9 ms | Max ms | Timeouts |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Native RMII + Embassy | 1 | 10000 | 0.212 | 0.235 | 0.251 | 0.279 | 0.566 | 1.910 | 0 |
| Native RMII + Embassy | 32 | 10000 | 0.234 | 0.250 | 0.266 | 0.297 | 0.476 | 1.692 | 0 |
| Native RMII + Embassy | 64 | 10000 | 0.245 | 0.268 | 0.283 | 0.324 | 0.755 | 1.024 | 0 |
| Native RMII + Embassy | 256 | 10000 | 0.351 | 0.374 | 0.390 | 0.424 | 0.832 | 1.438 | 0 |
| Native RMII + Embassy | 512 | 10000 | 0.488 | 0.516 | 0.532 | 0.565 | 0.911 | 1.914 | 0 |
| Native RMII + Embassy | 1024 | 10000 | 0.771 | 0.797 | 0.815 | 0.867 | 1.484 | 5.080 | 0 |
| Native RMII + Embassy | 1472 | 10000 | 1.017 | 1.042 | 1.062 | 1.125 | 1.839 | 2.451 | 0 |
| W5500 MACRAW + Embassy | 1 | 10000 | 0.523 | 1.338 | 1.349 | 1.449 | 3.656 | 6.050 | 0 |
| W5500 MACRAW + Embassy | 32 | 10000 | 0.604 | 1.368 | 1.381 | 1.823 | 4.328 | 49.136 | 0 |
| W5500 MACRAW + Embassy | 64 | 10000 | 0.902 | 1.459 | 1.472 | 2.648 | 4.782 | 17.666 | 0 |
| W5500 MACRAW + Embassy | 256 | 10000 | 1.095 | 1.854 | 1.866 | 2.099 | 5.352 | 34.190 | 0 |
| W5500 MACRAW + Embassy | 512 | 10000 | 1.768 | 2.401 | 2.415 | 3.893 | 7.698 | 54.977 | 0 |
| W5500 MACRAW + Embassy | 1024 | 10000 | 2.902 | 3.495 | 3.510 | 5.021 | 7.048 | 15.083 | 1 |
| W5500 MACRAW + Embassy | 1472 | 10000 | 3.979 | 4.438 | 4.459 | 6.873 | 10.712 | 50.471 | 1 |
| W5500 hardware offload | 1 | 10000 | 0.288 | 1.094 | 1.107 | 1.136 | 2.151 | 4.495 | 0 |
| W5500 hardware offload | 32 | 10000 | 0.279 | 1.185 | 1.198 | 1.245 | 3.692 | 15.059 | 0 |
| W5500 hardware offload | 64 | 10000 | 0.407 | 1.246 | 1.259 | 1.290 | 2.188 | 4.467 | 0 |
| W5500 hardware offload | 256 | 10000 | 0.821 | 1.641 | 1.653 | 1.675 | 1.989 | 9.271 | 0 |
| W5500 hardware offload | 512 | 10000 | 1.442 | 2.158 | 2.171 | 2.237 | 3.612 | 15.012 | 0 |
| W5500 hardware offload | 1024 | 10000 | 2.616 | 3.221 | 3.235 | 3.283 | 4.428 | 9.379 | 0 |
| W5500 hardware offload | 1472 | 10000 | 3.648 | 4.163 | 4.178 | 4.256 | 5.785 | 9.192 | 0 |

## Sustained bandwidth

| Variant | Bytes | Zero-loss Mbps | Peak goodput Mbps | Saturation observed |
|---|---:|---:|---:|:---:|
| Native RMII + Embassy | 64 | 2.500 | 3.940 | yes |
| Native RMII + Embassy | 256 | 10.000 | 10.000 | yes |
| Native RMII + Embassy | 512 | 4.000 | 13.637 | yes |
| Native RMII + Embassy | 1024 | 14.000 | 16.054 | yes |
| Native RMII + Embassy | 1472 | 16.000 | 17.673 | yes |
| W5500 MACRAW + Embassy | 64 | 0.312 | 0.356 | yes |
| W5500 MACRAW + Embassy | 256 | 1.000 | 1.109 | yes |
| W5500 MACRAW + Embassy | 512 | 1.500 | 1.698 | yes |
| W5500 MACRAW + Embassy | 1024 | 2.000 | 2.340 | yes |
| W5500 MACRAW + Embassy | 1472 | 2.000 | 2.644 | yes |
| W5500 hardware offload | 64 | 0.375 | 0.412 | yes |
| W5500 hardware offload | 256 | 1.000 | 1.248 | yes |
| W5500 hardware offload | 512 | 1.750 | 1.897 | yes |
| W5500 hardware offload | 1024 | 2.500 | 2.500 | yes |
| W5500 hardware offload | 1472 | 2.500 | 2.612 | yes |

### Bandwidth interpretation

All payload sizes reached an observed saturation region, so none of the peak
figures merely reflect the largest rate attempted. For 1,472-byte datagrams,
native RMII's strict zero-loss goodput was 6.4 times offload and 8 times
MACRAW. Its 17.673 Mbit/s peak returned goodput was about 6.7 times either
W5500 path.

The strict zero-loss column is deliberately unforgiving: one missing packet
makes a trial ineligible. Consequently it is not always monotonic. For
example, native 512-byte trials had isolated loss at 8, 10, and 7 Mbit/s even
though 4 Mbit/s was clean and peak returned goodput reached 13.637 Mbit/s.
Treat strict zero-loss as a conservative tested operating point, peak goodput
as the saturation plateau, and consult both with the soak result rather than
interpreting either as an exact physical limit.

## Burst reliability

| Variant | Bytes | Burst | Complete bursts | Missing packets | p99 RTT ms |
|---|---:|---:|---:|---:|---:|
| Native RMII + Embassy | 64 | 1 | 100.0% | 0 | 0.450 |
| Native RMII + Embassy | 64 | 4 | 100.0% | 0 | 0.572 |
| Native RMII + Embassy | 64 | 8 | 0.0% | 400 | 0.764 |
| Native RMII + Embassy | 64 | 16 | 0.0% | 1200 | 1.050 |
| Native RMII + Embassy | 64 | 32 | 0.0% | 2800 | 3.045 |
| Native RMII + Embassy | 64 | 64 | 0.0% | 5999 | 1.905 |
| Native RMII + Embassy | 64 | 128 | 0.0% | 12400 | 3.018 |
| Native RMII + Embassy | 512 | 1 | 100.0% | 0 | 0.561 |
| Native RMII + Embassy | 512 | 4 | 0.0% | 100 | 1.123 |
| Native RMII + Embassy | 512 | 8 | 0.0% | 500 | 1.492 |
| Native RMII + Embassy | 512 | 16 | 0.0% | 1300 | 2.047 |
| Native RMII + Embassy | 512 | 32 | 0.0% | 2900 | 2.780 |
| Native RMII + Embassy | 512 | 64 | 0.0% | 6100 | 4.637 |
| Native RMII + Embassy | 512 | 128 | 0.0% | 12500 | 8.348 |
| Native RMII + Embassy | 1472 | 1 | 100.0% | 0 | 1.196 |
| Native RMII + Embassy | 1472 | 4 | 0.0% | 300 | 2.615 |
| Native RMII + Embassy | 1472 | 8 | 0.0% | 700 | 2.857 |
| Native RMII + Embassy | 1472 | 16 | 0.0% | 1500 | 3.885 |
| Native RMII + Embassy | 1472 | 32 | 0.0% | 3100 | 5.824 |
| Native RMII + Embassy | 1472 | 64 | 0.0% | 6300 | 10.881 |
| Native RMII + Embassy | 1472 | 128 | 0.0% | 12700 | 17.569 |
| W5500 MACRAW + Embassy | 64 | 1 | 100.0% | 0 | 1.601 |
| W5500 MACRAW + Embassy | 64 | 4 | 100.0% | 0 | 6.093 |
| W5500 MACRAW + Embassy | 64 | 8 | 100.0% | 0 | 11.579 |
| W5500 MACRAW + Embassy | 64 | 16 | 100.0% | 0 | 23.190 |
| W5500 MACRAW + Embassy | 64 | 32 | 100.0% | 0 | 46.437 |
| W5500 MACRAW + Embassy | 64 | 64 | 99.0% | 15 | 93.150 |
| W5500 MACRAW + Embassy | 64 | 128 | 99.0% | 10 | 187.173 |
| W5500 MACRAW + Embassy | 512 | 1 | 100.0% | 0 | 4.947 |
| W5500 MACRAW + Embassy | 512 | 4 | 100.0% | 0 | 10.126 |
| W5500 MACRAW + Embassy | 512 | 8 | 100.0% | 0 | 19.234 |
| W5500 MACRAW + Embassy | 512 | 16 | 100.0% | 0 | 38.294 |
| W5500 MACRAW + Embassy | 512 | 32 | 0.0% | 235 | 71.406 |
| W5500 MACRAW + Embassy | 512 | 64 | 0.0% | 3400 | 71.793 |
| W5500 MACRAW + Embassy | 512 | 128 | 0.0% | 9681 | 74.122 |
| W5500 MACRAW + Embassy | 1472 | 1 | 100.0% | 0 | 4.943 |
| W5500 MACRAW + Embassy | 1472 | 4 | 100.0% | 0 | 18.243 |
| W5500 MACRAW + Embassy | 1472 | 8 | 100.0% | 0 | 35.923 |
| W5500 MACRAW + Embassy | 1472 | 16 | 0.0% | 604 | 44.775 |
| W5500 MACRAW + Embassy | 1472 | 32 | 0.0% | 2104 | 49.138 |
| W5500 MACRAW + Embassy | 1472 | 64 | 0.0% | 5200 | 53.206 |
| W5500 MACRAW + Embassy | 1472 | 128 | 0.0% | 11491 | 57.629 |
| W5500 hardware offload | 64 | 1 | 100.0% | 0 | 1.462 |
| W5500 hardware offload | 64 | 4 | 100.0% | 0 | 4.941 |
| W5500 hardware offload | 64 | 8 | 100.0% | 0 | 9.889 |
| W5500 hardware offload | 64 | 16 | 100.0% | 0 | 19.740 |
| W5500 hardware offload | 64 | 32 | 0.0% | 382 | 34.763 |
| W5500 hardware offload | 64 | 64 | 0.0% | 3549 | 35.277 |
| W5500 hardware offload | 64 | 128 | 0.0% | 9900 | 35.533 |
| W5500 hardware offload | 512 | 1 | 100.0% | 0 | 2.308 |
| W5500 hardware offload | 512 | 4 | 0.0% | 100 | 6.795 |
| W5500 hardware offload | 512 | 8 | 0.0% | 500 | 6.789 |
| W5500 hardware offload | 512 | 16 | 0.0% | 1300 | 6.827 |
| W5500 hardware offload | 512 | 32 | 0.0% | 2840 | 8.226 |
| W5500 hardware offload | 512 | 64 | 0.0% | 6000 | 8.626 |
| W5500 hardware offload | 512 | 128 | 0.0% | 12228 | 12.129 |
| W5500 hardware offload | 1472 | 1 | 100.0% | 0 | 5.548 |
| W5500 hardware offload | 1472 | 4 | 0.0% | 300 | 4.868 |
| W5500 hardware offload | 1472 | 8 | 0.0% | 700 | 5.012 |
| W5500 hardware offload | 1472 | 16 | 0.0% | 1500 | 5.523 |
| W5500 hardware offload | 1472 | 32 | 0.0% | 3000 | 8.565 |
| W5500 hardware offload | 1472 | 64 | 0.0% | 6100 | 12.572 |
| W5500 hardware offload | 1472 | 128 | 0.0% | 12323 | 20.099 |

### Burst interpretation

The burst result measures unpaced queue absorption, not sustained bandwidth.
Native RMII completed every 64-byte burst through four packets, while MACRAW
completed every such burst through 32 packets and 99% of 64- and 128-packet
bursts. MACRAW also completed every 512-byte burst through 16 packets and
every 1,472-byte burst through eight packets.

This is a meaningful tradeoff, not a contradiction of the throughput result:
MACRAW can buffer a burst inside the W5500 and then drain raw frames slowly
through SPI. Native RMII moves steady traffic much faster but this firmware
allocates only four receive DMA descriptors and four Embassy UDP metadata
slots. Offload processes at most one received datagram per 1 ms application
poll and showed limited large-packet burst depth. Buffer and queue tuning
would be a separate configuration and must be benchmarked separately.

## Reliability soak

| Variant | Duration | Target Mbps | Sent | Valid | Missing | Late | Duplicate | Reordered | Corrupt | p99 RTT ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Native RMII + Embassy | 900 s | 1.750 | 296113 | 296110 | 3 | 0 | 0 | 0 | 0 | 1.067 |
| W5500 MACRAW + Embassy | 900 s | 0.219 | 36864 | 36863 | 1 | 0 | 0 | 0 | 0 | 5.034 |
| W5500 hardware offload | 900 s | 0.262 | 44171 | 44171 | 0 | 0 | 0 | 0 | 0 | 4.665 |

The three soaks produced four missing replies in total: three native packets
in two one-minute intervals, one MACRAW packet in one interval, and none for
offload. That is an aggregate loss rate of about 0.0011% across all subjects,
but the offered rates differed because each was derived from that variant's
strict zero-loss capacity. These tests demonstrate short baseline stability;
they do not replace the plan's optional eight-hour release soak.

## Architecture conclusions

For this application, native RMII is the best choice when steady-state
latency and bandwidth matter. It avoids SPI serialization and its median RTT
increased predictably from 0.235 ms at one byte to 1.042 ms at 1,472 bytes.

W5500 hardware offload provides the smallest signed firmware image and
improves typical/tail behavior over MACRAW while keeping the protocol stack in
the chip. At the tested 20 MHz SPI clock and 1 ms polling interval, however,
it does not approach the native interface's performance. It also cannot echo
a zero-byte UDP payload and its current one-datagram-per-poll loop has weak
large-packet burst absorption.

MACRAW is valuable when the application needs Embassy's flexible network
stack over a shield, and its internal buffering handled bursts surprisingly
well. Its cost is moving complete Ethernet frames over SPI and polling the
W5500 every millisecond, which produced the highest latency and lowest
steady-state goodput here.

## Limitations and next experiments

- Traffic traversed the existing switched/router LAN rather than a dedicated
  direct link, so Windows and other LAN activity contribute rare outliers and
  loss.
- Firmware was built from commit
  `d5626aedcd2ac425fb9cb5c84717e7f20e3a414c` with a dirty worktree containing
  the benchmark implementation. The exact source and generated report should
  be committed together before using this as a regression baseline.
- Only one complete baseline run was recorded. Repeating it on a quiet LAN
  would provide confidence intervals and distinguish persistent differences
  from isolated host/network events.
- CPU cycles, device RAM high-water marks, SPI byte counts, and power were not
  instrumented; this report covers externally observed behavior.
- Recovery after cable removal, board reset, and DHCP disruption remains a
  separate manual experiment because an intentional outage should not be
  mixed into steady-state loss.
- Useful tuning studies include W5500 interrupt operation, 40/80 MHz SPI,
  shorter polling, larger native queues, and multiple offload datagrams per
  poll. Each must be labeled as a new configuration.

## Interpretation notes

- Results include the Windows UDP stack, LAN equipment, firmware scheduling, device driver, and active Ethernet interface.
- A zero-loss rate is the highest tested target with complete valid replies and at least 98% of requested offered load.
- Saturation means at least 0.1% loss, less than 95% of requested offered load, or less than 90% returned goodput.
- Each soak runs at 70% of that variant's lowest measured zero-loss rate across the tested payload sizes.
- W5500 hardware offload excludes zero-byte functional echo because the chip does not complete a zero-length SEND command.
- Raw JSON and CSV files are the authoritative measurements behind these rounded tables.
