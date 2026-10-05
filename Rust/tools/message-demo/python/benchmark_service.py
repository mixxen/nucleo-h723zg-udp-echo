"""Host benchmark service state machine. Explicit clock, bounded state, no I/O."""

import copy
from benchmark_wire import PROFILE, FORMATS, pattern

MAX = 2**32 - 1


def valid_configuration(config):
    return (
        set(config)
        == {
            "payload_bytes",
            "pattern_seed",
            "status_period_us",
            "health_period_us",
            "lease_ms",
        }
        and all(type(v) is int for v in config.values())
        and 0 <= config["payload_bytes"] <= 512
        and 0 <= config["pattern_seed"] <= MAX
        and (
            config["status_period_us"] == 0
            or 1000 <= config["status_period_us"] <= 10000000
        )
        and (
            config["health_period_us"] == 0
            or 10000 <= config["health_period_us"] <= 10000000
        )
        and 2000 <= config["lease_ms"] <= 30000
    )


class Service:
    def __init__(self, device, boot, encoding, crc, start):
        self.device, self.boot, self.encoding, self.crc = device, boot, encoding, crc
        self.start = self.last_time = self.bucket_time = start
        self.epoch = 1
        self.exhausted = False
        self.owner = self.config = self.last_control = None
        self.expiry = 0
        self.schedules = {}
        self.tokens = 10.0
        self.suppressed = 0
        self.reset_counters()

    def reset_counters(self):
        self.active_send_errors = set()
        self.counters = dict(
            received_requests=0,
            rejected_datagrams=0,
            send_errors=0,
            skipped_publications=0,
        )

    def count(self, name, amount=1):
        self.counters[name] = min(MAX, self.counters[name] + amount)

    def sent(self, kind, success):
        if success:
            self.active_send_errors.discard(kind)
        else:
            self.active_send_errors.add(kind)
            self.count("send_errors")

    def end(self):
        if self.owner is not None:
            self.owner = self.config = self.last_control = None
            self.schedules.clear()
            if self.epoch == MAX:
                self.exhausted = True
            else:
                self.epoch += 1

    def tick(self, now):
        if now < self.last_time:
            raise ValueError("clock rollback")
        self.last_time = now
        if self.owner is not None and now >= self.expiry:
            self.end()
        self.tokens = min(10.0, self.tokens + (now - self.bucket_time) / 1e9 * 10)
        self.bucket_time = now

    def reject(self):
        if self.owner is not None:
            self.count("rejected_datagrams")

    def header(self, session, kind, now):
        return dict(
            protocol_version=1,
            kind=kind,
            device_id=self.device,
            boot_id=self.boot,
            session_id=session,
            run_epoch=self.epoch,
            uptime_ms=((now - self.start) // 1_000_000) & MAX,
        )

    def handle(self, message, peer, now):
        self.tick(now)
        kind = message["kind"]
        if (
            kind not in (1, 2, 3, 4, 5)
            or message["device_id"] != self.device
            or (kind != 1 and message["boot_id"] != self.boot)
        ):
            self.reject()
            return None
        session = message["session_id"]
        owner = (peer, session)
        if kind == 1:
            reply = self.header(session, 11, now)
            reply.update(
                request_id=message["request_id"],
                limits=dict(
                    PROFILE["limits"],
                    encoding=FORMATS[self.encoding],
                    crc_enabled=self.crc == "on",
                    max_payload_bytes=512,
                    max_datagram_bytes=1024,
                    available=self.owner is None and not self.exhausted,
                ),
            )
        elif kind == 5:
            if (
                self.owner != owner
                or message["run_epoch"] != self.epoch
                or message["payload"]
                != pattern(
                    self.config["payload_bytes"],
                    self.config["pattern_seed"],
                    message["request_id"],
                )
            ):
                self.reject()
                return None
            self.count("received_requests")
            reply = self.header(session, 15, now)
            reply.update(
                request_id=message["request_id"],
                payload=message["payload"],
                probe_response={},
            )
            return reply
        else:
            request_id = message["request_id"]
            if message["run_epoch"] != self.epoch:
                code = 6
            elif self.exhausted:
                code = 10
            elif self.owner is not None and self.owner != owner:
                code = 5
            elif (
                self.last_control is not None
                and request_id == self.last_control["request_id"]
            ):
                code = 2 if message == self.last_control else 8
            elif (
                self.last_control is not None
                and request_id < self.last_control["request_id"]
            ):
                code = 7
            elif kind == 2 and not valid_configuration(message["configure"]):
                code = 4
            elif kind == 2 and self.owner is not None:
                code = 5
            elif kind in (3, 4) and self.owner is None:
                code = 9
            else:
                code = 3 if kind == 4 else 1
                if kind == 2:
                    self.reset_counters()
                    self.owner, self.config = owner, copy.deepcopy(message["configure"])
                    self.schedules = {
                        kind: [now + period * 1000, 0, period * 1000]
                        for kind, period in (
                            (21, self.config["status_period_us"]),
                            (22, self.config["health_period_us"]),
                        )
                        if period
                    }
                self.count("received_requests")
                self.last_control = copy.deepcopy(message)
                if kind == 4:
                    self.end()
                else:
                    self.expiry = now + self.config["lease_ms"] * 1_000_000
            if code not in (1, 2, 3):
                self.reject()
            remaining = (
                max(0, (self.expiry - now) // 1_000_000)
                if self.owner == owner and message["run_epoch"] == self.epoch
                else 0
            )
            reply = self.header(session, 12, now)
            reply.update(
                run_epoch=message["run_epoch"],
                request_id=request_id,
                result=dict(code=code, lease_remaining_ms=remaining),
            )
        if self.tokens < 1:
            self.suppressed += 1
            return None
        self.tokens -= 1
        return reply

    def publications(self, now):
        self.tick(now)
        output = []
        for kind, schedule in self.schedules.items():
            due, sequence, period = schedule
            if now < due:
                continue
            skipped = (now - due) // period
            self.count("skipped_publications", skipped)
            sequence = (sequence + skipped) & MAX
            schedule[:] = [due + (skipped + 1) * period, (sequence + 1) & MAX, period]
            message = self.header(self.owner[1], kind, now)
            message.update(
                sequence=sequence,
                payload=pattern(
                    self.config["payload_bytes"], self.config["pattern_seed"], sequence
                ),
            )
            if kind == 21:
                message["status"] = dict(
                    temperature_celsius=25.0, validity=1, synthetic=True
                )
            else:
                message["health"] = dict(
                    state=2 if self.active_send_errors else 1,
                    **self.counters,
                )
            output.append(message)
        return output
