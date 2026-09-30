# Phase 3: Multicast status and health

The Rust host responder now serves commands while independently publishing status
and health. Two Python listeners can receive the same streams without registering
with the service or acknowledging messages. CSV, JSON, and Protobuf use the same
behavior. The portable schedule also runs in a small NUCLEO smoke image.

Host verification is complete on Linux loopback. The board image builds and fits
the signed-image budget; physical board delivery remains pending. See the
[NUCLEO bench procedure](SMOKE_BENCH.md) for that separate gate.

## Run the complete host demo

Use the environment from [Phase 2](PHASE2.md). From the repository root:

```sh
python Rust/tools/message-demo/verify.py
mkdir -p Rust/artifacts/messaging
DEMO_HOST_TARGET="$(rustc +1.90.0 -vV | sed -n 's/^host: //p')"
```

Start the responder in terminal one:

```sh
cargo +1.90.0 run --locked --manifest-path Rust/tools/message-demo/Cargo.toml \
  --target "$DEMO_HOST_TARGET" -- serve --format protobuf \
  --bind 127.0.0.1:42000 --multicast-interface 127.0.0.1 \
  --log Rust/artifacts/messaging/phase3-server.jsonl
```

Start a listener in terminal two:

```sh
python Rust/tools/message-demo/python/listener.py --format protobuf \
  --interface 127.0.0.1 --log Rust/artifacts/messaging/listener-a.jsonl
```

Run the same listener in terminal three with `listener-b.jsonl` as its log path.
Both should show status snapshots at about 10 Hz and health at about 1 Hz. The
service publishes whether either listener is running or any commands arrive.

In terminal four, issue commands while watching both listeners:

```sh
python Rust/tools/message-demo/python/client.py --format protobuf \
  --commands info status health --repeat 5 \
  --log Rust/artifacts/messaging/phase3-client.jsonl
```

The replies are unicast and correlated to individual requests. Stream messages
have boot IDs and independent sequences, without request IDs or client sessions.
Both listeners should see matching stream sequences. Joining late establishes a
baseline; it does not count earlier unseen packets as lost.

Stop the responder with Ctrl+C. The listeners should report status stale after
0.5 seconds and health stale after 3 seconds. Restarting the responder creates a
new boot ID and new stream histories. A pause/resume within one boot instead
produces `stale` followed by `recovered` on new forward progress. Duplicate or
reordered packets cannot make a stale stream fresh.

Repeat with `--format csv` and `--format json` in every process. To demonstrate
fresh but unavailable data, add `--sample unavailable` to the responder. The status
has no temperature; arriving snapshots still keep the stream fresh.

The commands above use Bash. On PowerShell, use one line per command and the
host-target selection shown in Phase 2. Linux is the verified multicast platform;
Windows/macOS socket behavior still needs a bench run.

## Network and run settings

| Setting | Default / behavior |
|---|---|
| `--multicast-interface` (responder) | Required to enable publishers; choose a concrete local IPv4 address |
| `--interface` (listener) | Required; joins both streams through this local IPv4 address |
| `--group` | `239.255.42.1` |
| `--status-port`, `--health-port` | `42001`, `42002`; each is also its publisher's source port |
| `--status-hz`, `--health-hz` (responder) | `10`, `1`; supports 0.1..1000 Hz, rounded to whole-millisecond periods |
| Multicast TTL | Fixed to 1; local multicast loopback is enabled |
| `--status-stale`, `--health-stale` (listener) | `0.5`, `3.0` seconds without forward progress |
| `--device` (listener) | `board-01`; repeat for up to 32 expected IDs |
| `--source` (listener) | Optional producer IPv4 address filter |
| `--duration` (listener) | Optional finite run in seconds; otherwise Ctrl+C |

For two physical hosts, replace `127.0.0.1` with each machine's own LAN address.
Set the command client's `--host` to the responder and `--bind` to its local LAN
address. Set listener `--source` if you want to restrict the sender. Permit UDP
on both stream ports and IGMP group membership through the chosen NIC/firewall.

Stream ports must differ from each other and the command port. Omitting
`--multicast-interface` preserves the Phase 2 command-only demo. No CRC or format
auto-detection is added. Log paths are overwritten when reused. A finite listener
run exits successfully only if every selected device's two streams were seen at
least once; this is not a performance or freshness acceptance verdict.

## How timing and diagnostics work

`Rust/messaging/src/schedule.rs` is a small `no_std` schedule shared by the host
and the board smoke image. If a task wakes late, it advances over missed slots
and sends only the newest snapshot. Sequence numbers account for every due slot,
including skipped or failed sends. They wrap modulo 2^32 independently for each
stream. `skipped_publications` and `send_errors` saturate at uint32 maximum.
A failed nonblocking send increments both: that slot was not submitted.

The host uses a thread and nonblocking socket per publisher. Command replies
have a 50 ms local send timeout. Its counters are
shared through a mutex held only for short updates/snapshots. Command receive
waits and publisher sends cannot block the other stream. A separate logging
thread handles JSONL and human output with a 256-record queue. When it fills, diagnostics are dropped and the
cumulative `log_dropped` count appears in later records; publication continues.
Abrupt process termination can leave queued records unwritten. Use receiver logs
and account for diagnostic drops when interpreting a run.

The listener uses a bounded socket receive buffer and processes one datagram per
ready socket before checking timers again. It tracks at most 32 explicitly chosen
devices, two streams each, 64 recent sequence positions per stream and eight
retired boot IDs per device. A boot change resets both streams; delayed packets
from recently retired boots cannot switch the listener back. Boots older than
that bounded retirement history cannot be identified reliably.

| Event | Meaning |
|---|---|
| `not_seen` | No baseline for this stream/session yet |
| `baseline` | First valid packet establishes the starting sequence |
| `gap_pending` | A sequence was skipped in observed arrivals; a late packet may fill it |
| `reordered_publication` | A packet fills a provisional gap without refreshing freshness |
| `gap_final` | An observed gap left the 64-position window, the boot changed, or the run ended |
| `duplicate_publication` | Sequence already seen within the current window |
| `late_publication` | Packet is older than the retained window |
| `discontinuity` | Jump beyond the window or an ambiguous half-range comparison; no huge loss count is invented |
| `stale` / `recovered` | Forward progress stopped / resumed within the current session |
| `session_change` / `retired_session` | New boot selected / delayed packet from a recently retired boot ignored |

A forward jump beyond 64 re-establishes the sequence baseline. An exactly 2^31
jump is ambiguous and does not advance history or refresh freshness. Gaps are
receiver observations, not evidence of where or why a packet went missing.

Linux JSONL records include observed TTL and destination from receive metadata.
Other platforms log the configured group but do not claim these observations.
Payload validation, rejection samples, timing, and CRC-off labels remain as in
Phase 2. Listeners never send ACKs or request retransmission.

## Verification and code navigation

`verify.py` runs the 11 Phase 2 test groups, 7 multicast/tracking groups, two Rust
schedule tests, generation checks, Clippy, formatting and the ARM no-allocator
link probe. Process-level multicast/fault tests run on Linux and are skipped on
other systems; the deterministic history tests still run there. New checks exercise two listener processes with concurrent commands
in all formats, actual Linux multicast destination/TTL/source ports, stopping and
restarting, a SIGSTOP/SIGCONT pause, a blocked logging pipe, malformed publications,
sequence wrap, reordering, gap finalization, boot retirement, and freshness.

- `Rust/tools/message-demo/src/streams.rs`: socket setup and independent publishers.
- `Rust/tools/message-demo/src/log.rs`: bounded asynchronous diagnostics.
- `Rust/tools/message-demo/python/listener.py`: membership, receive loop and logs.
- `Rust/tools/message-demo/python/stream_state.py`: deterministic stream history.
- `Rust/src/bin/native_rmii_multicast_smoke.rs`: early Protobuf-only board publisher.

The codec's `heapless` pin moves from 0.9.1 to 0.9.3 to match the existing Embassy
revision's requirement. The portable toolchain stays at 1.90.0. Board builds use
the repository's existing stable toolchain because upstream manifests need newer
Cargo; this run used Rust 1.98.1. No Embassy or xarxa revision changed.
