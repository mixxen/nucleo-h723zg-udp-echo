# UDP benchmark refactor and migration plan

- Status: Proposed
- Target repository: `../software`
- Proposed target: `prototypes/udp-transport-benchmark/`
- Jira: TBD before migration
- Owner: TBD before migration
- Source revision: `b009dc08eca171083a4a52385d3583b3b3154d0b`
- Source repository: `https://github.com/mixxen/nucleo-h723zg-udp-echo`

This plan is intentionally stored with the source prototype. The official MMHEL
Common Software repository requires an owning Jira issue, owner, and reviewed
migration decision before prototype content is added. Moving this file into the
target prototype should be the first change after those fields are assigned.

## Goal

Create a small, target-neutral UDP benchmarking prototype that can be reused by
other MMHEL experiments on Windows and Linux. Preserve the verified measurement
behavior and final engineering evidence. Include minimal, buildable NUCLEO
UDP-echo fixtures for the four architectures under study without carrying over
the bootloader-management, SSH, credentials, or unrelated application code.

The prototype should answer one bounded question:

> Under controlled and documented configurations, how do C/LwIP native RMII,
> Rust/Embassy native RMII, W5500 hardware offload, and W5500 MACRAW compare in
> UDP latency, throughput, reliability, resource use, and maintained complexity,
> using a host method reusable by other prototypes?

This experiment does not approve Rust, UDP, Embassy, LwIP, W5500, a target
runtime, or a production network architecture. Rust is only the implementation
language of the proposed host tool and three experimental fixtures pending the
applicable MMHEL decision.

## Repository review findings

The official repository is a clean foundation. Its current rules materially
affect this migration:

- `prototypes/` is the correct initial home for a bounded experiment, not a
  declaration of production architecture.
- Every prototype needs a Jira issue, owner, question, assumptions, evidence,
  success criteria, and removal or promotion condition.
- EHEL or Prototype 0 material cannot be copied without an explicit provenance,
  ownership, licensing, and verification decision.
- Rust and the repository toolchain are not yet approved. Adding the crate and
  CI job must therefore be part of the owning issue or an accepted decision.
- Machine-specific paths, addresses, credentials, private keys, downloaded
  tools, and controlled test data must not be committed.

The current benchmark is useful but not yet lean:

- the host crate has approximately 2,800 source lines across `main.rs`,
  `runner.rs`, `model.rs`, `profile.rs`, `protocol.rs`, and `stats.rs`;
- CLI parsing, execution, result construction, comparison, and Markdown
  generation are concentrated in a 1,240-line `main.rs`;
- PowerShell runners combine host measurement with firmware compilation,
  signing, flashing, DHCP readiness, target profiling, and STM32 memory tools;
- CPU and stack telemetry use a project-specific UDP port and firmware wire
  protocol;
- four reports overlap and mix historical controls with later optimized images;
  and
- examples contain transient DHCP addresses and Windows-specific paths.

## Scope

### Include initially

- A host-only UDP benchmark executable.
- Self-validating packets with run ID, sequence, timestamp, and deterministic
  payload.
- Stop-and-wait latency measurement.
- Fixed-rate stream and rate-sweep measurement.
- Sustained-throughput measurement.
- Long-duration reliability soak.
- Missing, late, duplicate, reordered, corrupt, foreign, and host send-error
  accounting.
- JSON and CSV evidence suitable for later report generation.
- A local loopback echo fixture for automated tests.
- Four minimal NUCLEO-H723ZG UDP echo test fixtures:
  - C with LwIP over the native RMII interface;
  - Rust with Embassy over the native RMII interface;
  - Rust using W5500 hardware UDP offload; and
  - Rust using W5500 MACRAW with the IP/UDP stack on the STM32.
- Shared Rust board and UDP-server code where the architecture permits it,
  without hiding variant-specific bring-up or resource costs.
- Reproducible target build and direct-debug-probe flash commands separated
  from the host measurement library.
- One concise report describing the NUCLEO experiment, final configurations,
  results, limitations, and provenance.
- Windows PowerShell and Linux shell examples that invoke the same executable.

### Defer until justified

- Burst mode, unless a requirement or retained report depends on it.
- A generated Markdown comparison command. Prefer stable machine-readable output
  plus a small report template until multiple prototypes demonstrate a common
  comparison need.
- Target CPU and stack telemetry. Define it later as an optional adapter rather
  than embedding the current port-5001 protocol in the core benchmark.
- Automatic endpoint discovery. Require an explicit endpoint initially; add
  discovery only after its network and identity semantics are defined.
- Repository-wide packaging or promotion into `tools/`.

### Exclude from this migration

- Managed firmware update, MCUboot integration, SSH, and provisioning features.
- Network firmware unrelated to the four minimal UDP echo fixtures.
- Private-key signing and Ethernet self-update workflows.
- Private signing keys or generated firmware artifacts.
- The complete STM32CubeH7 tree. The C fixture should use a reviewed, pinned
  dependency or overlay strategy instead of vendoring the full SDK.
- Hard-coded DHCP addresses, developer directories, router configuration, and
  VS Code setup.
- Raw historical result directories unless data ownership and classification
  are explicitly approved.

The four firmware images are benchmark fixtures, not proposed product firmware.
Each must expose only the behavior required for the comparison and must retain
visible architecture boundaries so bring-up NCLOC, UDP-server NCLOC, image size,
RAM, clocks, transport, cache policy, and packet servicing remain measurable.

## Proposed target structure

```text
prototypes/udp-transport-benchmark/
|-- README.md
|-- host/
|   |-- Cargo.toml
|   |-- Cargo.lock
|   |-- rust-toolchain.toml
|   |-- src/
|   |   |-- main.rs          # thin CLI entry point
|   |   |-- lib.rs           # reusable benchmark API
|   |   |-- cli.rs           # arguments and validation only
|   |   |-- protocol.rs      # self-validating datagram format
|   |   |-- measurement.rs   # latency, stream, sweep, throughput, soak
|   |   |-- stats.rs         # counters and percentile summaries
|   |   `-- output.rs        # versioned JSON/CSV serialization
|   `-- tests/
|       |-- loopback.rs      # local UDP echo integration test
|       `-- output_schema.rs # stable evidence-format checks
|-- target/nucleo-h723zg/
|   |-- README.md            # hardware, wiring, clocks, and variant matrix
|   |-- c-lwip-rmii/         # minimal C integration and pinned SDK contract
|   `-- rust/
|       |-- Cargo.toml       # one workspace/crate with three explicit binaries
|       |-- src/bin/
|       |   |-- native_rmii.rs
|       |   |-- w5500_offload.rs
|       |   `-- w5500_macraw.rs
|       |-- src/bringup/     # transport-specific setup
|       `-- src/server/      # shared echo logic where genuinely common
|-- automation/
|   |-- build.ps1            # optional Windows orchestration
|   |-- build.sh             # Linux equivalent
|   |-- flash.ps1            # debug-probe flash only
|   |-- flash.sh
|   |-- run-matrix.ps1       # invokes host tool; no measurement logic
|   `-- run-matrix.sh
`-- reports/
    |-- README.md        # evidence and report-retention rules
    `-- nucleo-h723zg-udp-echo.md
```

Do not create empty directories. Add `scripts/` only if the executable alone is
insufficient, and add each report or test with real content in the same change.

## Refactoring design

### 1. Separate library behavior from the CLI

Move packet construction, socket execution, counters, statistics, and output
types into `lib.rs` modules. Keep `main.rs` responsible only for parsing,
dispatch, concise progress messages, and process exit status. Tests should call
the library directly rather than shelling out unless they are explicitly CLI
tests.

### 2. Reduce the command surface

Start with these commands:

```text
latency       stop-and-wait RTT distribution
throughput    offered-load/goodput search
stream        fixed-size, fixed-rate reliability run
sweep         a list or range of fixed stream rates
soak          long-duration fixed or mixed-size reliability run
```

Remove `suite` if it only sequences commands that a script can express. Defer
`compare` and `burst` as described above. Use shared argument structures for
endpoint, payload, duration, timeout, output, and run identity.

### 3. Make the evidence contract explicit

Every JSON result should include:

- schema version and benchmark-tool revision;
- UTC start time and duration;
- host operating system and architecture;
- explicit target label and endpoint;
- user-supplied target repository revision and configuration label;
- requested and achieved offered load;
- packet-format version and payload size;
- all reliability counters and latency statistics; and
- the exact command-line arguments.

Do not collect usernames, absolute paths, interface identifiers, or addresses
beyond the explicitly requested test endpoint. CSV is a derived convenience
format; JSON is the authoritative evidence record.

### 4. Keep host measurement target-neutral

The host core must know only that it is testing a UDP echo endpoint. Firmware
build and flash orchestration stays outside the host crate and passes an
explicit endpoint, variant label, firmware revision, and configuration label to
it. DHCP discovery, cache/register inspection, CPU telemetry, and memory
measurement remain optional target/testbed adapters. This keeps the measurement
library reusable for STM32, Linux processes, simulators, and future prototypes
while keeping this experiment reproducible from one prototype directory.

### 5. Keep all four fixtures comparable

Define a checked-in variant matrix containing the parameters that materially
change results: MCU/AHB clocks, compiler optimization, RMII or SPI clock,
network-stack location, checksum policy, cache policy, buffer/descriptor counts,
interrupt or polling behavior, and maintenance cadence. A comparison run must
record that matrix alongside its results.

Use the same echo contract and host workload for every fixture. Architecture-
specific setup remains separate and counted; common UDP echo behavior may be
shared only between the Rust native-RMII and MACRAW fixtures where both use the
same Embassy socket API. The W5500 offload server remains distinct because it
uses hardware sockets, and the C callback remains distinct because it uses
LwIP's raw API.

### 6. Consolidate documentation

Create one authoritative NUCLEO report by extracting, not blindly copying, the
current reports:

- measurement protocol and error definitions;
- configuration matrix for C/LwIP, native Rust, W5500 MACRAW, and W5500
  offload;
- retained optimized results, including the 1-30 kHz error table;
- matched 30-second 1 kHz results;
- SLOC/NCLOC and memory boundaries;
- Windows-host limitations and the planned Linux rerun; and
- source/result provenance.

Do not migrate `BENCHMARK_REPORT.md`, `PROFILED_STREAM_BENCHMARK_REPORT.md`,
`STREAM_BENCHMARK_REPORT.md`, and `TRADE_STUDY.md` as four parallel reports.
Preserve unique historical facts in a clearly labeled appendix or provenance
table, then treat the consolidated report as authoritative. Jira or an approved
document-control location should retain review history that does not belong in
the report.

## Source mapping

| Current source | Proposed treatment |
| --- | --- |
| `Rust/tools/udp-benchmark/src/protocol.rs` | Migrate with tests after provenance review |
| `Rust/tools/udp-benchmark/src/stats.rs` | Migrate with tests; keep independent of sockets |
| `Rust/tools/udp-benchmark/src/runner.rs` | Split by measurement behavior; delete duplication |
| `Rust/tools/udp-benchmark/src/model.rs` | Merge result types into `output.rs` or owning modules |
| `Rust/tools/udp-benchmark/src/main.rs` | Replace with thin `main.rs` plus `cli.rs` |
| `Rust/tools/udp-benchmark/src/profile.rs` | Do not migrate initially; candidate optional adapter |
| `run-*-comparison.ps1` | Do not copy; replace only with target-neutral wrappers if needed |
| `Src/`, `Inc/`, and C build integration | Extract only the C/LwIP native-RMII echo fixture; reference a pinned STM32CubeH7 dependency |
| Rust native-RMII binary and bring-up | Extract as one explicit target fixture |
| Rust W5500 MACRAW binary and bring-up | Extract as one explicit target fixture; share only genuine Embassy server code |
| Rust W5500 offload binary and bring-up | Extract as one explicit target fixture with its hardware-socket server |
| Rust SSH, update, managed application, and profiling modules | Exclude from the initial benchmark fixtures |
| build/flash scripts | Rewrite as small target orchestration; exclude signing, provisioning, and Ethernet update |
| four benchmark/trade-study reports | Consolidate into one reviewed report |
| ignored raw benchmark results | Retain externally until classification and retention are approved |

## Migration sequence

### Phase 0: authorization and provenance

- [ ] Assign the Jira issue and prototype owner.
- [ ] Confirm that UDP transport benchmarking is shared MMHEL work.
- [ ] Record the source repository URL and immutable source revision.
- [ ] Determine ownership and license for the host benchmark. The source
      repository currently contains ST and LwIP licenses but no obvious
      repository-wide license covering the newly authored Rust host tool.
- [ ] Review the C fixture's ST and LwIP provenance and determine whether the
      official repository may contain the required integration sources or must
      consume them as a pinned external dependency.
- [ ] Review licenses for the Rust firmware and every pinned Cargo dependency.
- [ ] Review report data for export, proprietary, privacy, and testbed-retention
      constraints.
- [ ] Confirm that using Rust for this bounded host prototype is authorized
      without implying a target-language decision.

No source code or result data should cross the repository boundary before these
items are resolved.

### Phase 1: behavior baseline

- [ ] Freeze the existing host tool at the source revision listed above.
- [ ] Freeze all four working firmware configurations, including compiler,
      clocks, SPI/RMII settings, cache policy, queues, and direct-flash method.
- [ ] Record command help, JSON examples, and the 16 passing unit tests.
- [ ] Create a behavior matrix for each command and decide whether `burst`,
      `suite`, and `compare` are retained or removed.
- [ ] Add golden tests for packet encoding, error classification, reliability
      gates, and output schema before structural changes.

### Phase 2: lean extraction

- [ ] Create the prototype charter in its `README.md` with issue, owner,
      question, assumptions, evidence, success criteria, and exit condition.
- [ ] Create one host crate with pinned dependencies and toolchain.
- [ ] Extract the protocol, counters, statistics, and socket runner into a
      library.
- [ ] Replace duplicated per-command setup and serialization with shared paths.
- [ ] Extract the four minimal firmware fixtures and delete SSH, firmware
      update, signing, provisioning, and profiling from their dependency graph.
- [ ] Keep target build/flash orchestration outside the host crate and remove
      developer-machine assumptions from both layers.
- [ ] Preserve separate bring-up and UDP-server source boundaries so NCLOC
      remains comparable.
- [ ] Make output creation fail clearly rather than silently overwriting a run.

### Phase 3: cross-platform verification

- [ ] Run formatting, Clippy with warnings denied, unit tests, and release build
      on Linux.
- [ ] Run the same checks on Windows if GitLab runners are available; otherwise
      record the exact manual Windows evidence.
- [ ] Run a local loopback integration test on both operating systems.
- [ ] Verify identical protocol bytes and compatible JSON schemas across hosts.
- [ ] Build all four firmware fixtures from a clean clone.
- [ ] Flash each fixture through the documented debug-probe path and verify
      binary echo at small, 100-byte, and maximum supported payload sizes.
- [ ] Run the same short stream and sweep matrix against all four endpoints.

### Phase 4: evidence consolidation

- [ ] Produce the single NUCLEO report from reviewed source results.
- [ ] Mark configuration mismatches and 3-second versus 30-second samples.
- [ ] Preserve the distinction between host pacing failure and packet errors.
- [ ] Link every retained result table to a run manifest or approved artifact.
- [ ] Remove transient IP addresses from commands; use placeholders or explicit
      run metadata.

### Phase 5: promotion or removal

- [ ] Demonstrate reuse by at least one additional prototype or test fixture.
- [ ] If reuse and maintenance ownership are established, propose promotion to
      `tools/udp-benchmark/` with the applicable decision record.
- [ ] If the experiment does not produce reusable evidence, retain the approved
      report and remove the prototype code.

Only after the target prototype is verified should the original benchmark be
deprecated or removed from its current repository.

## Verification gates

The migration is complete only when all applicable commands succeed from a
clean clone:

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
```

The owning implementation change must add equally explicit clean-clone build
commands for the C fixture and the three embedded Rust images. Do not invent
those commands in this plan before the STM32Cube dependency and direct-flash
layout are selected.

Additional observable checks:

- the local echo test detects injected missing, late, duplicate, reordered,
  corrupt, foreign, and send-error conditions where injection is practical;
- a fixed run ID produces deterministic payload bytes;
- malformed and too-short packets are rejected;
- a 1 kHz loopback run sends the planned packet count without host pacing
  failure on the supported host;
- JSON schema tests prevent accidental evidence-format drift; and
- CI retains no generated executable, raw result, credential, or machine-local
  path in the Git repository.

## Lean acceptance criteria

- One host crate and executable, one embedded-Rust fixture crate/workspace with
  three explicit binaries, one minimal C fixture, and one authoritative report.
- Exactly four buildable target images: C/LwIP RMII, Rust/Embassy RMII, W5500
  offload, and W5500 MACRAW.
- Target build and direct-debug-probe flash automation is separate from the
  reusable host library.
- No hard-coded board identity, IP address, developer path, or Windows-only
  behavior in the core.
- `main.rs` is a thin adapter; measurement behavior is unit-testable through the
  library.
- Every retained command has a requirement or report consumer.
- No duplicate result model or output path.
- First-party source size is measured before and after; any growth must be tied
  to a verified capability rather than abstraction alone.
- All imported code and evidence has approved provenance and licensing.

## Risks and controls

| Risk | Control |
| --- | --- |
| Migration silently changes benchmark semantics | Golden protocol, counter, pacing, and schema tests before refactoring |
| A prototype is mistaken for production architecture | Explicit charter, proposed status, and promotion/removal criteria |
| Board automation contaminates a reusable host tool | Keep minimal build/flash orchestration outside the host crate with explicit target interfaces |
| Shared code hides real architectural complexity | Count common code in every consuming variant and keep transport-specific bring-up visible |
| Historical results cannot be reproduced | Record immutable source revision, configuration, command, host, and artifact provenance |
| Windows timing behavior is generalized to Linux | Report hosts separately and rerun on the formal Linux testbed |
| SLOC reduction hides complexity in scripts | Count the crate, wrappers, and report-generation code together |
| Unlicensed or controlled material enters Common Software | Complete Phase 0 before copying code or evidence |

## Decisions required before implementation

1. What Jira issue owns the prototype, and who is its owner?
2. Are the newly authored host benchmark and Rust firmware approved for
   migration, and under what license?
3. Are raw benchmark results approved for the official repository, GitLab job
   artifacts only, or an external evidence store?
4. Are `burst` and generated comparison reports required by a current Phase 1
   question, or should they be removed from the lean first version?
5. Is Linux the normative benchmark host, with Windows supported for developer
   convenience, or are both normative?
6. Will the C fixture vendor reviewed integration sources, use a pinned
   STM32CubeH7 submodule/package, or require an externally installed SDK?
7. Should the benchmark fixtures direct-flash standalone images, or is MCUboot
   part of the controlled test configuration? The lean default is standalone
   direct flash unless an owning requirement says otherwise.
