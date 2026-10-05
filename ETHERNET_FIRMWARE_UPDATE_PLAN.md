# Ethernet Firmware Update Plan

## Implementation Progress

Last updated: 2026-07-31

| Phase | Status | Evidence / next checkpoint |
| --- | --- | --- |
| 0. Preserve recovery path | Complete | ST-LINK/OpenOCD mass erase, bootloader programming, application programming, halt/read, and reset have all been exercised on the board. |
| 1. Reproducible bootloader environment | Complete | Checked-in scripts build the pinned MCUboot configuration. The current minimal build is 41,136 bytes (31.38% of its partition). |
| 2. Flash layout and signed application | Complete | MCUboot validates and starts the signed Rust application at `0x08020200`, with the correct vector table, cache state, clocks, and global-interrupt handoff. |
| 3. Ethernet upload protocol | Complete | A real 188,120-byte signed image passed bounded staging, stream/read-back SHA-256, power-safe activation, swap, and reboot over Ethernet. |
| 4. SSH upload client and server commands | Complete | The authenticated `firmware-update` subsystem and non-interactive Windows client completed a fully scripted 0.1.7 -> 0.1.8 board update with progress reporting. |
| 5. Trial boot, confirmation, and rollback | Complete | Confirmed upgrades, an intentionally unconfirmed rollback, unknown-key rejection, and post-Ethernet-update confirmation have all passed on real flash. |
| 6. CI and release documentation | In progress | Host tests, ARM Clippy/build, disposable-key signing/verification, size enforcement, and private-key checks are in the workflow. Local validation is complete; the workflow still needs its first GitHub run. |

### Implementation log

- **2026-07-30 — Toolchain bootstrap:** selected Zephyr **v4.4.0** and
  checked out the minimal required modules. The MCUboot module is pinned at
  commit `ee39e2d694bd827ffd1bebbce2f571a9154e6ec2`.
- **2026-07-30 — Python compatibility adjustment:** Zephyr v4.4.0 requires
  Python 3.12 or newer, so the implementation environment uses Python
  **3.13.14** rather than the machine's existing Python 3.11 installation.
- **2026-07-30 — Compiler installed:** Zephyr SDK **1.0.1** with the
  `arm-zephyr-eabi` GNU toolchain is installed and registered. The minimal
  module set includes both legacy `cmsis` and `cmsis_6`; Zephyr's Cortex-M7
  headers require the latter.
- **2026-07-30 — Application sizing:** the current Rust application occupies
  approximately **281,508 bytes** of loadable flash. This still supports the
  proposed 384 KiB primary slot, but leaves limited growth room; every release
  build will enforce the slot-size limit.
- **2026-07-30 — First MCUboot build passed:** the NUCLEO-H723ZG configuration
  compiles with Ed25519 verification and offset swap. The linker reports
  **44,960 bytes** of 128 KiB flash (34.30%) and **15,296 bytes** of RAM. The
  build-time flash check calculated four 128 KiB sectors for the larger slot.
- **2026-07-30 — Relocation check caught an override:** Embassy's optional
  `memory-x` feature generated a default linker map at `0x08000000`, overriding
  the repository's MCUboot-aware `memory.x`. The feature is being removed so
  the checked-in layout is the single source of truth. No incorrectly linked
  image was flashed.
- **2026-07-30 — Erase-sector constraint discovered on hardware:** MCUboot
  correctly rejected the first signed image because offset swap reserves the
  primary slot's final 128 KiB sector for its trailer. A three-sector primary
  therefore has a **256 KiB effective signed-image limit**, not 384 KiB.
- **2026-07-30 — Size optimization preserves the architecture:** changing the
  Rust release profile from speed optimization (`3`) to size optimization
  (`z`) reduced the raw load image from about **281.5 KiB to 237.3 KiB**.
  This fits the effective limit with roughly 24 KiB of headroom, so the
  rollback-capable offset-swap architecture does not need to change.
- **2026-07-30 — First chain-load diagnosis:** MCUboot validated the signature
  and selected version 0.1.0, but the Rust application waited indefinitely
  while switching clocks. The board's normal Zephyr configuration leaves the
  CPU on a 550 MHz HSE/PLL clock, which is not the reset-like state expected by
  Embassy's clock initializer. The MCUboot overlay now uses the 64 MHz HSI for
  a clean handoff; hardware retest is pending.
- **2026-07-30 — Shared PLL source follow-up:** the first HSI retest progressed
  farther but waited for application PLL1 to lock. The board DTS separately
  enabled PLL2, and STM32H7 PLLs share their source selector; leaving PLL2
  active prevented Embassy from selecting HSI. The bootloader overlay now
  disables both PLL1 and PLL2.
- **2026-07-30 — Minimal bootloader console decision:** after both PLLs were
  disabled, Zephyr's board-default USART3 console waited forever for its
  transmit-ready flag because that console configuration assumed the original
  high-speed clock tree. MCUboot logging, `printk`, the console, and serial
  drivers are now disabled. Runtime application diagnostics remain available
  through defmt/RTT.
- **2026-07-30 — Interrupt handoff diagnosis:** the Rust executor ran after
  chain-load, but DHCP never completed. A live register read showed
  `VTOR=0x08000000`, so timer and Ethernet interrupts still targeted MCUboot's
  vector table. `CONFIG_BOOT_INTR_VEC_RELOC=y` now makes MCUboot set VTOR to
  the application's `0x08020200` vector table before jumping.
- **2026-07-30 — DMA/cache handoff:** after VTOR relocation the application
  reached DHCP polling but did not acquire its prior lease. Zephyr enables the
  Cortex-M7 caches; Ethernet DMA requires Embassy to establish cache state
  itself. `CONFIG_BOOT_DISABLE_CACHES=y` now flushes and disables instruction
  and data caches during chain-load, before Embassy reinitializes them.
- **2026-07-30 — Global-interrupt handoff diagnosis and fix:** a later live
  core-register check found `PRIMASK=1` after MCUboot's cleanup and jump.
  MCUboot disables all NVIC lines and globally masks interrupts before
  chain-loading; relocating `VTOR` alone does not clear that global mask.
  This explained why normal Rust instructions and MDIO reads worked while
  Embassy timers, PHY polling, DHCP, and Ethernet tasks did not advance. The
  Rust entry point now explicitly enables interrupts immediately after
  `embassy_stm32::init`, when the application clock, time driver, vector table,
  and handlers are ready. Hardware validation is the next checkpoint.
- **2026-07-30 — Host-client path fix:** the first live upload invocation
  exposed that Windows PowerShell may evaluate a parameter's default value
  before `$PSScriptRoot` is populated. The client now resolves its default
  signed-image path in the script body. The failure occurred before opening
  SSH or sending firmware, so it did not alter either flash slot.
- **2026-07-30 — Non-interactive SSH hardening:** the next client run reached
  desktop OpenSSH but waited for first-connection host-key input, which a
  binary upload must never answer. The client now enables batch mode and
  accepts only previously known or first-seen host keys without prompting.
  The stalled desktop processes were stopped; the board remained online on
  its unchanged active image.
- **2026-07-30 — First end-to-end Ethernet upgrade passed:** the authenticated
  SSH transport staged signed version 0.1.7, MCUboot swapped it into the
  primary slot, and the new image returned on DHCP at `192.168.68.57`.
  Direct flash inspection showed version 0.1.7 active; ping and all 25 UDP
  acceptance datagrams passed after reboot.
- **2026-07-30 — Post-success client cleanup:** the board's immediate reset
  left Windows OpenSSH waiting indefinitely even after the client received the
  updater's durable `OK` record. The client now gives SSH five seconds to exit
  and then reaps it locally. This changes only desktop cleanup; `OK` is emitted
  after image and activation-marker verification in flash.
- **2026-07-30 — Repeatable client flow proven:** a second signed update,
  version 0.1.8 (188,120 bytes), completed in about 15 seconds through the
  corrected Windows client. It displayed erase, ready, 12 progress, and final
  success records; returned normally after reset; reacquired the same DHCP
  lease; passed SSH status and all 25 UDP checks; and programmed MCUboot's
  `copy_done` and `image_ok` confirmation bytes. A live register read also
  confirmed `PRIMASK=0`.
- **2026-07-30 — Final local quality gate passed:** Rust formatting, all 16
  host tests, strict ARM Clippy, and the release firmware build pass. Every
  checked-in PowerShell script parses, the GitHub Actions YAML parses,
  `git diff --check` is clean, and no generated SSH or firmware-signing key is
  tracked. The remaining CI checkpoint is the workflow's first hosted run
  after publication.
- **2026-07-31 — Operator demo documented:** the main README now contains one
  numbered Windows walkthrough for key provisioning, MCUboot and Rust builds,
  first installation through ST-LINK, DHCP/IP discovery, UDP verification,
  interactive SSH, a newly versioned signed build, Ethernet upload through
  the isolated SSH subsystem, and post-update verification.
- **2026-07-31 — README demo executed verbatim:** the documented workflow was
  run from Windows PowerShell with the attached board. A pristine 41,136-byte
  MCUboot and signed version 0.1.0 were compiled and factory-flashed; DHCP,
  ping, 25 UDP cases, and interactive `help`/`status`/`echo`/`exit` all passed.
  Signed version 0.1.1 (188,120 bytes) was then uploaded over SSH with complete
  progress output. After reboot, ping, all 25 UDP cases, and SSH status passed
  again at `192.168.68.57`.
- **2026-07-31 — Pre-commit cleanup passed:** documentation now uses portable
  user paths and consistent flash-capacity figures. Formatting, 16 host tests,
  strict ARM Clippy, the release build, PowerShell syntax, workflow YAML,
  relative Markdown links, whitespace, and tracked-key/artifact checks all
  pass.
- **2026-07-30 — Signed chain-load proven:** the factory-flash script now
  erases the device, installs MCUboot, and installs the signed Rust image.
  MCUboot validates version 0.1.0 and transfers control successfully.
  A live register check confirmed the application vector table is active at
  `VTOR=0x08020200`.
- **2026-07-30 — Final size checkpoint:** release LTO plus `opt-level = "z"`
  reduced the signed application to **182,752 bytes**. The minimal, console-free
  MCUboot build occupies **41,136 bytes**, leaving comfortable headroom in both
  executable regions.
- **2026-07-30 — Physical-link checkpoint:** the Rust executor is running and
  the LAN8742 PHY responds to MDIO reads, but its Basic Status Register reads
  `0x7809`: link is down and auto-negotiation is incomplete. The application
  correctly remains in its link-wait state (error LED on, ready LED off).
  Router port/client state and the physical connection are being checked
  before the updater's end-to-end hardware test.
- **2026-07-30 — Update writer implemented:** the application now exposes an
  authenticated SSH subsystem named `firmware-update`. It accepts a fixed,
  versioned header, rejects images outside the effective 256 KiB limit, erases
  only the secondary partition, and writes the image at `0x080A0000` in
  STM32H7-aligned 32-byte units.
- **2026-07-30 — Transfer and flash verification implemented:** the updater
  checks the sender-provided SHA-256 digest while receiving, then hashes a
  complete flash read-back. MCUboot's secondary `swap_info` is programmed
  before its magic, making the magic the final one-way activation write.
- **2026-07-30 — Trial confirmation implemented:** a normal factory boot
  leaves the primary trailer untouched. After a test swap, a surviving
  application detects MCUboot's primary-slot magic and writes the aligned
  `image_ok` word. MCUboot can therefore retain a healthy image and revert an
  image that never reaches this startup checkpoint.
- **2026-07-30 — Host and ARM validation:** 16 host tests pass, including
  byte-for-byte fixtures derived from pinned MCUboot v4.4.0. Strict ARM Clippy
  passes, and signed updater version 0.1.1 is **188,088 bytes**, leaving
  74,056 bytes in the effective image area. MCUboot plus that signed image
  were erased, programmed, verified, and started on the attached board.
- **2026-07-30 — Trial confirmation proven on hardware:** signed version
  0.1.2 was placed at the exact updater staging address and activated with the
  same tested trailer bytes used by the SSH writer. MCUboot installed it over
  version 0.1.1. A flash read showed active version 0.1.2, `copy_done = 1`,
  `image_ok = 1`, and the expected 32-byte-aligned MCUboot magic.
- **2026-07-30 — Automatic rollback proven on hardware:** the opt-in
  `rollback-test` build produced signed version 0.1.3 and deliberately withheld
  `image_ok`. The first boot showed active version 0.1.3 with the confirmation
  word still erased. After the next reset, MCUboot restored confirmed version
  0.1.2 byte-for-byte. Normal builds do not enable this test feature.
- **2026-07-30 — Health checkpoint tightened:** confirmation now occurs only
  after the Ethernet runner and supervisor receive multiple executor turns.
  This avoids depending on a cable, DHCP, or a timer interrupt while proving
  that the network tasks can at least be scheduled. Signed version 0.1.5
  confirmed successfully at this later checkpoint on hardware.
- **2026-07-30 — CI packaging expanded:** GitHub Actions now installs pinned
  `imgtool` 2.4.0, generates an ephemeral Ed25519 test key, converts the ARM
  ELF to binary, signs and verifies it, enforces the 262,144-byte effective
  limit, and fails if either development private/public signing-key file is
  present in the checkout.
- **2026-07-30 — Unknown signing key rejected on hardware:** an otherwise
  valid version 9.9.9 image was signed with a newly generated unrelated
  Ed25519 key, staged, and marked pending. MCUboot rejected it and left
  confirmed version 0.1.5 active. The temporary private test key was removed.

### Changes from the original plan

- The planned offset-swap staging address is now explicit:
  the secondary partition begins at `0x08080000`, but an incoming signed image
  begins at `0x080A0000`. MCUboot reserves the first secondary sector as swap
  workspace.
- The original capacity calculation treated the complete primary partition as
  application space. MCUboot rounds its trailer reservation up to a whole
  erase sector, reducing the maximum signed image to 256 KiB. Release builds
  now fail when the signed artifact exceeds `0x40000` bytes.
- The implementation uses an external Zephyr workspace at
  `%USERPROFILE%\zephyrproject-v4.4.0` so generated dependencies and build
  products do not enter this application repository.
- The first protocol intentionally restarts interrupted transfers from byte
  zero, as allowed by the original plan. "Resumable" now means safe retry
  rather than retaining partial chunks: incomplete slot data has no MCUboot
  magic and cannot replace the active image.
- The marker order is more power-fail-safe than MCUboot's general helper:
  `swap_info` is written first and the recognizable magic is written last.
- MCUboot's Cortex-M cleanup intentionally leaves `PRIMASK` set. The Rust
  application now clears that global mask after `embassy_stm32::init`; this
  explicit ownership boundary is required for Embassy timers and Ethernet
  interrupts after chain-load.
- The Windows upload client now resolves its default artifact path in the
  script body, enforces non-interactive SSH behavior, and bounds cleanup after
  the board's success record and immediate reset.
  A reset before that last aligned write leaves only inert secondary data.

## Goal

Add safe firmware updates over the NUCLEO-H723ZG Ethernet connection while
keeping the existing Embassy-based Rust application.

The first installation and emergency recovery will continue to use the
onboard ST-LINK USB connection. After that initial installation, an
authenticated operator should be able to upload and activate a signed Rust
firmware image through Ethernet.

This is an application-mediated over-the-air (OTA) update, even though the
network is wired. It is not Ethernet support in the STM32 factory ROM
bootloader.

## Reference architecture

Zephyr demonstrates this workflow using MCUmgr's Simple Management Protocol
(SMP) over UDP and MCUboot. The running application receives a signed image,
writes it to a secondary flash slot, and asks MCUboot to try it after reset.
The new image must confirm itself or MCUboot restores the previous image.

Relevant upstream references:

- [Zephyr OTA overview](https://docs.zephyrproject.org/latest/services/device_mgmt/ota.html)
- [Zephyr MCUmgr documentation](https://docs.zephyrproject.org/latest/services/device_mgmt/mcumgr.html)
- [Zephyr SMP server sample](https://docs.zephyrproject.org/latest/samples/subsys/mgmt/mcumgr/smp_svr/README.html)
- [Zephyr NUCLEO-H723ZG support](https://docs.zephyrproject.org/latest/boards/st/nucleo_h723zg/doc/index.html)
- [MCUboot image-management model](https://docs.zephyrproject.org/latest/services/device_mgmt/smp_groups/smp_group_1.html)
- [STM32H723/733 reference manual](https://www.st.com/resource/en/reference_manual/dm00603761.pdf)

The upstream Zephyr SMP-over-UDP sample is not currently tested against the
NUCLEO-H723ZG, and the board's default device tree does not define MCUboot
image slots. We will use the architecture, but must supply and validate an
H723-specific flash layout.

## Proposed system

```text
Development computer
    |
    | SSH over Ethernet
    | authenticated with the existing client key
    v
Running Rust application
    |
    | receive, hash, and program signed image
    v
Secondary flash slot
    |
    | mark update pending and reset
    v
MCUboot
    |
    +-- reject an invalid signature
    |
    +-- install and trial-boot a valid image
            |
            +-- application confirms healthy -> keep it
            |
            +-- no confirmation -> restore previous image
```

MCUboot will be responsible only for image authenticity, installation,
trial-boot state, and rollback. It will not contain Ethernet or SSH. Keeping
networking in the main application makes the bootloader smaller and easier to
audit.

## Flash layout

The STM32H723ZG contains 1 MiB of single-bank flash organized as eight
128 KiB erase sectors. The current signed updater release is 188,120 bytes.

The initial target layout is:

| STM32 sectors | Address range | Size | Purpose |
|---|---|---:|---|
| 0 | `0x0800_0000`–`0x0801_FFFF` | 128 KiB | MCUboot |
| 1–3 | `0x0802_0000`–`0x0807_FFFF` | 384 KiB | Active image, slot 0 |
| 4–7 | `0x0808_0000`–`0x080F_FFFF` | 512 KiB | Update image, slot 1 |

The final primary sector is MCUboot's trailer workspace, so the effective
signed-image area is 256 KiB despite the 384 KiB partition. The current image
has 74,032 bytes of headroom. The secondary slot is one erase sector larger
than the active slot, as required by MCUboot's swap-using-offset mode.

### Why MCUboot instead of `embassy-boot`

[`embassy-boot`](https://docs.embassy.dev/embassy-boot/0.7.0/default/index.html)
is also power-fail-safe and supports signed trial boots and rollback. It
normally uses bootloader, active, DFU, and state partitions, with the DFU
partition at least one erase page larger than the active partition.

On this MCU, every internal-flash erase page is 128 KiB. Giving the state its
own page would consume a disproportionate share of the available flash.
MCUboot stores its swap state in slot trailers, making better use of this
board's coarse flash geometry.

This decision should be revisited if external QSPI flash is added or the
application becomes small enough to use two-sector active storage.

## Trust and signing model

Transport authentication and firmware authenticity are separate:

- The existing SSH client key decides who may initiate an update.
- A separate offline firmware-signing key decides what MCUboot may execute.
- MCUboot contains only the firmware-signing public key.
- The firmware-signing private key must never be embedded in the board,
  checked into Git, or reused as an SSH key.

For local development, a provisioning script will create a Git-ignored
development signing key. CI may use an explicitly non-production test key to
verify packaging. A production pipeline must obtain its signing key from an
appropriate secret store or hardware signer.

Every accepted image must include:

- an MCUboot image header;
- a board or product identity;
- an image version;
- the exact payload length;
- a SHA-256 digest; and
- a signature supported by the pinned MCUboot configuration.

MCUboot signature verification is mandatory even though SSH already provides
an authenticated and encrypted transport. This preserves secure boot if the
network-facing application is compromised.

## Ethernet upload protocol

The first implementation will reuse the existing SSH server rather than add
Zephyr's unauthenticated example UDP transport.

Preferred interface:

```text
SSH subsystem: firmware-update
```

The subsystem will carry a small framed protocol:

1. The client sends a fixed header containing protocol version, image length,
   and SHA-256 digest.
2. The board validates the header and slot capacity.
3. The board erases the secondary slot.
4. The client streams exactly the declared number of bytes.
5. The board hashes data while programming it.
6. The board reads back and verifies the completed image.
7. The board writes the MCUboot pending-update metadata last.
8. The board sends an explicit success record and resets.

Writing the pending marker last is important. A reset, cable disconnect, or
power failure before that point must leave the currently running image
selected and the incomplete secondary image ignored.

The first version may restart an interrupted upload from byte zero. Resumable
chunks can be added later if field conditions justify the complexity.

Because the STM32H723 has single-bank flash, erase and program operations may
temporarily stall code execution and network servicing. The client protocol
must use suitable timeouts, and the implementation must test the longest
128 KiB sector erase on real hardware.

## Boot and rollback lifecycle

```text
Normal boot
    |
    +-- no pending image -> boot slot 0
    |
    +-- pending signed image
            |
            +-- install/swap fails -> retain old image
            |
            +-- install succeeds -> trial boot new image
                                      |
                                      +-- self-tests pass
                                      |      -> confirm image
                                      |
                                      +-- crash/reset/timeout
                                             -> MCUboot reverts
```

The new application should confirm itself only after:

- clocks and memory initialize;
- the Ethernet driver and PHY initialize;
- the network executor runs;
- configuration is internally consistent; and
- any other product-critical self-test succeeds.

DHCP success should not be required for confirmation because the router may
be unavailable. Confirmation should instead require that the local network
stack and PHY can be initialized without a firmware fault.

## Implementation phases

### Phase 1: MCUboot feasibility spike

- Pin compatible Zephyr and MCUboot revisions.
- Create an H723ZG MCUboot board overlay and configuration.
- Define the proposed flash partitions.
- Select one signature algorithm and provision a development key.
- Build MCUboot and prove it fits in sector 0.
- Relink the Rust application for slot 0 with the required MCUboot header
  offset.
- Package and sign the Rust binary with `imgtool`.
- Produce a combined first-install image for ST-LINK.
- Boot the signed Rust application through MCUboot on the real board.

This phase must not yet expose remote flash writes.

### Phase 2: Flash writer and image staging

- Add the Embassy STM32 flash driver.
- Add bounded, sequential writes to slot 1.
- Enforce address, alignment, length, and partition boundaries.
- Hash the stream and perform read-back verification.
- Implement the pinned MCUboot trailer/pending metadata in a small isolated
  module.
- Test the trailer encoder against fixtures generated by the same MCUboot
  revision.
- Ensure no update operation can erase sector 0 or slot 0.

### Phase 3: SSH update transport

- Accept only the `firmware-update` subsystem or an equivalently isolated SSH
  execution request.
- Require the already-provisioned `board` account and Ed25519 client key.
- Implement the framed header, streaming body, progress responses, and final
  acknowledgement.
- Add a Windows PowerShell client that validates the local file before
  connecting and displays progress without printing keys.
- Reject concurrent updates and ordinary shell commands during an update.
- Preserve the existing UDP echo service whenever flash hardware is idle.

### Phase 4: Trial boot, confirmation, and rollback

- Request a non-permanent MCUboot test upgrade.
- Reset only after all image and metadata writes complete.
- Detect an unconfirmed trial boot.
- Run startup health checks.
- Confirm a healthy image.
- Demonstrate automatic rollback when confirmation is deliberately withheld.

### Phase 5: Hardening and release automation

- Protect the MCUboot sector from accidental application writes.
- Add image version and board-identity enforcement.
- Decide and document the downgrade policy.
- Add rate limits and failed-update counters.
- Define watchdog behavior during erases and swaps.
- Add production signing without exposing the private signing key to normal
  developers or CI logs.
- Generate a release bundle containing the signed update image, hash,
  version, and update instructions.

## Implemented repository additions

```text
Bootloader/
    README.md
    west.yml
    prj.conf
    boards/nucleo_h723zg.overlay
    tools/build-mcuboot.ps1
    tools/provision-signing-key.ps1
Rust/
    memory.x
    src/firmware_update.rs
    tools/build-signed.ps1
    tools/ethernet-flash.ps1
    tools/flash.ps1
```

Zephyr and MCUboot source should be fetched at pinned revisions rather than
copied wholesale into this repository.

The current ST-LINK flash script should remain available for first install,
debugging, and recovery.

## CI checks

CI should:

- build the pinned MCUboot configuration;
- build the Rust application at its slot-0 address;
- fail if either flash budget is exceeded;
- run existing Rust host tests and Clippy;
- unit-test upload framing, bounds checks, and MCUboot metadata encoding;
- package and sign an image with a non-production test key;
- validate the signed image with MCUboot's tooling;
- ensure private development and production keys are ignored; and
- publish signed test artifacts only where appropriate.

Hardware-in-the-loop tests remain necessary because desktop tests cannot
model STM32 flash stalls, resets, Ethernet DMA, or power interruption.

## Board acceptance tests

The ledger separates the implemented and exercised first release from deeper
fault-injection work needed before treating it as a production field updater:

| Acceptance test | Status | Evidence / remaining work |
| --- | --- | --- |
| Install MCUboot and initial Rust image through ST-LINK | Passed | Factory erase, program, verify, and boot repeated successfully. |
| Upload a valid signed update through Ethernet | Passed | Scripted 0.1.7 -> 0.1.8 SSH upload completed with progress and `OK`. |
| SSH and UDP echo after trial boot | Passed | SSH status and 25 binary UDP cases passed after DHCP recovery. |
| Confirm healthy image and retain it across reset | Passed | `copy_done=1` and `image_ok=1` observed in flash; repeated confirmed boots passed. |
| Withhold confirmation and restore previous image | Passed | Rollback-test 0.1.3 booted once, then restored 0.1.2 byte-for-byte. |
| Reject an image signed by an unknown key | Passed | MCUboot rejected unrelated-key version 9.9.9 and retained 0.1.5. |
| Reject wrong-board or oversized image | Partial | Host/unit bounds and signing-key isolation pass; explicit board-identity metadata is not part of version 1. |
| Interrupt uploads at multiple offsets | Not run | Activation-last design is unit-tested; physical network fault injection remains. |
| Interrupt power during erase, program, swap, and first boot | Not run | Requires controlled power fault injection. |
| Reject an unauthorized SSH client key | Not run | Authentication is key-only and provisioned-key login passes; negative board test remains. |
| Multiple alternating upgrades and rollbacks | Partial | Two consecutive Ethernet upgrades plus one rollback pass; longer cycling remains. |
| Recover after corrupting both application slots | Partial | ST-LINK mass erase/recovery passes, but deliberate dual-slot corruption was not performed. |

## Non-goals for the first version

- Replacing ST-LINK for initial provisioning or unbricking
- General-purpose SCP or SFTP access
- Updating MCUboot itself over Ethernet
- Delta or compressed firmware updates
- Multiple simultaneous uploads
- Cloud fleet management
- Resuming a partially transferred image

## Principal risks

- A flash-layout or linker mistake could overwrite MCUboot.
- The 384 KiB active slot limits future application growth.
- Single-bank flash operations can pause network service.
- MCUboot trailer formats must exactly match the pinned bootloader version.
- Power-fail safety requires testing resets at many intermediate states.
- A leaked firmware-signing key defeats secure boot and requires a planned
  key-rotation strategy.

These risks are why the work begins with a separately recoverable MCUboot
spike and keeps ST-LINK as the recovery path throughout development.

## Definition of done

Ethernet firmware update is considered production-ready when:

- only correctly signed images for this board can boot;
- interrupted transfers never damage the active image or bootloader;
- failed trial images roll back automatically;
- healthy images confirm and remain installed;
- the complete flow is scripted and documented for Windows;
- CI enforces image validity and flash budgets; and
- ST-LINK recovery has been exercised and documented.
