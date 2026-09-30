# Phase 3 checkpoint

Date: 2026-09-30. Base: merged Phase 2, `84a05a1` (PR #2).

**Host implementation and build checks complete. Physical multicast gate pending.**

## Delivered

- Independent Rust host status/health publishers with explicit IPv4 interface,
  configurable group/ports/rates, TTL 1, source-port binding and nonblocking sends.
- A portable `no_std` publication schedule shared with the board smoke image:
  missed slots advance sequences/counters without replaying stale snapshots.
- Python multicast listener with two stream sockets, bounded sequence/reorder
  windows, provisional/final gaps, boot retirement, stale/recovered diagnostics,
  explicit device/source filters and JSONL logs. No ACKs/retransmissions.
- Bounded host logging queue; a slow consumer cannot stall publication.
- Protobuf-only native-RMII NUCLEO smoke image with hardware boot nonce, DHCP/
  fallback supervision, two TX-only sockets, and local failure/skip counters.
- `-MulticastSmoke` selectors in the existing signing/factory-flash scripts, and
  CI checks for the host tests, ARM image, PowerShell syntax and image packaging.
- [Host walkthrough](PHASE3.md) and [physical bench procedure](SMOKE_BENCH.md).

## Verification performed locally

`python Rust/tools/message-demo/verify.py` passed with portable Rust 1.90.0:

- All 11 Phase 2 cross-language/command/rejection groups passed again.
- Seven new groups passed: two listener processes with commands in all three
  formats; observed multicast destination, source ports and TTL 1 on Linux;
  stop/restart; SIGSTOP/SIGCONT stale/recovery and skipped slots; blocked stdout
  logging while commands/publications continue; malformed traffic and no ACKs;
  deterministic reorder, wrap, discontinuity, gap and boot-history rules.
- Two Rust schedule tests passed; generation/format/lint gates and ARM linking
  without an allocator passed.

With the existing firmware stable toolchain, Rust 1.98.1:

- New smoke image release build and Clippy passed against the unchanged Embassy
  and xarxa commits. Zero-length RX queues and `try_send_to` compile and link.
- Existing native-RMII UDP echo release build passed with the updated lockfile.
- Smoke image signed with a disposable test key and verified with imgtool 2.4.0:
  65,928 unsigned bytes, 66,584 signed bytes, below the 262,144-byte limit.
- Root Rust formatting and `git diff --check` passed.

The codec's exact `heapless` pin was updated to 0.9.3 to satisfy the existing
Embassy dependency. No upstream revision changed. GitHub CI and PowerShell
execution results are separate from these local Linux checks.

## Remaining gates and limits

No board was attached or flashed. Physical Ethernet/IGMP/TTL/checksum behavior,
hardware RNG, DHCP/link recovery, signed boot and stack high-water are unverified.
The [bench checklist](SMOKE_BENCH.md) remains pending until actual evidence is
recorded. Host tests use Linux loopback; other desktop OS multicast behavior and
two physical receiver machines still need testing.

The smoke image publishes only Protobuf. The complete MCU command/response
service and per-encoding firmware variants remain Phase 4. CRC stays off until
Phase 5. Do not present the host test rates or image size as a controlled
three-format performance comparison.

## Next: Phase 4, with the Phase 3 bench gate retained

Integrate the complete read-command service and both publishers into a dedicated
NUCLEO application for each encoding, keep independent bounded work, finish
signing/boot confirmation and recovery integration, and measure actual resource
use. Run and record the physical Phase 3 multicast smoke test when a bench is
available; its gate is not waived by continuing implementation.
