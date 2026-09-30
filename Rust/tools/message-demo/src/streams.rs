use crate::{Counters, Log, Result};
use messaging_codec::{
    Format, MAX_BODY, encode,
    model::{Envelope, Status},
    schedule::Schedule,
};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    collections::BTreeMap,
    io,
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

pub struct Publisher {
    socket: UdpSocket,
    destination: SocketAddrV4,
    interface: Ipv4Addr,
    kind: i32,
    period_ms: u64,
}
impl Publisher {
    pub fn configure(options: &BTreeMap<String, String>, command_port: u16) -> Result<Vec<Self>> {
        let Some(interface) = options.get("multicast-interface") else {
            if [
                "group",
                "status-port",
                "health-port",
                "status-hz",
                "health-hz",
            ]
            .iter()
            .any(|key| options.contains_key(*key))
            {
                return Err("stream options require --multicast-interface IPv4".into());
            }
            return Ok(Vec::new());
        };
        let interface: Ipv4Addr = interface.parse()?;
        if interface.is_unspecified() || interface.is_multicast() || interface.is_broadcast() {
            return Err("choose a concrete local IPv4 multicast interface".into());
        }
        let group: Ipv4Addr = options
            .get("group")
            .map(String::as_str)
            .unwrap_or("239.255.42.1")
            .parse()?;
        if !group.is_multicast() {
            return Err("--group must be an IPv4 multicast address".into());
        }
        let status_port: u16 = options
            .get("status-port")
            .map(String::as_str)
            .unwrap_or("42001")
            .parse()?;
        let health_port: u16 = options
            .get("health-port")
            .map(String::as_str)
            .unwrap_or("42002")
            .parse()?;
        if status_port == 0
            || health_port == 0
            || status_port == health_port
            || [status_port, health_port].contains(&command_port)
        {
            return Err(
                "command, status and health ports must be distinct and stream ports nonzero".into(),
            );
        }
        let mut publishers = Vec::new();
        for (kind, port, flag, default) in [
            (21, status_port, "status-hz", "10"),
            (22, health_port, "health-hz", "1"),
        ] {
            let hz: f64 = options
                .get(flag)
                .map(String::as_str)
                .unwrap_or(default)
                .parse()?;
            if !hz.is_finite() || !(0.1..=1000.0).contains(&hz) {
                return Err("stream rates must be 0.1..1000 Hz".into());
            }
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
            socket.set_reuse_address(true)?;
            socket.bind(&SocketAddrV4::new(interface, port).into())?;
            socket.set_multicast_if_v4(&interface)?;
            socket.set_multicast_ttl_v4(1)?;
            socket.set_multicast_loop_v4(true)?;
            socket.set_send_buffer_size(16 * 1024)?;
            socket.set_nonblocking(true)?;
            publishers.push(Self {
                socket: socket.into(),
                destination: SocketAddrV4::new(group, port),
                interface,
                kind,
                period_ms: (1000.0 / hz).round() as u64,
            });
        }
        Ok(publishers)
    }
    pub fn start(
        self,
        format: Format,
        template: Envelope,
        status: Status,
        counters: Arc<Mutex<Counters>>,
        log: Log,
        start: Instant,
    ) -> io::Result<()> {
        let stream = if self.kind == 21 { "status" } else { "health" };
        let log = log.publisher(
            self.socket.local_addr()?.to_string(),
            self.interface.to_string(),
            stream,
            self.period_ms,
        );
        thread::Builder::new()
            .name(format!("publish-{stream}"))
            .spawn(move || {
                let mut schedule = Schedule::new(self.period_ms, 0).unwrap();
                let mut bytes = [0; MAX_BODY];
                loop {
                    let now = start.elapsed().as_millis() as u64;
                    let Some(due) = schedule.take_due(now) else {
                        thread::sleep(Duration::from_millis(
                            schedule.next_ms().saturating_sub(now),
                        ));
                        continue;
                    };
                    let mut message = Envelope {
                        kind: Some(self.kind),
                        sequence: Some(due.sequence),
                        uptime_ms: Some(now as u32),
                        ..template.clone()
                    };
                    {
                        let mut shared = counters.lock().unwrap();
                        shared.skipped = shared
                            .skipped
                            .saturating_add(due.skipped.min(u32::MAX as u64) as u32);
                        if due.skipped != 0 {
                            shared.active_stream_error[(self.kind - 21) as usize] = true;
                        }
                        if self.kind == 21 {
                            message.status = Some(status.clone());
                        } else {
                            message.health = Some(shared.health());
                        }
                    }
                    let length = match encode(format, &message, &mut bytes) {
                        Ok(length) => length,
                        Err(_) => {
                            let mut shared = counters.lock().unwrap();
                            shared.skipped = shared.skipped.saturating_add(1);
                            shared.active_stream_error[(self.kind - 21) as usize] = true;
                            drop(shared);
                            let _ = log.event(
                                "encode_error",
                                Some(self.destination.to_string()),
                                0,
                                Some(&message),
                                None,
                            );
                            continue;
                        }
                    };
                    let sent = self.socket.send_to(&bytes[..length], self.destination);
                    let outcome = {
                        let mut shared = counters.lock().unwrap();
                        let success = matches!(sent,Ok(count) if count==length);
                        shared.active_stream_error[(self.kind - 21) as usize] =
                            !success || due.skipped != 0;
                        if !success {
                            shared.skipped = shared.skipped.saturating_add(1);
                            shared.send_errors = shared.send_errors.saturating_add(1);
                            "send_error"
                        } else if due.skipped != 0 {
                            "publication_after_skip"
                        } else {
                            "sent_publication"
                        }
                    };
                    let _ = log.event(
                        outcome,
                        Some(self.destination.to_string()),
                        length,
                        Some(&message),
                        None,
                    );
                }
            })?;
        Ok(())
    }
}
