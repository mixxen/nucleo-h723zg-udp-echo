# Phase 1 checkpoint

Date: 2026-09-30. Base repository revision: b009dc08eca171083a4a52385d3583b3b3154d0b.

## Delivered

- Authoritative proto3 schema for three read requests, their responses, an error
  response, and status/health publications.
- Decision to use `.proto` plus limited companion metadata, not a YAML compiler.
- Explicit custom JSON and CSV mappings, bounds, presence, correlation, stream
  identity/freshness, CRC format and diagnostic outcomes.
- 18 positive semantic fixtures, 20 semantic rejection expectations, 5 wire
  rejection expectations, and an authored three-format request wire example.
- Pinned networking API inspection, socket/buffer budget and build/signing plan.
- Saved six-phase plan and navigation from Rust/README.md.

## Verification performed

`python Rust/protocol/tools/check_phase1.py` passed with grpcio-tools 1.76.0 and
protobuf 6.33.6. Schema compiled; profile references resolved; all 18 positive
fixtures survived generated Protobuf encode/decode; authored CSV/JSON/Protobuf
request examples agreed. `git diff --check` passed.

The check is a schema/fixture check, not a production codec. Negative fixtures
are authored expected outcomes for Phase 2/5, not passing rejection tests.
Examples were inspected during implementation; independent human review remains
available through the pull request. No independent reviewer or board test is
claimed.

## Not performed / remaining gates

No Rust toolchain is installed in this execution workspace, so no ARM build or
micropb generation was run. No NUCLEO is connected here. Multicast, DHCP recovery,
boot nonce generation, CRC corruption rejection, and signed-demo boot remain
unverified. Existing firmware code, Cargo dependencies and network behavior are
unchanged. Phase 1 can be reviewed without flashing anything.

## Next: Phase 2

Implement the narrow descriptor adapter and no-heap codec configurations, then
Rust host responder and Python client. Add true validation/negative tests and
cross-language fixtures. Resolve the JSON escaping library issue before claiming
full support. Check each codec on ARM early. Do not advance to claims of physical
multicast success until Phase 3's bench gate is actually exercised.
