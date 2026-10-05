//! One bounded coordinator with nonblocking queues and independent stream schedules.
//! No await while handling/encoding a packet; timers run even under an RX flood.
use crate::{messaging, messaging_config::*};
use core::sync::atomic::Ordering;
use embassy_futures::yield_now;
use embassy_net::{
    IpAddress, Stack,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_time::{Instant, Timer};
use messaging_codec::{
    CodecError,
    benchmark::{
        self,
        model::*,
        service::{Peer, Service, pattern},
    },
};

const COMMAND: u16 = 42010;
const STATUS: u16 = 42011;
const HEALTH: u16 = 42012;

fn service(boot: &heapless::String<16>) -> Service {
    Service::new(
        DEVICE_ID.try_into().unwrap(),
        boot.clone(),
        FORMAT,
        CRC,
        Instant::now().as_micros(),
    )
}
pub fn self_check(boot: &heapless::String<16>) -> bool {
    let mut state = service(boot);
    let now = Instant::now().as_micros();
    let peer = Peer {
        address: [127, 0, 0, 1],
        port: 1,
    };
    let mut bytes = [0; DATAGRAM_CAPACITY];
    let mut request = Envelope {
        protocol_version: Some(1),
        kind: Some(2),
        device_id: Some(DEVICE_ID.try_into().unwrap()),
        boot_id: Some(boot.clone()),
        session_id: Some(boot.clone()),
        run_epoch: Some(1),
        request_id: Some(1),
        configure: Some(Configuration {
            payload_bytes: Some(512),
            pattern_seed: Some(723),
            status_period_us: Some(1000),
            health_period_us: Some(10000),
            lease_ms: Some(2000),
        }),
        ..Default::default()
    };
    let Ok(n) = benchmark::encode(FORMAT, CRC, &request, &mut bytes) else {
        return false;
    };
    let Ok(decoded) = benchmark::decode(FORMAT, CRC, &bytes[..n]) else {
        return false;
    };
    let Some(reply) = state.handle(&decoded, peer, now) else {
        return false;
    };
    if reply.result.as_ref().and_then(|r| r.code) != Some(1) {
        return false;
    }
    request.kind = Some(5);
    request.configure = None;
    request.probe = Some(Empty {});
    request.payload = pattern(512, 723, 1);
    let Some(reply) = state.handle(&request, peer, now) else {
        return false;
    };
    for message in [
        Some(reply),
        state.publication(0, now + 10000),
        state.publication(1, now + 10000),
    ] {
        let Some(message) = message else {
            return false;
        };
        let Ok(n) = benchmark::encode(FORMAT, CRC, &message, &mut bytes) else {
            return false;
        };
        if benchmark::decode(FORMAT, CRC, &bytes[..n]).as_ref() != Ok(&message) {
            return false;
        }
    }
    state.end();
    !state.active() && state.epoch() == 2
}

#[embassy_executor::task]
pub async fn run(stack: Stack<'static>, boot: heapless::String<16>) -> ! {
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx = [0; 2048];
    let mut tx = [0; 2048];
    let mut input = [0; 1024];
    let mut output = [0; 1024];
    let mut status_meta = [PacketMetadata::EMPTY; 1];
    let mut status_tx = [0; 1024];
    let mut health_meta = [PacketMetadata::EMPTY; 1];
    let mut health_tx = [0; 1024];
    let mut command = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
    let mut status = UdpSocket::new(stack, &mut [], &mut [], &mut status_meta, &mut status_tx);
    let mut health = UdpSocket::new(stack, &mut [], &mut [], &mut health_meta, &mut health_tx);
    let mut state = service(&boot);
    let mut generation = (state.epoch(), state.active());
    let mut previous_config = stack.config_v4().map(|c| (c.address, c.gateway));
    // Bind even without cable/DHCP: initialization is a local boot checkpoint.
    if command.bind(COMMAND).is_err()
        || status.bind(STATUS).is_err()
        || health.bind(HEALTH).is_err()
    {
        command.close();
        status.close();
        health.close();
        defmt::error!("benchmark socket checkpoint failed");
        core::future::pending::<()>().await;
    }
    status.set_hop_limit(Some(1));
    health.set_hop_limit(Some(1));
    messaging::STARTED.fetch_or(16, Ordering::Release);
    loop {
        yield_now().await;
        let now = Instant::now().as_micros();
        state.tick(now);
        let configuration = stack.config_v4().map(|c| (c.address, c.gateway));
        let ready = stack.is_link_up() && configuration.is_some();
        if !ready || configuration != previous_config {
            state.end();
            command.close();
            status.close();
            health.close();
            previous_config = configuration;
        }
        if generation != (state.epoch(), state.active()) {
            command.close();
            status.close();
            health.close();
            generation = (state.epoch(), state.active());
        }
        if !ready {
            Timer::after_millis(1).await;
            continue;
        }
        if (!command.is_open() && command.bind(COMMAND).is_err())
            || (!status.is_open() && status.bind(STATUS).is_err())
            || (!health.is_open() && health.bind(HEALTH).is_err())
        {
            state.end();
            command.close();
            status.close();
            health.close();
            defmt::error!("benchmark rebind failed; stopped");
            core::future::pending::<()>().await;
        }
        let received = command.try_recv_from(&mut input);
        if matches!(received, Err(embassy_net::TryError::Other(_))) {
            state.reject(false);
        }
        if let Ok((length, remote)) = received {
            let local = configuration.map(|c| IpAddress::Ipv4(c.0.address()));
            if remote.local_address != local {
                state.reject(false);
            } else {
                #[allow(unreachable_patterns)]
                let peer = match remote.endpoint.addr {
                    IpAddress::Ipv4(ip) => Some(Peer {
                        address: ip.octets(),
                        port: remote.endpoint.port,
                    }),
                    _ => None,
                };
                if let Some(peer) = peer {
                    match benchmark::decode(FORMAT, CRC, &input[..length]) {
                        Ok(request) => {
                            let reply = state.handle(&request, peer, Instant::now().as_micros());
                            if generation != (state.epoch(), state.active()) {
                                command.close();
                                status.close();
                                health.close();
                                generation = (state.epoch(), state.active());
                            }
                            if !command.is_open() && command.bind(COMMAND).is_err() {
                                state.end();
                                continue;
                            }
                            if let Some(reply) = reply {
                                let encoded = benchmark::encode(FORMAT, CRC, &reply, &mut output);
                                state.tick(Instant::now().as_micros());
                                if reply.kind != Some(15)
                                    || reply.run_epoch == Some(state.epoch()) && state.active()
                                {
                                    let sent = encoded.is_ok_and(|n| {
                                        command.try_send_to(&output[..n], remote).is_ok()
                                    });
                                    state.counters.sent(0, sent);
                                }
                            }
                        }
                        Err(error) => state.reject(error == CodecError::Crc),
                    }
                }
            }
        }
        if generation != (state.epoch(), state.active()) {
            command.close();
            status.close();
            health.close();
            generation = (state.epoch(), state.active());
        }
        for (index, socket, port) in [(0, &mut status, STATUS), (1, &mut health, HEALTH)] {
            if !socket.is_open() && socket.bind(port).is_err() {
                state.end();
                break;
            }
            socket.set_hop_limit(Some(1));
            if let Some(message) = state.publication(index, Instant::now().as_micros()) {
                // Replace an unsent prior sample instead of keeping stale work
                // while discarding the newest scheduled opportunity.
                if socket.try_flush().is_err() {
                    state.counters.skip(1);
                    socket.close();
                    if socket.bind(port).is_err() {
                        state.end();
                        break;
                    }
                    socket.set_hop_limit(Some(1));
                }
                let encoded = benchmark::encode(FORMAT, CRC, &message, &mut output);
                state.tick(Instant::now().as_micros());
                if !state.active() || message.run_epoch != Some(state.epoch()) {
                    continue;
                }
                let sent =
                    encoded.is_ok_and(|n| socket.try_send_to(&output[..n], (GROUP, port)).is_ok());
                state.counters.sent(index + 1, sent);
                if !sent {
                    state.counters.skip(1);
                }
            }
        }
        if generation != (state.epoch(), state.active()) {
            command.close();
            status.close();
            health.close();
            generation = (state.epoch(), state.active());
        }
        // 100 us polling bounds idle timer/lease work; actual responsiveness under
        // executor/network load must be measured on the board, not inferred here.
        Timer::after_micros(100).await;
    }
}
