"""Host harness for the portable Rust service used by the NUCLEO firmware."""

import ctypes
import json
import socket
from benchmark_wire import Wire


class RustService:
    def __init__(self, device, boot, encoding, crc, start):
        self.wire = Wire(encoding, crc)
        self.boot = boot
        self.start = start
        self.last = 0
        self.epoch = 1
        self.owner = None
        self.pointer = None
        self.new = self.wire.library.benchmark_service_new
        self.new.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        self.new.restype = ctypes.c_void_p
        self.call = self.wire.library.benchmark_service_call
        self.call.argtypes = [
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_size_t,
            ctypes.c_void_p,
            ctypes.c_size_t,
        ]
        self.call.restype = ctypes.c_ssize_t
        self.free = self.wire.library.benchmark_service_free
        self.free.argtypes = [ctypes.c_void_p]
        self.free.restype = None
        data = json.dumps(
            dict(device=device, boot=boot, format=encoding, crc=crc)
        ).encode()
        self.pointer = self.new(ctypes.create_string_buffer(data), len(data))
        if not self.pointer:
            raise ValueError("Rust service initialization failed")

    def _call(self, op, now=None, **values):
        if not self.pointer:
            raise ValueError("Rust service is closed")
        if now is not None:
            self.last = (now - self.start) // 1000
        data = json.dumps(
            dict(op=op, now=self.last, **values), allow_nan=False
        ).encode()
        source = ctypes.create_string_buffer(data)
        output = ctypes.create_string_buffer(4096)
        length = self.call(self.pointer, source, len(data), output, len(output))
        if length < 0:
            raise ValueError("Rust service call rejected")
        result = json.loads(output.raw[:length])
        self.epoch = result["epoch"]
        self.owner = True if result["active"] else None
        return result["reply"]

    def handle(self, message, peer, now):
        return self._call(
            "handle",
            now,
            message=message,
            address=list(socket.inet_aton(peer[0])),
            port=peer[1],
        )

    def tick(self, now):
        self._call("tick", now)

    def end(self):
        self._call("end")

    def reject(self):
        self._call("reject", crc=False)

    def sent(self, kind, success):
        self._call("sent", index=0 if kind == 0 else kind - 20, success=success)

    def count(self, name, amount=1):
        if name != "skipped_publications":
            raise ValueError("unsupported diagnostic counter")
        self._call("skip", amount=amount)

    def publications(self, now):
        return [
            message
            for index in (0, 1)
            if (message := self._call("publication", now, index=index)) is not None
        ]

    def close(self):
        if self.pointer:
            self.free(self.pointer)
            self.pointer = None

    def __del__(self):
        if getattr(self, "pointer", None):
            self.close()
