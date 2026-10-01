# Phase 4: complete NUCLEO messaging application

**Implementation and local build/package checks complete; physical acceptance
pending.** Base: merged Phase 3, `4a431b8` (PR #3). No board was attached or flashed.
The earlier Phase 3 physical multicast gate remains open.

## What runs on the board

`nucleo-h723zg-native-rmii-messaging` uses the existing native RMII interface,
400 MHz CPU configuration, DHCP with `192.168.0.10/24` fallback, and MCUboot
application layout. It has one command task and two independent publisher tasks.
CSV, JSON (`prototype-json-v1`), and Protobuf use the existing contract. CRC is off.

| Service | Default | Behavior |
|---|---|---|
| Commands | UDP 42000, unicast | Device info, synthetic status, and health; correlation echoed |
| Status | 239.255.42.1:42001, 10 Hz | Synthetic 25.5 C, valid, separate sequence |
| Health | 239.255.42.1:42002, 1 Hz | Service state and saturating counters, separate sequence |
| Profiling, optional | UDP 5001 | Existing executor and painted-stack snapshot protocol |

Both multicast source ports match their destination ports; TTL is 1. Commands
must be addressed to the board's current unicast IPv4 address. There are no
multicast ACKs, retries, SSH service, updater, or echo endpoint in this image.

Edit `src/servers/messaging_config.rs` to change device ID, ports, multicast group,
periods or firmware version, then rebuild. Use identical settings across format
comparisons. Device ID defaults to `board-01`; the shared native MAC is still
`02:00:00:00:00:00`. Assign unique IDs/MACs before using multiple boards. Firmware
version in device info is a source constant, separate from the MCUboot signing
version supplied to the packaging script.

## Build and install

From `Rust/`, select exactly one encoding:

```sh
cargo +stable build --locked --release --no-default-features \
  --features messaging-protobuf --bin nucleo-h723zg-native-rmii-messaging
```

Replace `messaging-protobuf` with `messaging-csv` or `messaging-json`. The portable
crate retains all three codecs by default for host tools. Firmware disables its
default features and enables only the selected codec; CI verifies the resolved
feature set. The binary rejects missing/multiple formats and mixed application
variants. It links without an allocator. No dependency revisions changed.

On the configured Windows bench workstation, from `Rust/`:

```powershell
.\tools\build-signed.ps1 -Messaging protobuf -Version 0.1.0
.\tools\flash.ps1 -Messaging protobuf -Version 0.1.0
```

The resulting artifact is `artifacts/firmware-messaging-protobuf-signed.bin`.
Use `csv` or `json` for the other images. The existing factory-flash command
**mass-erases internal flash** and installs MCUboot plus the signed application.
Use the board's provisioned signing key, not the disposable verification key.
`-SkipBuild` uses the selected existing artifact. Select only one application.
`-Benchmark` and `-Performance` are not accepted for messaging; the baseline is
already quiet and keeps the same clock/optimization settings in all formats.

## Demo: two listeners plus commands

Install Python dependencies from `Rust/protocol/requirements-phase1.txt`.
Use two physical receiver machines on the board's Ethernet segment. Record each
NIC address, OS, firewall, switch/IGMP settings and MTU using the
[physical multicast checklist](SMOKE_BENCH.md). Read the board's actual IPv4
address and boot ID from RTT; the addresses below are examples.

From the repository root on receiver A:

```sh
python Rust/tools/message-demo/python/listener.py --format protobuf \
  --interface 192.168.0.20 --source 192.168.0.10 --device board-01 \
  --duration 300 --log board-listener-a.jsonl
```

Run the same listener on receiver B using **its own** interface address and a
separate log. While both listen, run commands from receiver A in another terminal:

```sh
python Rust/tools/message-demo/python/client.py --format protobuf \
  --host 192.168.0.10 --bind 192.168.0.20 --device board-01 \
  --commands info status health --repeat 100 --interval 1 \
  --log board-commands.jsonl
```

`--interval` waits between completed attempts. There is one outstanding command,
a one-second response deadline, and no retries. Expected: device info identifies
Protobuf; status is synthetic/valid; health's received count advances; both
listeners independently receive status and health with the same boot ID. Normal
successful sends generate no per-packet RTT output on the MCU.

Repeat with the CSV and JSON images, matching every host `--format`. Log files
are JSONL in all cases, irrespective of wire format. A listener's exit code only
establishes that both streams were seen; inspect gaps, freshness and errors as
well. Host loopback tests do not substitute for these physical runs.

## Bounded work and recovery

The command socket has two RX/TX packet slots. Each publisher has one TX slot and
zero RX slots. Payload storage totals 10,240 bytes. The command task explicitly
yields between received packets, including invalid ones. Parsing and encoding
run outside the short critical sections used to update counters. Error replies
are limited to a burst of 10 and 10/second thereafter by the same portable
`CommandPolicy` used by the host responder.

The pinned stack has one network-state wake slot. Only the supervisor uses its
event waits; messaging tasks sample readiness every 10 ms while waiting. This
avoids tasks continually waking each other. The optional profiling startup wait
uses the same sampling approach.

Each send enqueues immediately, then waits at most 50 ms for the local queue to
drain (or one period if a publisher's period is shorter). A link/configuration
loss or timeout closes the socket, clearing queued data; the task rebinds when
ready. This bounds application queue age and does not establish that the peer
received a frame. Ethernet DMA may already own a frame after the UDP queue drains.

Each publisher's absolute schedule advances sequences for missed slots, attempts
and failures. Late wakeups produce at most one current sample, not a catch-up
burst. While offline, due publications increase the skipped counter. Historical
counters survive reconnects, and activity-specific error state clears on a
successful send for that activity. A first health response during recovery can
therefore still report degraded state before its own successful send.

The shared network supervisor now cancels DHCP waiting if the cable disappears;
it restarts DHCP after loss of link or IPv4 configuration. Its ordinary 30-second
DHCP fallback remains. Link events must actually be observed by the PHY/stack;
a sufficiently brief interruption can go unnoticed. A new hardware RNG boot ID
is generated only on reboot, not on reconnect. RNG failure stops service and
withholds trial confirmation. The async timeout cannot preempt synchronous
hardware waits inside the pinned RNG/Ethernet drivers.

Counters describe application observations. Oversized datagrams delivered to the
command socket are rejected without partial parsing. Packets dropped by the NIC,
IP layer, checksum validation or full socket queues cannot be counted by this
application. `send_errors` reports local enqueue/drain failures, not proven loss
at a receiver. CRC rejection instrumentation remains Phase 5.

## MCUboot checkpoint and rollback

Before confirming a trial, the image obtains its boot nonce, checks request
encode/decode and all response/publication encoders, starts the network/command/
publisher tasks, and observes timer progress plus initialization flags. This
checkpoint needs no cable, DHCP lease, command client or multicast receiver.
It establishes local initialization, not physical Ethernet health.

`boot_confirmation.rs` checks MCUboot trailer magic, writes only an erased
`image_ok` word, and verifies it by reading back. An ordinary factory image has
no trial trailer and is left unchanged. Failure is reported over RTT. There is
no automatic reset/watchdog added here; reset the board to exercise rollback.

To test a trial upgrade, begin with a working **managed** firmware image whose
SSH updater is available, then upload the selected signed messaging artifact:

```powershell
.\tools\ethernet-flash.ps1 -HostName 192.168.0.10 -KeyPath <operator-private-key> `
  -ImagePath .\artifacts\firmware-messaging-protobuf-signed.bin
```

After a normal messaging trial confirms, it remains on subsequent reset and its
SSH endpoint is gone. Returning to managed firmware then requires the bench
factory install flow. Plan that transition before testing.

For a rollback test, start from managed firmware again and build:

```powershell
.\tools\build-signed.ps1 -Messaging protobuf -RollbackTest -Version 0.1.1
```

Upload `firmware-messaging-protobuf-rollback-test-signed.bin` through the managed
updater. Verify messaging runs, RTT says confirmation was withheld, and the next
manual reset restores managed firmware. Factory-flashing a rollback-test image
does not create a trial and is not a rollback test. All of these boot tests are
pending physical evidence.

## Build measurements and repeatable checks

Local build: Rust 1.98.1, ARM hard-float target, normal release (`opt-level=z`, fat
LTO), Phase 4 working tree based on `4a431b8`, CRC off, no profiling. Signed sizes
include the 512-byte header and signature TLVs; debug information in the ELF is
not flashed. These are build measurements, not a protocol recommendation.

| Encoding | Unsigned image | Signed image | Static RAM | RAM remaining for stack |
|---|---:|---:|---:|---:|
| CSV | 116,056 B | 116,712 B | 28,348 B | 102,724 B |
| JSON | 119,568 B | 120,224 B | 28,348 B | 102,724 B |
| Protobuf | 78,056 B | 78,712 B | 28,348 B | 102,724 B |

All signed images fit the 262,144-byte effective MCUboot image limit. Static RAM
sums linked RAM sections, including task storage, DMA/network storage and RTT.
It excludes runtime stack usage. The remaining region is not a measured stack
margin. Current symbols allocate 6,912 B for the command task pool, 5,088 B for
both publisher tasks, 2,272 B for network resources, and 12,360 B for native
Ethernet packet storage; these are subsets of static RAM, not additions to it.

From the repository root, with the documented Rust toolchains installed:

```sh
python -m pip install -r Rust/protocol/requirements-phase1.txt imgtool==2.4.0
python Rust/tools/message-demo/verify.py
python Rust/tools/message-demo/verify_board.py
```

The first runs generation, portable lint/link, five Rust tests and 18 Python test
groups. The second builds/lints all three firmware formats, verifies exact codec
features, signs/verifies each with a temporary disposable key, checks the size
ceiling, builds profiling/rollback variants, rejects ambiguous format selections,
and writes `Rust/artifacts/messaging/resources.json`, ELF files and section/symbol
reports. CI uploads only resource reports, not signing keys or flashable images.
The temporary private key is deleted when verification exits.

For stack high-water evidence, make a separate instrumented image:

```powershell
.\tools\flash.ps1 -Messaging protobuf -Profiling -Version 0.1.0
```

After exercising commands, malformed input and both streams, query from the host:

```sh
python Rust/tools/message-demo/python/profile.py --host 192.168.0.10 --bind 192.168.0.20
```

`--reset` resets profiling counters before a measured interval. Record the
reported stack high-water and capacity. Executor busy percentage is not total
MCU utilization. Instrumented resource/runtime results must stay separate from
the baseline table. Actual stack high-water is still pending.

## Physical acceptance record

For **each encoding**, preserve image hash/version, source revision, board/network
settings, two listener logs, command log, RTT output and a packet capture:

- Confirm signed factory boot and all three command responses while both streams
  run on two physical receivers. Observe group, source ports, TTL 1, valid UDP
  checksum and no fragmentation. Keep UDP checksum validation enabled.
- Unplug/reconnect during streaming and while initially waiting for DHCP. Observe
  stale then recovered streams, no old-sample burst, same boot ID after reconnect,
  advancing independent sequences, and resumption of read commands. Record DHCP
  vs fallback timing and whether the assigned address changes.
- Reset with listeners active. Observe a new boot ID and rejection of delayed old
  boot packets within the listener's retained history; do not label restart as
  sequence reordering. Repeat multicast reception after IGMP membership aging.
- Inject malformed and oversized requests, then make valid commands. Record
  bounded rejection behavior and continuing streams; counters omit packets the
  underlying stack never delivers. Full CRC/fault matrix remains Phase 5.
- Exercise confirmed and deliberately unconfirmed trial upgrades as above. Record
  no-cable startup confirmation and restoration of the prior image on rollback.
- Measure stack high-water in separate profiling runs, including invalid traffic.

**Every physical item above remains pending.** The implementation is reviewable
without treating ARM compilation, signing verification or host tests as bench
success.

## Rust walkthrough

Start at `native_rmii_messaging.rs`: peripheral ownership is consumed once, the
copyable network handle is passed to independent tasks, and `StaticCell` gives
the stack storage a lifetime covering the entire program. In `embassy_messaging.rs`,
local fixed arrays become part of each async task's static future when borrowed
across an `.await`. The socket API borrows that memory rather than allocating it.

`select` cancels the losing wait when a readiness check or socket operation finishes.
`with_timeout` bounds queue draining. `Schedule` holds absolute deadlines and
sequence arithmetic. The portable `CommandPolicy` decides whether a decoded
request is accepted, ignored, rejected with an error, or rate-limited; transports
handle their own buffers, counters and I/O. Review that small module alongside
its tests before following the Embassy-specific task code.
