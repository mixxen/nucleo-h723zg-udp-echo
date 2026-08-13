# UDP Ethernet architecture trade study

This study compares four UDP-only firmware applications on the same
NUCLEO-H723ZG. The managed native application with SSH and firmware update is
retained, but it is deliberately excluded from this comparison.

## Peer executables

| Variant | Entry point | Network stack |
|---|---|---|
| C/LwIP native RMII | `../Src/main.c` | LwIP over STM32 HAL Ethernet DMA and LAN8742A PHY |
| Native RMII | `src/bin/native_rmii_udp_echo.rs` | Embassy over STM32 Ethernet DMA and LAN8742A PHY |
| W5500 MACRAW | `src/bin/w5500_macraw_udp_echo.rs` | Embassy over raw frames transported through W5500 SPI |
| W5500 offload | `src/bin/w5500_offload_udp_echo.rs` | W5500 hardwired IPv4/UDP sockets |

The original managed firmware is now clearly named
`src/bin/native_rmii_managed.rs`. Its Cargo binary name remains
`nucleo-h723zg-udp-echo` so existing build, flash, SSH, and update commands do
not break.

## Initial source metrics

The first two requested metrics are kept in physically separate source files:

1. **Network bring-up NCLOC** includes the thin entry-point integration,
   physical device driver setup, DHCP/link supervision, and any required SPI
   adapter. It ends when a usable UDP socket can be served.
2. **UDP server NCLOC** includes socket buffers, receive/send behavior, and
   server-specific error handling. It excludes hardware and DHCP setup.

Run the repeatable measurement from the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File .\Rust\tools\measure-variants.ps1
```

NCLOC here means nonblank lines excluding full-line comments. It is a
simple, auditable project metric—not a claim that every line has equal
complexity. Declarations and braces count. Shared source counts in each
variant that depends on it, which represents the maintenance footprint of
that deliverable.

Shared `board.rs` clock and MCUboot handoff code is reported outside the
Ethernet comparison. Tests, scripts, MCUboot, SSH, and firmware update are
also excluded. External crates are not added to first-party SLOC; binary size
and dependency inventory capture their effect separately.

Current release-build results (2026-08-13):

| Variant | Bring-up NCLOC | UDP server NCLOC | Study total | ELF flash bytes | MCU RAM bytes |
|---|---:|---:|---:|---:|---:|
| C/LwIP native RMII | 620 | 29 | 649 | 126,824 | 53,051 |
| Native RMII + Embassy | 144 | 48 | 192 | 59,420 | 29,488 |
| W5500 MACRAW + Embassy | 264 | 48 | 312 | 66,160 | 30,232 |
| W5500 hardware offload | 262 | 60 | 322 | 25,444 | 3,188 |

Image size is not proportional to first-party NCLOC because it also contains
the selected C libraries or Rust crates and their compiled protocol
machinery. In particular, the offload build delegates the network stack to
the W5500 and therefore has the smallest image despite having more bring-up
source than the native-RMII variant.

The C bring-up count includes `main.c`, where Cube generation couples clock,
MPU, main-loop, and netif integration. External LwIP/HAL implementation files
and external Rust crates are both excluded. These numbers therefore measure
the checked-in integration and maintenance surface, not language syntax in
isolation. The RAM column is `.data + .bss + .uninit` from the release ELF.
It includes statically allocated Embassy tasks and buffers but not call-stack
high-water usage. The W5500's own 32 KiB packet memory is external to MCU RAM
and is not included. Recreate these columns after matching release builds with:

```powershell
.\Rust\tools\measure-variants.ps1 -Benchmark
```

### Native RMII: C versus Rust snapshot

| Metric | C/LwIP | Rust/Embassy | Observed relationship |
|---|---:|---:|---|
| Network bring-up NCLOC | 620 | 144 | C is 4.31x larger |
| UDP server NCLOC | 29 | 48 | C echo loop is 40% smaller |
| Total study NCLOC | 649 | 192 | C is 3.38x larger |
| ELF flash | 126,824 B | 59,412 B | C is 2.13x larger |
| Static MCU RAM | 53,051 B | 29,488 B | C is 1.80x larger |
| 1 kHz p50 RTT | 0.118 ms | 0.278 ms | C was 58% lower |
| 1 kHz p99 RTT | 0.352 ms | 0.369 ms | C was 5% lower |
| Continuous zero-error range, 3-second sweep | 1-15 kHz | 1-7 kHz | C reached about 2.1x the rate |

The C UDP callback is especially small after replacing the original
allocate/copy/connect/send sequence with `udp_sendto()` on the received pbuf.
Most of C's checked-in complexity is the generated STM32/LwIP bring-up layer.
These performance ratios are observations of the present implementations:
C uses a 520 MHz clock and LwIP/HAL, while Rust uses 400 MHz and Embassy. A
language-focused experiment should first align clock trees, compiler goals,
buffers, logging, and profiling instrumentation.

The complete configuration-control matrix—including MCU/AHB clocks, RMII and
SPI clocks, packet wakeup mechanism, maintenance cadence, queue/buffer sizes,
DHCP fallback, and optimization settings—is in
[STREAM_BENCHMARK_REPORT.md](STREAM_BENCHMARK_REPORT.md#firmware-configuration-matrix).
The most consequential current differences are:

| Parameter | C/LwIP native RMII | Rust/Embassy native RMII |
|---|---|---|
| MCU / AHB clock | 520 / 260 MHz | 400 / 200 MHz normally; 520 / 260 MHz performance build |
| Packet servicing | Unbounded main-loop polling | Ethernet interrupt + DMA wake |
| Network stack | LwIP raw API | Embassy async (`xarxa` at pinned revision) |
| Release optimization | GCC `-O2` | Rust size optimization (`z`) + fat LTO |
| Cortex-M7 cache policy | I-cache + D-cache with MPU-isolated DMA memory | I-cache; D-cache disabled for DMA coherency |
| IPv4/UDP checksums | STM32 MAC RX/TX offload | STM32 MAC RX/TX offload (pinned Embassy revision) |
| Static MCU RAM | 53,051 B | 29,488 B |

Accordingly, “C versus Rust” in this study means the complete C/LwIP/HAL and
Rust/Embassy implementations as configured. A later language-isolation test
should hold CPU/AHB clocks and optimization intent constant before drawing a
language-specific performance conclusion.

### Optimized native-RMII result

The retained Rust performance build now aligns the CPU/AHB clocks at
520/260 MHz, uses `opt-level=3`, enlarges the UDP queues, enables I-cache, and
uses STM32 MAC RX/TX checksum offload. At the 1 kHz requirement it returned
30,000/30,000 packets with 0.146 ms p50 and 0.220 ms p99 RTT. Three 30-second
15 kHz trials missed 2, 4, and 20 packets out of 450,000 each (99.9981%
average delivery); the C comparison missed 0 with 0.124/0.218 ms p50/p99.
This is sufficiently close that the proposed duplicate polling/raw-frame
Rust path is deferred. It would increase first-party complexity without a
demonstrated requirement-level benefit.

The checksum support comes from exact Embassy revision `0af1937a`; it adds no
application NCLOC. The performance ELF occupies 70,948 bytes of flash and
29,504 bytes of static MCU RAM, and its signed image is 71,608 bytes. See the
controlled trials and limitations in
[STREAM_BENCHMARK_REPORT.md](STREAM_BENCHMARK_REPORT.md#rustembassy-native-optimization).

### Optimized W5500-offload result

The W5500 hardwired-socket path now has its own performance build. It combines
`opt-level=3`, a 520/260 MHz MCU/AHB clock, a dedicated 40 MHz SPI kernel
path, W5500-only D-cache, and a cached UDP destination register. Its short
sweep was zero-error from 1 through 15 kHz, compared with 1 through 7 kHz for
the fresh control, and its saturation plateau increased from about 7,034 to
15,421 valid packets/s. The earlier 1 kHz trial returned 30,000/30,000 with
0.231/0.435 ms p50/p99 RTT.

The cached peer increases the offload UDP-server metric by seven NCLOC and
static RAM by 56 bytes; making SPI frequency explicit adds one shared bring-up
NCLOC to both W5500 variants. The D-cache-enabled offload performance signed
image is 37,056 bytes. The final 30-second 15 kHz boundary run returned all
450,000 packets. Full staged results, the failed 50 MHz trial, and the reason
DMA was deferred are in
[STREAM_BENCHMARK_REPORT.md](STREAM_BENCHMARK_REPORT.md#w5500-hardware-offload-optimization).

The previous MACRAW timer shim was removed, saving first-party bring-up code.
Offload gained explicit socket-interrupt setup and safe flag clearing, so its
source grew; that is real device-management complexity and belongs in the
bring-up comparison rather than being hidden in the UDP echo loop.

## Source boundaries

```text
src/board.rs                         shared platform initialization

../Src/main.c                        C clock, MPU, loop, netif integration
../Src/app_ethernet.c                C DHCP and link supervision
../Src/ethernetif.c                  C STM32 MAC/RMII/LwIP adapter
../Src/udp_echoserver.c              C UDP server

src/bringup/native_rmii.rs           native MAC/RMII/PHY device
src/bringup/w5500_spi.rs             shared W5500 SPI transport
src/bringup/w5500_macraw.rs          W5500 raw-frame device
src/bringup/w5500_offload.rs         W5500 sockets, DHCP, link/LED state
src/bringup/embassy_network.rs       shared Embassy DHCP/link supervision

src/servers/embassy_udp_echo.rs      shared native + MACRAW UDP server
src/servers/w5500_offload_udp_echo.rs  offload UDP server
```

This layout exposes two useful controlled comparisons:

- C/LwIP native RMII versus Rust/Embassy native RMII preserves the board,
  MAC, PHY, LAN, payload, and host test while changing the language, network
  stack, runtime model, clock configuration, and driver implementation.
- Native RMII versus W5500 MACRAW changes the network device while preserving
  the exact Embassy network supervisor and UDP server.
- W5500 MACRAW versus W5500 offload preserves the shield and SPI transport
  while moving DHCP, IPv4, and UDP from Embassy into W5500 hardware.

## Build each UDP-only image

```powershell
# C/LwIP native STM32 MAC + RMII PHY
.\Rust\tools\build-c.ps1

# Native STM32 MAC + RMII PHY
.\Rust\tools\build-signed.ps1 -Version 0.3.0 -NativeUdp

# W5500 raw Ethernet frames + Embassy
.\Rust\tools\build-signed.ps1 -Version 0.3.0 -W5500

# W5500 hardwired UDP offload
.\Rust\tools\build-signed.ps1 -Version 0.3.0 -W5500Offload
```

The outputs are distinct, so building one does not overwrite another:

- `firmware-native-udp-signed.bin`
- `firmware-w5500-signed.bin`
- `firmware-w5500-offload-signed.bin`

## Refactor verification

On 2026-08-12 all three Rust UDP-only variants and the retained managed firmware
passed strict Clippy and release builds; all 17 host tests also passed. The
W5500 MACRAW image was flashed to the connected shield and echoed 200 binary
datagrams across 0, 1, 32, 256, and 1,472-byte payloads. The offload image was
then flashed and echoed 200 datagrams across its supported non-empty sizes.

After both Ethernet cables were connected, the native-RMII UDP-only image was
flashed independently. It obtained `192.168.68.66` by DHCP, answered ICMP,
and echoed 100 binary datagrams across 0, 1, 32, 256, and 1,472-byte payloads.
Its MAC address `02-00-00-00-00-00` remained distinct from the W5500 at
`192.168.68.74`. The board was left running the native-RMII UDP-only image.

On 2026-08-13 the C/LwIP image built with GCC `-O2`, flashed through ST-LINK,
obtained `192.168.68.119` using MAC `02-00-00-00-00-C0`, and passed binary UDP
echo tests from 0 through 1,472 bytes. Its matched 30-second stream and
three-second rate sweep are reported in
[STREAM_BENCHMARK_REPORT.md](STREAM_BENCHMARK_REPORT.md).

## Complexity and performance follow-on metrics

The controlled host-side methodology and implementation sequence are defined
in [BENCHMARK_PLAN.md](BENCHMARK_PLAN.md). The completed 2026-08-12 baseline
and architecture conclusions are in
[BENCHMARK_REPORT.md](BENCHMARK_REPORT.md).

NCLOC should be considered alongside:

- number of async tasks and explicit runtime states;
- DMA descriptors, socket buffers, and static RAM;
- interrupt, DMA, peripheral, and pin requirements;
- error branches and cable/DHCP recovery behavior;
- signed flash image size and dependency count;
- boot-to-link and boot-to-DHCP time;
- UDP latency distributions and packets per second;
- CPU cycles and SPI bytes per echoed packet; and
- loss under bursts at 1, 32, 256, and 1,472-byte payloads.

Zero-byte UDP must be reported as a compatibility result rather than hidden:
the two Embassy variants echo it, while the W5500 offload SEND command cannot.
