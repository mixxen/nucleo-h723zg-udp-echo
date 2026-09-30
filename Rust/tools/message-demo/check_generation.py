"""Prove reproducibility and an isolated schema-only change without editing the checkout."""

from pathlib import Path
import tempfile
import generate
from google.protobuf import descriptor_pb2

with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    baseline, changed, repeated, protocol = [
        root / name for name in ("baseline", "changed", "repeated", "protocol")
    ]
    for directory in (baseline, changed, repeated, protocol):
        directory.mkdir()
    outputs = generate.generate(baseline)
    for filename, destination in outputs.items():
        assert (
            baseline / filename
        ).read_bytes() == destination.read_bytes(), (
            f"Stale generated output: {destination}"
        )
    source = (generate.PROTOCOL / "messaging.proto").read_text()
    old = "DEGRADED = 2;"
    assert old in source
    (protocol / "messaging.proto").write_text(
        source.replace(old, old + "\n  STARTING = 3;")
    )
    (protocol / "profile.json").write_bytes(
        (generate.PROTOCOL / "profile.json").read_bytes()
    )
    generate.PROTOCOL = protocol
    generate.generate(changed)
    generate.generate(repeated)
    for filename in outputs:
        assert (changed / filename).read_bytes() == (
            repeated / filename
        ).read_bytes(), filename
    for filename in ("protobuf.rs", "messaging_pb2.py", "schema.py", "REFERENCE.md"):
        assert (changed / filename).read_bytes() != (
            baseline / filename
        ).read_bytes(), filename
    descriptor = descriptor_pb2.FileDescriptorSet.FromString(
        (changed / "schema.pb").read_bytes()
    )
    health = next(
        enum for enum in descriptor.file[0].enum_type if enum.name == "HealthState"
    )
    assert health.value[-1].name == "STARTING" and health.value[-1].number == 3
print(
    "Generation is current; an isolated enum addition reproducibly changes Rust/Python Protobuf, metadata, and reference docs."
)
