# Rust UDP Echo and SSH Server for NUCLEO-H723ZG

This is a standalone `no_std` Rust implementation of the adjacent STM32Cube
LwIP example. It uses Embassy for the STM32 Ethernet driver, DHCP, IPv4, ICMP,
and UDP. The C project is unchanged.

For one linear, copy-and-paste walkthrough from compilation through an
Ethernet firmware update, see the main README's
[end-to-end Rust demo](../README.md#end-to-end-rust-demo).

For the separate UDP-only executable that uses a WIZnet W5500 Arduino shield
instead of the onboard RMII Ethernet hardware, see [W5500.md](W5500.md).

The firmware:

- runs the STM32H723 at 400 MHz with AHB at 200 MHz;
- drives the onboard LAN8742A PHY over RMII;
- requests an IPv4 address with DHCP;
- falls back to `192.168.0.10/24` after 30 seconds without DHCP;
- answers ICMP echo requests;
- listens on UDP port 7 and echoes to the sender's actual source port;
- runs a public-key-authenticated SSH management service on TCP port 2222;
- stages signed, rollback-capable firmware through an isolated SSH subsystem;
- uses LED2 for network-ready and LED3 for disconnected/not-configured.

## Architecture

This is a bare-metal application: there is no operating system, RTOS, process,
or background network service. Embassy supplies a small async executor,
hardware drivers, and a TCP/IP stack designed for embedded Rust.

```text
                           ETH interrupt
                                |
                                v
LAN8742A PHY <---RMII---> STM32 Ethernet driver
                                |
                                v
                        Embassy network runner
                                |
                    shared embassy-net Stack handle
                   /              |               \
                  v               v                v
       Link/DHCP supervisor  UDP echo task      SSH task
       LEDs + fallback       receive -> send    auth -> command/update
```

Startup and runtime proceed as follows:

1. On every reset, MCUboot verifies the signed primary image and performs any
   requested trial swap or rollback before entering Rust at `0x08020200`.
2. MCUboot relocates the vector table, flushes and disables its caches, clears
   its NVIC state, and jumps with global interrupts masked. `main` configures
   the STM32 clock tree and takes ownership of the GPIO,
   Ethernet, and PHY-management peripherals.
3. After `embassy_stm32::init` has installed the application's clock, timer,
   vector, and interrupt configuration, Rust explicitly enables global
   interrupts. This handoff is essential: ordinary instructions can run with
   `PRIMASK` set, but async timers and Ethernet tasks cannot wake.
4. It constructs the Ethernet driver with statically allocated DMA packet
   descriptors and the board's RMII pins.
5. `embassy_net::new` creates a network `Stack` and a `Runner`. The stack is
   the application-facing handle; the runner performs packet and protocol
   processing.
6. Embassy's executor starts four tasks:
   - the network runner, which drives Ethernet, ARP, IPv4, ICMP, DHCP, UDP, and TCP;
   - the supervisor, which watches link/DHCP state and controls the LEDs;
   - the UDP server, which waits for datagrams on port 7 and echoes them; and
   - the SSH server, which accepts one TCP connection, performs Sunset's SSH
     handshake and Ed25519 authentication, then runs a management command or
     the isolated firmware-update protocol.
7. When Ethernet hardware raises an interrupt, Embassy's handler wakes the
   relevant async work. A task waiting at `.await` consumes no CPU until its
   event, packet, or timer is ready.
8. The tasks run cooperatively on one MCU core. There are no OS threads and no
   preemptive task switching between these application tasks.

The data path for one request is:

```text
Ethernet frame -> DMA queue -> embassy-net -> UDP socket
    -> payload slice -> same UDP socket -> DMA queue -> Ethernet frame
```

An SSH request takes a parallel path:

```text
TCP stream -> Sunset transport -> authenticated SSH channel
    -> command parser -> bounded response -> encrypted TCP stream
```

A firmware update follows a deliberately separate branch after SSH
authentication:

```text
signed .bin -> firmware-update subsystem -> bounded flash writer
    -> secondary slot -> SHA-256 read-back -> MCUboot test marker
    -> reset -> signature verification -> trial boot -> confirm or roll back
```

Sunset is an embedded `no_std` SSH implementation. It owns the SSH state
machine and cryptography while Embassy owns TCP and task scheduling. The
application supplies fixed buffers, a persistent host key, one authorized
client key, and entropy from the STM32 hardware random-number generator.

All important memory is bounded at compile time. `StaticCell` provides
one-time initialization for the DMA queue and network resources. The UDP task
owns fixed arrays for socket metadata and payload buffers. There is no heap,
allocator, `malloc`, or garbage collector.

The application owns the STM32 internal-flash peripheral just as it owns
Ethernet. Only the SSH updater receives a mutable borrow of that driver, so
ordinary commands cannot write flash and two uploads cannot run concurrently.
Addresses and maximum lengths are constants tested on the host. Rust slice
checks still apply at every received chunk, while MCUboot provides an
independent authenticity boundary at the next reset.

### Rust and Embassy mental model

For an experienced C/C++ programmer, these are the most important mappings:

| Familiar embedded concept | Rust/Embassy equivalent |
|---|---|
| Peripheral handle and initialization discipline | An owned peripheral token that cannot be reused accidentally |
| Global DMA arrays | `StaticCell<T>`, initialized once and borrowed for `'static` |
| Main-loop polling or callbacks | Async tasks that suspend at `.await` |
| RTOS task blocking on an event | An async future yielding to Embassy's executor |
| Error code plus output parameter | `Result<T, E>` |
| Nullable pointer or absent configuration | `Option<T>` |
| Pointer plus length | A checked slice such as `&payload[..length]` |
| Interrupt service routine registration | `bind_interrupts!` connecting an IRQ to a typed handler |

Rust's ownership rules are central to this design. Calling
`embassy_stm32::init` returns one value for each peripheral and pin. Moving
`peripherals.ETH` into the Ethernet driver means other code cannot also
configure that same peripheral. Similarly, a mutable borrow such as
`&mut rx_buffer` guarantees exclusive access while the socket uses it. These
rules are checked at compile time and add no runtime reference-counting
overhead.

`'static` does not necessarily mean “global variable.” It means a value or
borrow remains valid for the entire firmware run. Long-lived drivers and
spawned tasks require this because they never return to a caller that could
reclaim their memory.

Rust `async fn` also does not create a thread. The compiler transforms each
async function into a state machine. At `.await`, that state is retained and
the executor runs another ready task. Embassy stores these task state machines
in fixed static storage, so spawning does not allocate memory.

Finally, `no_std` means the firmware does not link Rust's operating-system
standard library. It still uses the language, `core`, and embedded crates such
as Embassy. Logging uses `defmt`, a compact embedded format transported over
the ST-LINK debugger rather than a terminal or filesystem.

### Ethernet hardware: MAC, RMII, and PHY

The network software ultimately reaches the cable through two hardware
components connected by RMII:

```text
embassy-net -> STM32 Ethernet MAC -> RMII -> LAN8742A PHY -> Ethernet cable
```

**RMII**, or **Reduced Media Independent Interface**, is the digital link
between the STM32's Ethernet MAC and the external LAN8742A PHY. The MAC
constructs Ethernet frames and transfers them through DMA. The PHY translates
between digital frame data and the electrical signaling on the cable.

RMII uses two transmit and two receive data bits. Two bits transferred on each
50 MHz reference-clock cycle provide 100 Mbit/s while using fewer pins than
classic four-bit MII.

| Signal | STM32 pin | Purpose |
|---|---|---|
| `REF_CLK` | PA1 | Shared 50 MHz RMII reference clock |
| `CRS_DV` | PA7 | Indicates valid receive data |
| `RXD0`, `RXD1` | PC4, PC5 | Two-bit receive data bus |
| `TX_EN` | PG11 | Indicates an active transmission |
| `TXD0`, `TXD1` | PG13, PB13 | Two-bit transmit data bus |
| `MDIO` | PA2 | PHY configuration and status data |
| `MDC` | PC1 | Clock for the MDIO management interface |

`MDIO` and `MDC` accompany RMII but do not carry frame payloads. Embassy uses
them to configure the PHY and query information such as link status.

## Windows prerequisites

Install Rust and the VS Code Rust extension:

```powershell
winget install --id Rustlang.Rustup -e
rustup toolchain install stable
rustup target add thumbv7em-none-eabihf
rustup component add rust-src
code --install-extension rust-lang.rust-analyzer
```

Restart VS Code after installing Rust so its integrated terminal inherits the
updated `PATH`.

Rust's Windows host tools also need the Microsoft C++ linker. Install
**Visual Studio Build Tools 2022** with the **Desktop development with C++**
workload if it is not already present.

OpenOCD is supplied by the xPack installed for STM32 for VS Code. If needed,
install it from a PowerShell terminal using a current system Node.js:

```powershell
npx xpm install --global @xpack-dev-tools/openocd@latest
```

STM32CubeIDE is not required.

## Build

Open this `Rust` directory as the VS Code workspace, then run the default
build task. The task provisions keys automatically for a new checkout. From a
terminal, provision them once and then build:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\provision_ssh.ps1
cargo fmt --check
cargo clippy --target thumbv7em-none-eabihf -- -D warnings
cargo build --release
```

The ELF is written to:

```text
target\thumbv7em-none-eabihf\release\nucleo-h723zg-udp-echo
```

The linker intentionally places all writable sections in AXI SRAM beginning
at `0x24000000`. Ethernet DMA cannot access the STM32H7's DTCM at
`0x20000000`.

## Factory flash and recovery

Connect the board's ST-LINK USB port, then run the **Rust: flash board** VS
Code task or:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\flash.ps1
```

The script builds pinned MCUboot, builds and signs the Rust application,
erases stale swap metadata, and programs both images through the onboard
ST-LINK. This is the initial provisioning and unbricking path; subsequent
application versions can use Ethernet. Success ends with:

```text
MCUboot and signed Rust firmware programmed, verified, and started.
```

The bootloader occupies `0x08000000..0x0801ffff`. The signed application slot
starts at `0x08020000`, with the Rust vector table after MCUboot's 512-byte
header at `0x08020200`. Do not use the old standalone-ELF programming command:
it would bypass the signed boot layout.

## Find and connect to the board

The firmware currently uses the locally administered MAC address
`02-00-00-00-00-00`, matching the C example. Use only one board with this
example MAC on a LAN.

Look for that MAC in the router's DHCP client list, or populate and inspect the
Windows ARP table:

```powershell
ping BOARD_IP
arp -a
```

On the network used for bring-up, DHCP assigned `192.168.68.57`. A router may
assign another address. If DHCP is unavailable, directly configure the
computer for the `192.168.0.0/24` subnet and use `192.168.0.10`.

## SSH management service

The board is not running Linux and does not have files, processes, or a
general-purpose command interpreter. SSH is a secure transport around a small
firmware management shell:

| Command | Result |
|---|---|
| `help` | List available commands |
| `status` | Show link, address, gateway, and service ports |
| `echo TEXT` | Return `TEXT` |
| `exit` | Close the session |

### Provision keys

Run this once before the first firmware build:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\provision_ssh.ps1
```

It creates these Git-ignored files under `.ssh/`:

- `host_ed25519.seed`: the board's persistent private host-key seed;
- `client_ed25519`: a local private login key;
- `client_ed25519.pub`: its public half; and
- `authorized_ed25519.hex`: the public key embedded in the firmware.

To authorize an existing Ed25519 key instead, supply its public-key file:

```powershell
.\tools\provision_ssh.ps1 `
    -AuthorizedPublicKey "$env:USERPROFILE\.ssh\id_ed25519.pub"
```

Keep the host seed private and backed up. Regenerating it and reflashing makes
SSH clients correctly warn that the board's identity changed. Changing the
authorized key also requires a rebuild and reflash.

### Connect

Run the VS Code **Rust: connect with SSH** task, or use OpenSSH:

```powershell
ssh -i .\.ssh\client_ed25519 -p 2222 board@192.168.68.57
```

Replace the address with the current DHCP address. On first connection, verify
and accept the host-key fingerprint. Authentication is Ed25519 public-key
only: the fixed account is `board`, and there is no password fallback.

The server handles one SSH connection at a time to keep RAM use bounded.
Interactive use is recommended. Sunset 0.5 does not expose a server-side SSH
exit-status operation, so `ssh ... status` returns the output but desktop
OpenSSH may report that the remote closed the connection and use a nonzero
process exit code.

## Signed firmware updates over Ethernet

MCUboot and SSH solve different security problems. SSH authenticates the
operator and encrypts the transfer. MCUboot contains only the firmware-signing
public key and independently rejects an altered or incorrectly signed image,
even if it arrived through a valid SSH session. Never reuse the SSH login key
as the firmware-signing key.

The 1 MiB internal flash is divided as follows:

| Region | Address | Size | Purpose |
|---|---:|---:|---|
| MCUboot | `0x08000000` | 128 KiB | Signature check, swap, rollback |
| Primary slot | `0x08020000` | 384 KiB | Running signed image plus trailer sector |
| Secondary slot | `0x08080000` | 512 KiB | Swap workspace, staged image, trailer |

STM32H723 erase sectors are 128 KiB. MCUboot's primary trailer therefore owns
the final primary sector, leaving an effective signed-image limit of 256 KiB.
For offset swap, the first secondary sector is workspace and uploaded bytes
begin at `0x080A0000`.

### Build a signed release

Choose a version newer than the currently installed image:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\build-signed.ps1 -Version 0.1.2
```

The output is `artifacts\firmware-signed.bin`. The script checks the Ed25519
signature and rejects an image larger than the effective 256 KiB area.
`Bootloader\root-ed25519.pem` is a Git-ignored development private key; keep a
production signing key offline or in an appropriate signing service.

### Upload through SSH

Use the same authorized client key as the management shell:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\ethernet-flash.ps1 `
    -HostName 192.168.68.57 `
    -KeyPath .\.ssh\client_ed25519
```

The client sends a 44-byte header containing `FWUP`, protocol version 1, the
little-endian image length, and SHA-256. The board then:

1. rejects invalid lengths before touching flash;
2. erases only the secondary partition;
3. receives and programs 32-byte-aligned chunks;
4. checks the transfer SHA-256;
5. hashes the complete flash read-back;
6. writes MCUboot test metadata, with activation magic last; and
7. reports success and resets.

The STM32H723 has single-bank flash, so execution and networking pause briefly
during sector erase and programming. The client waits for `READY` before
sending the body, and TCP backpressure bounds buffered data.

After reset, MCUboot verifies the image's Ed25519 signature and performs a test
swap. A new application that reaches clock, memory, flash, and peripheral
initialization writes MCUboot's `image_ok` flag. If it crashes or resets before
that checkpoint, MCUboot restores the previous image on the following boot.

An interrupted upload is safe to retry from byte zero. Partial secondary data
has no final activation magic and cannot replace the primary image. ST-LINK
remains necessary to replace MCUboot itself or recover when both application
slots are damaged.

For a controlled hardware rollback test only, `build-signed.ps1
-RollbackTest` enables a feature that deliberately withholds `image_ok`.
Clearly label and use that artifact only for acceptance testing: MCUboot should
boot it once and restore the previously confirmed version after the next
reset. Normal builds never enable this feature.

## Test

### Host tests

The STM32 hardware drivers cannot execute as ordinary Windows or Linux
processes. Hardware-independent production rules therefore live in
`src/lib.rs`, which has no Embassy or STM32 dependencies. The firmware imports
that library, while `tests/host.rs` consumes the same public API through the
host's normal Rust test harness. The opt-in `host-tests` feature prevents Rust
Analyzer from trying to use that harness on the bare-metal ARM target.

On Windows, run the **Rust: run unit tests** VS Code task or:

```powershell
cargo test --locked --no-default-features --features host-tests --test host --target x86_64-pc-windows-msvc
```

The explicit host target is necessary because `.cargo/config.toml` defaults
normal firmware commands to the ARM target. The host suite checks:

- empty, partial, and full-buffer echo payloads;
- rejection of an invalid receive length;
- locally administered/unicast MAC address bits;
- fallback address and gateway subnet consistency; and
- capacity for a standard unfragmented Ethernet/IPv4 UDP payload;
- SSH endpoint constants and management-command parsing;
- firmware-header version, length, and reserved-field validation; and
- MCUboot image magic, trailer offsets, and exact aligned trailer fixtures.

These tests exercise deterministic application rules, not the Ethernet
peripheral, PHY, interrupts, DHCP server, or physical network. Those remain
covered by the board-level test below.

The GitHub Actions workflow in `../.github/workflows/rust.yml` runs the host tests
on Linux for every push and pull request. It also checks formatting, runs
Clippy against the embedded ARM target with warnings denied, and builds the
release firmware using `Cargo.lock`.

### Board-level UDP test

Run the VS Code **Rust: test UDP echo** task or:

```powershell
.\tools\udp_echo_test.ps1 -BoardIp 192.168.68.57
```

The test sends binary datagrams of 0, 1, 32, 256, and 1472 bytes from an
ordinary ephemeral client port. It verifies the response endpoint, length,
and every byte. Increase the repetition count with `-Count`:

```powershell
.\tools\udp_echo_test.ps1 -BoardIp 192.168.68.57 -Count 1000
```

Unlike the original C example, this implementation replies to the actual
source port. The Rust test client therefore does not need to bind local port
7.

## Verified hardware result

On 2026-07-30 this release configuration was built, programmed, and verified
on the connected NUCLEO-H723ZG. The board obtained `192.168.68.57`, answered
ping, returned a byte-identical UDP payload from port 7, authenticated the
provisioned SSH client key, and served three consecutive SSH status sessions
without disrupting UDP.

The MCUboot/updater build was also exercised end to end on the same board.
The Windows client uploaded signed version 0.1.8 over authenticated SSH,
displayed progress, and returned normally after the board reset. MCUboot
verified and installed it, the Rust health checkpoint programmed `image_ok`,
DHCP restored `192.168.68.57`, SSH status passed, and all 25 binary UDP
acceptance datagrams echoed byte-for-byte. Separate hardware tests also proved
automatic rollback of an unconfirmed image and rejection of an image signed
by an unknown key. Detailed evidence remains in
`../ETHERNET_FIRMWARE_UPDATE_PLAN.md`.

## Source layout

- `src/lib.rs`: hardware-independent constants and checked payload logic
- `tests/host.rs`: host-side tests of the library's public production API
- `src/main.rs`: clocks, board pins, Ethernet construction, and task startup
- `src/bin/w5500_udp_echo.rs`: separate SPI W5500 shield executable
- `src/bin/w5500_offload_udp_echo.rs`: W5500 hardware-socket/offload executable
- `src/network.rs`: DHCP, static fallback, link state, and LEDs
- `src/udp_echo.rs`: bounded-buffer UDP echo task
- `src/ssh_server.rs`: Sunset authentication, channel handling, and commands
- `src/firmware_update.rs`: bounded staging, read-back, activation, and confirmation
- `build.rs`: converts provisioned key files into firmware constants
- `../.github/workflows/rust.yml`: host tests and embedded build checks on GitHub
- `memory.x`: MCUboot primary-slot and AXI SRAM linker layout
- `tools/flash.ps1`: MCUboot plus signed-app factory/recovery flash flow
- `tools/build-signed.ps1`: signed, versioned MCUboot image packaging
- `tools/ethernet-flash.ps1`: authenticated Windows Ethernet-update client
- `tools/provision_ssh.ps1`: persistent host and authorized-client key setup
- `tools/udp_echo_test.ps1`: host-side binary UDP acceptance test
