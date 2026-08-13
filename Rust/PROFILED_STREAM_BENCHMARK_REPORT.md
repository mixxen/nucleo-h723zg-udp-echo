# Profiled one-hour UDP stream benchmark

## Executive summary

On 2026-08-13, all three Rust firmware variants sustained the representative
100-byte, 1,000-datagram/s stream for one hour. None achieved literal zero
loss over 3.6 million requests, but all loss rates were below 0.0006%. W5500
hardware offload returned the most packets, native RMII used the least
executor CPU and had the lowest median latency, and W5500 MACRAW used the most
CPU, stack, and static MCU RAM.

| Variant | Valid / sent | Missing | Loss | p50 RTT | p99 RTT | Max RTT | Executor CPU | Cycles / valid | Stack high-water | Static MCU RAM |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Native RMII + Embassy | 3,599,981 / 3,600,000 | 19 | 0.000528% | 0.303 ms | 0.543 ms | 22.518 ms | 25.68% | 106,606 | 25,492 B | 20,560 B |
| W5500 MACRAW + Embassy | 3,599,979 / 3,600,000 | 21 | 0.000583% | 0.674 ms | 1.500 ms | 29.184 ms | 54.06% | 224,403 | 31,404 B | 21,304 B |
| W5500 hardware offload | 3,599,996 / 3,600,000 | 4 | 0.000111% | 0.448 ms | 0.565 ms | 29.559 ms | 34.20% | 141,980 | 920 B | 3,168 B |

There were no late, duplicate, reordered, corrupt, foreign, or host send-error
events in any one-hour stream. The missing packets were intermittent rather
than a permanent outage. Native RMII's largest 60-second event contained 12
missing packets; MACRAW's largest contained four; offload's four losses were
single-packet events in four separate intervals.

These are end-to-end observations. A missing reply can be lost in the Windows
host, LAN, Ethernet device, driver, firmware queue, or return path; this test
does not assign every loss to the board alone.

If the control requirement is absolute zero loss, **none of these end-to-end
configurations passes that requirement based on this run**. UDP itself does
not guarantee delivery. A real control protocol may need sequence checking,
stale-command handling, and an explicitly safe behavior for a missed update.

## CPU and memory interpretation

At 1 kHz, native RMII required the least measured Embassy executor time.
Relative to native, W5500 hardware offload used about 1.33 times the executor
cycles per valid packet, while MACRAW used about 2.10 times as many. Offload
nevertheless had the smallest MCU memory footprint because the W5500 owns the
UDP/IP socket state and packet RAM:

| Variant | Static + observed stack | Stack capacity | Executor polls |
|---|---:|---:|---:|
| Native RMII + Embassy | 46,052 B | 110,512 B | 14,461,125 |
| W5500 MACRAW + Embassy | 52,708 B | 109,768 B | 17,941,353 |
| W5500 hardware offload | 4,088 B | 127,904 B | 7,208,434 |

These are profiling-build measurements. Executor CPU is the percentage of
wall time spent polling Embassy tasks; it excludes interrupt-handler time,
DMA activity, and computation performed inside the W5500. Static MCU RAM is
the profiling image's allocation before runtime stack use. The W5500's
external 32 KiB packet RAM is not included.

## Thirty-second rate sweep

Each variant was also tested with 100-byte datagrams for 30 seconds at every
integer rate from 1 through 20 kHz. A point is marked reliable only when the
host offers at least 98% of the target and every packet returns exactly once,
in order, uncorrupted, and before the 50 ms late threshold.

| Variant | Continuous zero-error range | First failure | Practical saturation evidence |
|---|---:|---:|---|
| Native RMII + Embassy | 1-3 kHz | 4 kHz: 1 missing | 6 kHz had 2 missing; 7 kHz had 13,326 error events |
| W5500 MACRAW + Embassy | 1 kHz | 2 kHz: 57,630 events | At 2 kHz, 1,648 missing and 55,982 late |
| W5500 hardware offload | 1-3 kHz | 4 kHz: 21,364 missing | Loss increased at every rate from 4 kHz upward |

Native RMII was non-monotonic near its boundary: 4 kHz lost one packet, 5 kHz
lost none, and 6 kHz lost two. Its continuous zero-error result is therefore
3 kHz, but the clear capacity knee is between 6 and 7 kHz. This distinction is
more informative than claiming either 3 or 6 kHz as a single exact limit.

### Error events at every rate

| Target kHz | Native RMII | W5500 MACRAW | W5500 offload |
|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 |
| 2 | 0 | 57,630 | 0 |
| 3 | 0 | 89,744 | 0 |
| 4 | 1 | 119,826 | 21,364 |
| 5 | 0 | 149,852 | 51,357 |
| 6 | 2 | 179,868 | 81,359 |
| 7 | 13,326 | 209,876 | 111,360 |
| 8 | 53,283 | 239,880 | 141,349 |
| 9 | 83,050 | 269,884 | 171,353 |
| 10 | 133,650 | 299,888 | 201,349 |
| 11 | 177,024 | 329,892 | 231,341 |
| 12 | 219,855 | 359,892 | 261,343 |
| 13 | 260,052 | 389,894 | 291,349 |
| 14 | 302,003 | 419,896 | 321,349 |
| 15 | 341,813 | 449,896 | 351,347 |
| 16 | 383,816 | 479,896 | 381,371 |
| 17 | 426,399 | 509,898 | 411,353 |
| 18 | 469,063 | 539,900 | 441,357 |
| 19 | 511,697 | 569,900 | 471,351 |
| 20 | 554,345 | 599,900 | 501,351 |

An error event is one missing, late, duplicate, reordered, corrupt, foreign,
or host send-error observation. One packet can contribute to more than one
event. The raw CSV retains every component.

CPU measurements are trustworthy for the 1 kHz one-hour streams and for
non-saturated sweep points. Do not use the reported offload CPU percentage at
4 kHz and above: its continuously-ready task can remain in one executor poll
longer than the Cortex-M7 DWT cycle counter's approximately 10.7-second wrap
period, causing undercounting. Network reliability counters are unaffected.

## Method and reproducibility

The run used firmware version `0.4.2`, release optimization, profiling enabled
on every architecture, a 20 MHz W5500 SPI clock, the same Windows host and LAN,
and the following command:

```powershell
.\Rust\tools\run-stream-benchmark-comparison.ps1 `
    -Version 0.4.2 `
    -NativeIp 192.168.68.117 `
    -W5500Ip 192.168.68.74 `
    -SkipFirmwareBuild
```

The complete machine-readable result set is in the locally generated,
Git-ignored directory:

```text
Rust/benchmark-results/profile-full-20260812-231558/
```

It contains per-variant stream JSON/CSV, sweep JSON/CSV, generated summaries,
`comparison.json`, and `rate-comparison.csv`. All 20 artifacts were present,
all JSON was parsed, and SHA-256 hashes were calculated after the run to
confirm stable artifacts during review. The run lasted 13,130 seconds (3
hours, 38 minutes, 50 seconds), including flashing, readiness checks,
one-hour streams, sweeps, and drains.

The board was left running the W5500 hardware-offload profiling image at
`192.168.68.74`.

## Conclusions

For a 100-byte, 1 kHz command stream:

- native RMII offers the lowest MCU executor cost and best typical latency;
- W5500 hardware offload offers the lowest observed loss and by far the
  smallest MCU RAM requirement, with moderate CPU and latency cost; and
- W5500 MACRAW is the least attractive of these implementations for this
  workload because it has the highest CPU, latency, stack use, and observed
  loss, and its reliable rate ceiling is only 1 kHz.

The next control-oriented test should add command-age deadlines and the
application's behavior after a sequence gap. An eight-hour 1 kHz soak would
then provide a stronger reliability estimate than repeating another broad
1-20 kHz sweep.
