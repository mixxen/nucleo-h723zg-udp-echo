"""Host binding to the actual no_std Rust benchmark codec, with no subprocess per packet."""

import ctypes
import hashlib
import json
import os
from pathlib import Path
import sys
import struct

from codec import CodecError

FORMATS = {"csv": 1, "json": 2, "protobuf": 3}
ROOT = Path(__file__).resolve().parents[3]
PROFILE = json.loads((ROOT / "protocol/benchmark/profile.json").read_text())
ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def pattern(length, seed, token):
    return "".join(ALPHABET[(seed % 62 + token % 62 + i) % 62] for i in range(length))


class Wire:
    def __init__(self, encoding, crc, library=None):
        if encoding not in FORMATS or crc not in ("off", "on"):
            raise ValueError("explicit format and CRC mode required")
        if library is None:
            name = (
                "benchmark_codec.dll"
                if sys.platform == "win32"
                else (
                    "libbenchmark_codec.dylib"
                    if sys.platform == "darwin"
                    else "libbenchmark_codec.so"
                )
            )
            library = os.environ.get("BENCHMARK_CODEC_LIB")
            if not library:
                candidates = sorted(
                    (ROOT / "tools/message-demo/target").glob(f"*/debug/{name}")
                )
                if len(candidates) != 1:
                    raise ValueError(
                        "build message-demo for the host and set BENCHMARK_CODEC_LIB to its shared library"
                    )
                library = candidates[0]
        self.path = Path(library).resolve()
        self.sha256 = hashlib.sha256(self.path.read_bytes()).hexdigest()
        self.encoding, self.crc = encoding, crc
        self.library = ctypes.CDLL(str(self.path))
        self.translate = self.library.benchmark_translate
        self.translate.argtypes = [
            ctypes.c_uint32,
            ctypes.c_bool,
            ctypes.c_bool,
            ctypes.c_void_p,
            ctypes.c_size_t,
            ctypes.c_void_p,
            ctypes.c_size_t,
        ]
        self.translate.restype = ctypes.c_ssize_t

    def _call(self, data, encode):
        if len(data) > 4096:
            raise CodecError("size_error")
        source = ctypes.create_string_buffer(data)
        destination = ctypes.create_string_buffer(4096)
        result = self.translate(
            FORMATS[self.encoding],
            self.crc == "on",
            encode,
            source,
            len(data),
            destination,
            len(destination),
        )
        if result < 0:
            raise CodecError(
                {
                    -2: "crc_error",
                    -3: "decode_error",
                    -4: "validation_error",
                    -5: "version_error",
                    -6: "size_error",
                }.get(result, "binding_error")
            )
        return destination.raw[:result]

    def encode(self, message):
        return self._call(
            json.dumps(message, separators=(",", ":"), allow_nan=False).encode(), True
        )

    def decode(self, datagram):
        message = json.loads(self._call(datagram, False))
        # Rust JSON prints the shortest f32 decimal; restore its exact binary32
        # value instead of treating that decimal as a distinct binary64 value.
        status = message.get("status", {})
        if "temperature_celsius" in status:
            status["temperature_celsius"] = struct.unpack(
                "<f", struct.pack("<f", status["temperature_celsius"])
            )[0]
        return message
