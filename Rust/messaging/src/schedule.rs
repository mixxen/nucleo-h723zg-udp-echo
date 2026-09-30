//! A monotonic publication schedule that skips missed slots instead of replaying them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    pub sequence: u32,
    pub skipped: u64,
}
#[derive(Debug)]
pub struct Schedule {
    period_ms: u64,
    next_ms: u64,
    sequence: u32,
}
impl Schedule {
    pub fn new(period_ms: u64, started_ms: u64) -> Option<Self> {
        (period_ms != 0).then_some(Self {
            period_ms,
            next_ms: started_ms.saturating_add(period_ms),
            sequence: 0,
        })
    }
    pub fn next_ms(&self) -> u64 {
        self.next_ms
    }
    /// Return at most the newest due slot. Sequence numbers include skipped slots.
    pub fn take_due(&mut self, now_ms: u64) -> Option<Due> {
        if now_ms < self.next_ms {
            return None;
        }
        let skipped = (now_ms - self.next_ms) / self.period_ms;
        let sequence = self.sequence.wrapping_add(skipped as u32);
        self.sequence = sequence.wrapping_add(1);
        self.next_ms = self
            .next_ms
            .saturating_add(skipped.saturating_add(1).saturating_mul(self.period_ms));
        Some(Due { sequence, skipped })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_wake_sends_one_current_snapshot() {
        let mut schedule = Schedule::new(100, 0).unwrap();
        assert_eq!(schedule.take_due(99), None);
        assert_eq!(
            schedule.take_due(100),
            Some(Due {
                sequence: 0,
                skipped: 0
            })
        );
        assert_eq!(
            schedule.take_due(550),
            Some(Due {
                sequence: 4,
                skipped: 3
            })
        );
        assert_eq!(schedule.next_ms(), 600);
        assert_eq!(schedule.take_due(550), None);
        assert_eq!(
            schedule.take_due(600),
            Some(Due {
                sequence: 5,
                skipped: 0
            })
        );
    }
    #[test]
    fn sequence_wrap_and_independent_periods() {
        let mut status = Schedule::new(100, 0).unwrap();
        let mut health = Schedule::new(1000, 0).unwrap();
        status.sequence = u32::MAX;
        assert_eq!(status.take_due(100).unwrap().sequence, u32::MAX);
        assert_eq!(status.take_due(200).unwrap().sequence, 0);
        assert_eq!(health.take_due(200), None);
        assert_eq!(health.take_due(1000).unwrap().sequence, 0);
        assert!(Schedule::new(0, 0).is_none());
    }
}
