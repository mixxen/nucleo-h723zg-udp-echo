# UDP Ethernet architecture trade study

This study compares three UDP-only firmware applications on the same
NUCLEO-H723ZG. The managed native application with SSH and firmware update is
retained, but it is deliberately excluded from this comparison.

## Peer executables

| Variant | Entry point | Network stack |
|---|---|---|
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

NCLOC here means nonblank lines excluding full-line `//` comments. It is a
simple, auditable project metric—not a claim that every line has equal
complexity. Declarations and braces count. Shared source counts in each
variant that depends on it, which represents the maintenance footprint of
that deliverable.

Shared `board.rs` clock and MCUboot handoff code is reported outside the
Ethernet comparison. Tests, scripts, MCUboot, SSH, and firmware update are
also excluded. External crates are not added to first-party SLOC; binary size
and dependency inventory capture their effect separately.

Current release-build results (2026-08-12):

| Variant | Bring-up NCLOC | UDP server NCLOC | Study total | Signed bytes |
|---|---:|---:|---:|---:|
| Native RMII + Embassy | 136 | 43 | 179 | 60,032 |
| W5500 MACRAW + Embassy | 272 | 43 | 315 | 65,680 |
| W5500 hardware offload | 213 | 39 | 252 | 24,288 |

The signed size is not proportional to first-party NCLOC because it also
contains the selected Rust crates and their compiled protocol machinery. In
particular, the offload build delegates the network stack to the W5500 and
therefore has the smallest image despite having more bring-up source than the
native-RMII variant.

## Source boundaries

```text
src/board.rs                         shared platform initialization

src/bringup/native_rmii.rs           native MAC/RMII/PHY device
src/bringup/w5500_spi.rs             shared W5500 SPI transport
src/bringup/w5500_macraw.rs          W5500 raw-frame device
src/bringup/w5500_offload.rs         W5500 sockets, DHCP, link/LED state
src/bringup/embassy_network.rs       shared Embassy DHCP/link supervision

src/servers/embassy_udp_echo.rs      shared native + MACRAW UDP server
src/servers/w5500_offload_udp_echo.rs  offload UDP server
```

This layout exposes two useful controlled comparisons:

- Native RMII versus W5500 MACRAW changes the network device while preserving
  the exact Embassy network supervisor and UDP server.
- W5500 MACRAW versus W5500 offload preserves the shield and SPI transport
  while moving DHCP, IPv4, and UDP from Embassy into W5500 hardware.

## Build each UDP-only image

```powershell
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

On 2026-08-12 all three UDP-only variants and the retained managed firmware
passed strict Clippy and release builds; all 17 host tests also passed. The
W5500 MACRAW image was flashed to the connected shield and echoed 200 binary
datagrams across 0, 1, 32, 256, and 1,472-byte payloads. The offload image was
then flashed and echoed 200 datagrams across its supported non-empty sizes.

After both Ethernet cables were connected, the native-RMII UDP-only image was
flashed independently. It obtained `192.168.68.66` by DHCP, answered ICMP,
and echoed 100 binary datagrams across 0, 1, 32, 256, and 1,472-byte payloads.
Its MAC address `02-00-00-00-00-00` remained distinct from the W5500 at
`192.168.68.74`. The board was left running the native-RMII UDP-only image.

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
