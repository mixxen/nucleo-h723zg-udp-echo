# NUCLEO multicast smoke test

**Status: build/package checks passed; physical execution pending.** No board is
attached to the implementation workspace. This procedure closes Phase 3's
physical-path gate. Host loopback success and an ARM ELF do not establish it.

## Image and setup

The smoke image publishes Protobuf status at 10 Hz to `239.255.42.1:42001` and
health at 1 Hz to `239.255.42.1:42002`, TTL 1. It reuses native RMII bring-up and
DHCP/fallback supervision. Source ports match destination stream ports. Device
ID is `board-01`; use one board with the current default MAC or provision unique
IDs/MACs before using several.

This is a publisher-only bench image. Its request/rejection counters stay zero;
there is no command endpoint, SSH updater, or trial-image confirmation service.
The [complete Phase 4 application](PHASE4.md) now adds commands and all three format choices. Use the existing factory
install path for this smoke image, not a remote trial upgrade.

From `Rust/`, compile without SSH keys:

```sh
rustup update stable
cargo +stable build --locked --release --no-default-features \
  --features multicast-smoke --bin nucleo-h723zg-native-rmii-multicast-smoke
```

On the configured Windows bench workstation, the new selector uses the existing
signing key and bootloader setup:

```powershell
.\tools\build-signed.ps1 -MulticastSmoke -Version 0.1.0
```

After reviewing the board selection, factory-install with:

```powershell
.\tools\flash.ps1 -MulticastSmoke -Version 0.1.0
```

That existing factory-install script mass-erases internal flash and writes both
MCUboot and the signed application. It replaces the current board contents.
Do not combine `-MulticastSmoke` with other variants, profiling, benchmark, or
performance switches. Existing selectors keep their previous behavior. Keys stay
outside version control. The disposable CI signing key is for verification only
and will not match a bench bootloader's provisioned key.

## Receive on physical hosts

1. Connect the board and two receiver machines to the intended Ethernet segment.
   Record switch model/configuration, IGMP snooping/querier settings, VLAN, NIC
   address, MTU, OS and firewall rules. Confirm MTU is at least 1052 bytes for the
   v1 datagram ceiling; do not rely on application fragmentation.
2. Read RTT startup diagnostics. Record the boot ID, DHCP address (or documented
   fallback), firmware revision and image version. RNG error/timeout stops the
   service instead of inventing a boot ID. The actual RNG/clock error path still
   needs bench validation; driver initialization/recovery includes synchronous
   hardware operations that an async timeout cannot preempt.
3. On each receiver, run the following from the repository root, replacing
   `192.168.0.20` with that receiver's own address and `192.168.0.10` with the
   board's observed address:

   ```sh
   python Rust/tools/message-demo/python/listener.py --format protobuf \
     --interface 192.168.0.20 --source 192.168.0.10 --device board-01 \
     --duration 60 --log Rust/artifacts/messaging/board-receiver-a.jsonl
   ```

   Use a distinct log filename on receiver B. Create the artifact directory first.
4. Capture packets on a receiving physical NIC, for example on Linux:

   ```sh
   sudo tcpdump -ni eth0 -s 0 -w board-multicast.pcap \
     'igmp or (udp and dst host 239.255.42.1)'
   ```

   Confirm group membership on the selected NIC, multicast destination, source
   ports 42001/42002, TTL 1, and usable UDP checksums. A sender-side capture before
   checksum offload is not sufficient evidence of an invalid on-wire checksum.
5. Confirm both receivers obtain both streams with the same boot ID and matching
   sequences. The first received sequence may be nonzero because DHCP/link waits
   consume scheduled slots. Each receiver establishes its own baseline.
6. Unplug/reconnect Ethernet. Both listeners should become stale and resume with
   current snapshots, without a backlog burst. Record DHCP/address changes,
   skipped-publication counters and any local send errors. Update a `--source`
   filter if DHCP changes the board address. Local enqueue success does not
   prove delivery.
7. Reset the board. Confirm the boot ID changes, the listener resets both stream
   histories, and it ignores any delayed packet from the retired boot. Leave the
   run active long enough to exercise switch membership aging/refresh.

## Resource and behavior notes

The smoke image has three socket slots: DHCP plus two TX-only sockets. Each
publisher has one 1016-byte TX payload slot and one 1016-byte encode buffer,
with zero RX payload slots. The 4064-byte payload subtotal excludes metadata,
Ethernet DMA queues, decoded values, task state and stack. This is not a measured
RAM/stack budget for the complete Phase 4 application.

Both publishers use the shared monotonic schedule and `try_send_to`; there is no
pending send future to cancel. Missed due slots are skipped. A link/configuration
outage closes each socket to clear its bounded pending TX packet. Reconnect
reopens it at the stream's source port. Health describes active local problems;
old error counts alone do not keep the service degraded.

## Evidence to record

| Gate | Evidence | Current result |
|---|---|---|
| ARM build and lint | Rust 1.98.1; pinned Embassy/xarxa; release ELF | Passed locally |
| Signed-image budget | Disposable-key sign and verification, imgtool 2.4.0 | 66,584 bytes; limit 262,144 |
| Factory boot | Bootloader and RTT log | Pending hardware |
| Hardware boot nonce | Boot IDs across reset; RNG fault behavior | Pending hardware |
| Two physical receivers | Both JSONL logs and NIC details | Pending hardware |
| TTL, ports, UDP checksum and IGMP | Receiver-side packet capture | Pending hardware |
| Link recovery and no backlog burst | Logs before/during/after cable interruption | Pending hardware |
| Membership aging and reset | Longer run and new boot history | Pending hardware |

Attach results with the exact revision and configuration. Leave failed or
unperformed gates explicit. Controlled performance comparisons and complete
command/stream firmware integration remain later phases.
