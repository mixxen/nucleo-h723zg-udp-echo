"""B1 reference checks, not a runtime codec or lease implementation.

Compile the proposed schema, independently author a golden request, round-trip
fixtures through descriptor-driven CSV/JSON and protoc Protobuf, and bound sizes.
Run from any directory with requirements-phase1.txt installed.
"""

import copy
import csv
import importlib.util
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

from google.protobuf import json_format

HERE = Path(__file__).resolve().parent
PROFILE = json.loads((HERE / "profile.json").read_text())
CASES = json.loads((HERE / "fixtures.json").read_text())
BODIES = {int(k): v for k, v in PROFILE["kind_body"].items()}
DATA_KINDS = (5, 15, 21, 22)
ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def pattern(length, seed, token):
    return "".join(ALPHABET[(seed % 62 + token % 62 + i) % 62] for i in range(length))


def require(condition, outcome="validation_error"):
    if not condition:
        raise ValueError(outcome)


def validate(message, descriptor, encoding, crc):
    """Reference semantic validator; transport/ownership checks belong to B2/B3."""

    def types(value, shape):
        require(isinstance(value, dict))
        require(set(value) <= set(shape.fields_by_name))
        for name, item in value.items():
            field = shape.fields_by_name[name]
            if field.message_type:
                types(item, field.message_type)
            elif field.type == field.TYPE_STRING:
                require(isinstance(item, str))
            elif field.type == field.TYPE_BOOL:
                require(type(item) is bool)
            elif field.type == field.TYPE_FLOAT:
                require(type(item) in (int, float))
            else:
                require(type(item) is int and 0 <= item <= 2**32 - 1)

    types(message, descriptor)
    require(
        message.get("protocol_version") == 1,
        "version_error" if "protocol_version" in message else "validation_error",
    )
    kind = message.get("kind")
    require(kind in BODIES)
    body = BODIES[kind]
    require(set(message) & set(BODIES.values()) == {body})
    require(bool(re.fullmatch(r"[A-Za-z0-9_.-]{1,32}", message.get("device_id", ""))))
    for name in ("session_id",) if kind == 1 else ("session_id", "boot_id"):
        value = message.get(name, "")
        require(bool(re.fullmatch(r"[0-9a-f]{16}", value)) and value != "0" * 16)
    request = kind in (1, 2, 3, 4, 5)
    publication = kind in (21, 22)
    require(("uptime_ms" in message) == (not request))
    require(("sequence" in message) == publication)
    require(("request_id" in message) == (not publication))
    if not publication:
        require(message["request_id"] > 0)
    if kind == 1:
        require("boot_id" not in message and "run_epoch" not in message)
    else:
        require(message.get("run_epoch", 0) > 0)
    require(("payload" in message) == (kind in DATA_KINDS))
    if kind in DATA_KINDS:
        require(len(message["payload"]) <= PROFILE["max_payload_bytes"])
        require(all(c in ALPHABET for c in message["payload"]))
    values = message[body]
    required = set(PROFILE["body_csv_columns"][body])
    if body == "status" and values.get("validity") in (2, 3):
        required.remove("temperature_celsius")
    require(set(values) == required)
    if body == "configure":
        limits = PROFILE["limits"]
        require(
            values["payload_bytes"] <= PROFILE["max_payload_bytes"],
            "invalid_configuration",
        )
        require(
            limits["min_lease_ms"] <= values["lease_ms"] <= limits["max_lease_ms"],
            "invalid_configuration",
        )
        for name in ("status", "health"):
            period = values[name + "_period_us"]
            require(
                period == 0
                or limits["min_" + name + "_period_us"]
                <= period
                <= limits["max_period_us"],
                "invalid_configuration",
            )
    elif body == "limits":
        expected = dict(
            PROFILE["limits"],
            encoding=encoding,
            crc_enabled=crc,
            max_payload_bytes=512,
            max_datagram_bytes=1024,
            available=values["available"],
        )
        require(values == expected)
    elif body == "result":
        require(1 <= values["code"] <= 10 and values["lease_remaining_ms"] <= 30000)
    elif body == "status":
        require(values["synthetic"] is True and values["validity"] in (1, 2, 3))
        if values["validity"] == 1:
            require(-100 <= values["temperature_celsius"] <= 200)
    elif body == "health":
        require(values["state"] in (1, 2))


def csv_encode(message):
    body = BODIES[message["kind"]]
    cells = [message.get(name, "") for name in PROFILE["common_csv_columns"]]
    cells += [message[body].get(name, "") for name in PROFILE["body_csv_columns"][body]]
    cells = [str(value).lower() if type(value) is bool else value for value in cells]
    output = io.StringIO(newline="")
    csv.writer(output, lineterminator="\r\n").writerow(cells)
    return output.getvalue().encode()


def csv_decode(wire, descriptor):
    rows = list(csv.reader(io.StringIO(wire.decode(), newline=""), strict=True))
    assert len(rows) == 1 and wire.endswith(b"\r\n")
    cells = rows[0]
    kind = int(cells[1])
    body = BODIES[kind]
    names = PROFILE["common_csv_columns"] + PROFILE["body_csv_columns"][body]
    assert len(cells) == len(names)
    message = {body: {}}
    common_count = len(PROFILE["common_csv_columns"])
    for index, (name, text) in enumerate(zip(names, cells, strict=True)):
        shape = (
            descriptor
            if index < common_count
            else descriptor.fields_by_name[body].message_type
        )
        target = message if index < common_count else message[body]
        # Kind determines whether an empty CSV payload cell is present-empty.
        if text == "" and not (name == "payload" and kind in DATA_KINDS):
            continue
        field = shape.fields_by_name[name]
        if field.type == field.TYPE_STRING:
            value = text
        elif field.type == field.TYPE_BOOL:
            assert text in ("true", "false")
            value = text == "true"
        elif field.type == field.TYPE_FLOAT:
            value = float(text)
        else:
            value = int(text)
        target[name] = value
    return message


def expanded():
    cases = copy.deepcopy(CASES["valid"])
    originals = {c["name"]: c["message"] for c in cases}
    bounds = CASES["boundary_expansions"]
    for name in bounds["data_messages"]:
        for length in bounds["payload_lengths"]:
            m = copy.deepcopy(originals[name])
            m["device_id"] = "x" * bounds["metadata"]["device_id_length"]
            for key in ("request_id", "sequence", "uptime_ms", "run_epoch"):
                if key in m:
                    m[key] = bounds["metadata"][key]
            m["payload"] = pattern(
                length, 2**32 - 1, m.get("sequence", m.get("request_id"))
            )
            if "health" in m:
                for key in m["health"]:
                    if key != "state":
                        m["health"][key] = bounds["health_counters"]
            cases.append(dict(name=f"{name}_boundary_{length}", message=m))
    # Worst control metadata, disabled streams, maximum settings, all result codes.
    for name in ("capabilities", "limits", "configure", "renew", "stop", "applied"):
        m = copy.deepcopy(originals[name])
        m["device_id"] = "x" * 32
        for key in ("request_id", "uptime_ms", "run_epoch"):
            if key in m:
                m[key] = 2**32 - 1
        if name == "configure":
            m["configure"].update(
                payload_bytes=512,
                pattern_seed=2**32 - 1,
                status_period_us=10000000,
                health_period_us=10000000,
                lease_ms=30000,
            )
        cases.append(dict(name=name + "_boundary", message=m))
    m = copy.deepcopy(originals["configure"])
    m["configure"].update(
        payload_bytes=0, status_period_us=0, health_period_us=0, lease_ms=2000
    )
    cases.append(dict(name="command_only", message=m))
    for code in range(1, 11):
        m = copy.deepcopy(originals["applied"])
        m["result"] = dict(code=code, lease_remaining_ms=0)
        cases.append(dict(name=f"control_code_{code}", message=m))
    return cases, originals


def main():
    with tempfile.TemporaryDirectory() as directory:
        root = HERE.parent
        subprocess.run(
            [
                sys.executable,
                "-m",
                "grpc_tools.protoc",
                f"-I{root}",
                f"--python_out={directory}",
                str(root / "messaging.proto"),
                str(HERE / "benchmark.proto"),
            ],
            check=True,
        )
        sys.path.insert(0, directory)
        spec = importlib.util.spec_from_file_location(
            "benchmark_pb2", Path(directory) / "benchmark/benchmark_pb2.py"
        )
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        descriptor = module.Envelope.DESCRIPTOR
        for body, columns in PROFILE["body_csv_columns"].items():
            assert len(columns) == len(set(columns))
            assert set(columns) == set(
                descriptor.fields_by_name[body].message_type.fields_by_name
            )
        assert set(PROFILE["common_csv_columns"]) == {
            f.name for f in descriptor.fields if not f.message_type
        }
        for name, capacity in PROFILE["string_max_utf8_bytes"].items():
            field = descriptor.fields_by_name[name.split(".")[1]]
            assert field.type == field.TYPE_STRING and capacity > 0
        cases, originals = expanded()
        golden = CASES["golden_capabilities"]
        expected = originals["capabilities"]
        assert csv_encode(expected).decode() == golden["csv"]
        assert json.loads(golden["json"]) == expected
        proto = json_format.ParseDict(expected, module.Envelope())
        assert proto.SerializeToString().hex() == golden["protobuf_hex"]
        maxima = {}
        for encoding, format in enumerate(("csv", "json", "protobuf"), 1):
            for crc in (False, True):
                largest = (0, "")
                for case in cases:
                    m = copy.deepcopy(case["message"])
                    if "limits" in m:
                        m["limits"].update(encoding=encoding, crc_enabled=crc)
                    validate(m, descriptor, encoding, crc)
                    if format == "csv":
                        wire = csv_encode(m)
                        restored = csv_decode(wire, descriptor)
                    elif format == "json":
                        wire = json.dumps(
                            m, separators=(",", ":"), allow_nan=False
                        ).encode()
                        restored = json.loads(wire)
                    else:
                        value = json_format.ParseDict(m, module.Envelope())
                        wire = value.SerializeToString()
                        restored = json_format.MessageToDict(
                            module.Envelope.FromString(wire),
                            preserving_proto_field_name=True,
                            use_integers_for_enums=True,
                        )
                    assert restored == m, case["name"]
                    assert len(wire) <= PROFILE["max_body_bytes"], (
                        format,
                        case["name"],
                        len(wire),
                    )
                    framed_length = len(wire) + (8 if crc else 0)
                    assert framed_length <= PROFILE["max_datagram_bytes"]
                    largest = max(largest, (framed_length, case["name"]))
                maxima[format + ("/on" if crc else "/off")] = largest
        for case in CASES["invalid"]:
            m = copy.deepcopy(originals[case["base"]])
            m.update(case.get("set", {}))
            if "remove" in case:
                m.pop(case["remove"])
            if "payload_repeat" in case:
                m["payload"] = "A" * case["payload_repeat"]
            if "configure_set" in case:
                m["configure"].update(case["configure_set"])
            try:
                validate(m, descriptor, 1, False)
            except ValueError as error:
                assert str(error) == case["expected"], case["name"]
            else:
                raise AssertionError(case["name"])
        assert pattern(5, 0, 0) == "01234"
        assert pattern(4, 61, 0) == "z012"
        assert pattern(4, 2**32 - 1, 2**32 - 1) == "6789"
        print(
            f"B1: {len(cases)} fixtures x 6 configurations round-trip; golden request agrees."
        )
        print(f"{len(CASES['invalid'])} reference semantic rejections checked.")
        print(
            "Largest fixture datagrams (bytes, fixture): "
            + json.dumps(maxima, sort_keys=True)
        )
        print(
            f"{len(CASES['state_cases'])} state scenarios specified, NOT runtime tested. CRC length accounting only; CRC correctness remains covered by Phase 5."
        )


if __name__ == "__main__":
    main()
