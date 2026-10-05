//! Leased benchmark control and scheduling shared by firmware and host tests.
//! All timestamps are monotonic microseconds. No socket or allocator dependency.
use super::{configuration_valid, model::*, validate};
use crate::{Format, framing::CrcMode, service::Counters};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub address: [u8; 4],
    pub port: u16,
}
#[derive(Clone, PartialEq)]
struct Control {
    kind: i32,
    id: u32,
    configuration: Option<Configuration>,
}
struct Lease {
    peer: Peer,
    session: heapless::String<16>,
    configuration: Configuration,
    last: Control,
    expiry: u64,
}
const _: () = assert!(core::mem::size_of::<Lease>() <= 256);
#[derive(Clone, Copy, Default)]
struct Publication {
    due: u64,
    period: u32,
    sequence: u32,
}

pub struct Service {
    device: heapless::String<32>,
    boot: heapless::String<16>,
    format: Format,
    crc: CrcMode,
    epoch: u32,
    exhausted: bool,
    lease: Option<Lease>,
    schedules: [Publication; 2],
    tokens: u8,
    token_time: u64,
    last_time: u64,
    pub counters: Counters,
    pub suppressed_replies: u32,
}
pub fn pattern(length: u32, seed: u32, token: u32) -> Option<heapless::String<512>> {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    if length > 512 {
        return None;
    }
    let mut out = heapless::String::new();
    for i in 0..length {
        out.push(ALPHABET[((seed % 62 + token % 62 + i) % 62) as usize] as char)
            .ok()?;
    }
    Some(out)
}
impl Service {
    pub fn new(
        device: heapless::String<32>,
        boot: heapless::String<16>,
        format: Format,
        crc: CrcMode,
        now: u64,
    ) -> Self {
        Self {
            device,
            boot,
            format,
            crc,
            epoch: 1,
            exhausted: false,
            lease: None,
            schedules: [Publication::default(); 2],
            tokens: 10,
            token_time: now,
            last_time: now,
            counters: Counters::new(),
            suppressed_replies: 0,
        }
    }
    pub fn epoch(&self) -> u32 {
        self.epoch
    }
    pub fn active(&self) -> bool {
        self.lease.is_some()
    }
    /// Terminate once; an obsolete run can never acquire its old epoch again.
    pub fn end(&mut self) {
        if self.lease.take().is_some() {
            self.schedules = [Publication::default(); 2];
            if self.epoch == u32::MAX {
                self.exhausted = true;
            } else {
                self.epoch += 1;
            }
        }
    }
    pub fn tick(&mut self, now: u64) {
        if now < self.last_time {
            self.end();
            self.token_time = now;
        }
        self.last_time = now;
        if self.lease.as_ref().is_some_and(|l| now >= l.expiry) {
            self.end();
        }
        let refill = now.saturating_sub(self.token_time) / 100_000;
        if refill > 0 {
            self.tokens = self.tokens.saturating_add(refill.min(10) as u8).min(10);
            self.token_time = now - now.saturating_sub(self.token_time) % 100_000;
        }
    }
    pub fn reject(&mut self, crc: bool) {
        if self.active() {
            self.counters.rejected = self.counters.rejected.saturating_add(1);
            if crc {
                self.counters.crc_rejections = self.counters.crc_rejections.saturating_add(1);
            }
        }
    }
    fn base(&self, session: heapless::String<16>, kind: i32, now: u64) -> Envelope {
        Envelope {
            protocol_version: Some(1),
            kind: Some(kind),
            device_id: Some(self.device.clone()),
            boot_id: Some(self.boot.clone()),
            session_id: Some(session),
            run_epoch: Some(self.epoch),
            uptime_ms: Some((now / 1000) as u32),
            ..Default::default()
        }
    }
    pub fn handle(&mut self, request: &Envelope, peer: Peer, now: u64) -> Option<Envelope> {
        self.tick(now);
        if validate(request, self.format, self.crc).is_err()
            || request.device_id.as_ref() != Some(&self.device)
            || !matches!(request.kind, Some(1..=5))
            || request.kind != Some(1) && request.boot_id.as_ref() != Some(&self.boot)
        {
            self.reject(false);
            return None;
        }
        let kind = request.kind?;
        let session = request.session_id.as_ref()?;
        let id = request.request_id?;
        let owner = self
            .lease
            .as_ref()
            .is_some_and(|l| l.peer == peer && &l.session == session);
        let mut reply = if kind == 1 {
            let mut out = self.base(session.clone(), 11, now);
            out.limits = Some(Limits {
                encoding: Some(self.format.id()),
                crc_enabled: Some(self.crc == CrcMode::On),
                max_payload_bytes: Some(512),
                max_datagram_bytes: Some(1024),
                min_status_period_us: Some(1000),
                min_health_period_us: Some(10000),
                max_period_us: Some(10000000),
                min_lease_ms: Some(2000),
                max_lease_ms: Some(30000),
                available: Some(!self.active() && !self.exhausted),
            });
            out
        } else if kind == 5 {
            if !owner || request.run_epoch != Some(self.epoch) {
                self.reject(false);
                return None;
            }
            let config = &self.lease.as_ref()?.configuration;
            if request.payload != pattern(config.payload_bytes?, config.pattern_seed?, id) {
                self.reject(false);
                return None;
            }
            self.counters.received = self.counters.received.saturating_add(1);
            let mut out = self.base(session.clone(), 15, now);
            out.payload = request.payload.clone();
            out.probe_response = Some(Empty {});
            out.request_id = Some(id);
            return Some(out);
        } else {
            let control = Control {
                kind,
                id,
                configuration: request.configure.clone(),
            };
            let code = if request.run_epoch != Some(self.epoch) {
                6
            } else if self.exhausted {
                10
            } else if self.active() && !owner {
                5
            } else if self.lease.as_ref().is_some_and(|l| l.last.id == id) {
                if self.lease.as_ref()?.last == control {
                    2
                } else {
                    8
                }
            } else if self.lease.as_ref().is_some_and(|l| id < l.last.id) {
                7
            } else if kind == 2 && !configuration_valid(request.configure.as_ref()?) {
                4
            } else if kind == 2 && self.active() {
                5
            } else if kind != 2 && !self.active() {
                9
            } else {
                if kind == 2 {
                    let config = request.configure.as_ref()?.clone();
                    for (index, period) in [config.status_period_us?, config.health_period_us?]
                        .into_iter()
                        .enumerate()
                    {
                        self.schedules[index] = Publication {
                            due: now.saturating_add(u64::from(period)),
                            period,
                            sequence: 0,
                        };
                    }
                    self.counters = Counters::new();
                    self.lease = Some(Lease {
                        peer,
                        session: session.clone(),
                        configuration: config,
                        last: control.clone(),
                        expiry: 0,
                    });
                }
                self.counters.received = self.counters.received.saturating_add(1);
                if kind == 4 {
                    self.end();
                    3
                } else {
                    let lease = self.lease.as_mut()?;
                    lease.last = control;
                    lease.expiry =
                        now.saturating_add(u64::from(lease.configuration.lease_ms?) * 1000);
                    1
                }
            };
            if !matches!(code, 1..=3) {
                self.reject(false);
            }
            let remaining = self
                .lease
                .as_ref()
                .filter(|l| {
                    l.peer == peer && &l.session == session && request.run_epoch == Some(self.epoch)
                })
                .map_or(0, |l| (l.expiry.saturating_sub(now) / 1000) as u32);
            let mut out = self.base(session.clone(), 12, now);
            out.run_epoch = request.run_epoch;
            out.result = Some(ControlResult {
                code: Some(code),
                lease_remaining_ms: Some(remaining),
            });
            out
        };
        reply.request_id = Some(id);
        if self.tokens == 0 {
            self.suppressed_replies = self.suppressed_replies.saturating_add(1);
            None
        } else {
            self.tokens -= 1;
            Some(reply)
        }
    }
    /// Return at most the newest due sample for one stream, advancing skipped slots.
    pub fn publication(&mut self, index: usize, now: u64) -> Option<Envelope> {
        self.tick(now);
        let lease = self.lease.as_ref()?;
        let schedule = self.schedules.get_mut(index)?;
        if schedule.period == 0 || now < schedule.due {
            return None;
        }
        let skipped = (now - schedule.due) / u64::from(schedule.period);
        let sequence = schedule.sequence.wrapping_add(skipped as u32);
        schedule.sequence = sequence.wrapping_add(1);
        schedule.due = now.saturating_add(
            u64::from(schedule.period) - (now - schedule.due) % u64::from(schedule.period),
        );
        self.counters.skip(skipped);
        let payload = pattern(
            lease.configuration.payload_bytes?,
            lease.configuration.pattern_seed?,
            sequence,
        )?;
        let mut out = self.base(lease.session.clone(), 21 + index as i32, now);
        out.sequence = Some(sequence);
        out.payload = Some(payload);
        if index == 0 {
            out.status = Some(Status {
                temperature_celsius: Some(25.0),
                validity: Some(1),
                synthetic: Some(true),
            });
        } else {
            let h = self.counters.health();
            out.health = Some(Health {
                state: h.state,
                received_requests: h.received_requests,
                rejected_datagrams: h.rejected_datagrams,
                send_errors: h.send_errors,
                skipped_publications: h.skipped_publications,
            });
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PEER: Peer = Peer {
        address: [127, 0, 0, 1],
        port: 50000,
    };
    fn state() -> Service {
        Service::new(
            "board-01".try_into().unwrap(),
            "0123456789abcdef".try_into().unwrap(),
            Format::Json,
            CrcMode::Off,
            0,
        )
    }
    fn request(kind: i32, id: u32) -> Envelope {
        let mut m = Envelope {
            protocol_version: Some(1),
            kind: Some(kind),
            device_id: Some("board-01".try_into().unwrap()),
            boot_id: Some("0123456789abcdef".try_into().unwrap()),
            session_id: Some("abcdef0123456789".try_into().unwrap()),
            run_epoch: Some(1),
            request_id: Some(id),
            ..Default::default()
        };
        match kind {
            2 => {
                m.configure = Some(Configuration {
                    payload_bytes: Some(512),
                    pattern_seed: Some(723),
                    status_period_us: Some(1000),
                    health_period_us: Some(10000),
                    lease_ms: Some(2000),
                })
            }
            3 => m.renew = Some(Empty {}),
            4 => m.stop = Some(Empty {}),
            5 => {
                m.probe = Some(Empty {});
                m.payload = pattern(512, 723, id);
            }
            _ => unreachable!(),
        }
        m
    }
    fn code(s: &mut Service, m: &Envelope, now: u64) -> i32 {
        s.handle(m, PEER, now)
            .unwrap()
            .result
            .unwrap()
            .code
            .unwrap()
    }
    #[test]
    fn ownership_idempotency_and_atomic_rejection() {
        let mut s = state();
        let m = request(2, 1);
        let mut bad = m.clone();
        bad.configure.as_mut().unwrap().payload_bytes = Some(513);
        assert_eq!(code(&mut s, &bad, 0), 4);
        assert!(!s.active());
        assert_eq!(code(&mut s, &m, 0), 1);
        assert_eq!(code(&mut s, &m, 1), 2);
        assert_eq!(s.lease.as_ref().unwrap().expiry, 2_000_000);
        assert_eq!(code(&mut s, &bad, 2), 8);
        assert_eq!(code(&mut s, &request(3, 2), 3), 1);
        assert_eq!(code(&mut s, &request(3, 1), 4), 7);
        assert_eq!(code(&mut s, &request(2, 3), 5), 5);
        let other = Peer {
            port: 50001,
            ..PEER
        };
        assert_eq!(
            s.handle(&request(3, 3), other, 6)
                .unwrap()
                .result
                .unwrap()
                .code,
            Some(5)
        );
        assert_eq!(s.counters.received, 2);
    }
    #[test]
    fn exact_expiry_stop_and_delayed_old_run() {
        let mut s = state();
        assert_eq!(code(&mut s, &request(2, 1), 0), 1);
        assert_eq!(code(&mut s, &request(3, 2), 2_000_000), 6);
        assert_eq!(s.epoch(), 2);
        assert!(s.handle(&request(5, 1), PEER, 2_000_000).is_none());
        let mut new = request(2, 1);
        new.run_epoch = Some(2);
        assert_eq!(code(&mut s, &new, 2_000_000), 1);
        assert_eq!(code(&mut s, &request(4, 3), 2_000_000), 6);
        assert!(s.active());
        let mut stop = request(4, 2);
        stop.run_epoch = Some(2);
        assert_eq!(code(&mut s, &stop, 2_000_000), 3);
        assert_eq!(code(&mut s, &stop, 2_000_000), 6);
        assert_eq!(code(&mut s, &new, 2_000_000), 6);
        stop.run_epoch = Some(3);
        assert_eq!(code(&mut s, &stop, 2_000_000), 9);
    }
    #[test]
    fn lease_renewal_suppression_and_link_loss() {
        let mut s = state();
        code(&mut s, &request(2, 1), 0);
        assert_eq!(code(&mut s, &request(3, 2), 1_000_000), 1);
        assert_eq!(code(&mut s, &request(3, 2), 1_100_000), 2);
        assert_eq!(s.lease.as_ref().unwrap().expiry, 3_000_000);
        s.tokens = 0;
        s.token_time = 1_100_000;
        assert!(s.handle(&request(3, 3), PEER, 1_100_000).is_none());
        assert_eq!(s.lease.as_ref().unwrap().expiry, 3_100_000);
        s.end();
        assert!(!s.active());
        assert_eq!(s.epoch(), 2);
        assert!(s.publication(0, 1_200_000).is_none());
    }
    #[test]
    fn exhaustion_never_wraps_and_clock_rollback_terminates() {
        let mut s = state();
        s.epoch = u32::MAX;
        let mut m = request(2, u32::MAX);
        m.run_epoch = Some(u32::MAX);
        assert_eq!(code(&mut s, &m, 0), 1);
        let mut renew = request(3, 1);
        renew.run_epoch = Some(u32::MAX);
        assert_eq!(code(&mut s, &renew, 0), 7);
        s.tick(2_000_000);
        assert!(s.exhausted);
        assert!(!s.active());
        assert_eq!(s.epoch(), u32::MAX);
        assert_eq!(code(&mut s, &m, 2_000_000), 10);
        let mut s = state();
        code(&mut s, &request(2, 1), 100);
        s.tick(99);
        assert!(!s.active());
    }
    #[test]
    fn probes_and_independent_schedules_have_bounded_catchup() {
        let mut s = state();
        code(&mut s, &request(2, 1), 0);
        let m = request(5, 1);
        assert_eq!(s.handle(&m, PEER, 0).unwrap().payload, m.payload);
        let mut bad = m;
        bad.payload = Some("bad".try_into().unwrap());
        assert!(s.handle(&bad, PEER, 0).is_none());
        assert!(s.publication(0, 999).is_none());
        assert_eq!(s.publication(0, 3500).unwrap().sequence, Some(2));
        assert!(s.publication(1, 3500).is_none());
        assert_eq!(s.counters.skipped, 2);
        assert!(s.publication(0, 3500).is_none());
        s.schedules[0].sequence = u32::MAX;
        assert_eq!(s.publication(0, 4000).unwrap().sequence, Some(u32::MAX));
        assert_eq!(s.publication(0, 5000).unwrap().sequence, Some(0));
        s.counters.sent(1, false);
        assert_eq!(s.counters.health().state, Some(2));
        s.counters.sent(1, true);
        assert_eq!(s.counters.health().state, Some(1));
    }
    #[test]
    fn wrong_boot_and_invalid_input_never_mutate() {
        let mut s = state();
        let mut m = request(2, 1);
        m.boot_id = Some("1111111111111111".try_into().unwrap());
        assert!(s.handle(&m, PEER, 0).is_none());
        assert!(!s.active());
        m = request(2, 1);
        m.configure.as_mut().unwrap().lease_ms = None;
        assert!(s.handle(&m, PEER, 0).is_none());
        assert!(!s.active());
        assert_eq!(pattern(4, u32::MAX, u32::MAX).unwrap().as_str(), "6789");
        assert!(pattern(513, 0, 0).is_none());
    }
}
