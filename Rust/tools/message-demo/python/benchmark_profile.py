"""Optional, nonblocking samples from the existing instrumented-board endpoint."""

import socket
import struct


def decode_snapshot(data):
    if len(data) != 48 or data[:8] != b"STRMPRF1":
        raise ValueError("invalid profiling response")
    _, cpu_hz, ticks_hz, busy, ticks, polls, high_water, capacity = struct.unpack(
        "<8sIIQQQII", data
    )
    if not cpu_hz or not ticks_hz or not high_water <= capacity <= 131072:
        raise ValueError("invalid profiling metrics")
    return dict(
        cpu_hz=cpu_hz,
        ticks_hz=ticks_hz,
        busy_cycles=busy,
        elapsed_ticks=ticks,
        elapsed_seconds=ticks / ticks_hz,
        executor_busy_percent=(
            busy * ticks_hz * 100 / (cpu_hz * ticks) if ticks else None
        ),
        executor_polls=polls,
        stack_high_water_bytes=high_water,
        stack_capacity_bytes=capacity,
    )


class Sampler:
    """One pending request, one prior snapshot, no history or blocking waits."""

    def __init__(self, target, interface, interval, now):
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            self.socket.bind((interface, 0))
            self.socket.connect((target, 5001))
            self.socket.setblocking(False)
        except BaseException:
            self.socket.close()
            raise
        self.period = int(interval * 1e9)
        self.due = now
        self.pending = None
        self.previous = None

    def tick(self, now):
        result = None
        try:
            data = self.socket.recv(49)
        except BlockingIOError:
            data = None
        except OSError as error:
            data = None
            self.pending = None
            result = dict(status="socket_error", error=str(error))
        if data is not None and self.pending is not None:
            self.pending = None
            try:
                value = decode_snapshot(data)
                old = self.previous
                delta = None
                reset = False
                if old:
                    ticks = value["elapsed_ticks"] - old["elapsed_ticks"]
                    busy = value["busy_cycles"] - old["busy_cycles"]
                    reset = ticks < 0 or busy < 0
                    if (
                        ticks > 0
                        and busy >= 0
                        and value["cpu_hz"] == old["cpu_hz"]
                        and value["ticks_hz"] == old["ticks_hz"]
                    ):
                        delta = (
                            busy * value["ticks_hz"] * 100 / (value["cpu_hz"] * ticks)
                        )
                self.previous = value
                result = dict(
                    status="sample",
                    metrics=value,
                    interval_executor_busy_percent=delta,
                    counter_reset_observed=reset,
                )
            except ValueError as error:
                result = dict(status="invalid", error=str(error))
        if self.pending is not None and now >= self.pending:
            self.pending = None
            result = dict(status="timeout")
        if self.pending is None and now >= self.due:
            self.due = now + self.period
            try:
                self.socket.send(b"STRMPRF1S")
                self.pending = now + 1_000_000_000
            except OSError as error:
                result = dict(status="send_error", error=str(error))
        return result

    def close(self):
        self.socket.close()
