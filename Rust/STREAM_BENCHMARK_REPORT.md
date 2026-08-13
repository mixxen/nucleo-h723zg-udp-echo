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

## Firmware configuration matrix

These settings materially affect the measurements and must accompany any
C-versus-Rust interpretation:

| Parameter | C/LwIP native RMII | Rust/Embassy native RMII | Rust/Embassy W5500 MACRAW | Rust W5500 offload |
|---|---|---|---|---|
| MCU / AHB clock | 520 / 260 MHz | 400 / 200 MHz normally; 520 / 260 MHz with `performance` | 400 / 200 MHz | 400 / 200 MHz |
| MCU clock source | HSE bypass + PLL | HSI + PLL | HSI + PLL | HSI + PLL |
| Ethernet device path | STM32 MAC + LAN8742A | STM32 MAC + LAN8742A | W5500 MACRAW | W5500 hardwired sockets |
| MCU-to-Ethernet transport | RMII, 50 MHz reference | RMII, 50 MHz reference | SPI1, 20 MHz | SPI1, 20 MHz |
| Physical Ethernet link | Negotiated 10/100 Mb/s | Negotiated 10/100 Mb/s | Negotiated 10/100 Mb/s | Negotiated 10/100 Mb/s |
| IPv4/UDP stack location | MCU: LwIP raw API | MCU: Embassy/smoltcp | MCU: Embassy/smoltcp | W5500 hardware |
| IPv4/UDP checksums | STM32 MAC hardware offload | MCU software | MCU software | W5500 hardware |
| Packet receive servicing | Tight-loop `ethernetif_input()` polling of DMA completion | STM32 Ethernet interrupt + DMA; async task wake | W5500 INTn on PG14/EXTI14 | W5500 INTn on PG14/EXTI14 |
| Normal packet polling period | No fixed period; runs once per main-loop iteration | None; event driven | None; event driven | None; event driven |
| Link/maintenance cadence | Link 100 ms; DHCP 500 ms | PHY link check 500 ms; DHCP timeout 30 s | W5500 link check 500 ms; DHCP by Embassy | DHCP/link recovery 1 s or socket interrupt |
| DMA/raw-frame queues | 4 RX + 4 TX descriptors; 12 × 1,000-byte RX buffers | 4 RX + 4 TX packet queue | 4 RX + 4 TX MCU queues of 1,514-byte frames, plus W5500 memory | W5500 socket memory; allocation left at chip defaults |
| UDP application buffers | LwIP pbufs; 14 KiB LwIP heap | 4 RX + 4 TX slots with 6,144-byte RX/TX storage; 1,536-byte work buffer | Same Embassy UDP buffers as native Rust | 1,536-byte MCU work buffer; W5500 socket RX/TX memory |
| DHCP fallback | After more than 4 attempts: `192.168.0.10/24` | After 30 s: `192.168.0.10/24` | After 30 s: `192.168.0.10/24` | No static fallback |
| Release optimization | GCC `-O2` | Rust `opt-level="z"`, fat LTO | Rust `opt-level="z"`, fat LTO | Rust `opt-level="z"`, fat LTO |
| Cortex-M7 cache policy | I-cache + D-cache; DMA memory made non-cacheable by MPU | I-cache; D-cache off for DMA coherency | I-cache; D-cache off for SPI DMA coherency | I-cache; D-cache off for SPI DMA coherency |
| Success-path logging during benchmark | None | Disabled | Disabled | Disabled |
| Firmware CPU/stack telemetry | Not implemented | Optional profiling feature | Optional profiling feature | Optional profiling feature |

“Event driven” does not mean no CPU work: an interrupt wakes firmware, which
then drains queued packets. The maintenance cadences apply to link and DHCP
housekeeping, not to the normal receive path. The C loop has no configured
poll frequency; its effective rate varies with CPU clock and work performed,
so assigning it an invented hertz value would be misleading.

The two native-RMII rows offer the closest C-versus-Rust comparison because
they use the same MCU MAC, PHY, RMII wiring, LAN, and workload. They still
differ in CPU clock, network stack, driver, execution model, buffer layout,
and compiler optimization goal. Those are part of the current implementation
trade, but they prevent attributing a result to programming language alone.

## Rust/Embassy native optimization

The existing Embassy path was optimized before considering a separate
polling implementation. Four changes were retained:

1. a separate Cargo `performance` profile uses `opt-level=3` while leaving the
   production release optimized for size;
2. the native performance feature uses the same 520 MHz CPU and 260 MHz AHB
   rates as the C sample; and
3. four UDP metadata entries now have four datagrams' worth of byte storage,
   rather than competing for one 1,536-byte buffer; and
4. the Cortex-M7 instruction cache is enabled after Embassy initialization.

| Rust stage | CPU | Optimization | UDP byte capacity | Continuous zero-error range (3 s/rate) | 20 kHz errors |
|---|---:|---|---:|---:|---:|
| Original benchmark baseline | 400 MHz | size (`z`) | 1 datagram/direction | 1-7 kHz | 52,093 |
| Speed compiler only | 400 MHz | speed (`3`) | 1 datagram/direction | 1-8 kHz | 8,179 |
| Speed compiler + socket queues | 400 MHz | speed (`3`) | 4 datagrams/direction | 1-10 kHz | 8,053 |
| Final: speed + queues + clock parity | 520 MHz | speed (`3`) | 4 datagrams/direction | 1-12 kHz | 103 |
| I-cache pass (separate run) | 520 MHz | speed (`3`) + I-cache | 4 datagrams/direction | 1-11 kHz | 63 |

The pre-cache final sweep was non-monotonic: 13 and 14 kHz each lost one or two packets,
15 and 16 kHz were zero-error, and 20 kHz returned 59,897 of 60,000 packets.
Therefore 12 kHz is the strict continuous zero-error range, while the
practical saturation knee is near 20 kHz.

The I-cache performance image uses 69,856 bytes of ELF flash, 29,464 bytes of
static MCU RAM, and produces a 70,512-byte signed MCUboot artifact. The larger
socket queues account for most of the RAM increase relative to the original
single-datagram byte capacity.

The I-cache follow-up was functionally correct and the live Cortex-M7 CCR
changed from `0x00040210` to `0x00060210`, proving that I-cache was active and
D-cache remained off. Its short sweep returned 59,937/60,000 packets at
20 kHz with 0.344 ms p99 RTT. However, an anomalous 58-packet loss at 12 kHz
reduced the strict continuous zero-error range to 11 kHz. Matched 30-second
runs returned 29,999/30,000 at 1 kHz (0.192/0.280 ms p50/p99) and
449,918/450,000 at 15 kHz (0.248/0.397 ms). These samples show improved
instruction-fetch policy but no demonstrated reliability gain; LAN and host
timing noise is large enough that repeated trials are required.

A D-cache experiment placed the 12,360-byte Embassy Ethernet packet queue in
a dedicated 16 KiB non-cacheable MPU region, then enabled D-cache for the
remaining RAM. The board faulted with a precise bus error inside that queue,
so the experiment was fully reverted. D-cache must remain disabled until the
driver performs explicit clean/invalidate maintenance or a separately tested
DMA-memory layout is available.

A matched 30-second 15 kHz run returned 449,939/450,000 packets for optimized
Rust (61 missing, 0.0136% loss) versus 450,000/450,000 for C. Rust p50/p99 RTT
was 0.276/0.383 ms; C was 0.124/0.218 ms. This is close enough in throughput
to retain Embassy for now: the optimized Rust implementation delivered
99.986% of replies at 15 kHz and both implementations are comfortably above
the 1 kHz target. A new polling stack would add substantial code and safety
risk for a margin that the application does not currently require.

The next optimization should be STM32 Ethernet checksum offload in the
Embassy driver, followed by repeated 15 and 20 kHz trials. A specialized
polling/raw-frame Rust path remains a fallback only if a future requirement
demands zero loss near saturation; it is not justified by the current 1 kHz
control workload.

At the target 1 kHz rate, the final image returned 30,000/30,000 packets with
no errors and a 0.193 ms median RTT. Its 0.824 ms p99 and 19.716 ms maximum in
that run reinforce that Windows/LAN tail latency needs repeated trials even
when delivery is complete.

## Connected-board result

Measured on 2026-08-12 and 2026-08-13 using the same Windows host, router, NUCLEO-H723ZG,
cables, 20 MHz W5500 SPI clock, release optimization, and benchmark logging
configuration:

| Variant | Valid / sent | Missing | Late | Duplicate | Reordered | Corrupt | p50 RTT | p99 RTT | Max RTT |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| C/LwIP native RMII | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.118 ms | 0.352 ms | 9.436 ms |
| Native RMII + Embassy | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.278 ms | 0.369 ms | 16.173 ms |
| W5500 MACRAW + Embassy | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.687 ms | 1.761 ms | 31.528 ms |
| W5500 hardware offload | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.438 ms | 0.914 ms | 11.995 ms |

All four met the short-run reliability gate and achieved exactly 1,000.000
sent commands/s. C/LwIP native RMII had the lowest observed typical and tail
RTT; Rust/Embassy native RMII was close at p99. W5500 offload was the better
of the two SPI designs in this run. The C image runs the MCU at 520 MHz while
the Rust images use 400 MHz, so this is an implementation comparison, not
evidence that language alone caused the latency difference. Use repeatable
one-hour and 8-hour runs before treating tail values as qualification data.

## Preliminary reliability knee

A 3-second-per-rate engineering sweep tested the 100-byte command stream from
1 through 20 kHz in 1 kHz increments. "Reliable" requires at least 98%
achieved offered rate and zero missing, late (50 ms timeout), duplicate,
reordered, corrupt, foreign, or send-error packets.

| Variant | Reliable range | First unreliable | First-failure evidence |
|---|---:|---:|---|
| C/LwIP native RMII | 1 through 15 kHz | 16 kHz | 47,999 / 48,000 valid; 1 missing |
| Native RMII + Embassy | 1 through 7 kHz | 8 kHz | 21,462 / 24,000 valid; 2,538 error events |
| W5500 MACRAW + Embassy | 1 kHz only | 2 kHz | 5,705 / 6,000 valid; 4,783 error events |
| W5500 hardware offload | 1 through 3 kHz | 4 kHz | 10,215 / 12,000 valid; 1,785 error events |

### Error events at each increment

| Target kHz | C/LwIP RMII | Rust/Embassy RMII | W5500 offload | W5500 MACRAW |
|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 |
| 2 | 0 | 0 | 0 | 4,783 |
| 3 | 0 | 0 | 0 | 8,777 |
| 4 | 0 | 0 | 1,785 | 11,838 |
| 5 | 0 | 0 | 4,785 | 14,864 |
| 6 | 0 | 0 | 7,785 | 17,875 |
| 7 | 0 | 0 | 10,785 | 20,883 |
| 8 | 0 | 2,538 | 13,787 | 23,888 |
| 9 | 0 | 6,969 | 16,785 | 26,892 |
| 10 | 0 | 9,839 | 19,785 | 29,896 |
| 11 | 0 | 15,042 | 22,784 | 32,896 |
| 12 | 0 | 19,380 | 25,785 | 35,898 |
| 13 | 0 | 23,235 | 28,785 | 38,900 |
| 14 | 0 | 27,334 | 31,785 | 41,900 |
| 15 | 0 | 31,350 | 34,782 | 44,903 |
| 16 | 1 | 35,565 | 37,784 | 47,904 |
| 17 | 0 | 39,568 | 40,784 | 50,904 |
| 18 | 23 | 43,800 | 43,785 | 53,904 |
| 19 | 0 | 47,876 | 46,787 | 56,904 |
| 20 | 0 | 52,088 | 49,785 | 59,904 |

An error event is one missing, late, duplicate, reordered, corrupt, foreign,
or send-error observation. A packet can contribute more than one event; for
example, MACRAW at 2 kHz recorded 295 missing and 4,488 late events. The CSV
output preserves that detailed breakdown rather than only the total.

These are preliminary knees from short trials. Confirm with the default
30-second dwell, repeat the boundary rates, and use a longer soak at the
selected operating margin before establishing a requirement or regression
threshold.

## Complexity and memory

| Variant | Bring-up NCLOC | UDP server NCLOC | Total NCLOC | Signed image | Static MCU RAM |
|---|---:|---:|---:|---:|---:|
| C/LwIP native RMII | 620 | 29 | 649 | 126,824 B ELF flash | 53,051 B |
| Native RMII + Embassy | 144 | 48 | 192 | 60,080 B signed | 29,488 B |
| W5500 MACRAW + Embassy | 263 | 48 | 311 | 66,816 B signed | 30,232 B |
| W5500 hardware offload | 261 | 53 | 314 | 25,896 B signed | 3,132 B |

Static MCU RAM is the release ELF's `.data + .bss + .uninit`; the W5500's
external 32 KiB packet RAM is not counted. A separate `profiling` firmware
feature now measures Embassy executor busy cycles and runtime stack
high-water. This keeps instrumentation overhead out of the baseline numbers
above while allowing repeatable CPU and memory comparisons.

The profile reports executor task-polling time, not total MCU utilization: it
does not count interrupt-handler time, DMA, or work offloaded into the W5500.
Use profiling results comparatively and do not combine them with results from
an ordinary benchmark image.

The C count includes `main.c` because Cube-generated clock, MPU, main-loop,
and network integration are coupled in that file. External LwIP/HAL sources
and external Rust crates are excluded. Consequently, NCLOC describes the
checked-in integration burden, not the total implementation size of either
language ecosystem. C's image is an unsigned ELF footprint; Rust's column is
the signed MCUboot artifact, so use the RAM and NCLOC columns for the cleaner
source-level comparison.

## Reproduction

```powershell
# Quick validation: 30-second stream and 3 seconds per sweep rate
.\Rust\tools\run-stream-benchmark-comparison.ps1 `
    -Version 0.4.2 `
    -CNativeIp 192.168.68.119 `
    -NativeIp 192.168.68.117 `
    -W5500Ip 192.168.68.74 `
    -Quick

# Rate sweep only; defaults to 30 seconds at every 1 kHz step
.\Rust\tools\udp-benchmark\target\x86_64-pc-windows-msvc\release\udp-benchmark.exe stream-sweep `
    --board 192.168.68.74 `
    --profile

# Source and current profiling-image memory metrics
.\Rust\tools\measure-variants.ps1 -Profiling
```

Without `-Quick`, the comparison runner uses a one-hour 1 kHz stream for each
implementation before its 30-second-per-rate sweep. Raw output includes all
network error counters and static RAM for every image. Executor CPU,
cycles-per-valid-packet, and stack high-water are present for the three Rust
profiling images; those fields are blank for C/LwIP.

Addresses are DHCP leases, not firmware constants. Raw JSON/CSV results are
written below `Rust/benchmark-results/` and intentionally ignored by Git.

This profile was motivated by the fast-steering-mirror command stream, but it
is a generic transport benchmark rather than a complete mirror-control test:
the firmware echoes bytes but does not yet decode commands, latch them on a
1 kHz control tick, or drive and measure the mirror. Those actuator and
command-age checks belong in a later hardware-in-the-loop acceptance test.
