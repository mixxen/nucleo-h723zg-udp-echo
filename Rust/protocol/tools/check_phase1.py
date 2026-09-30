"""Check the Phase 1 schema and authored examples, not firmware or codecs.

Run with dependencies from requirements-phase1.txt. Generated Python and
protoc descriptors are temporary and are never maintained as schema sources.
"""
import csv
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from google.protobuf import descriptor_pb2, json_format

ROOT = Path(__file__).resolve().parents[1]


def main():
    profile = json.loads((ROOT / "profile.json").read_text())
    cases = json.loads((ROOT / "fixtures/cases.json").read_text())
    golden = json.loads((ROOT / "fixtures/golden_request.json").read_text())
    with tempfile.TemporaryDirectory() as directory:
        subprocess.run([
            sys.executable, "-m", "grpc_tools.protoc", f"-I{ROOT}",
            f"--python_out={directory}", f"--descriptor_set_out={directory}/schema.pb",
            str(ROOT / "messaging.proto"),
        ], check=True)
        spec = importlib.util.spec_from_file_location("messaging_pb2", Path(directory) / "messaging_pb2.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        descriptors = descriptor_pb2.FileDescriptorSet.FromString((Path(directory) / "schema.pb").read_bytes())
        messages = {m.name: m for m in descriptors.file[0].message_type}
        envelope = module.Envelope.DESCRIPTOR
        for column in profile["common_csv_columns"]:
            assert column in envelope.fields_by_name, column
        for body, columns in profile["body_csv_columns"].items():
            fields = envelope.fields_by_name[body].message_type.fields_by_name
            assert len(columns) == len(set(columns))
            assert set(columns) == set(fields), body
        for path, capacity in profile["string_max_utf8_bytes"].items():
            parent, name = path.split(".")
            field = next(f for f in messages[parent].field if f.name == name)
            assert field.type == descriptor_pb2.FieldDescriptorProto.TYPE_STRING and capacity > 0
        for message in messages.values():
            for field in message.field:
                if field.type != descriptor_pb2.FieldDescriptorProto.TYPE_MESSAGE:
                    assert field.proto3_optional, (message.name, field.name)
        assert profile["max_body_bytes"] + profile["crc_trailer_bytes"] == profile["max_datagram_bytes"]
        names = [case["name"] for case in cases["valid"] + cases["invalid"]]
        assert len(names) == len(set(names))
        for case in cases["valid"]:
            message = json_format.ParseDict(case["message"], module.Envelope())
            assert module.Envelope.FromString(message.SerializeToString()) == message
            assert message.WhichOneof("body")
        expected = next(c["message"] for c in cases["valid"] if c["name"] == golden["name"])
        assert json.loads(golden["json"]) == expected
        assert list(csv.reader(io.StringIO(golden["csv"]), strict=True)) == [
            ["1", "1", "board-01", "", "0123456789abcdef", "1", "", ""]]
        decoded = module.Envelope.FromString(bytes.fromhex(golden["protobuf_hex"]))
        assert decoded == json_format.ParseDict(expected, module.Envelope())
        print(f"Schema compiles; profile references resolve; {len(cases['valid'])} valid fixtures are representable.")
        print("Manually specified CSV/JSON/Protobuf request examples agree.")
        print(f"{len(cases['invalid'])} semantic and {len(cases['wire_rejections'])} wire rejection cases specified, NOT runtime-tested.")


if __name__ == "__main__":
    main()
