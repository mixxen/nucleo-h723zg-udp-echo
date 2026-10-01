# Phase 2: Run the host messaging demo

This phase gives you a Rust UDP responder and a Python client for the same three
read commands in CSV, JSON, and Protobuf. It runs on your computer; no board,
SSH key, bootloader, or signing key is needed. Each process uses one explicitly
selected format. CRC is off. See [Phase 3](PHASE3.md) for multicast publishing and listeners.

## Set up and verify

Run these commands from the repository root. Rust 1.90.0 is pinned for generation
and checks; Python 3.12 is the CI baseline.

```sh
rustup toolchain install 1.90.0 --profile minimal --component rustfmt --component clippy
rustup target add --toolchain 1.90.0 thumbv7em-none-eabihf
python -m venv .venv-messaging
. .venv-messaging/bin/activate
python -m pip install -r Rust/protocol/requirements-phase1.txt
python Rust/tools/message-demo/verify.py
```

On PowerShell, activate with `.\.venv-messaging\Scripts\Activate.ps1` instead.
The verification script selects your host target automatically and invokes Cargo
from the repository root, avoiding the existing firmware's default ARM linker
configuration. It compiles and links an ARM codec probe separately.

The gates check the authored fixtures, generated-file drift, a reproducible
schema change, Rust formatting/lints, real Python/Rust UDP exchanges, rejection
behavior, and an ARM link without an allocator. The ordinary firmware workflow
continues separately. A successful codec link is not a flashable board image.

## Start the responder

In terminal one, from the repository root:

```sh
mkdir -p Rust/artifacts/messaging
DEMO_HOST_TARGET="$(rustc +1.90.0 -vV | sed -n 's/^host: //p')"
cargo +1.90.0 run --locked --manifest-path Rust/tools/message-demo/Cargo.toml \
  --target "$DEMO_HOST_TARGET" -- serve --format csv \
  --bind 127.0.0.1:42000 --device board-01 \
  --log Rust/artifacts/messaging/csv-server.jsonl
```

PowerShell host-target selection and launch:

```powershell
New-Item -ItemType Directory -Force Rust/artifacts/messaging | Out-Null
$demoHostTarget = (rustc +1.90.0 -vV | Select-String '^host: ').ToString().Substring(6)
cargo +1.90.0 run --locked --manifest-path Rust/tools/message-demo/Cargo.toml --target $demoHostTarget -- serve --format csv --bind 127.0.0.1:42000 --device board-01 --log Rust/artifacts/messaging/csv-server.jsonl
```

Wait for `Ready: 127.0.0.1:42000 / csv / board-01`. Stop it with Ctrl+C.
A new process has a fresh nonzero boot ID obtained from the OS random source.

## Send all three commands

In terminal two, activate the same Python environment and run:

```sh
python Rust/tools/message-demo/python/client.py --format csv \
  --host 127.0.0.1 --port 42000 --bind 127.0.0.1 --device board-01 \
  --log Rust/artifacts/messaging/csv-client.jsonl
```

On PowerShell, put this command on one line. The defaults send `info`, `status`,
then `health`, with one outstanding request and a one-second deadline. You should
see these body values (identity, request IDs, and uptime are also printed):

| Command | Expected body |
|---|---|
| `info` | firmware version `host-demo-0.1.0`; encoding `1` for CSV |
| `status` | temperature `25.5`; validity `1`; synthetic `true` |
| `health` | state `1`; received requests `3`; rejection/send/skipped counters `0` |

The health count includes the health request itself. Counts persist until the
responder restarts. Run `--commands status --repeat 5` to request five snapshots.
The client does not retry a timeout. Its exit status is nonzero after a timeout,
an error response, or a local I/O error.

Restart both tools with `--format json`, then `--format protobuf`, using new log
filenames. Device-info encoding becomes `2` or `3`. Both ends must agree; there
is no automatic format detection. Log files are overwritten when reused.

For another host, bind the responder to its LAN address and set the client's
`--host` to that address and `--bind` to the client's LAN interface address. The
client currently uses IPv4. Loopback tests do not establish physical LAN delivery.

## Demonstrate missing data and a timeout

Start the responder with `--sample unavailable` (or `--sample fault`). A status
reply then contains validity `2` (or `3`), synthetic `true`, and **no temperature
field**. It does not invent a zero measurement. The messaging service remains
healthy: missing synthetic data does not itself make transport health degraded.

Stop the responder and issue a request with `--timeout 0.25`. The client reports
that no valid reply arrived before the deadline. That observation does not prove
network packet loss. Use `verify.py` for deterministic rejection, delayed-reply,
duplicate-reply, unknown-kind, and error-rate-limit demonstrations.

## Read the implementation

| File or directory | Responsibility |
|---|---|
| `Rust/protocol/messaging.proto` | Authoritative field types, tags, presence, and enum values |
| `Rust/protocol/profile.json` | Explicit CSV column order, string capacities, and demo defaults |
| `Rust/tools/message-demo/generate.py` | Narrow descriptor adapter; generated model, CSV mapping, Python metadata, reference |
| `Rust/tools/message-demo/generator/` | Pinned `micropb-gen` invocation for the embedded Protobuf implementation |
| `Rust/messaging/src/lib.rs` | Bounded encode/decode API with shared semantic validation |
| `Rust/messaging/src/validation.rs` | Handwritten application rules for all three Rust codecs |
| `Rust/tools/message-demo/src/main.rs` | Host sockets, read responses, counters, error-reply budget, JSONL diagnostics |
| `Rust/tools/message-demo/python/` | Python codec and command client |
| `Rust/tools/message-demo/test_demo.py` | Independent fixtures and cross-language/network checks |

`messaging-codec` is a separate crate so firmware can later depend on it without
bringing in host sockets, files, or the standard library. `#![no_std]` excludes
the standard library; `heapless::String<N>` stores at most N bytes inline.
`Option<T>` distinguishes an absent field from a present zero or false. Generated
models retain numeric enum values until shared validation checks whether they are
allowed. Protobuf uses the standard micropb decoder, not a custom wire parser.

The Rust libraries are pinned: micropb/micropb-gen 0.6.0, csv-core 0.1.13, and
serde-json-core 0.6.0. CSV gets a strict single-record quoting check because
csv-core is intentionally permissive. JSON uses the library's escaped-string
reader plus a literal-control-byte check. Quotes and backslashes are exercised by
the authored fixture in every format. Text temperatures round-trip binary32;
negative zero normalizes to zero. No YAML layer is introduced.

## Regenerate after a schema edit

```sh
python Rust/tools/message-demo/generate.py
python Rust/tools/message-demo/generate.py --check
python Rust/tools/message-demo/check_generation.py
```

The generator supports this experiment's optional scalar fields and Envelope
body messages. Repeated fields and general nested structures are deliberately
unsupported. A new field also needs its CSV column order recorded in the profile;
a string needs a capacity. Application semantics still need handwritten changes.
Do not edit files under `src/generated/` or `python/generated/` directly.

`check_generation.py` adds one temporary enum value to a copy of the `.proto`,
regenerates twice, and verifies identical Rust/Python Protobuf, metadata and
reference outputs. It leaves the real schema unchanged. Adding an enum to the
schema does not automatically authorize it in application validation.

## Diagnostics and parser differences

JSONL records contain a run ID, revision, format, CRC mode, local endpoint and
interface address, peer, wall/monotonic times, raw byte count, decoded message and
outcome. Rejected payload samples are capped at 64 bytes. Client RTT uses a local
monotonic clock. Duplicate/late history retains the last 64 completed or expired
requests; older replies are unmatched. No history grows indefinitely.

- `size_error`: encoded body exceeds 1016 bytes, checked before parsing.
- `decode_error`: malformed syntax, incorrect scalar type, or a field exceeding
  its fixed string capacity. The Phase 1 oversized-version fixture is clarified
  to this outcome because a bounded message cannot be constructed.
- `version_error`: a present unsupported version.
- `validation_error`: decoded fields violate required presence, enum, identity,
  body/kind, measurement, or endpoint-role rules.

JSON rejects unknown/duplicate keys and explicit null. Protobuf skips unknown
fields and uses normal scalar/oneof replacement and submessage merging; those
cases have explicit tests. CSV chooses the body from the kind, so conflicting
body tags cannot be represented. Unknown CSV kinds are dropped because their
column mapping is unknown; recognizable unknown JSON/Protobuf kinds can receive
`UNSUPPORTED_KIND`. Correlatable malformed known requests can receive
`INVALID_REQUEST`, subject to the 10/s, burst-10 error-reply budget.

Counters saturate at uint32 maximum. A successful send clears active send-error
health; historical errors stay counted. `skipped_publications` is zero in this
phase because no publisher is running. Statistics export and CRC checks remain
later phases.
