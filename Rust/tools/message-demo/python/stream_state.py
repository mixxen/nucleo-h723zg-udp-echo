"""Bounded stream history. Gaps describe observations, never a proven network cause."""

from collections import deque

MASK = 2**32 - 1
HALF = 2**31
WINDOW = 64


class StreamState:
    def __init__(self, stale_seconds):
        self.stale_seconds = stale_seconds
        self.highest = None
        self.seen = set()
        self.missing = set()
        self.last_progress = None
        self.stale = False

    def finish(self, reason):
        events = []
        if self.missing:
            events.append(
                dict(
                    outcome="gap_final",
                    sequences=sorted(self.missing),
                    count=len(self.missing),
                    reason=reason,
                )
            )
            self.missing.clear()
        return events

    def tick(self, now):
        if (
            self.last_progress is not None
            and not self.stale
            and now - self.last_progress >= self.stale_seconds
        ):
            self.stale = True
            return [dict(outcome="stale", age_ms=(now - self.last_progress) * 1000)]
        return []

    def observe(self, sequence, now):
        events = []
        if self.highest is None:
            self.highest = sequence
            self.seen = {sequence}
            events.append(dict(outcome="baseline"))
        else:
            advance = (sequence - self.highest) & MASK
            if advance == HALF:
                return [
                    dict(
                        outcome="discontinuity",
                        reason="ambiguous_half_range",
                        advance=advance,
                    )
                ]
            if advance == 0 or sequence in self.seen:
                return [dict(outcome="duplicate_publication")]
            if advance > HALF:
                if sequence in self.missing:
                    self.missing.remove(sequence)
                    self.seen.add(sequence)
                    return [dict(outcome="reordered_publication")]
                return [dict(outcome="late_publication")]
            if advance > WINDOW:
                events.extend(self.finish("discontinuity"))
                self.seen = set()
                events.append(
                    dict(
                        outcome="discontinuity",
                        reason="beyond_reorder_window",
                        advance=advance,
                    )
                )
            else:
                new_missing = {
                    (self.highest + offset) & MASK for offset in range(1, advance)
                }
                self.missing.update(new_missing)
                if new_missing:
                    events.append(
                        dict(
                            outcome="gap_pending",
                            sequences=sorted(new_missing),
                            count=len(new_missing),
                        )
                    )
                finalized = {
                    value
                    for value in self.missing
                    if (sequence - value) & MASK >= WINDOW
                }
                if finalized:
                    events.append(
                        dict(
                            outcome="gap_final",
                            sequences=sorted(finalized),
                            count=len(finalized),
                            reason="window_expired",
                        )
                    )
                    self.missing.difference_update(finalized)
                self.seen = {
                    value for value in self.seen if (sequence - value) & MASK < WINDOW
                }
            self.highest = sequence
            self.seen.add(sequence)
        self.last_progress = now
        if self.stale:
            events.append(dict(outcome="recovered"))
        self.stale = False
        events.append(dict(outcome="accepted_publication"))
        return events


class DeviceStreams:
    """Two streams and eight retired boot IDs per explicitly selected device."""

    def __init__(self, status_stale=0.5, health_stale=3.0):
        self.thresholds = {21: status_stale, 22: health_stale}
        self.boot = None
        self.retired = deque(maxlen=8)
        self.streams = {
            kind: StreamState(threshold) for kind, threshold in self.thresholds.items()
        }

    def tick(self, now):
        return [
            dict(event, kind=kind, boot_id=self.boot)
            for kind, state in self.streams.items()
            for event in state.tick(now)
        ]

    def finish(self, reason="run_end"):
        return [
            dict(event, kind=kind, boot_id=self.boot)
            for kind, state in self.streams.items()
            for event in state.finish(reason)
        ]

    def observe(self, message, now):
        boot, kind = message["boot_id"], message["kind"]
        if boot in self.retired:
            return [dict(outcome="retired_session", boot_id=boot, kind=kind)]
        events = []
        if boot != self.boot:
            previous = self.boot
            events.extend(self.finish("session_change"))
            if previous is not None:
                self.retired.append(previous)
            self.boot = boot
            self.streams = {
                key: StreamState(threshold)
                for key, threshold in self.thresholds.items()
            }
            events.append(
                dict(
                    outcome="session_change",
                    previous_boot_id=previous,
                    boot_id=boot,
                    kind=kind,
                )
            )
            events.extend(
                dict(outcome="not_seen", boot_id=boot, kind=other)
                for other in self.streams
                if other != kind
            )
        events.extend(
            dict(event, kind=kind, boot_id=boot)
            for event in self.streams[kind].observe(message["sequence"], now)
        )
        return events
