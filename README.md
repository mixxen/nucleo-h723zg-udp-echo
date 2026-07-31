# NUCLEO-H723ZG UDP Echo Server

This guide describes the Windows and Visual Studio Code setup used to build,
flash, and test this example on a NUCLEO-H723ZG.

The original ST application notes remain in [readme.txt](readme.txt).

A tested Embassy-based Rust implementation is available in
[Rust/README.md](Rust/README.md). It builds and flashes independently and
does not modify the C application.

The implementation plan and live progress ledger for signed, rollback-safe
Ethernet updates is in
[ETHERNET_FIRMWARE_UPDATE_PLAN.md](ETHERNET_FIRMWARE_UPDATE_PLAN.md).

## What the application does

The board:

- obtains an IPv4 address from DHCP;
- listens for UDP datagrams on port 7; and
- echoes each datagram to UDP port 7 on the client.

The Rust version also runs a public-key-authenticated SSH management shell on
TCP port 2222 and an isolated `firmware-update` SSH subsystem. The shell
exposes a small command set (`help`, `status`, `echo`, and `exit`) rather than
an operating-system shell.

The firmware uses the locally administered MAC address
`02-00-00-00-00-00`. If DHCP fails, it eventually falls back to
`192.168.0.10/24`.

Because the original C server always sends its response to client port 7, its
test client must bind its local UDP socket to port 7. The Rust implementation
used by the demo below replies to the sender's actual source port.

## End-to-end Rust demo

This walkthrough starts with source code, performs the initial USB/ST-LINK
installation, exercises UDP and SSH, and then installs a newer signed release
over Ethernet. The complete sequence was last executed successfully on the
connected NUCLEO-H723ZG on **2026-07-31**.

Run the commands in **Windows PowerShell** from the repository root:

```powershell
cd C:\Users\USER\git\mmhel\nucleo-h723zg-udp-echo
```

The demo assumes Rust, the ARM target, OpenOCD, the pinned Zephyr workspace,
and Windows OpenSSH are installed as described elsewhere in this README. Keep
both the ST-LINK USB connection and Ethernet cable connected.

### 1. Provision development keys

Create the firmware-signing key once. The conditional prevents accidentally
replacing an existing key, because a new key would require rebuilding and
reflashing MCUboot:

```powershell
if (-not (Test-Path .\Bootloader\root-ed25519.pem)) {
    powershell -ExecutionPolicy Bypass -File .\Bootloader\tools\provision-signing-key.ps1
}
```

Create the persistent board SSH host key and a project-local client login key:

```powershell
powershell -ExecutionPolicy Bypass -File .\Rust\tools\provision_ssh.ps1
```

These private keys are under Git-ignored paths. The SSH login key and firmware
signing key serve different purposes and must not be reused or committed.

### 2. Compile MCUboot and the first signed application

Build MCUboot from its pinned Zephyr configuration:

```powershell
powershell -ExecutionPolicy Bypass -File .\Bootloader\tools\build-mcuboot.ps1 -Pristine
```

Build the optimized Rust firmware, convert it to a binary, sign it, verify the
signature, and enforce the flash-size limit:

```powershell
powershell -ExecutionPolicy Bypass -File .\Rust\tools\build-signed.ps1 -Version 0.1.0
```

The important outputs are:

- `Bootloader\build\zephyr\zephyr.hex`
- `Rust\artifacts\firmware-signed.bin`

### 3. Perform the initial flash through ST-LINK

The factory operation erases internal MCU flash, then programs and verifies
MCUboot and the signed Rust application:

```powershell
powershell -ExecutionPolicy Bypass -File .\Rust\tools\flash.ps1 -SkipBuild
```

The final message should be:

```text
MCUboot and signed Rust firmware programmed, verified, and started.
```

Wait several seconds for Ethernet auto-negotiation and DHCP.

### 4. Find and verify the board's IP address

Look in the router's DHCP client list for MAC address
`02-00-00-00-00-00`. During development the router normally assigned
`192.168.68.57`, but DHCP may choose a different address. Store the actual
address once so the remaining commands are easy to copy:

```powershell
$boardIp = "192.168.68.57"
Test-Connection $boardIp -Count 2
```

PowerShell variables exist only in the terminal session where they were set.
If you open a new VS Code terminal later, set `$boardIp` again. The remaining
examples repeat the assignment so each block can also be run independently.

If the router is unavailable, `arp -a` can show the address after the computer
has communicated with the board. The firmware falls back to
`192.168.0.10/24` after 30 seconds when DHCP is unavailable.

### 5. Run the UDP echo demo

Run the binary acceptance client against UDP port 7:

```powershell
$boardIp = "192.168.68.57" # Replace this if DHCP assigned another address.
powershell -ExecutionPolicy Bypass -File .\Rust\tools\udp_echo_test.ps1 `
    -BoardIp $boardIp
```

It sends 25 datagrams using payload sizes from zero through 1472 bytes and
checks the source address, source port, length, and every returned byte. A
successful run ends with output similar to:

```text
PASS: 25 UDP datagrams echoed byte-for-byte by 192.168.68.57:7.
Tested payload sizes: 0, 1, 32, 256, 1472 bytes.
```

### 6. SSH onto the board

Connect to the management service on TCP port 2222 using the provisioned
client key:

```powershell
$boardIp = "192.168.68.57" # Replace this if DHCP assigned another address.
ssh -i .\Rust\.ssh\client_ed25519 -p 2222 "board@$boardIp"
```

At the board prompt, try:

```text
help
status
echo hello from ssh
exit
```

This is a deliberately small embedded management shell, not Linux or a
general-purpose command prompt. It supports only the documented commands.
After `exit`, Windows OpenSSH may print `Connection ... closed by remote host`
and return a nonzero status because the embedded server closes the channel
without a separate SSH exit-status message. The displayed `bye` confirms the
command itself completed.

### 7. Build a newer signed release

Every update should have a version higher than the installed release. For
this demo, build version 0.1.1:

```powershell
powershell -ExecutionPolicy Bypass -File .\Rust\tools\build-signed.ps1 -Version 0.1.1
```

### 8. Flash the new release over SSH/Ethernet

Run the uploader on the PC after leaving the interactive board shell:

```powershell
$boardIp = "192.168.68.57" # Replace this if DHCP assigned another address.
powershell -ExecutionPolicy Bypass -File .\Rust\tools\ethernet-flash.ps1 `
    -HostName $boardIp `
    -KeyPath .\Rust\.ssh\client_ed25519
```

The client authenticates to the isolated `firmware-update` SSH subsystem,
shows erase and transfer progress, and finishes with output like:

```text
ERASING
READY
PROGRESS 188120/188120
OK rebooting into trial image
Firmware accepted. The board is rebooting into an MCUboot trial image.
```

The exact image length can change as the code changes. MCUboot verifies the
firmware signature, installs it as a trial, and starts it. Once the Rust
application reaches its health checkpoint it confirms the image; otherwise,
MCUboot restores the previous confirmed release after another reset.

### 9. Verify the updated board

Allow several seconds for the reboot and DHCP, then repeat the network checks:

```powershell
$boardIp = "192.168.68.57" # Replace this if DHCP assigned another address.
Test-Connection $boardIp -Count 2

powershell -ExecutionPolicy Bypass -File .\Rust\tools\udp_echo_test.ps1 `
    -BoardIp $boardIp

ssh -i .\Rust\.ssh\client_ed25519 -p 2222 "board@$boardIp" status
```

The final SSH command should report an active Ethernet link, the DHCP address,
UDP port 7, and SSH port 2222. It may then print the same expected
`closed by remote host` message described above. ST-LINK is still the recovery
path for replacing MCUboot or recovering a board whose application slots are
both unusable.

## Rust implementation architecture

The Rust version is a separate bare-metal application built with Embassy.
Embassy provides STM32 hardware drivers, an embedded async executor, and the
`embassy-net` TCP/IP stack; it does not require an RTOS.

```text
LAN8742A PHY -> STM32 Ethernet driver -> Embassy network runner
                                             |
                                      shared Stack handle
                                /             |             \
                       Link/DHCP task    UDP echo task     SSH task
                       LEDs + fallback   receive -> send   auth -> command
```

At startup, `main.rs` configures the clocks, takes ownership of the STM32
peripherals, builds the Ethernet driver, and starts four cooperative async
tasks. One task drives the network stack, one supervises link and DHCP state,
one serves UDP port 7, and one serves SSH on TCP port 2222. Sunset provides
the SSH protocol and cryptography; the STM32 hardware RNG supplies entropy for
key exchange. The Ethernet interrupt wakes network work when a packet arrives;
a task waiting at `.await` consumes no CPU.

Memory is fixed at compile time. DMA descriptors and network resources use
`StaticCell`, while the UDP and SSH sockets own bounded packet and payload arrays.
There is no heap, allocator, garbage collector, OS thread, or dynamically
allocated task.

Rust ownership prevents two drivers from configuring the same peripheral, and
mutable borrowing gives the UDP socket exclusive access to its buffers.
`Result` represents errors, `Option` represents values that may be absent, and
checked slices replace pointer-plus-length pairs.

See [the complete Rust architecture and Embassy primer](Rust/README.md#architecture)
for the request data path, task lifecycle, and a C/C++-to-Rust mental-model
table.

The boot and update path adds a small, separately built MCUboot stage:

```text
Reset -> MCUboot verifies active image -> Rust/Embassy application
                                              |
authenticated SSH update -> secondary slot --+
                                              |
                         reset -> verify -> trial boot
                                             /        \
                                      confirm          fail/reset
                                         |                 |
                                      keep it           roll back
```

ST-LINK remains the initial-install and recovery path. Normal releases travel
over the existing encrypted SSH connection, but MCUboot independently checks
their Ed25519 signature before executing them. An interrupted upload cannot
select a partial image because the application writes MCUboot's activation
magic only after the length, SHA-256, flash read-back, and slot-boundary checks
all pass.

### Connect to the Rust SSH service

SSH keys are embedded at build time but are never committed. From `Rust/`,
provision a persistent board host key and a local client key, then flash:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\provision_ssh.ps1
powershell -ExecutionPolicy Bypass -File .\tools\flash.ps1
```

Connect using the address assigned by DHCP:

```powershell
ssh -i .\.ssh\client_ed25519 -p 2222 board@BOARD_IP
```

The service accepts only the provisioned Ed25519 key; it has no password
login. See [the detailed SSH setup and security notes](Rust/README.md#ssh-management-service).

After the MCUboot factory image is installed once, build a newly versioned
signed image and upload it over Ethernet:

```powershell
cd Rust
powershell -ExecutionPolicy Bypass -File .\tools\build-signed.ps1 -Version 0.1.2
powershell -ExecutionPolicy Bypass -File .\tools\ethernet-flash.ps1 `
    -HostName BOARD_IP `
    -KeyPath .\.ssh\client_ed25519
```

The board stages the image outside the running slot, verifies SHA-256 from
flash, reboots into a trial, and confirms only after the new application
reaches its startup health checkpoint. See
[the detailed update workflow](Rust/README.md#signed-firmware-updates-over-ethernet).

### Rust testing strategy

Hardware-independent production rules live in `Rust/src/lib.rs` and are
tested from `Rust/tests/host.rs` on an ordinary Windows or Linux host. This
includes checked UDP payload
boundaries, MAC-address properties, fallback-network consistency, and payload
capacity. The STM32/Embassy dependencies are compiled only for ARM, so host
tests do not need an emulator or attached board.

The workflow in `.github/workflows/rust.yml` runs those host tests for every
push and pull request. It also checks formatting, runs strict Clippy against
the embedded target, and builds the release firmware. Physical Ethernet, PHY,
DHCP, and interrupt behavior remain covered by the board-level UDP test.

### Ethernet hardware: MAC, RMII, and PHY

The STM32 does not connect its digital Ethernet controller directly to the
cable. The board has two Ethernet components joined by RMII:

```text
UDP/IP software
      |
      v
STM32H723 Ethernet MAC
      |
      | RMII
      v
LAN8742A Ethernet PHY
      |
      v
RJ45 connector and Ethernet cable
```

**RMII** means **Reduced Media Independent Interface**. It is the digital
interface between the STM32's Ethernet MAC and the external LAN8742A PHY:

- The **MAC** constructs and receives Ethernet frames, filters MAC addresses,
  and moves frames between memory and the peripheral using DMA.
- The **PHY** converts digital frame data into the electrical signaling used
  by the Ethernet cable, and converts received electrical signals back into
  digital data.
- **RMII** transports data and control signals between the MAC and PHY.

RMII is “reduced” because it uses two data bits in each direction instead of
the four used by classic MII. At 100 Mbit/s, it transfers two bits on each
50 MHz reference-clock cycle, reducing the number of MCU pins required.

The NUCLEO-H723ZG uses these signals:

| Signal | STM32 pin | Purpose |
|---|---|---|
| `REF_CLK` | PA1 | Shared 50 MHz RMII reference clock |
| `CRS_DV` | PA7 | Indicates valid receive data |
| `RXD0`, `RXD1` | PC4, PC5 | Two-bit receive data bus |
| `TX_EN` | PG11 | Indicates an active transmission |
| `TXD0`, `TXD1` | PG13, PB13 | Two-bit transmit data bus |
| `MDIO` | PA2 | PHY configuration and status data |
| `MDC` | PC1 | Clock for the MDIO management interface |

`MDIO` and `MDC` are the management interface used alongside RMII. The
firmware uses them to configure the PHY and read state such as whether the
Ethernet link is up. They do not carry Ethernet frame payloads.

## Hardware

- NUCLEO-H723ZG
- USB cable connected to the board's ST-LINK USB connector
- Ethernet cable connected to a DHCP-enabled LAN

With the USB cable connected, Windows should expose:

- `ST-Link Debug`
- `STMicroelectronics STLink Virtual COM Port`
- a removable drive named `NOD_H723ZG`

Check them in PowerShell:

```powershell
Get-PnpDevice -PresentOnly |
    Where-Object {
        $_.FriendlyName -match 'ST-Link|ST-LINK' -or
        $_.InstanceId -match '^USB\\VID_0483'
    }

Get-Volume -FileSystemLabel NOD_H723ZG
```

## Project layout and C dependencies

The Embassy application under `Rust/` is standalone: cloning this repository
is sufficient to build, flash, and test the Rust firmware.

The original C application is retained as a reference implementation. Its IDE
projects use relative paths to HAL, CMSIS, BSP, and LwIP components from the
surrounding STM32CubeH7 repository, so the C version does not build standalone
from this extracted repository.

To build the C version, clone the matching STM32CubeH7 revision and initialize
the required dependencies:

```powershell
git clone https://github.com/STMicroelectronics/STM32CubeH7.git
cd STM32CubeH7
git checkout a2de035db3d87b6dff5ff055613489f273afac19
```

Then, from that `STM32CubeH7` repository root:

```powershell
git submodule update --init -- `
    Drivers/STM32H7xx_HAL_Driver `
    Drivers/CMSIS/Device/ST/STM32H7xx `
    Drivers/BSP/STM32H7xx_Nucleo `
    Drivers/BSP/Components/Common `
    Drivers/BSP/Components/lan8742 `
    Middlewares/Third_Party/LwIP
```

The C reference project is then available at:

```text
Projects\NUCLEO-H723ZG\Applications\LwIP\LwIP_UDP_Echo_Server
```

The files in this public repository were extracted from that location. The
Rust implementation does not use the STM32CubeH7 submodules.

## Install Visual Studio Code support

Install these VS Code extensions:

- `bmd.stm32-for-vscode`
- `marus25.cortex-debug`

The first extension normally installs its own compiler and OpenOCD. Version
3.2.13 bundles Node.js 16, however, while current `xpm` releases require
Node.js 20 or newer. Its **STM32: Install all the build tools...** command may
therefore fail with:

```text
Unsupported engine ... required: { node: '>=20.0' }
No such built-in module: node:readline/promises
```

Do not update npm inside the bundled Node 16 installation. Install an
up-to-date system Node.js instead. For example:

```powershell
winget install OpenJS.NodeJS.LTS
```

Close and reopen the terminal, then confirm:

```powershell
& "C:\Program Files\nodejs\node.exe" --version
```

Use the system Node installation to install the xPack tools:

```powershell
& "C:\Program Files\nodejs\npx.cmd" --yes xpm@latest install --global `
    @xpack-dev-tools/openocd@latest

& "C:\Program Files\nodejs\npx.cmd" --yes xpm@latest install --global `
    @xpack-dev-tools/arm-none-eabi-gcc@latest
```

GNU Make is also required. On this machine it was installed through
Chocolatey:

```powershell
choco install make
```

The xPacks are installed below:

```text
%APPDATA%\xPacks\@xpack-dev-tools\openocd\<version>\.content\bin
%APPDATA%\xPacks\@xpack-dev-tools\arm-none-eabi-gcc\<version>\.content\bin
```

## Import and configure the project

Open this folder as the VS Code workspace:

```text
Projects\NUCLEO-H723ZG\Applications\LwIP\LwIP_UDP_Echo_Server\STM32CubeIDE
```

Open the Command Palette with `Ctrl+Shift+P`, then run:

1. **STM32: Import CubeIDEProject**
2. **STM32: Check if the required build tools are present for STM32 for VSCode**

If the tools are not detected automatically, add workspace settings similar
to the following, substituting the versions installed on the computer:

```json
{
    "stm32-for-vscode.openOCDPath": "C:\\Users\\USERNAME\\AppData\\Roaming\\xPacks\\@xpack-dev-tools\\openocd\\VERSION\\.content\\bin\\openocd.exe",
    "stm32-for-vscode.armToolchainPath": "C:\\Users\\USERNAME\\AppData\\Roaming\\xPacks\\@xpack-dev-tools\\arm-none-eabi-gcc\\VERSION\\.content\\bin",
    "cortex-debug.armToolchainPath": "C:\\Users\\USERNAME\\AppData\\Roaming\\xPacks\\@xpack-dev-tools\\arm-none-eabi-gcc\\VERSION\\.content\\bin",
    "cortex-debug.openocdPath": "C:\\Users\\USERNAME\\AppData\\Roaming\\xPacks\\@xpack-dev-tools\\openocd\\VERSION\\.content\\bin\\openocd.exe"
}
```

After changing settings, run **Developer: Reload Window**.

The generated `openocd.cfg` must select the STM32H7 target:

```tcl
source [find interface/stlink.cfg]
source [find target/stm32h7x.cfg]
```

If the importer generates `source [find target/.cfg]`, replace it with the
second line above.

## Build

Run this Command Palette command:

```text
STM32: Build STM32 project
```

The debug build outputs are:

```text
STM32CubeIDE\build\debug\LwIP_UDP_Echo_Server.elf
STM32CubeIDE\build\debug\LwIP_UDP_Echo_Server.hex
STM32CubeIDE\build\debug\LwIP_UDP_Echo_Server.bin
```

## Flash the board

### From VS Code

Connect the ST-LINK USB cable and run:

```text
STM32: Build and flash to an STM32 platform
```

The expected OpenOCD result includes:

```text
** Programming Finished **
** Verified OK **
** Resetting Target **
```

### Direct OpenOCD command

If the extension's Flash command does not work, run the following from the
`STM32CubeIDE` project directory:

```powershell
$openOcd = Get-ChildItem `
    "$env:APPDATA\xPacks\@xpack-dev-tools\openocd\*\.content\bin\openocd.exe" |
    Sort-Object FullName |
    Select-Object -Last 1 -ExpandProperty FullName

$elf = (Resolve-Path ".\build\debug\LwIP_UDP_Echo_Server.elf").Path.Replace('\', '/')

& $openOcd `
    -f interface/stlink.cfg `
    -f target/stm32h7x.cfg `
    -c "adapter speed 3300" `
    -c "program {$elf} verify reset exit"
```

### ST-LINK removable-drive alternative

The HEX file can also be copied to the `NOD_H723ZG` drive:

```powershell
$volume = Get-Volume -FileSystemLabel NOD_H723ZG
$destination = "$($volume.DriveLetter):\"

Copy-Item `
    ".\build\debug\LwIP_UDP_Echo_Server.hex" `
    $destination
```

Use the HEX file rather than the raw BIN file because HEX contains its flash
addresses. Direct OpenOCD programming is preferable when an explicit
verification result is needed.

## Find the board on the network

The easiest method is to look in the router's DHCP client list for MAC address
`02-00-00-00-00-00`.

After learning the address, populate and inspect the Windows ARP cache:

```powershell
ping BOARD_IP
arp -a
```

For the setup used while writing this guide, the board received
`192.168.68.57`. DHCP can assign a different address later.

The application LEDs indicate Ethernet state:

- LED2: Ethernet cable connected
- LED3: Ethernet cable disconnected

## Test the UDP echo server

Replace the address below with the board's current DHCP address. Run this in
Windows PowerShell:

```powershell
$boardIp = "192.168.68.57"
$localEndpoint = New-Object Net.IPEndPoint([Net.IPAddress]::Any, 7)
$udp = New-Object Net.Sockets.UdpClient($localEndpoint)

try {
    $udp.Client.ReceiveTimeout = 3000
    $message = "STM32 UDP echo verified"
    $payload = [Text.Encoding]::ASCII.GetBytes($message)

    [void]$udp.Send($payload, $payload.Length, $boardIp, 7)

    $remote = New-Object Net.IPEndPoint([Net.IPAddress]::Any, 0)
    $reply = $udp.Receive([ref]$remote)

    "Reply from $($remote.Address):$($remote.Port): " +
        [Text.Encoding]::ASCII.GetString($reply)
}
finally {
    $udp.Close()
}
```

Expected output:

```text
Reply from 192.168.68.57:7: STM32 UDP echo verified
```

If ping works but the UDP test times out, confirm that the client is bound to
local port 7 and that Windows Firewall is not blocking inbound UDP port 7.

## Troubleshooting

### `node:readline/promises` during tool installation

The extension is running its bundled Node 16 against a newer `xpm`. Install
the xPacks manually with system Node 20 or newer as shown above. Do not rerun
the extension's broken installer.

### Headers such as `stm32h7xx_hal.h` or `lwip/opt.h` are missing

Initialize the Git submodules, then rerun **STM32: Import CubeIDEProject** so
the makefile is regenerated with all sources.

### OpenOCD reports `target/.cfg` not found

Correct `openocd.cfg` to use:

```tcl
source [find target/stm32h7x.cfg]
```

### The board stops responding after a debugging session

A debugger can leave the CPU halted. Stop the debug session with **Continue**
or **Disconnect and Resume**, or press the board's reset button. A normal
flash command ending in `verify reset exit` resets and runs the target.

## Origin and licensing

The C application is derived from STMicroelectronics STM32CubeH7,
NUCLEO-H723ZG LwIP UDP Echo Server, based on upstream commit
`a2de035db3d87b6dff5ff055613489f273afac19`.

ST-originated files retain their original copyright notices and are
covered by `LICENSE-ST.md`.

The retained LwIP-derived headers are covered by `LICENSE-LWIP.md`.

The Rust implementation is an independent Embassy-based port.
