# B3: opt-in NUCLEO benchmark firmware

Implementation and local verification complete, based on merged PR #10
(`51e1e80`). Physical board acceptance is pending. No board was attached, flashed
or timed for this phase. B4 stress/soak orchestration is the next software phase.

## Firmware behavior

Add `messaging-benchmark` to exactly one existing format, with CRC explicitly
selected. Default firmware builds do not enable it. Benchmark images retain
normal commands on 42000, 10 Hz status on 42001 and 1 Hz health on 42002.

| Benchmark service | Address | Behavior |
|---|---|---|
| Control/probes | Board unicast IPv4:42010 | Discover, configure, renew, stop, deterministic payload echo |
| Status | 239.255.42.1:42011 | Independently scheduled synthetic 25 C status |
| Health | 239.255.42.1:42012 | Independent schedule, run counters and send-failure state |

Multicast TTL is 1; source and destination ports match. The B1 limits apply:
0–512 application bytes, status period 1,000–10,000,000 us, health period
10,000–10,000,000 us, zero to disable either stream, and a 2–30 second lease.
Payload size excludes headers and the optional eight-byte CRC trailer. No
format/CRC autodetection or application retransmission is added.

The portable Rust service owns peer/session/boot/epoch validation, atomic
configuration, control replay checks, lease expiry and separate stream schedules.
The host responder now exercises that same service by default. Its independent
Python reference remains available with `responder --service python`.

One bounded firmware task handles at most one received datagram, each due
publication and lease checks before yielding. Socket operations are nonblocking;
there is no per-packet logging or allocation. Delayed schedules advance sequence
numbers over missed opportunities and emit only the newest sample. An older
sample still in the local stream TX queue is discarded before enqueueing the
next due sample. Idle polling requests a 100 us timer; this is not a guaranteed
latency bound under executor/network load.

Stop, expiry, link loss or an observed IPv4 address/prefix/gateway change cancels
the run. All three local socket queues are cleared, including at the final
nonwrapping epoch. Reconnection stays idle and requires discovery/configuration
for a new run. Already handed-off network frames cannot be recalled; receivers
must keep filtering boot/session/epoch. Probes do not renew the lease. Normal
services continue after benchmark termination. The lease is not authentication.

Counters reset on accepted configuration. `received_requests` counts accepted
new controls and valid probes, not discovery or duplicate controls. Rejections
are counted during an active run. Send errors describe local encode/enqueue
failures; successful enqueue is not proof of transmission or delivery. Skipped
publications include missed schedule slots, replacement of locally queued old
samples and failed local publication sends. These diagnostics cannot locate every
host/NIC/switch drop. Health reflects the latest attempted send outcome per
service; a sample cannot include its own subsequent send result.

Startup tests the selected benchmark codec with maximum payloads and requires
all benchmark sockets to bind before the existing MCUboot confirmation checkpoint.
It needs no cable or DHCP lease. Failed initialization leaves a trial unconfirmed.
The existing deliberately unconfirmed `rollback-test` variant remains available.

## Build and board trial

From `Rust/`:

```sh
cargo +stable build --locked --release --no-default-features \
  --features messaging-protobuf,messaging-crc,messaging-benchmark \
  --bin nucleo-h723zg-native-rmii-messaging
```

On the configured Windows bench workstation, from `Rust/`:

```powershell
.\tools\build-signed.ps1 -Messaging protobuf -Crc -MessagingBenchmark -Version 0.1.0
```

This creates `artifacts/firmware-messaging-protobuf-crc-benchmark-signed.bin`.
Use the provisioned signing key and the existing [installation/recovery
procedure](PHASE4.md). `flash.ps1` accepts the same `-MessagingBenchmark` switch;
its factory path mass-erases internal flash. No flashing is performed by the
verification script. The older `-Benchmark` switch is for raw UDP echo only.
Use `csv`/`json` and omit `-Crc` to build the other five combinations.

Build the host library as in [B2](BENCHMARK_B2_LIVE.md). From the repository root,
with example board/NIC addresses replaced by actual addresses:

```sh
IMAGE_SHA=$(sha256sum Rust/artifacts/firmware-messaging-protobuf-crc-benchmark-signed.bin | cut -d ' ' -f 1)
python Rust/tools/message-demo/python/benchmark_live.py run \
  --format protobuf --crc on --interface 192.168.0.20 \
  --target 192.168.0.10 --target-kind board --image-sha256 "$IMAGE_SHA" \
  --payload-bytes 512 --command-hz 100 --outstanding 64 \
  --status-period-us 10000 --health-period-us 100000 \
  --lease-ms 10000 --deadline-ms 1000 --warmup 1 --duration 30 \
  --report-interval 1 --output benchmark-results/b3-protobuf-on
python Rust/tools/message-demo/python/benchmark_live.py inspect \
  benchmark-results/b3-protobuf-on/events.jsonl
```

The output path must be new. A supplied image hash identifies the intended
artifact; it does not attest the flashed firmware. Retain boot/confirmation logs,
compiler/build command, image, host library, network settings and reports.

Physical acceptance, still pending:

1. Repeat a low-load command-only, stream-only and combined run for each of the
   six format/CRC images, at payload 0 and 512. Check patterns, correlation and
   both streams; keep normal commands and two normal multicast listeners active.
2. Confirm normal streams persist after stop. Interrupt normally and verify stop
   cleanup. Then kill the host abruptly, observe expiry, and start a fresh run.
   Confirm delayed old controls/probes cannot restart the previous run.
3. Disconnect/reconnect Ethernet and exercise a changed IPv4 configuration.
   Confirm the run ends, reconnection is idle and a fresh configuration succeeds.
   Check responsiveness during increasing load; B4 will automate sweeps/recovery.
4. Exercise signed trial confirmation and deliberate rollback using the existing
   managed-update/recovery procedure. A successful build is not boot evidence.
5. Build separately with `-Profiling`, give that image's hash and `--instrumented`
   to the host run, and query `python Rust/tools/message-demo/python/profile.py
   --host 192.168.0.10 --bind 192.168.0.20` before/after load. Record stack
   high-water and executor busy time separately from baseline measurements.
   Profiling is optional UDP 5001 traffic; busy time is not total MCU utilization.

## Local verification and measured resources

`verify.py` passes with Rust 1.90.0: generation/schema checks, host and ARM lint,
14 Rust unit tests, the existing 32 Python groups, 16 metric tests and 10 live
codec/service/socket test methods. The socket test uses the actual Rust service
for all six modes, injected faults and interrupted cleanup. The ARM link probe
now reaches service handling/publication code as well as codecs without an
allocator. The owner/configuration record is compile-time bounded to 256 bytes,
and the ARM envelope to 1,536 bytes.

`verify_board.py --benchmark` builds, lints, signs with a disposable key and verifies
six baseline plus six profiling images. It also builds the rollback variant and
rejects missing/multiple codec selections. CI retains resource/section/symbol
reports, not keys or flashable test images. `verify_board.py` without the flag
continues checking the ordinary six-image matrix.

Measured with rustc 1.99.0 (`b940084d7`, 2026-09-28), release,
`thumbv7em-none-eabihf`, on the B3 working tree based on `51e1e80`. The checked-in
[raw report](results/benchmark-b3-resources.json) deliberately records the dirty
base revision; it is build evidence, not an on-board result. Re-run the verifier
at the reviewed commit for CI provenance.

| Encoding | CRC | Signed baseline bytes | Signed profiling bytes |
|---|---|---:|---:|
| CSV | Off | 132,056 | 134,008 |
| CSV | On | 133,384 | 135,328 |
| JSON | Off | 140,824 | 142,776 |
| JSON | On | 142,136 | 144,072 |
| Protobuf | Off | 95,344 | 97,240 |
| Protobuf | On | 96,776 | 98,664 |

All are below the 262,144-byte signed-image gate. Static RAM is 38,144 bytes for
each baseline and 38,456 with profiling, below the 65,536-byte gate. Additional
fixed benchmark packet storage is 8,192 bytes: command RX/TX queues, input,
one shared encoding buffer and two stream TX queues. Eight network socket slots
cover DHCP, normal services, benchmark services and optional profiling.
Linker space remaining is not measured stack margin. No physical latency,
drop rate, sustainable frequency, stress, soak or stack result is claimed.
