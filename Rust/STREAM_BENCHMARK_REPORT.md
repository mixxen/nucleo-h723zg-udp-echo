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
| MCU / AHB clock | 520 / 260 MHz | 400 / 200 MHz normally; 520 / 260 MHz with `performance` | 400 / 200 MHz normally; 520 / 260 MHz with `performance` | 400 / 200 MHz normally; 520 / 260 MHz with `performance` |
| MCU clock source | HSE bypass + PLL | HSI + PLL | HSI + PLL | HSI + PLL |
| Ethernet device path | STM32 MAC + LAN8742A | STM32 MAC + LAN8742A | W5500 MACRAW | W5500 hardwired sockets |
| MCU-to-Ethernet transport | RMII, 50 MHz reference | RMII, 50 MHz reference | SPI1, 20 MHz release; dedicated 40 MHz PLL2-P clock with `performance` | SPI1, 50 MHz release; dedicated 40 MHz PLL2-P clock with `performance` |
| Physical Ethernet link | Negotiated 10/100 Mb/s | Negotiated 10/100 Mb/s | Negotiated 10/100 Mb/s | Negotiated 10/100 Mb/s |
| IPv4/UDP stack location | MCU: LwIP raw API | MCU: Embassy network stack (`xarxa` at pinned revision) | MCU: Embassy network stack (`xarxa` at pinned revision) | W5500 hardware |
| IPv4/UDP checksums | STM32 MAC hardware offload | STM32 MAC hardware RX/TX offload | MCU software | W5500 hardware |
| Packet receive servicing | Tight-loop `ethernetif_input()` polling of DMA completion | STM32 Ethernet interrupt + DMA; async task wake | W5500 INTn on PG14/EXTI14 | W5500 INTn on PG14/EXTI14 |
| Normal packet polling period | No fixed period; runs once per main-loop iteration | None; event driven | None; event driven | None; event driven |
| Link/maintenance cadence | Link 100 ms; DHCP 500 ms | PHY link check 500 ms; DHCP timeout 30 s | W5500 link check 500 ms; DHCP by Embassy | DHCP/link recovery 1 s or socket interrupt |
| DMA/raw-frame queues | 4 RX + 4 TX descriptors; 12 × 1,000-byte RX buffers | 4 RX + 4 TX packet queue | 4 RX + 4 TX MCU queues of 1,514-byte frames, plus W5500 memory | W5500 socket memory; allocation left at chip defaults |
| UDP application buffers | LwIP pbufs; 14 KiB LwIP heap | 4 RX + 4 TX slots with 6,144-byte RX/TX storage; 1,536-byte work buffer | Same Embassy UDP buffers as native Rust | 1,536-byte MCU work buffer; W5500 socket RX/TX memory |
| DHCP fallback | After more than 4 attempts: `192.168.0.10/24` | After 30 s: `192.168.0.10/24` | After 30 s: `192.168.0.10/24` | No static fallback |
| Release optimization | GCC `-O2` | Rust `opt-level="z"` normally; `3` for `performance`; fat LTO | Rust `opt-level="z"` normally; `3` for `performance`; fat LTO | Rust `opt-level="z"` normally; `3` for `performance`; fat LTO |
| Cortex-M7 cache policy | I-cache + D-cache; DMA memory made non-cacheable by MPU | I-cache; D-cache off for DMA coherency | I-cache normally; I-cache + D-cache with `performance` | I-cache normally; I-cache + D-cache with `performance` because the SPI path is CPU-driven |
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
polling implementation. Five changes were retained:

1. a separate Cargo `performance` profile uses `opt-level=3` while leaving the
   production release optimized for size;
2. the native performance feature uses the same 520 MHz CPU and 260 MHz AHB
   rates as the C sample; and
3. four UDP metadata entries now have four datagrams' worth of byte storage,
   rather than competing for one 1,536-byte buffer; and
4. the Cortex-M7 instruction cache is enabled after Embassy initialization;
   and
5. Embassy is pinned to reviewed revision `0af1937a`, whose STM32H7 driver
   enables full IPv4/TCP/UDP checksum insertion and validation in the MAC.

| Rust stage | CPU | Optimization | UDP byte capacity | Continuous zero-error range (3 s/rate) | 20 kHz errors |
|---|---:|---|---:|---:|---:|
| Original benchmark baseline | 400 MHz | size (`z`) | 1 datagram/direction | 1-7 kHz | 52,093 |
| Speed compiler only | 400 MHz | speed (`3`) | 1 datagram/direction | 1-8 kHz | 8,179 |
| Speed compiler + socket queues | 400 MHz | speed (`3`) | 4 datagrams/direction | 1-10 kHz | 8,053 |
| Final: speed + queues + clock parity | 520 MHz | speed (`3`) | 4 datagrams/direction | 1-12 kHz | 103 |
| I-cache pass (separate run) | 520 MHz | speed (`3`) + I-cache | 4 datagrams/direction | 1-11 kHz | 63 |
| RX checksum offload experiment | 520 MHz | speed (`3`) + I-cache + MAC RX checks | 4 datagrams/direction | 1-10 kHz | 11 |
| **Retained full RX/TX checksum offload** | **520 MHz** | **speed (`3`) + I-cache + MAC RX/TX checks** | **4 datagrams/direction** | **1-11 kHz** | **0** |

The RX-only checksum row records a reverted controlled experiment. The full
RX/TX row is the retained firmware configuration. Its strict continuous
range stops at 11 kHz only because the Windows sender produced 35,999 rather
than 36,000 packets at 12 kHz; the board lost none of the packets actually
sent. The same sweep was lossless at 14, 15, and 20 kHz, so the strict range
is useful for automation but is not a monotonic saturation boundary.

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

An RX-checksum-offload experiment then enabled `MACCR.IPC`, kept
`MTLRXQOMR.DIS_TCP_EF` clear so the MAC dropped checksum-error packets, and
wrapped Embassy's driver capabilities so smoltcp generated TX checksums but
trusted hardware on RX. Live registers confirmed both settings and 100
functional datagrams spanning 0 through 1,472 bytes passed. The short sweep
had only 11 errors at 20 kHz, but one miss at 11 kHz made its strict
zero-error range 1-10 kHz. Three 30-second 15 kHz trials missed 87, 87, and 55
of 450,000 packets (76 average), which overlaps the previous 61-82 range. Two
30-second 20 kHz trials missed 323 and 335 of 600,000 packets. Because RX-only
offload added about 55 integration lines and a direct PAC dependency without
a demonstrated delivery improvement, it was reverted. A malformed-checksum
injection test could not run because Windows denied creation of the required
raw socket without elevation; the hardware configuration itself was verified
through live registers.

A matched 30-second 15 kHz run before full offload returned
449,939/450,000 packets for optimized Rust (61 missing, 0.0136% loss) versus
450,000/450,000 for C. Rust p50/p99 RTT was 0.276/0.383 ms; C was
0.124/0.218 ms.

The retained upstream Embassy driver sets `CIC=0b11` in every TX descriptor,
enables `MACCR.IPC`, rejects hardware-reported RX checksum failures, and tells
the IP stack not to repeat those checks in software. The board passed 200
binary echo requests across 0, 1, 100, and 1,472-byte payloads, and live
`MACCR=0x0810e003` confirmed that IPC was enabled. Three 30-second trials at
15 kHz missed 2, 4, and 20 of 450,000 packets (8.7 average), an 89% reduction
from the RX-only experiment's 76-packet average. Their p99 RTTs were
0.287, 0.284, and 0.315 ms. Two 30-second 20 kHz trials missed 36 and 240 of
600,000 packets, compared with 323 and 335 during RX-only offload. At 1 kHz,
the retained image returned 30,000/30,000 with 0.146/0.220 ms p50/p99 RTT.

These results bring the existing async path close enough to C that a second
polling/raw-frame implementation is not justified now. C remained perfect in
its single 15 kHz comparison and had a lower 0.218 ms p99, but full-offload
Rust delivered 99.9981% on average across three trials and has substantial
margin over the 1 kHz requirement. More repeated C trials are needed before
interpreting the residual difference as architectural rather than host/LAN
variation.

## W5500 MACRAW optimization

MACRAW now has a separate `performance` build so the original 400 MHz,
20 MHz SPI, size-optimized release remains an unchanged control. The retained
performance image applies the same low-risk board-level changes validated on
offload: Cargo `opt-level=3`, 520/260 MHz MCU/AHB clocks, the independent
40 MHz SPI1 clock, D-cache for the CPU-driven SPI path, and disabled
success-path logging. It does not change the MACRAW protocol architecture or
Embassy UDP server.

| MACRAW stage | MCU | Effective SPI | Cache | Strict zero-error sweep | Saturation plateau |
|---|---:|---:|---|---:|---:|
| Release control | 400 MHz, size (`z`) | 20 MHz | I-cache | 1 kHz | about 1,900 valid/s |
| **Retained performance** | **520 MHz, speed (`3`)** | **40 MHz** | **I-cache + D-cache** | **1-9 kHz** | **about 10,634 valid/s** |

The short-sweep clean boundary improved by 9x and the plateau by about 5.6x.
At 10 kHz the latest optimized sweep returned 29,634/30,000 packets; above the
plateau, p99 latency increased to approximately 12 ms as frames queued. The
3-second 1 kHz point returned 3,000/3,000 with 0.312 ms p99, compared with
0.687/1.761 ms p50/p99 in the earlier 30-second release-control run.

Longer boundary trials refined the operating margin: 9 kHz returned
269,993/270,000 packets, while 8 kHz returned all 240,000 packets with
0.505 ms p99. Therefore 8 kHz is the current conservative sustained point.
An eight-frame-per-direction driver-queue experiment was rejected: its
30-second 9 kHz run returned only 269,662/270,000 while consuming additional
SRAM. The original four-frame queues were restored, showing that sustained
raw-frame processing and SPI transactions—not queue depth—set the next
bottleneck.

The connected board passed 100 randomized binary echoes spanning 1, 100, and
1,472 bytes, and live `CCR=0x00070210` confirmed both caches. The performance
signed image is 83,696 bytes and uses 30,240 bytes of static MCU RAM.

## W5500 hardware-offload optimization

The offload path was optimized separately using the current interrupt-driven
firmware as its control. WIZnet's official EVB loopback example informed the
tight drain loop and explicit socket-oriented design, while the W5500's
variable-data-length SPI mode was already provided by `w5500-ll`. The old EVB
example itself uses only a 10 MHz blocking SPI implementation, so it is an API
reference rather than a modern DMA performance baseline.

Five changes were retained:

1. the ordinary offload build requests a 50 MHz SPI clock instead of 20 MHz;
2. a separate offload `performance` build uses `opt-level=3` and the same
   520/260 MHz MCU/AHB clock as the native performance build; and
3. the echo server caches its last UDP peer, avoiding a redundant six-byte
   W5500 destination-register write for every packet in a same-host stream;
   and
4. performance-mode SPI1 uses an independent 80 MHz PLL2-P kernel clock and
   its /2 divider, preserving the 520 MHz CPU while raising SPI from about
   32.5 MHz to a board-validated 40 MHz; and
5. D-cache is enabled only for performance-mode W5500 firmware, whose blocking
   SPI path has no DMA coherency hazard.

| Offload stage | MCU | Effective SPI | Strict zero-error sweep | Saturation plateau | 1 kHz p50 / p99 |
|---|---:|---:|---:|---:|---:|
| Fresh control | 400 MHz, size (`z`) | existing 20 MHz request | 1-7 kHz | 7,034 valid/s | 0.296 / 0.403 ms |
| SPI clock only | 400 MHz, size (`z`) | 50 MHz | 1-7 kHz | 7,554 valid/s | 0.288 / 0.977 ms |
| Performance clock/compiler | 520 MHz, speed (`3`) | approximately 32.5 MHz | 1-11 kHz | 11,561 valid/s | 0.234 / 0.403 ms |
| Performance + cached peer | 520 MHz, speed (`3`) | approximately 32.5 MHz | 1-12 kHz on repeat sweep | 12,117 valid/s | 0.231 / 0.435 ms |
| Dedicated SPI clock | 520 MHz, speed (`3`) | 40 MHz | 1-14 kHz | about 14,242 valid/s | not rerun at 1 kHz |
| **Retained W5500 D-cache** | **520 MHz, speed (`3`) + I/D-cache** | **40 MHz** | **1-15 kHz** | **about 15,421 valid/s** | **not rerun at 1 kHz** |

The final plateau is about 119% above the fresh control. The preceding
cached-peer stage's first full sweep had four missing packets at 5 kHz but
was clean from 6 through 12 kHz; a repeated 1-12 kHz sweep and a separate
50,000-packet 5 kHz run were entirely clean.
A 30-second 12 kHz trial on the 32.5 MHz stage returned 359,870/360,000
packets (0.036% loss). The retained 40 MHz build subsequently returned all
420,000/420,000 packets during a 30-second 14 kHz boundary dwell, making
14 kHz the measured sustained point for that stage. Enabling D-cache then
returned all 450,000/450,000 packets during a 30-second 15 kHz dwell, making
15 kHz the new measured sustained point. All earlier 1 kHz trials returned
30,000/30,000.

The live Cortex-M7 cache-control register was `CCR=0x00070210`, confirming
that both I-cache and D-cache were enabled in the retained image. The cache
change is deliberately gated by both the W5500/SPI and `performance` choices;
native RMII retains I-cache only because its Ethernet DMA buffers are not
cache-maintained.

An attempted 520 MHz build put SPI1 near 65 MHz. It obtained no DHCP lease and
answered neither UDP nor ARP through the stacked shield headers, despite that
rate being below the chip's nominal 80 MHz limit. The retained performance
clock initially used a larger PLL divider and approximately 32.5 MHz SPI.
A follow-up independent PLL2-P experiment also failed DHCP at 50 MHz with the
520 MHz CPU, while 40 MHz passed DHCP, binary echo at 1/100/1,472 bytes, the
complete sweep, and the longer boundary dwell. This records the tested
board-level limit without claiming whether the failure was inside the shield,
connector signal path, or MCU SPI timing.

DMA was reviewed but not added in this pass. The high-level hardwired-socket
API is synchronous, and a DMA transfer cannot overlap the next W5500 command
on the same serial bus. Adding a synchronous-to-async DMA bridge would mainly
reduce MCU busy cycles while adding setup cost to many 1-9 byte register
transactions. The measured first priorities were therefore SPI clock,
compiler/CPU speed, and transaction removal. DMA remains a useful follow-up
for CPU-utilization and large-payload measurements, not an assumed throughput
win for the 100-byte stream.

## Connected-board result

Measured on 2026-08-12 and 2026-08-13 using the same Windows host, router,
NUCLEO-H723ZG, cables, and 100-byte/1 kHz workload. The rows intentionally
state their firmware configuration and duration because the retained
performance images are not configuration-identical to their controls:

| Variant | Configuration | Duration | Valid / sent | Missing | Late | Duplicate | Reordered | Corrupt | p50 RTT | p99 RTT | Max RTT |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| C/LwIP native RMII | 520 MHz, GCC `-O2` | 30 s | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.118 ms | 0.352 ms | 9.436 ms |
| Native RMII + Embassy control | 400 MHz, size (`z`) | 30 s | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.278 ms | 0.369 ms | 16.173 ms |
| **Native RMII retained performance** | **520 MHz, speed (`3`), full checksum offload** | **30 s** | **30,000 / 30,000** | **0** | **0** | **0** | **0** | **0** | **0.146 ms** | **0.220 ms** | **1.831 ms** |
| W5500 MACRAW control | 400 MHz, 20 MHz SPI, size (`z`) | 30 s | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.687 ms | 1.761 ms | 31.528 ms |
| **W5500 MACRAW retained performance** | **520 MHz, 40 MHz SPI, speed (`3`), I/D-cache** | **3 s** | **3,000 / 3,000** | **0** | **0** | **0** | **0** | **0** | **0.230 ms** | **0.527 ms** | **2.192 ms** |
| W5500 offload fresh control | 400 MHz, 20 MHz SPI, size (`z`) | 30 s | 30,000 / 30,000 | 0 | 0 | 0 | 0 | 0 | 0.296 ms | 0.403 ms | 3.439 ms |
| **W5500 offload retained performance** | **520 MHz, 40 MHz SPI, speed (`3`), I/D-cache** | **3 s** | **3,000 / 3,000** | **0** | **0** | **0** | **0** | **0** | **0.194 ms** | **0.340 ms** | **1.834 ms** |

Every row met the short-run 1 kHz reliability gate and achieved exactly the
requested offered rate. The retained native image has a matched 30-second
sample; the retained W5500 values come from the 1 kHz points of their latest
3-second sweeps. Their latency maxima therefore must not be compared directly
with the 30-second maxima. The latest samples show substantial latency gains
for both W5500 paths, with hardware offload remaining faster than MACRAW.
These are complete-stack implementation comparisons, not evidence that
language alone caused a latency difference. Use matched repeatable one-hour
and 8-hour runs before treating tail values as qualification data.

## Preliminary reliability knee

A 3-second-per-rate engineering sweep tested the 100-byte command stream from
1 through 30 kHz in 1 kHz increments. "Reliable" requires at least 98%
achieved offered rate and zero missing, late (50 ms timeout), duplicate,
reordered, corrupt, foreign, or send-error packets.

The reliability label also requires the host to send every planned packet.
Consequently, zero error events with `reliable=false` means the applied host
load was incomplete; it does not by itself indicate a board-side failure.

| Variant | Reliable range | First unreliable | First-failure evidence |
|---|---:|---:|---|
| C/LwIP native RMII | 1 through 15 kHz | 16 kHz | 47,999 / 48,000 valid; 1 missing |
| Native RMII + Embassy | 1 through 7 kHz | 8 kHz | 21,462 / 24,000 valid; 2,538 error events |
| Native RMII performance + full checksum offload | 1 through 11 kHz | 12 kHz | Host sent 35,999 / 36,000 planned; all sent packets valid |
| W5500 MACRAW + Embassy | 1 kHz only | 2 kHz | 5,705 / 6,000 valid; 4,783 error events |
| Optimized W5500 MACRAW | 1 through 9 kHz | 10 kHz | 29,634 / 30,000 valid; 366 missing |
| W5500 hardware offload, fresh control | 1 through 7 kHz | 8 kHz | 21,104 / 24,000 valid; 2,896 missing |
| Optimized W5500 hardware offload, 40 MHz SPI + D-cache | 1 through 15 kHz | 16 kHz | 46,269 / 48,000 valid; 1,731 missing |

### Error events at each increment

| Target kHz | C/LwIP RMII | Rust/Embassy RMII | Rust performance + full checksum | W5500 offload | Optimized W5500 offload | W5500 MACRAW | Optimized MACRAW |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 2 | 0 | 0 | 0 | 0 | 0 | 4,783 | 0 |
| 3 | 0 | 0 | 0 | 0 | 0 | 8,777 | 0 |
| 4 | 0 | 0 | 0 | 0 | 0 | 11,838 | 0 |
| 5 | 0 | 0 | 0 | 0 | 0 | 14,864 | 0 |
| 6 | 0 | 0 | 0 | 0 | 0 | 17,875 | 0 |
| 7 | 0 | 0 | 0 | 0 | 0 | 20,883 | 0 |
| 8 | 0 | 2,538 | 0 | 2,896 | 0 | 23,888 | 0 |
| 9 | 0 | 6,969 | 0 | 5,894 | 0 | 26,892 | 0 |
| 10 | 0 | 9,839 | 0 | 8,893 | 0 | 29,896 | 366 |
| 11 | 0 | 15,042 | 0 | 11,897 | 0 | 32,896 | 1,078 |
| 12 | 0 | 19,380 | 0 | 14,896 | 0 | 35,898 | 4,109 |
| 13 | 0 | 23,235 | 1 | 17,897 | 0 | 38,900 | 7,085 |
| 14 | 0 | 27,334 | 0 | 20,898 | 0 | 41,900 | 10,106 |
| 15 | 0 | 31,350 | 0 | 23,898 | 0 | 44,903 | 13,104 |
| 16 | 1 | 35,565 | 2 | 26,897 | 1,731 | 47,904 | 16,073 |
| 17 | 0 | 39,568 | 1 | 29,896 | 4,747 | 50,904 | 19,090 |
| 18 | 23 | 43,800 | 2 | 32,895 | 7,736 | 53,904 | 22,094 |
| 19 | 0 | 47,876 | 2 | 35,894 | 10,731 | 56,904 | 25,100 |
| 20 | 0 | 52,088 | 0 | 38,898 | 13,739 | 59,904 | 28,081 |
| 21 | 0 | 60,344 | 4 | 46,848 | 16,713 | 62,920 | 31,122 |
| 22 | 0 | 63,868 | 21 | 48,791 | 19,724 | 65,924 | 34,107 |
| 23 | 0 | 67,412 | 27 | 51,791 | 22,724 | 68,924 | 37,068 |
| 24 | 186 | 70,920 | 80 | 54,802 | 25,728 | 71,924 | 40,090 |
| 25 | 0 | 74,452 | 68 | 57,877 | 28,729 | 74,924 | 43,096 |
| 26 | 0 | 78,000 | 2 | 60,875 | 31,728 | 77,924 | 46,067 |
| 27 | 0 | 81,000 | 105 | 63,876 | 34,719 | 80,924 | 49,054 |
| 28 | 0 | 84,000 | 3 | 66,868 | 37,730 | 83,924 | 52,052 |
| 29 | 0 | 87,000 | 202 | 69,873 | 40,727 | 86,924 | 55,047 |
| 30 | 0 | 90,000 | 287 | 72,872 | 43,722 | 89,924 | 58,039 |

An error event is one missing, late, duplicate, reordered, corrupt, foreign,
or send-error observation. A packet can contribute more than one event; for
example, MACRAW at 2 kHz recorded 295 missing and 4,488 late events. The CSV
output preserves that detailed breakdown rather than only the total.
The optimized Rust run has zero error events at 12 kHz because every packet
actually sent was returned correctly, but it is still marked unreliable in
the preceding table: the Windows host sent 35,999 of 36,000 planned packets
and therefore did not achieve the exact requested offered load.
The C/LwIP continuation initially appeared offline because its distinct
`02-00-00-00-00-c0` MAC received `192.168.68.74`, rather than the native Rust
image's former `.117` lease. A subnet-wide UDP discovery found it and the retry
completed through 30 kHz. Its 186 missing packets at 24 kHz were isolated: the
25-30 kHz points all completed with zero errors. Three immediate 24 kHz repeats
then recorded 0, 0, and 15 errors, respectively, confirming intermittent loss
rather than a monotonic throughput knee. The W5500 offload continuation used the
current code rebuilt with the control's 20 MHz SPI request; it is useful context
but is not a bit-for-bit rerun of the earlier control firmware.
The optimized W5500 column is the retained dedicated-40-MHz SPI plus D-cache
run. Its 1-15 kHz points were all zero-error, and a separate 30-second 15 kHz
dwell also returned all 450,000 packets.
The optimized MACRAW column is its retained performance run. Its short sweep
was clean through 9 kHz; the longer tests establish 8 kHz, rather than 9 kHz,
as the conservative sustained point.

These are preliminary knees from short trials. Confirm with the default
30-second dwell, repeat the boundary rates, and use a longer soak at the
selected operating margin before establishing a requirement or regression
threshold.

## Complexity and memory

| Variant | Bring-up NCLOC | UDP server NCLOC | Total NCLOC | Signed image | Static MCU RAM |
|---|---:|---:|---:|---:|---:|
| C/LwIP native RMII | 620 | 29 | 649 | 126,824 B ELF flash | 53,051 B |
| Native RMII + Embassy | 144 | 48 | 192 | 60,080 B signed | 29,488 B |
| Native RMII retained performance | 144 | 48 | 192 | 71,608 B signed | 29,504 B |
| W5500 MACRAW + Embassy | 269 | 48 | 317 | 66,816 B signed | 30,232 B |
| W5500 MACRAW performance | 269 | 48 | 317 | 83,696 B signed | 30,240 B |
| W5500 hardware offload | 262 | 60 | 322 | 26,296 B signed | 3,204 B |
| W5500 offload retained performance | 262 | 60 | 322 | 37,056 B signed | 3,196 B |

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

The retained native performance image uses 70,948 bytes of ELF flash,
29,504 bytes of static MCU RAM, and a 71,608-byte signed artifact. Checksum
offload was obtained by updating an external driver revision, so it adds no
first-party bring-up or UDP-server NCLOC.

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
