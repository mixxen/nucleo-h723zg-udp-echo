# Fixed-rate UDP stream benchmark

## Workload

This engineering baseline sends exactly 30,000 UDP command datagrams per
variant: 100 bytes at 1,000 Hz for 30 seconds. The 100 bytes include a
sequence, run ID, host timestamp, and deterministic command-like payload.
Every implementation returns the complete packet, allowing the host to count
loss, lateness, duplication, reordering, corruption, and application RTT.

Both W5500 implementations use the shield's active-low INTn output routed by
Arduino D2 to STM32 PG14/EXTI14. The old 1 ms receive polling object has been
deleted. MACRAW retains Embassy's 500 ms link check; offload retains a
one-second DHCP/recovery timer. Neither timer is used for normal packet
delivery.

## Connected-board result

Measured on 2026-08-12 using the same Windows host, router, NUCLEO-H723ZG,
cables, 20 MHz W5500 SPI clock, release optimization, and benchmark logging
configuration:

| Variant | Valid / sent | Missing | Late | Duplicate | Reordered | Corrupt | p50 RTT | p99 RTT | Max RTT |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Native RMII + Embassy | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.278 ms | 0.369 ms | 16.173 ms |
| W5500 MACRAW + Embassy | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.687 ms | 1.761 ms | 31.528 ms |
| W5500 hardware offload | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.438 ms | 0.914 ms | 11.995 ms |

All three met the short-run reliability gate and achieved exactly 1,000.000
sent commands/s. Native RMII had the lowest typical and p99 RTT. W5500
offload was the better of the two SPI designs in this run. The 30-second
result validates implementation and wiring; use the repeatable 15-minute and
8-hour runs before treating tail values as qualification data.

## Preliminary reliability knee

A 3-second-per-rate engineering sweep tested the 100-byte command stream from
1 through 20 kHz in 1 kHz increments. "Reliable" requires at least 98%
achieved offered rate and zero missing, late (50 ms timeout), duplicate,
reordered, corrupt, foreign, or send-error packets.

| Variant | Reliable range | First unreliable | First-failure evidence |
|---|---:|---:|---|
| Native RMII + Embassy | 1 through 7 kHz | 8 kHz | 21,462 / 24,000 valid; 2,538 error events |
| W5500 MACRAW + Embassy | 1 kHz only | 2 kHz | 5,705 / 6,000 valid; 4,783 error events |
| W5500 hardware offload | 1 through 3 kHz | 4 kHz | 10,215 / 12,000 valid; 1,785 error events |

### Error events at each increment

| Target kHz | Native RMII | W5500 offload | W5500 MACRAW |
|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 |
| 2 | 0 | 0 | 4,783 |
| 3 | 0 | 0 | 8,777 |
| 4 | 0 | 1,785 | 11,838 |
| 5 | 0 | 4,785 | 14,864 |
| 6 | 0 | 7,785 | 17,875 |
| 7 | 0 | 10,785 | 20,883 |
| 8 | 2,538 | 13,787 | 23,888 |
| 9 | 6,969 | 16,785 | 26,892 |
| 10 | 9,839 | 19,785 | 29,896 |
| 11 | 15,042 | 22,784 | 32,896 |
| 12 | 19,380 | 25,785 | 35,898 |
| 13 | 23,235 | 28,785 | 38,900 |
| 14 | 27,334 | 31,785 | 41,900 |
| 15 | 31,350 | 34,782 | 44,903 |
| 16 | 35,565 | 37,784 | 47,904 |
| 17 | 39,568 | 40,784 | 50,904 |
| 18 | 43,800 | 43,785 | 53,904 |
| 19 | 47,876 | 46,787 | 56,904 |
| 20 | 52,088 | 49,785 | 59,904 |

An error event is one missing, late, duplicate, reordered, corrupt, foreign,
or send-error observation. A packet can contribute more than one event; for
example, MACRAW at 2 kHz recorded 295 missing and 4,488 late events. The CSV
output preserves that detailed breakdown rather than only the total.

These are preliminary knees from short trials. Confirm with the default
10-second dwell, repeat the boundary rates, and use a longer soak at the
selected operating margin before establishing a requirement or regression
threshold.

## Complexity and memory

| Variant | Bring-up NCLOC | UDP server NCLOC | Total NCLOC | Signed image | Static MCU RAM |
|---|---:|---:|---:|---:|---:|
| Native RMII + Embassy | 136 | 47 | 183 | 59,848 B | 20,248 B |
| W5500 MACRAW + Embassy | 255 | 47 | 302 | 66,592 B | 20,992 B |
| W5500 hardware offload | 236 | 53 | 289 | 25,728 B | 3,124 B |

Static MCU RAM is the release ELF's `.data + .bss + .uninit`; it does not
measure runtime stack high-water use. The W5500's external 32 KiB packet RAM
is not counted. CPU utilization is also not claimed from host RTT: accurate
MCU CPU accounting requires Embassy executor tracing or an external trace
probe, and adding that instrumentation would define a new benchmark build.

## Reproduction

```powershell
# Quick 30-second comparison used for this engineering baseline
.\Rust\tools\run-stream-benchmark-comparison.ps1 `
    -Version 0.4.1 `
    -NativeIp 192.168.68.117 `
    -W5500Ip 192.168.68.74 `
    -Quick

# Rate sweep only; defaults to 10 seconds at every 1 kHz step
.\Rust\tools\udp-benchmark\target\x86_64-pc-windows-msvc\release\udp-benchmark.exe stream-sweep `
    --board 192.168.68.74

# Source and current release-ELF memory metrics
.\Rust\tools\measure-variants.ps1 -Benchmark
```

Addresses are DHCP leases, not firmware constants. Raw JSON/CSV results are
written below `Rust/benchmark-results/` and intentionally ignored by Git.

This profile was motivated by the fast-steering-mirror command stream, but it
is a generic transport benchmark rather than a complete mirror-control test:
the firmware echoes bytes but does not yet decode commands, latch them on a
1 kHz control tick, or drive and measure the mirror. Those actuator and
command-age checks belong in a later hardware-in-the-loop acceptance test.
