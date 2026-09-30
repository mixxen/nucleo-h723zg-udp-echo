# Phase 1 integration and resource plan

## Inspected baseline

Repository commit: b009dc08eca171083a4a52385d3583b3b3154d0b.
No AGENTS.md was present in its recursive tree or local workspace ancestry.

- `Rust/src/bin/native_rmii_udp_echo.rs`: native RMII, StackResources<3>, DHCP,
  board/network supervisor and echo task; optional profiling task.
- `Rust/src/servers/embassy_udp_echo.rs`: four RX and four TX packet slots,
  1536-byte maximum payload, 6144 bytes per socket direction and a work buffer.
- `Rust/src/lib.rs`: no_std host-testable code; ports 7 and 5001 already used.
- `Rust/Cargo.toml`: Embassy git pin below, heapless, ARM-only dependencies;
  default managed application includes SSH/update support.
- `Rust/build.rs`: SSH key generation requires firmware feature only.
- `Rust/tools/build-signed.ps1`: explicit binary selection and effective 256 KiB
  signed-image ceiling. `flash.ps1` also needs a new variant selector later.
- `.github/workflows/rust.yml`: current host tests, embedded builds, benchmark
  checks and managed-image signing gate must remain intact.

## Pinned networking inspection

Reviewed upstream at `0af1937a5ac77f52fea943b677088c38ef9e91c6`:

- [UDP API](https://github.com/embassy-rs/embassy/blob/0af1937a5ac77f52fea943b677088c38ef9e91c6/embassy-net/src/udp.rs):
  caller-owned RX/TX storage, `set_hop_limit(Some(1))`, receive truncation error,
  and send/try-send interfaces. Treat send errors as local outcomes, not peer ACKs.
- [Stack API](https://github.com/embassy-rs/embassy/blob/0af1937a5ac77f52fea943b677088c38ef9e91c6/embassy-net/src/lib.rs):
  `join_multicast_group`, `leave_multicast_group`, `has_multicast_group`, gated by
  the `multicast` feature.
- [Features](https://github.com/embassy-rs/embassy/blob/0af1937a5ac77f52fea943b677088c38ef9e91c6/embassy-net/Cargo.toml):
  `multicast` maps to `xarxa/multicast`; repo Cargo.toml does not currently enable
  it. Cargo.lock pins xarxa `1f332ac32cc33d86aefc8e1c1a9749b93234a6de`.

The board publishes only; group membership is needed at receivers, not merely to
send. Host listener joins with its explicit NIC; enable embedded multicast support
for the demo integration and verify actual egress before inferring feature needs
from join APIs alone. No upstream pin change is proposed. Hardware multicast,
IGMP snooping/querier behavior, TTL and checksum behavior remain untested here.

## Proposed fixed budget (not measured use)

New native messaging binary has command, status and health sockets. Give each
stream its own TX socket/task so a stalled stream cannot block the other.

| Item | Slots/capacity | Payload storage |
|---|---|---:|
| Command RX | 2 x 1024 | 2048 B |
| Command TX | 2 x 1024 | 2048 B |
| Command receive work | 1024 | 1024 B |
| Command encode work | 1024 | 1024 B |
| Status TX | 1 x 1024 | 1024 B |
| Status encode work | 1024 | 1024 B |
| Health TX | 1 x 1024 | 1024 B |
| Health encode work | 1024 | 1024 B |
| **Payload subtotal** | | **10240 B** |

Publication sockets should use empty RX queues if supported by the pinned stack;
otherwise budget a bounded fallback explicitly. This subtotal excludes packet
metadata, socket storage, Ethernet DMA buffers, task futures, decoded structs,
stack, DHCP, RNG state and optional profiling. Measure those in Phase 4.

Plan StackResources<5>: one DHCP socket, three application sockets, one optional
profiling socket. Echo and SSH are absent from this new demo binary, not removed
from their existing binaries. Recount if features change. Existing managed and
benchmark applications keep their resource settings.

Use one encoding feature per image. Place portable dependencies where host tests
can compile them; avoid moving board dependencies into host builds. Check fixed
string storage (32/16-byte limits) and no allocator at link time. No maps/lists
in the schema. Main finite counters and identities fit bounded structs.

The 1024-byte datagram limit is below ordinary 1500-byte Ethernet MTU but actual
path validation is still required. Keep maximum encoding-size checks in Phase 2.
An oversized message fails encoding explicitly; never truncate a string to fit.

## Build and verification sequence

Phase 1: compile schema with protoc, inspect generated descriptors, verify
supplemental field references and examples. No dependency or firmware edits.

Phase 2: pin generator/runtime versions; generate Rust via micropb and Python via
protoc; implement narrow descriptor-derived CSV/JSON adapters. Check regeneration
drift in CI. Add host-target checks explicitly because `.cargo/config.toml`
defaults to ARM. Compile each codec for thumbv7em-none-eabihf without an allocator.
Validate quoted/backslashed strings before choosing the embedded JSON library.

Phase 3: two listeners on distinct physical hosts where possible; inspect real
multicast packets, TTL=1, explicit NIC selection and group joins. Verify command
responses remain unicast. Do not treat local loopback as physical evidence.

Phase 4: add new binary/feature and signing/flash selection; preserve bootloader
layout and new-image confirmation behavior. Review whether shared initialization
already confirms the image or an explicit confirmation step is needed. Retain
signature verification and the 262144-byte signed limit for each encoding. Record
actual buffer/stack/flash use and build flags.

Phase 5: CRC and controlled failure cases; Phase 6: six comparison configurations.
Do not run the throughput benchmark and treat its echo results as schema evidence.

## Remaining implementation checks

- Micropb generator version/accessor layout and bounded storage configuration.
- Narrow CSV/JSON adapter; no production code generation has been implemented.
- Board hardware RNG integration for boot IDs; source API and boot-time error path.
- Send cancellation/try-send semantics, timer fairness and queue accounting.
- Packet reception after switch membership aging, NIC changes, and link recovery.
- UDP checksum offload verification at a meaningful observation point (host captures
  before checksum offload may appear invalid).

These are explicit Phase 2–4 gates, not claims of passing hardware tests.
