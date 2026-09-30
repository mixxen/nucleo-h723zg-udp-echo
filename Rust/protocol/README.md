# Messaging prototype definitions

Start with [the contract](../docs/messaging/CONTRACT.md). This directory contains
the maintained definitions for the [Phase 2 host demo](../docs/messaging/PHASE2.md).
The board service is a later phase.

- `messaging.proto`: authoritative message names, field types, tags and presence.
- `profile.json`: CSV column ordering, capacity limits and demo settings.
- `fixtures/cases.json`: 18 positive semantic examples, 20 semantic rejection
  cases and 5 wire rejection expectations. Phase 2 exercises the codecs; the CRC
  rejection expectation remains deferred to Phase 5. Requests/identity/counters
  have no invented unavailable measurement; the fixture file explains these N/A cases.
- `fixtures/golden_request.json`: manually specified equivalent request bodies in
  CSV, JSON and Protobuf. This checks an independent byte expectation against
  protoc rather than only testing an encoder against its own decoder.
- `tools/check_phase1.py`: schema/profile/positive-fixture consistency check.

## Run the Phase 1 check

From the repository root, create an isolated host Python environment:

```sh
python -m venv .venv-messaging
# Linux/macOS:
.venv-messaging/bin/python -m pip install -r Rust/protocol/requirements-phase1.txt
.venv-messaging/bin/python Rust/protocol/tools/check_phase1.py
```

On Windows, use `.venv-messaging\Scripts\python.exe` instead of the POSIX path.
Keep this environment outside version control. The check invokes the protoc
bundled in grpcio-tools; it writes generated code/descriptors into a temporary
directory. It does not require Cargo, a board, SSH keys or a network service.

These tests establish that the schema compiles, metadata references real fields,
positive examples can be represented, and one independently authored wire example
agrees across formats. They do not establish runtime rejection behavior, micropb
compatibility, heap-free MCU linking, throughput or multicast delivery.

The [Phase 2 verification script](../tools/message-demo/verify.py) checks the
actual CSV/JSON/Protobuf codecs, validation and host command/response service. Keep
the generated outputs derived from these definitions rather than hand-editing each language's message types.
