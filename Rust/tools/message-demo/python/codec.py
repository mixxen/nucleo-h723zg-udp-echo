"""Prototype mapping derived from .proto metadata; this is deliberately not ProtoJSON."""

import csv
import io
import json
import math
import re
import struct
from google.protobuf.message import DecodeError
from generated import messaging_pb2
from generated.schema import SHAPES, KIND_BODY, PROFILE

FORMATS = {"csv": 1, "json": 2, "protobuf": 3}
MAX_BODY = PROFILE["max_body_bytes"]


class CodecError(ValueError):
    def __init__(self, outcome):
        super().__init__(outcome)
        self.outcome = outcome


def require(condition, outcome="validation_error"):
    if not condition:
        raise CodecError(outcome)


def shape(message, name="Envelope"):
    """Check descriptor types/capacities and round temperatures to the wire's f32."""
    require(type(message) is dict, "decode_error")
    fields = SHAPES[name]
    require(not message.keys() - fields.keys(), "decode_error")
    result = {}
    for key, value in message.items():
        field = fields[key]
        kind = field["type"]
        if kind == 11:
            value = shape(value, field["message"])
        elif kind == 9:
            require(type(value) is str, "decode_error")
            require(len(value.encode("utf-8")) <= field["capacity"], "decode_error")
        elif kind in (13, 14):
            require(type(value) is int, "decode_error")
            low, high = (0, 2**32 - 1) if kind == 13 else (-(2**31), 2**31 - 1)
            require(low <= value <= high, "decode_error")
        elif kind == 8:
            require(type(value) is bool, "decode_error")
        elif kind == 2:
            require(type(value) in (int, float), "decode_error")
            try:
                value = struct.unpack("<f", struct.pack("<f", value))[0]
            except (OverflowError, struct.error):
                raise CodecError("validation_error") from None
            if value == 0:
                value = 0.0
        result[key] = value
    return result


def session(value):
    return (
        isinstance(value, str)
        and bool(re.fullmatch(r"[0-9a-f]{16}", value))
        and value != "0" * 16
    )


def validate(message, format):
    m = shape(message)
    require("protocol_version" in m)
    require(m["protocol_version"] == 1, "version_error")
    require(bool(re.fullmatch(r"[a-zA-Z0-9_.-]{1,32}", m.get("device_id", ""))))
    kind = m.get("kind")
    require(kind in KIND_BODY)
    bodies = {k for k, f in SHAPES["Envelope"].items() if f["type"] == 11}
    require(m.keys() & bodies == {KIND_BODY[kind]})
    if kind in (1, 2, 3):
        require(not m.keys() & {"boot_id", "sequence", "uptime_ms"})
    else:
        require(session(m.get("boot_id")) and "uptime_ms" in m)
    if kind in (21, 22):
        require("sequence" in m and not m.keys() & {"client_session", "request_id"})
    else:
        require(
            session(m.get("client_session"))
            and m.get("request_id", 0) != 0
            and "sequence" not in m
        )
    if "device_info" in m:
        info = m["device_info"]
        require(info.get("encoding") == FORMATS[format])
        require(
            bool(re.fullmatch(r"[\x20-\x7e]{1,32}", info.get("firmware_version", "")))
        )
    if "status" in m:
        status = m["status"]
        require(status.get("synthetic") is True)
        if status.get("validity") == 1:
            temperature = status.get("temperature_celsius")
            require(
                temperature is not None
                and math.isfinite(temperature)
                and -100 <= temperature <= 200
            )
        else:
            require(
                status.get("validity") in (2, 3) and "temperature_celsius" not in status
            )
    if "health" in m:
        require(
            m["health"].keys() == SHAPES["Health"].keys()
            and m["health"].get("state") in (1, 2)
        )
    if "error" in m:
        require(m["error"].get("code") in (1, 2))
    return m


def unique_object(pairs):
    output = {}
    for key, value in pairs:
        require(key not in output, "decode_error")
        output[key] = value
    return output


def to_proto(message, output):
    for key, value in message.items():
        if type(value) is dict:
            child = getattr(output, key)
            child.SetInParent()
            to_proto(value, child)
        else:
            setattr(output, key, value)
    return output


def from_proto(message):
    return {
        field.name: from_proto(value) if field.type == 11 else value
        for field, value in message.ListFields()
    }


def csv_value(text, field):
    if field["type"] in (13, 14):
        require(bool(re.fullmatch(r"0|-?[1-9][0-9]*", text)), "decode_error")
        return int(text)
    if field["type"] == 2:
        # Rust's f32 parser accepts decimal and special values; validation rejects nonfinite.
        require(text == text.strip() and "_" not in text, "decode_error")
        return float(text)
    if field["type"] == 8:
        require(text in ("true", "false"), "decode_error")
        return text == "true"
    return text


def decode(format, data):
    require(len(data) <= MAX_BODY, "size_error")
    require(bool(data), "decode_error")
    try:
        if format == "json":
            message = json.loads(
                data.decode("utf-8"),
                object_pairs_hook=unique_object,
                parse_constant=lambda _: (_ for _ in ()).throw(
                    CodecError("decode_error")
                ),
            )
        elif format == "protobuf":
            proto = messaging_pb2.Envelope.FromString(data)
            message = from_proto(proto)
        else:
            text = data.decode("utf-8")
            require(
                text.endswith("\r\n")
                and "\r" not in text[:-2]
                and "\n" not in text[:-2],
                "decode_error",
            )
            # csv.reader accepts bare quotes; enforce the same grammar as the Rust adapter.
            state = 0
            for char in text[:-2]:
                if state == 0:
                    state = 2 if char == '"' else 0 if char == "," else 1
                elif state == 1:
                    require(char != '"', "decode_error")
                    state = 0 if char == "," else 1
                elif state == 2:
                    state = 3 if char == '"' else 2
                else:
                    require(char in '",', "decode_error")
                    state = 2 if char == '"' else 0
            require(state != 2, "decode_error")
            row = next(csv.reader(io.StringIO(text, newline=""), strict=True))
            common = PROFILE["common_csv_columns"]
            require(len(row) >= len(common), "decode_error")
            message = {
                key: csv_value(value, SHAPES["Envelope"][key])
                for key, value in zip(common, row)
                if value
            }
            body = KIND_BODY.get(message.get("kind"))
            require(body is not None)
            columns = PROFILE["body_csv_columns"][body]
            require(len(row) == len(common) + len(columns), "decode_error")
            fields = SHAPES[SHAPES["Envelope"][body]["message"]]
            message[body] = {
                key: csv_value(value, fields[key])
                for key, value in zip(columns, row[len(common) :])
                if value
            }
        return validate(message, format)
    except CodecError:
        raise
    except (ValueError, TypeError, UnicodeError, DecodeError, csv.Error, StopIteration):
        raise CodecError("decode_error") from None


def float32_text(value):
    """Shortest significant-digit decimal that rounds back to this binary32."""
    for digits in range(1, 10):
        text = format(value, f".{digits}g")
        if struct.pack("<f", float(text)) == struct.pack("<f", value):
            return text
    raise AssertionError("nine decimal digits must round-trip binary32")


def json_wire(value):
    # Standard JSON quoting/scalars, with f32 rather than Python f64 precision.
    if type(value) is dict:
        return (
            "{"
            + ",".join(json.dumps(k) + ":" + json_wire(v) for k, v in value.items())
            + "}"
        )
    if type(value) is float:
        return float32_text(value)
    return json.dumps(value, allow_nan=False)


def encode(format, message):
    message = validate(message, format)
    if format == "json":
        data = json_wire(message).encode()
    elif format == "protobuf":
        data = to_proto(message, messaging_pb2.Envelope()).SerializeToString()
    else:
        body = KIND_BODY[message["kind"]]
        values = [message.get(key) for key in PROFILE["common_csv_columns"]]
        values += [message[body].get(key) for key in PROFILE["body_csv_columns"][body]]
        values = [
            (
                ""
                if v is None
                else (
                    str(v).lower()
                    if type(v) is bool
                    else float32_text(v) if type(v) is float else v
                )
            )
            for v in values
        ]
        output = io.StringIO(newline="")
        csv.writer(output, lineterminator="\r\n").writerow(values)
        data = output.getvalue().encode()
    require(len(data) <= MAX_BODY, "size_error")
    return data
