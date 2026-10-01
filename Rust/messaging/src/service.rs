//! Bounded command decisions shared by the host and MCU transports.
use crate::{CodecError, Format, model::Envelope, validation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Error(i32),
    Ignore,
    RateLimited,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assessment {
    pub decision: Decision,
    pub rejection: Option<CodecError>,
}

/// Error replies only: burst 10, replenishing one token every 100 ms.
/// Millisecond credits retain fractional tokens without floats or allocation.
pub struct CommandPolicy {
    credits_ms: u64,
    updated_ms: u64,
}
impl CommandPolicy {
    pub const fn new(now_ms: u64) -> Self {
        Self {
            credits_ms: 1000,
            updated_ms: now_ms,
        }
    }
    pub fn assess(
        &mut self,
        request: &Envelope,
        format: Format,
        device: &str,
        now_ms: u64,
    ) -> Assessment {
        let validity = validation::validate(request, format);
        let is_request = matches!(request.kind, Some(1..=3));
        let target = request.device_id.as_deref() == Some(device);
        if validity.is_ok() && is_request && target {
            return Assessment {
                decision: Decision::Accept,
                rejection: None,
            };
        }
        let rejection = Some(validity.err().unwrap_or(CodecError::Validation));
        let decision = if !target
            || !validation::correlation(request)
            || matches!(request.kind, Some(11..=14 | 21 | 22))
        {
            Decision::Ignore
        } else {
            self.credits_ms = self
                .credits_ms
                .saturating_add(now_ms.saturating_sub(self.updated_ms))
                .min(1000);
            self.updated_ms = now_ms;
            if self.credits_ms < 100 {
                Decision::RateLimited
            } else {
                self.credits_ms -= 100;
                Decision::Error(if request.kind.is_some() && !is_request {
                    1
                } else {
                    2
                })
            }
        };
        Assessment {
            decision,
            rejection,
        }
    }
}

/// One activity bit per socket; recovery on one socket cannot hide another failure.
#[derive(Default)]
pub struct Counters {
    pub received: u32,
    pub rejected: u32,
    pub crc_rejections: u32,
    pub send_errors: u32,
    pub skipped: u32,
    pub active: [bool; 3],
}
impl Counters {
    pub const fn new() -> Self {
        Self {
            received: 0,
            rejected: 0,
            crc_rejections: 0,
            send_errors: 0,
            skipped: 0,
            active: [false; 3],
        }
    }
    pub fn skip(&mut self, amount: u64) {
        self.skipped = self
            .skipped
            .saturating_add(amount.min(u32::MAX as u64) as u32);
    }
    pub fn sent(&mut self, socket: usize, success: bool) {
        self.active[socket] = !success;
        if !success {
            self.send_errors = self.send_errors.saturating_add(1);
        }
    }
    pub fn health(&self) -> crate::model::Health {
        crate::model::Health {
            state: Some(if self.active.iter().any(|v| *v) { 2 } else { 1 }),
            received_requests: Some(self.received),
            rejected_datagrams: Some(self.rejected),
            send_errors: Some(self.send_errors),
            skipped_publications: Some(self.skipped),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Envelope {
        Envelope {
            protocol_version: Some(1),
            kind: Some(3),
            device_id: Some("board-01".try_into().unwrap()),
            client_session: Some("123456789abcdef0".try_into().unwrap()),
            request_id: Some(1),
            get_health: Some(crate::model::Empty {}),
            ..Default::default()
        }
    }
    #[test]
    fn error_budget_and_read_commands() {
        let mut policy = CommandPolicy::new(0);
        let valid = request();
        let mut bad = valid.clone();
        bad.kind = Some(99);
        for _ in 0..10 {
            assert_eq!(
                policy
                    .assess(&bad, Format::Protobuf, "board-01", 0)
                    .decision,
                Decision::Error(1)
            );
        }
        assert_eq!(
            policy
                .assess(&bad, Format::Protobuf, "board-01", 99)
                .decision,
            Decision::RateLimited
        );
        assert_eq!(
            policy
                .assess(&valid, Format::Protobuf, "board-01", 99)
                .decision,
            Decision::Accept
        );
        assert_eq!(
            policy
                .assess(&bad, Format::Protobuf, "board-01", 100)
                .decision,
            Decision::Error(1)
        );
        bad.kind = Some(3);
        bad.get_health = None;
        assert_eq!(
            policy
                .assess(&bad, Format::Protobuf, "board-01", 200)
                .decision,
            Decision::Error(2)
        );
    }
    #[test]
    fn do_not_reply_to_uncorrelatable_or_nonrequests() {
        let mut policy = CommandPolicy::new(0);
        for kind in [11, 12, 13, 14, 21, 22] {
            let mut bad = request();
            bad.kind = Some(kind);
            assert_eq!(
                policy.assess(&bad, Format::Csv, "board-01", 0).decision,
                Decision::Ignore
            );
        }
        for which in 0..4 {
            let mut bad = request();
            match which {
                0 => bad.protocol_version = Some(2),
                1 => bad.device_id = Some("other".try_into().unwrap()),
                2 => bad.request_id = Some(0),
                _ => bad.client_session = None,
            }
            assert_eq!(
                policy.assess(&bad, Format::Json, "board-01", 0).decision,
                Decision::Ignore
            );
        }
    }
    #[test]
    fn counters_saturate_and_recover_independently() {
        let mut counters = Counters::new();
        counters.sent(0, false);
        counters.sent(1, false);
        counters.sent(0, true);
        assert_eq!(counters.health().state, Some(2));
        counters.sent(1, true);
        assert_eq!(counters.health().state, Some(1));
        assert_eq!(counters.health().send_errors, Some(2));
        counters.skip(u64::MAX);
        counters.skip(10);
        assert_eq!(counters.skipped, u32::MAX);
        counters.send_errors = u32::MAX;
        counters.sent(2, false);
        assert_eq!(counters.send_errors, u32::MAX);
    }
}
