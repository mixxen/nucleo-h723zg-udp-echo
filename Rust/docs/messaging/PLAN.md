# Network Messaging Prototype: Phased Implementation Plan

Date: 2026-09-30  
Status: Original six-phase plan; Phases 1 and 2 are delivered; Phases 3 through 5 implementation/build work is delivered, with physical multicast and MCU acceptance gates pending. See [current checkpoint](CHECKPOINT.md).

Repository: https://github.com/mixxen/nucleo-h723zg-udp-echo  
Reviewed baseline: `b009dc08eca171083a4a52385d3583b3b3154d0b`

## Goal and scope

Demonstrate the same read-only command/response and multicast status/health behavior using CSV, JSON, and Protobuf. Compare runtime cost, reliability diagnostics, code generation, and developer effort before choosing a format.

Start with a Rust host responder and Python tools, then run the same message code on the NUCLEO-H723ZG through native Ethernet using Embassy. Keep MCU message handling free of heap allocation. W5500 integration is follow-on work; native Ethernet is an experimental starting point, not a production architecture selection.

Add a dedicated messaging application alongside the existing echo examples. Reuse the repository's Ethernet bring-up, bounded buffers, testing patterns, profiling, and signed-image tooling. Use generic device information and synthetic measurements suitable for this public repository.

The prototype excludes actuation, application-control behavior, a GUI, a database, a general RPC framework, runtime schema discovery, automatic backward-compatibility migration, and a production protocol commitment.

## Schema authoring clarification

The initial discussion proposed YAML as a format-neutral source that could generate the CSV mapping, JSON/Rust/Python definitions, Protobuf schema, and documentation. This was a proposal for the multi-format experiment, not a requirement imposed by Protobuf.

Protobuf already has a schema language: `.proto`. For a Protobuf-only solution, maintain `.proto` directly and use established generators. A YAML-to-Protobuf translator adds another schema language, mapping rules, error reporting, and tooling to maintain.

**Updated recommendation for Phase 1, pending the initial design decision:** prefer `.proto` as the maintained message structure if a small adapter can derive the required CSV and JSON mappings from its descriptors. Keep embedded capacities and other transport/application metadata in a small supplemental configuration only where necessary, without duplicating message field definitions. Freeze CSV column order explicitly rather than depending on incidental generator order.

If the neutral YAML approach is demonstrably simpler for all three formats, keep it narrowly bounded to this experiment's supported field types. Do not build a general schema compiler. Record the decision before implementation. In either approach, preserve one authoritative definition of message structure and reproducible generation.

The JSON comparison must also specify whether it uses standard ProtoJSON or an explicitly defined JSON mapping. Do not assume those representations are identical, especially for integer widths, enums, missing values, and field naming.

## Demonstration behavior

| Interaction | Behavior |
|---|---|
| `get_device_info` | Unicast request/response with device ID, firmware version, protocol version, and selected encoding. |
| `get_status` | Unicast request/response with uptime, a synthetic measurement, and its validity. |
| `get_health` | Unicast request/response with service state and diagnostic counters. |
| Status stream | Periodic multicast publication of the same status information. |
| Health stream | Periodic multicast publication of service state and counters. |
| Two listeners | Both receive publications independently while command/response continues. |

Commands initially mean these read-only requests. Measurements are explicitly synthetic; firmware counters report actual observations. Proposed starting rates are 10 Hz status and 1 Hz health, configurable for testing. These are demonstration settings, not performance requirements.

## Phase 1: Define the message contract and integration boundaries

Write definitions and independently reviewed examples before implementing codecs.

Specify:

- Message kinds, types, units, bounds, and unavailable-value behavior.
- Protocol version, device identity, and boot/session identity.
- Request correlation, including protection against confusing delayed replies from earlier client sessions.
- Separate sequence numbers for each published stream, including wrap and restart behavior.
- Sender uptime and host receive time without assuming synchronized clocks.
- One message per UDP datagram, explicit maximum size, and no sync word.
- Fixed encoding and CRC mode per run with no automatic format detection.
- Configurable unicast ports, multicast group/ports, host interface, and multicast TTL of 1.
- Common validation and logging outcomes across encodings.

Resolve the schema authoring choice described above. Generate only the types, mappings, necessary codec support, and reference documentation needed by this experiment. Application behavior remains handwritten.

Inspect the pinned Embassy APIs and socket capacity before selecting multicast details. The current native example uses `StackResources<3>`; budget additional sockets explicitly. Confirm frame sizes avoid fragmentation on the selected network path.

**Exit:** Reviewed valid, unavailable, and invalid examples for every message; an explicit authoring decision; and a resource/build plan.

## Phase 2: Host command/response and three codecs

Implement a Rust host responder and Python client using the same hardware-independent message code intended for firmware.

Develop codecs incrementally:

1. CSV: quoting, column counts, typed conversion, and missing values.
2. JSON: bounded strings, correct escaping, and the chosen mapping.
3. Protobuf: `micropb`, with explicit collection/string limits.

Use shared application validation so parser differences do not silently change the contract. Check all codecs against the ARM target early. Add readable terminal output and host JSON Lines logs from the beginning.

**Exit:**

- Python requests and decodes all three responses from Rust in every format.
- Cross-language fixtures agree on values and validity, including independent expected results rather than only self-round-trips.
- Incorrect types, versions, lengths, and required-field omissions produce predictable outcomes.
- Embedded codec builds require no allocator.
- One definition change regenerates the affected outputs reproducibly.

## Phase 3: Multicast status and health

Extend the host responder with independent periodic publishers and a Python listener. The listener joins the configured group on the selected interface. Publishing does not depend on a command arriving or a listener being present.

Implement independent publication schedules; per-device/session/stream tracking; duplicate, reordering, and provisional gap reporting; configurable stale-data indication; bounded queues; and counters for skipped publications or local send failures. Use no multicast acknowledgments or automatic retransmission. Avoid accumulating stale publications for a burst after reconnection.

Perform an early NUCLEO multicast smoke test using native Ethernet. Record firewall, interface, and switch/IGMP behavior. Host-only reception does not prove board delivery; loopback reception does not prove reception by another machine.

**Exit:** Two listeners receive both streams while commands remain responsive, and the physical multicast path is demonstrated. Host work can continue if bench access is unavailable, but mark the hardware gate pending.

## Phase 4: Complete NUCLEO application

Add a dedicated native-Ethernet messaging binary using existing board and network bring-up. Separate command handling from periodic publication so a receive wait or send operation cannot indefinitely stall the other activities. Use fixed buffers and bounded work.

Extend signing/flashing scripts to select the new application. Retain the existing MCUboot layout and 262,144-byte signed-image limit. Build one encoding per comparison image initially to make footprint measurements meaningful.

**Exit:**

- Each format serves requests and publishes both streams.
- Two listeners operate while requests continue.
- Disconnect/reconnect and reset recover predictably.
- Session changes distinguish reboot from ordinary sequence reordering.
- The demo boots through the existing signed-image flow.

## Phase 5: CRC and failure demonstrations

Add the shared CRC-32C option after basic exchanges work. Freeze parameters, coverage, byte order, representation, and independent test vectors. Cover the exact application header and payload bytes, excluding the CRC itself; verify before decoding. Keep CRC mode fixed per run and UDP checksums enabled.

| Failure | Expected evidence |
|---|---|
| Modified bytes with CRC enabled | CRC rejection counter and receiver log |
| Malformed payload with a valid CRC | Decode rejection |
| Decodable but invalid values | Validation rejection |
| Unsupported version | Version rejection |
| Oversized/truncated datagram | Rejection without partial-message processing |
| Delayed or duplicate response | Correct request correlation and outcome |
| Stream gaps/reordering | Listener observations without claiming a proven network cause |
| Publisher stops | Stale indication and subsequent recovery |

Inject corruption after computing the application CRC but before UDP transmission so a valid UDP checksum can carry the deliberately invalid application CRC to the receiver. Do not expect every mutation without a CRC to cause a parsing failure.

**Exit:** Expected failures are demonstrated without panic or unbounded memory growth; valid traffic continues afterward.

## Phase 6: Controlled comparison and walkthrough

Run the three-format by two-CRC-mode matrix on native Ethernet. Proposed baseline: ten measured minutes per configuration after warm-up, with 10 Hz status, 1 Hz health, one command per second, and two listeners.

Record payload and complete application-datagram sizes; command success rate and RTT percentiles; actual publication/receive rates; gaps, duplicates, and stale intervals; encoding/decoding cost; flash, static RAM, and stack high-water where supported; dependencies; handwritten code; and effort to change one definition.

Keep clocks, compiler settings, message values, network setup, and logging policy consistent. Record instrumented runs separately where measurement overhead matters. Existing profiling measures executor utilization, not total MCU utilization. Use host monotonic time for RTT; do not claim one-way latency from unsynchronized host and MCU clocks.

Logs identify run, source revision, schema/protocol version, encoding, CRC mode, device/session, message kind, request or sequence, timing, and outcome. Produce CSV statistical exports from the same host logs for all wire formats. Timeouts mean no valid response observed before the deadline, not proven network loss.

**Exit:** Reproducible walkthrough and comparison report with unresolved failures disclosed. A format recommendation follows the evidence; selecting a winner is not required to complete the prototype.

## Proposed repository additions

| Location | Purpose |
|---|---|
| `Rust/protocol/` | Authoritative definitions, supplemental metadata, and reviewed fixtures |
| `Rust/messaging/` | Portable crate: generated types, codecs, validation, and later CRC |
| `Rust/src/servers/` | Command service and multicast publishers |
| `Rust/src/bin/native_rmii_messaging.rs` | New firmware entry point |
| `Rust/tools/message-demo/` | Host responder, Python tools, generation, and analysis |
| `Rust/docs/messaging/` | Contract, phased walkthrough, bench procedure, and results |
| `.github/workflows/rust.yml` | Generation checks, host tests, and embedded builds |

## Execution and stopping point

Each phase is a small reviewable change with exact run instructions and a checkpoint recording what was actually verified. Explain new Rust concepts and use descriptive names. Keep host CI, ARM compilation, and physical-board evidence distinct. Preserve the existing echo/benchmark behavior and retain current signing/key-handling rules.

Completion means the six configurations have reproducible command/response and multicast demonstrations, failure evidence, logs, and a comparison report. Hardware results must remain pending until a physical test is performed. No repository, Jira, or Confluence changes were made while saving this plan.

## Reference baseline

- [Reviewed source tree](https://github.com/mixxen/nucleo-h723zg-udp-echo/tree/b009dc08eca171083a4a52385d3583b3b3154d0b)
- [Rust setup and existing verification notes](https://github.com/mixxen/nucleo-h723zg-udp-echo/blob/b009dc08eca171083a4a52385d3583b3b3154d0b/Rust/README.md)
- [Native UDP entry point](https://github.com/mixxen/nucleo-h723zg-udp-echo/blob/b009dc08eca171083a4a52385d3583b3b3154d0b/Rust/src/bin/native_rmii_udp_echo.rs)
- [Existing UDP service](https://github.com/mixxen/nucleo-h723zg-udp-echo/blob/b009dc08eca171083a4a52385d3583b3b3154d0b/Rust/src/servers/embassy_udp_echo.rs)
- [Signed-image tooling](https://github.com/mixxen/nucleo-h723zg-udp-echo/blob/b009dc08eca171083a4a52385d3583b3b3154d0b/Rust/tools/build-signed.ps1)
- [Micropb](https://github.com/YuhanLiin/micropb)
- [Protobuf language guide](https://protobuf.dev/programming-guides/proto3/)
- [Protobuf JSON mapping](https://protobuf.dev/programming-guides/json/)

Repository observations refer to the reviewed commit. Dependency APIs, schema mapping, and hardware behavior must be verified during implementation.
