# Rust UDP Echo Server Port Plan

- Status: MVP implemented and hardware-verified on 2026-07-23
- Target board: NUCLEO-H723ZG
- Target MCU: STM32H723ZGTx
- Rust target: `thumbv7em-none-eabihf`

The implemented project is in [`Rust/`](Rust/), with reproducible build,
flash, and UDP test instructions in [`Rust/README.md`](Rust/README.md).
Hardware verification completed through DHCP, ICMP ping, and a byte-identical
UDP echo on port 7. The longer reconnect, clock-uplift, and 100,000-packet
reliability phases below remain future hardening work.

## Objective

Create a standalone, `no_std` Rust implementation of the existing LwIP UDP
echo server without modifying or replacing the working C implementation.

The Rust firmware will:

- run on the NUCLEO-H723ZG through its onboard ST-LINK;
- initialize the onboard LAN8742A PHY in RMII mode;
- obtain an IPv4 address through DHCP;
- fall back to `192.168.0.10/24` if DHCP cannot provide an address;
- listen on UDP port 7;
- return each received datagram to its sender;
- expose link, DHCP, address, and error state through RTT logs; and
- be buildable and flashable from Windows and VS Code.

## Recommended architecture

Use the Embassy embedded Rust ecosystem:

- `embassy-stm32` for STM32H723 clocks, GPIO, Ethernet MAC, interrupts, and
  LAN8742-compatible generic PHY handling;
- `embassy-net` for Ethernet, IPv4, DHCP, ICMP, and UDP;
- `embassy-executor` for the asynchronous network runner and application task;
- `embassy-time` for DHCP timeout and retry handling;
- `defmt`, `defmt-rtt`, and `panic-probe` for diagnostics; and
- `static_cell` for statically allocated packet queues and network resources.

This is preferable to directly translating the C HAL and LwIP glue because
Embassy already has an STM32 Ethernet driver and an allocation-free async
network stack. Current `embassy-stm32` device metadata explicitly includes
`stm32h723zg`, while the upstream STM32H7 Ethernet example uses the same RMII
pins required by this board.

The initial candidate dependency versions, based on Embassy upstream as of
2026-07-23, are:

```toml
[dependencies]
embassy-stm32 = { version = "0.6.0", features = [
    "defmt",
    "stm32h723zg",
    "time-driver-tim2",
    "memory-x",
] }
embassy-executor = { version = "0.10.0", features = [
    "platform-cortex-m",
    "executor-thread",
    "defmt",
] }
embassy-time = { version = "0.5.1", features = [
    "defmt",
    "defmt-timestamp-uptime",
    "tick-hz-32_768",
] }
embassy-net = { version = "0.9.1", features = [
    "defmt",
    "dhcpv4",
    "medium-ethernet",
    "proto-ipv4",
    "udp",
    "auto-icmp-echo-reply",
] }

defmt = "1.0.1"
defmt-rtt = "1.0.0"
panic-probe = { version = "1.0.0", features = ["print-defmt"] }
static_cell = "2"
cortex-m = { version = "0.7.6", features = [
    "inline-asm",
    "critical-section-single-core",
] }
cortex-m-rt = "0.7.0"
```

These versions must be validated together with `cargo check` before
implementation begins, then committed in `Cargo.lock`. Embassy evolves
quickly, so source code should target the locked API rather than floating Git
dependencies.

## Proposed directory structure

Keep the Rust port beside, but isolated from, the vendor projects:

```text
LwIP_UDP_Echo_Server/
├── Inc/                         # Existing C implementation
├── Src/
├── STM32CubeIDE/
├── Rust/
│   ├── .cargo/
│   │   └── config.toml
│   ├── .vscode/
│   │   ├── launch.json
│   │   ├── settings.json
│   │   └── tasks.json
│   ├── src/
│   │   ├── main.rs
│   │   ├── board.rs
│   │   ├── network.rs
│   │   └── udp_echo.rs
│   ├── tools/
│   │   └── udp_echo_test.ps1
│   ├── build.rs
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── memory.x
│   ├── rust-toolchain.toml
│   └── README.md
├── README.md
└── RUST_PORT_PLAN.md
```

The existing C project remains the known-good reference and a regression
oracle throughout the port.

## Hardware mapping

The Rust implementation must use the pin mapping already proven by the C
firmware:

| Signal | STM32 pin | Rust peripheral argument |
|---|---:|---|
| RMII reference clock | PA1 | `ref_clk` |
| MDIO | PA2 | `mdio` |
| MDC | PC1 | `mdc` |
| CRS_DV | PA7 | `crs_dv` |
| RXD0 | PC4 | `rx_d0` |
| RXD1 | PC5 | `rx_d1` |
| RXER | PG2 | Not required by Embassy's RMII constructor |
| TX_EN | PG11 | `tx_en` |
| TXD0 | PG13 | `tx_d0` |
| TXD1 | PB13 | `tx_d1` |

Other board parameters:

- Ethernet MAC peripheral: `ETH`
- Ethernet interrupt: `ETH`
- Station-management peripheral: `ETH_SMA`
- PHY: onboard LAN8742A
- PHY interface: RMII
- Initial packet queue: four RX and four TX descriptors
- UDP server port: 7

The first bring-up build should retain the C example's locally administered
MAC address, `02-00-00-00-00-00`, so DHCP and ARP results can be compared
directly. Before the Rust port is treated as reusable, replace it with a stable
locally administered address derived from the STM32 unique device ID. This
avoids collisions when more than one board runs the example.

## Clock strategy

The C implementation runs the CPU at 520 MHz, HCLK at 260 MHz, and APB clocks
at 130 MHz using the ST-LINK-provided HSE clock in bypass mode.

Bring the Rust version up in two steps:

1. Start from Embassy's upstream STM32H7 Ethernet clock configuration:
   400 MHz SYSCLK, 200 MHz AHB, and 100 MHz APB. This is already exercised by
   the upstream Ethernet example and reduces initial clock/debug risk.
2. After DHCP, ping, and UDP echo are reliable, change the RCC configuration
   to match the C firmware's 520/260/130 MHz clocks and voltage scale 0.
   Re-run all network and reset-cycle tests.

Network behavior is not performance-sensitive enough to make 520 MHz a
bring-up requirement. Correctness at 400 MHz comes first.

## Memory, DMA, and cache strategy

This is the highest-risk part of the port.

The STM32H723ZG has 1 MiB of flash, 128 KiB of guaranteed AXI SRAM starting at
`0x24000000`, 128 KiB DTCM at `0x20000000`, two 16 KiB D2 SRAM banks, and
additional configurable AXI/ITCM RAM. Ethernet DMA cannot use DTCM.

Use a deliberately conservative linker layout for the first version:

```ld
MEMORY
{
    FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
    RAM   : ORIGIN = 0x24000000, LENGTH = 128K
}
```

This keeps the program stack, Embassy packet queue, UDP buffers, and network
resources in AXI SRAM that the Ethernet DMA can access. Do not copy the
upstream H743 `memory.x` unchanged: that example declares 512 KiB at
`0x24000000`, which is not the conservative guaranteed map for the H723.

For the first milestone:

- use `StaticCell<PacketQueue<4, 4>>`;
- use `StaticCell<StackResources<4>>`;
- use four RX and four TX UDP packet metadata entries;
- use 1536-byte RX and TX UDP payload buffers;
- avoid `alloc`, a global allocator, and dynamic memory;
- rely on the Embassy Ethernet driver's descriptor ownership and cache
  barriers; and
- test with both debug and release optimization because cache/timing bugs often
  change with optimization.

Before enabling the Cortex-M7 D-cache explicitly, confirm from the locked
`embassy-stm32` implementation whether its H7 Ethernet driver owns all required
cache maintenance. If that cannot be demonstrated, keep Ethernet buffers in a
non-cacheable MPU region or add explicit clean/invalidate operations. Never
assume that Rust's memory safety solves DMA cache coherence.

After the MVP is stable, memory can be expanded to use the H723's configurable
AXI SRAM and DTCM deliberately. That optimization is outside the first port.

## Firmware design

### `board.rs`

Responsibilities:

- configure RCC and voltage scaling;
- initialize Embassy for `stm32h723zg`;
- bind the Ethernet interrupt;
- instantiate `Ethernet` with the RMII pin mapping;
- configure the LAN8742A through `ETH_SMA`;
- provide the MAC address;
- optionally expose LED2 and LED3 for link state; and
- return the Ethernet device and any status outputs needed by `main`.

The first implementation should be closely patterned after Embassy's upstream
`examples/stm32h7/src/bin/eth.rs`, changing only the MCU feature, clock
configuration, board LEDs, MAC policy, and application protocol.

### `network.rs`

Responsibilities:

- allocate `PacketQueue` and `StackResources` statically;
- create `embassy_net::Config::dhcpv4(Default::default())`;
- create the Embassy stack and runner;
- spawn a `net_task` that calls `runner.run().await`;
- wait for link-up and network configuration;
- log the assigned address, prefix, gateway, and MAC; and
- implement the DHCP timeout/static fallback policy.

The C application falls back to `192.168.0.10/24` after its DHCP retry limit.
For parity, the Rust version should:

1. start in DHCP mode;
2. wait for configuration for a defined interval, initially 30 seconds;
3. if no lease arrives, call `Stack::set_config_v4` with a static configuration:
   - address `192.168.0.10/24`;
   - gateway `192.168.0.1`; and
   - no DNS servers; and
4. return to DHCP after a cable disconnect/reconnect or an explicit retry
   interval.

Use LED2 for link/configuration ready and LED3 for link down, matching the C
example where practical.

### `udp_echo.rs`

Create one `embassy_net::udp::UdpSocket` with statically allocated metadata and
payload buffers:

```rust
loop {
    let (length, remote) = socket.recv_from(&mut payload).await?;
    socket.send_to(&payload[..length], remote).await?;
}
```

Bind the socket to local port 7.

The C implementation receives on server port 7 but always sends its response
to client port 7, regardless of the source port. The Rust version should use
normal UDP echo semantics and reply to the actual source endpoint returned by
`recv_from`. This works with clients bound to port 7 and also fixes the
surprising behavior for ordinary ephemeral-port clients.

Document this intentional compatibility improvement. If exact wire-level
parity is required, add a Cargo feature such as `fixed-client-port-7` that
replaces the remote port with 7 before `send_to`.

Handle errors without panicking:

- retry after transient send errors;
- recreate or rebind the socket after interface reconfiguration;
- count received, echoed, dropped, and errored datagrams; and
- report counters periodically through `defmt`.

## Windows and VS Code tooling

Install:

```powershell
winget install Rustlang.Rustup
rustup toolchain install stable
rustup target add thumbv7em-none-eabihf
```

VS Code extensions:

- `rust-lang.rust-analyzer`
- `probe-rs.probe-rs-debugger` if probe-rs is adopted
- the existing Cortex-Debug extension may remain installed for C/OpenOCD work

Primary build commands:

```powershell
cd Rust
cargo fmt --check
cargo clippy --target thumbv7em-none-eabihf -- -D warnings
cargo build
cargo build --release
```

Use the already working OpenOCD/ST-LINK setup as the initial flashing path:

```powershell
$elf = (Resolve-Path ".\target\thumbv7em-none-eabihf\release\udp-echo-server").Path.Replace('\', '/')

& $openOcd `
    -f interface/stlink.cfg `
    -f target/stm32h7x.cfg `
    -c "adapter speed 3300" `
    -c "program {$elf} verify reset exit"
```

Evaluate probe-rs only after the OpenOCD-flashed Rust image works. Probe-rs can
provide `cargo run`, RTT/defmt logs, and VS Code debugging, but changing Windows
USB drivers must not disrupt the currently working ST-LINK/OpenOCD workflow.
If probe-rs recognizes the existing driver and probe, configure:

```toml
[target.thumbv7em-none-eabihf]
runner = "probe-rs run --chip STM32H723ZGTx"

[build]
target = "thumbv7em-none-eabihf"

[env]
DEFMT_LOG = "info"
```

Confirm the exact chip identifier locally with:

```powershell
probe-rs chip list | Select-String STM32H723
```

## Phased implementation

### Phase 0: preserve the reference

- Record the current C firmware size and successful OpenOCD verification.
- Record DHCP lease, MAC address, ping behavior, and UDP echo result.
- Keep the existing PowerShell UDP test as the acceptance oracle.
- Do not alter C sources, linker scripts, or generated build files.

Exit criterion: the C firmware can still be rebuilt, flashed, and tested.

### Phase 1: Rust project skeleton

- Create `Rust/` with `Cargo.toml`, locked toolchain, target configuration,
  `build.rs`, `memory.x`, and a minimal `main.rs`.
- Build a `no_std` image for `thumbv7em-none-eabihf`.
- Flash a minimal RTT/defmt heartbeat.
- Verify reset, panic output, and repeated flashing.

Exit criterion: a Rust image boots reliably and emits RTT logs after five
power/reset cycles.

### Phase 2: Ethernet PHY and link

- Add the exact RMII pins and `GenericPhy`.
- Bind and enable the Ethernet interrupt.
- Allocate a 4 RX / 4 TX packet queue in AXI SRAM.
- Report PHY link up/down and negotiated speed/duplex.
- Exercise cable removal and reconnection.

Exit criterion: link state follows the cable for ten reconnect cycles without
a panic, lockup, or required reset.

### Phase 3: Embassy network stack and DHCP

- Spawn the Embassy network runner.
- Enable DHCPv4 and automatic ICMP echo replies.
- Log the assigned IPv4 configuration.
- Confirm router lease and ARP entry for the configured MAC.
- Implement the 30-second static fallback and DHCP retry behavior.

Exit criterion: the board receives a DHCP lease, answers ping, and recovers
after lease renewal and cable reconnect.

### Phase 4: UDP echo

- Bind UDP port 7.
- Echo to the received source endpoint.
- Add counters and bounded buffers.
- Add the optional exact-parity fixed-client-port feature only if needed.
- Add `tools/udp_echo_test.ps1`.

Exit criterion: the server passes single-packet, repeated-packet, maximum
payload, zero-length payload, and reconnect tests.

### Phase 5: cache, load, and reliability

- Verify buffer addresses are outside DTCM.
- Test debug and release builds.
- Run at the initial 400 MHz clock.
- Enable or validate cache handling.
- Move to the 520/260/130 MHz clock configuration.
- Send at least 100,000 sequential UDP datagrams while checking payload and
  response counts.
- Power-cycle and reset repeatedly.

Exit criterion: zero corrupt replies, no unrecovered socket errors, no DHCP
loss, and no hard faults during the stress run.

### Phase 6: developer experience

- Add a Rust-specific README.
- Add VS Code build, flash, run, and debug tasks.
- Document OpenOCD as the guaranteed flashing route.
- Document probe-rs as an optional enhanced workflow.
- Add CI checks for formatting, Clippy, and release compilation.

Exit criterion: a clean Windows checkout can follow the README and reach a
verified UDP echo response without using STM32CubeIDE.

## Test matrix

| Test | Expected result |
|---|---|
| Cold boot with DHCP | Lease acquired and logged |
| Warm reset | Same or new valid lease, server resumes |
| No DHCP server | Static `192.168.0.10/24` after timeout |
| Cable removed | Link-down logged; LED3 indicates down |
| Cable restored | Link and DHCP recover without reset |
| ICMP ping | Replies when configured |
| UDP from client port 7 | Payload echoed exactly |
| UDP from ephemeral port | Payload echoed to that source port |
| Empty UDP payload | Empty datagram returned or behavior documented |
| 1472-byte IPv4 UDP payload | Payload echoed without corruption |
| Burst traffic | Drops bounded and counted; firmware remains responsive |
| 100,000-request soak | No corruption, panic, hard fault, or leak |
| Debugger disconnect | MCU resumes instead of remaining halted |
| Five power cycles | Reliable link, DHCP, and echo each time |

## Definition of done

The Rust port is complete when:

- `cargo build` and `cargo build --release` succeed on stable Rust;
- `cargo fmt --check` and Clippy pass;
- OpenOCD programs and verifies the ELF through the onboard ST-LINK;
- the board receives and logs a DHCP address;
- Windows resolves the board's configured MAC through ARP;
- ping succeeds;
- UDP port 7 returns byte-identical payloads;
- static fallback works without DHCP;
- link loss and restoration recover without a reset;
- DMA buffers are demonstrably accessible by Ethernet and cache-coherent;
- the C example remains unchanged and operational; and
- the Rust README reproduces the full Windows setup from a clean checkout.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Ethernet DMA buffer placed in DTCM | No traffic or hard fault | Link only into AXI SRAM initially; inspect map file |
| Cortex-M7 D-cache incoherence | Intermittent packet corruption | Validate Embassy cache handling; use MPU/non-cacheable buffers if needed |
| H743 example memory map copied to H723 | Runtime memory corruption | Use conservative H723-specific 1 MiB flash / 128 KiB AXI SRAM map |
| RCC configuration invalid at 520 MHz | Boot failure | Bring up at upstream 400 MHz first; raise clock only after network passes |
| Duplicate fixed MAC | DHCP/ARP conflicts | Derive a stable local MAC from the device UID before finalization |
| Embassy API/version drift | Build failure | Pin released crate versions and commit `Cargo.lock` |
| Generic PHY incompatibility | Link never comes up | Compare MDIO registers with the working LAN8742 C driver; add a small PHY adapter if required |
| probe-rs Windows driver change | Breaks existing OpenOCD workflow | Keep OpenOCD primary; adopt probe-rs only after non-disruptive validation |
| Debugger leaves MCU halted | Apparent network failure | Make flash tasks end in reset/run; document disconnect-and-resume |

## Reference material

- [Embassy repository and framework overview](https://github.com/embassy-rs/embassy)
- [Embassy upstream STM32H7 Ethernet example](https://github.com/embassy-rs/embassy/blob/main/examples/stm32h7/src/bin/eth.rs)
- [Embassy STM32 device documentation](https://docs.embassy.dev/embassy-stm32/latest/)
- [`embassy-stm32` Ethernet module](https://docs.embassy.dev/embassy-stm32/git/stm32h725ag/eth/index.html)
- [`embassy-net` 0.9.1 API](https://docs.rs/embassy-net/0.9.1/embassy_net/)
- [`embassy-net::Stack::set_config_v4`](https://docs.rs/embassy-net/latest/src/embassy_net/lib.rs.html)
- [STM32H723ZG product and memory summary](https://www.st.com/en/microcontrollers-microprocessors/stm32h723zg.html)
- [STM32H723xE/G datasheet](https://www.st.com/resource/en/datasheet/stm32h723vg.pdf)
- [STM32H723/733 and STM32H725/735 reference manual](https://www.st.com/resource/en/reference_manual/dm00603761-stm32h723733-stm32h725735-and-stm32h730-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf)
- [probe-rs installation](https://probe.rs/docs/getting-started/installation/)
- [probe-rs runner documentation](https://probe.rs/docs/tools/probe-rs/)
