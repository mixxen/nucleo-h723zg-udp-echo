"""Bounded accounting for B2 load tests; all times are host monotonic nanoseconds.

This module does no networking. Callers must validate payloads and correlation
before recording a reply. Keeping time explicit makes fault tests reproducible.
"""

from bisect import bisect_left
from collections import Counter, OrderedDict
from dataclasses import dataclass
import math

U32_MAX = 2**32 - 1
HALF_RANGE = 2**31
MAX_TRACKED = 4096
MAX_RTT_NS = 60_000_000_000


def integer(value, minimum, maximum, name):
    if type(value) is not int or not minimum <= value <= maximum:
        raise ValueError(f"{name} must be an integer in {minimum}..{maximum}")
    return value


def stamp(value):
    return integer(value, 0, 2**63 - 1, "monotonic_ns")


class Clocked:
    def __init__(self):
        self.last_now = 0

    def clock(self, now):
        stamp(now)
        if now < self.last_now:
            raise ValueError("monotonic clock moved backwards")
        self.last_now = now


class Histogram:
    """Nearest-rank upper bounds: <=1% +1 ns error above 1 us; exact extrema.

    Values above 60 s increment overflow. A quantile falling in overflow is
    unknown (None), never silently clamped to the highest representable value.
    """

    edges = [0, 1000]
    while edges[-1] < MAX_RTT_NS:
        edges.append(min(MAX_RTT_NS, (edges[-1] * 101 + 99) // 100))
    edges = tuple(edges)

    def __init__(self):
        self.buckets = [0] * len(self.edges)
        self.count = self.total_ns = self.overflow = 0
        self.minimum = self.maximum = None

    def add(self, value):
        stamp(value)
        self.count += 1
        self.total_ns += value
        self.minimum = value if self.minimum is None else min(self.minimum, value)
        self.maximum = value if self.maximum is None else max(self.maximum, value)
        index = bisect_left(self.edges, value)
        if index == len(self.edges):
            self.overflow += 1
        else:
            self.buckets[index] += 1

    def merge(self, other):
        for index, count in enumerate(other.buckets):
            self.buckets[index] += count
        self.count += other.count
        self.total_ns += other.total_ns
        self.overflow += other.overflow
        if other.count:
            self.minimum = (
                other.minimum
                if self.minimum is None
                else min(self.minimum, other.minimum)
            )
            self.maximum = (
                other.maximum
                if self.maximum is None
                else max(self.maximum, other.maximum)
            )

    def quantile(self, fraction):
        if not math.isfinite(fraction) or not 0 < fraction <= 1:
            raise ValueError("quantile fraction must be in (0, 1]")
        if not self.count:
            return None
        rank = math.ceil(fraction * self.count)
        for edge, count in zip(self.edges, self.buckets):
            rank -= count
            if rank <= 0:
                return min(edge, self.maximum)
        return None

    def snapshot(self):
        return dict(
            count=self.count,
            sum_ns=self.total_ns,
            min_ns=self.minimum,
            max_ns=self.maximum,
            p50_upper_ns=self.quantile(0.50),
            p95_upper_ns=self.quantile(0.95),
            p99_upper_ns=self.quantile(0.99),
            overflow=self.overflow,
            # Fixed bin indexes permit exact merging without retaining samples.
            bins={str(i): n for i, n in enumerate(self.buckets) if n},
        )


@dataclass(frozen=True)
class RunIdentity:
    peer_ip: str
    peer_port: int
    device_id: str
    boot_id: str
    session_id: str
    run_epoch: int


class ProbeTracker(Clocked):
    """One run, strictly increasing IDs, bounded pending and recent history.

    Deadlines are exclusive: a valid reply observed at the deadline is late.
    Timeout totals never decrease when a late reply is subsequently observed.
    """

    def __init__(self, identity, limit=64, deadline_ns=1_000_000_000, history=4096):
        super().__init__()
        self.identity = identity
        self.limit = integer(limit, 1, MAX_TRACKED, "outstanding limit")
        self.deadline_ns = integer(deadline_ns, 1_000_000, MAX_RTT_NS, "deadline_ns")
        self.history = integer(history, 1, MAX_TRACKED, "recent history")
        self.pending = OrderedDict()
        self.recent = OrderedDict()
        self.last_id = 0
        self.counts = Counter()
        self.rtt = Histogram()
        self.interval_rtt = Histogram()
        self.high_water = 0
        self.closed = False

    def _remember(self, request_id, state):
        self.recent[request_id] = state
        while len(self.recent) > self.history:
            self.recent.popitem(last=False)
            self.counts["history_evictions"] += 1

    def tick(self, now):
        self.clock(now)
        while self.pending:
            request_id, sent = next(iter(self.pending.items()))
            if sent + self.deadline_ns > now:
                break
            self.pending.popitem(last=False)
            self._remember(request_id, "timeout")
            self.counts["deadline_misses"] += 1

    def sent(self, request_id, now, datagram_bytes):
        """Record only successful local send acceptance, with pre-send timestamp."""
        integer(request_id, self.last_id + 1, U32_MAX, "request_id")
        integer(datagram_bytes, 1, 1024, "datagram_bytes")
        self.tick(now)
        if self.closed or len(self.pending) >= self.limit:
            raise ValueError("closed tracker or full outstanding window")
        self.pending[request_id] = now
        self.last_id = request_id
        self.high_water = max(self.high_water, len(self.pending))
        self.counts["sent"] += 1
        self.counts["sent_bytes"] += datagram_bytes

    def reply(self, identity, request_id, kind, now, datagram_bytes, valid=True):
        """Record after decoding/pattern verification; invalid replies consume no ID."""
        self.tick(now)
        integer(datagram_bytes, 0, 65535, "datagram_bytes")
        self.counts["received_datagrams"] += 1
        self.counts["received_bytes"] += datagram_bytes
        if not valid:
            outcome = "invalid_reply"
        elif identity != self.identity or kind != 15:
            outcome = "foreign_reply"
        elif self.closed:
            outcome = "after_close"
        elif request_id in self.pending:
            sent = self.pending.pop(request_id)
            self._remember(request_id, "replied")
            self.rtt.add(now - sent)
            self.interval_rtt.add(now - sent)
            outcome = "timely_reply"
        elif self.recent.get(request_id) == "timeout":
            self.recent[request_id] = "late"
            outcome = "late_reply"
        elif request_id in self.recent:
            outcome = "duplicate_reply"
        else:
            outcome = "outside_history_or_unsent"
        self.counts[outcome] += 1
        return outcome

    def finish(self, now, interrupted=False):
        self.tick(now)
        if self.pending and not interrupted:
            raise ValueError("normal completion requires deadline drain")
        if interrupted:
            self.counts["unresolved_at_stop"] += len(self.pending)
            self.pending.clear()
        self.closed = True

    def snapshot(self, reset_interval=False):
        result = dict(
            counts=dict(self.counts),
            outstanding=len(self.pending),
            outstanding_high_water=self.high_water,
            retained_recent=len(self.recent),
            rtt=self.rtt.snapshot(),
            interval_rtt=self.interval_rtt.snapshot(),
            closed=self.closed,
        )
        if reset_interval:
            self.interval_rtt = Histogram()
        return result


class Pacer(Clocked):
    """Absolute rational schedule, one newest due send at most; no catch-up burst."""

    def __init__(self, start_ns, hz):
        super().__init__()
        self.start = stamp(start_ns)
        self.hz = integer(hz, 0, 10000, "command_hz")
        self.next_slot = 0
        self.counts = Counter()

    def due(self, now, window_available=True):
        self.clock(now)
        if not self.hz or now < self.start:
            return False
        latest = (now - self.start) * self.hz // 1_000_000_000
        count = latest - self.next_slot + 1
        if count <= 0:
            return False
        self.next_slot = latest + 1
        self.counts["planned_slots"] += count
        self.counts["scheduling_skips"] += count - 1
        if not window_available:
            self.counts["window_skips"] += 1
            return False
        self.counts["send_opportunities"] += 1
        return True

    def next_due_ns(self):
        if not self.hz:
            return None
        return self.start + (self.next_slot * 1_000_000_000 + self.hz - 1) // self.hz


class StreamTracker(Clocked):
    """Fixed bitmap reorder window; gaps are receiver observations, never loss proof.

    First accepted sequence establishes a baseline. Before-first and after-last
    publications cannot be inferred from receiver evidence alone.
    """

    def __init__(self, identity, kind, stale_ns, window=4096):
        super().__init__()
        if kind not in (21, 22):
            raise ValueError("stream kind must be 21 or 22")
        self.identity, self.kind = identity, kind
        self.stale_ns = integer(stale_ns, 1, 2**63 - 1, "stale_ns")
        self.window = integer(window, 1, MAX_TRACKED, "reorder window")
        self.mask = (1 << self.window) - 1
        self.highest = None
        self.bits = self.width = 0
        self.last_progress = None
        self.counts = Counter()
        self.closed = False

    def observe(self, identity, kind, sequence, now, datagram_bytes, valid=True):
        self.clock(now)
        integer(sequence, 0, U32_MAX, "sequence")
        integer(datagram_bytes, 0, 65535, "datagram_bytes")
        self.counts["received_datagrams"] += 1
        self.counts["received_bytes"] += datagram_bytes
        if not valid:
            outcome = "invalid_publication"
        elif identity != self.identity or kind != self.kind:
            outcome = "foreign_publication"
        elif self.closed:
            outcome = "after_close"
        elif self.highest is None:
            self.highest, self.bits, self.width = sequence, 1, 1
            self.last_progress = now
            outcome = "baseline"
        else:
            advance = (sequence - self.highest) & U32_MAX
            if advance == HALF_RANGE:
                outcome = "ambiguous_sequence"
            elif 0 < advance < HALF_RANGE:
                evicted = max(0, self.width + advance - self.window)
                retained_old = max(0, self.window - advance)
                seen_evicted = (self.bits >> retained_old).bit_count()
                self.counts["final_gaps"] += evicted - seen_evicted
                # Never shift by a potentially huge network-derived value.
                self.bits = (
                    1
                    if advance >= self.window
                    else ((self.bits << advance) | 1) & self.mask
                )
                self.width = min(self.window, self.width + advance)
                self.highest, self.last_progress = sequence, now
                outcome = "progress"
            else:
                behind = (self.highest - sequence) & U32_MAX
                if behind >= self.width:
                    outcome = "outside_history"
                elif self.bits & (1 << behind):
                    outcome = "duplicate_publication"
                else:
                    self.bits |= 1 << behind
                    outcome = "reordered_publication"
        self.counts[outcome] += 1
        return outcome

    def snapshot(self, now):
        self.clock(now)
        age = None if self.last_progress is None else now - self.last_progress
        return dict(
            counts=dict(self.counts),
            pending_gaps=self.width - self.bits.bit_count(),
            highest_sequence=self.highest,
            age_ns=age,
            freshness=(
                "not_seen"
                if age is None
                else "stale" if age >= self.stale_ns else "fresh"
            ),
            closed=self.closed,
        )

    def finish(self, now):
        self.clock(now)
        if not self.closed:
            self.counts["final_gaps"] += self.width - self.bits.bit_count()
            self.bits = self.width = 0
            self.closed = True
