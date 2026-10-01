//! Independent, fixed-storage command and publication tasks for native Ethernet.
use crate::messaging_config::*;
use core::{
    cell::RefCell,
    sync::atomic::{AtomicU8, Ordering},
};
use critical_section::Mutex;
use defmt::unwrap;
use embassy_futures::{
    select::{Either, select},
    yield_now,
};
use embassy_net::{
    Stack,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use messaging_codec::{
    CodecError,
    framing::{decode, encode, parse},
    model::*,
    schedule::Schedule,
    service::{CommandPolicy, Counters, Decision},
};

static COUNTERS: Mutex<RefCell<Counters>> = Mutex::new(RefCell::new(Counters::new()));
pub static STARTED: AtomicU8 = AtomicU8::new(0);
pub const ALL_STARTED: u8 = 0b1111; // network runner, command, status, health
fn counters<R>(update: impl FnOnce(&mut Counters) -> R) -> R {
    critical_section::with(|cs| update(&mut COUNTERS.borrow(cs).borrow_mut()))
}
fn status() -> Status {
    Status {
        temperature_celsius: Some(25.5),
        validity: Some(1),
        synthetic: Some(true),
    }
}
fn base(boot: &heapless::String<16>, now: u64) -> Envelope {
    Envelope {
        protocol_version: Some(1),
        device_id: Some(DEVICE_ID.try_into().unwrap()),
        boot_id: Some(boot.clone()),
        uptime_ms: Some(now as u32),
        ..Default::default()
    }
}
fn response(
    boot: &heapless::String<16>,
    request: Envelope,
    error: Option<i32>,
    now: u64,
) -> Envelope {
    let mut message = Envelope {
        kind: Some(if error.is_some() {
            14
        } else {
            request.kind.unwrap() + 10
        }),
        client_session: request.client_session,
        request_id: request.request_id,
        ..base(boot, now)
    };
    match message.kind {
        Some(11) => {
            message.device_info = Some(DeviceInfo {
                firmware_version: Some(FIRMWARE_VERSION.try_into().unwrap()),
                encoding: Some(FORMAT.id()),
            })
        }
        Some(12) => message.status = Some(status()),
        Some(13) => message.health = Some(counters(|c| c.health())),
        Some(14) => message.error = Some(Error { code: error }),
        _ => unreachable!(),
    }
    message
}
fn publication(boot: &heapless::String<16>, kind: i32, sequence: u32, now: u64) -> Envelope {
    let mut message = Envelope {
        kind: Some(kind),
        sequence: Some(sequence),
        ..base(boot, now)
    };
    if kind == 21 {
        message.status = Some(status());
    } else {
        message.health = Some(counters(|c| c.health()));
    }
    message
}

/// Check the selected decoder, request policy, and every generated response/publication
/// before confirming a trial. This is a local checkpoint, not proof of Ethernet delivery.
pub fn self_check(boot: &heapless::String<16>) -> bool {
    if !GROUP.is_multicast() {
        return false;
    }
    let mut bytes = [0; DATAGRAM_CAPACITY];
    let mut policy = CommandPolicy::new(0);
    for kind in 1..=3 {
        let mut request = Envelope {
            protocol_version: Some(1),
            kind: Some(kind),
            device_id: Some(DEVICE_ID.try_into().unwrap()),
            client_session: Some(boot.clone()),
            request_id: Some(1),
            ..Default::default()
        };
        match kind {
            1 => request.get_device_info = Some(Empty {}),
            2 => request.get_status = Some(Empty {}),
            _ => request.get_health = Some(Empty {}),
        }
        let Ok(n) = encode(FORMAT, CRC, &request, &mut bytes) else {
            return false;
        };
        let Ok(decoded) = decode(FORMAT, CRC, &bytes[..n]) else {
            return false;
        };
        if decoded != request
            || policy.assess(&decoded, FORMAT, DEVICE_ID, 0).decision != Decision::Accept
        {
            return false;
        }
        if encode(
            FORMAT,
            CRC,
            &response(boot, decoded.clone(), None, 0),
            &mut bytes,
        )
        .is_err()
            || encode(
                FORMAT,
                CRC,
                &response(boot, decoded, Some(2), 0),
                &mut bytes,
            )
            .is_err()
        {
            return false;
        }
    }
    [21, 22]
        .iter()
        .all(|kind| encode(FORMAT, CRC, &publication(boot, *kind, 0, 0), &mut bytes).is_ok())
}
fn ready(stack: Stack<'_>) -> bool {
    stack.is_link_up() && stack.is_config_up()
}
async fn down(stack: Stack<'_>) {
    // The pinned stack has ONE state WakerRegistration. Multiple tasks awaiting
    // wait_link/config_* would wake each other continually. Only the supervisor
    // owns that waiter; application tasks sample readiness at a bounded interval.
    while ready(stack) {
        Timer::after_millis(10).await;
    }
}

#[embassy_executor::task]
pub async fn commands(stack: Stack<'static>, boot: heapless::String<16>) -> ! {
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx = [0; 2 * DATAGRAM_CAPACITY];
    let mut tx = [0; 2 * DATAGRAM_CAPACITY];
    let mut input = [0; DATAGRAM_CAPACITY];
    let mut output = [0; DATAGRAM_CAPACITY];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
    unwrap!(socket.bind(COMMAND_PORT));
    let mut policy = CommandPolicy::new(Instant::now().as_millis());
    STARTED.fetch_or(2, Ordering::Release);
    loop {
        // Always yield, even if malformed traffic keeps the RX queue ready.
        yield_now().await;
        if !ready(stack) {
            socket.close();
            counters(|c| c.active[0] = true);
            Timer::after_millis(10).await;
            continue;
        }
        if !socket.is_open() {
            unwrap!(socket.bind(COMMAND_PORT));
        }
        let received = match select(socket.recv_from(&mut input), down(stack)).await {
            Either::First(result) => result,
            Either::Second(()) => {
                socket.close();
                counters(|c| c.active[0] = true);
                continue;
            }
        };
        let (length, remote) = match received {
            Ok(packet) => packet,
            Err(_) => {
                counters(|c| c.rejected = c.rejected.saturating_add(1));
                continue;
            }
        };
        let local = stack
            .config_v4()
            .map(|config| embassy_net::IpAddress::Ipv4(config.address.address()));
        if remote.local_address != local {
            counters(|c| c.rejected = c.rejected.saturating_add(1));
            continue;
        }
        let request = match parse(FORMAT, CRC, &input[..length]) {
            Ok(request) => request,
            Err(error) => {
                counters(|c| {
                    c.rejected = c.rejected.saturating_add(1);
                    if error == CodecError::Crc {
                        c.crc_rejections = c.crc_rejections.saturating_add(1);
                    }
                });
                continue;
            }
        };
        let assessment = policy.assess(&request, FORMAT, DEVICE_ID, Instant::now().as_millis());
        if assessment.rejection.is_some() {
            counters(|c| c.rejected = c.rejected.saturating_add(1));
        }
        let error = match assessment.decision {
            Decision::Accept => {
                counters(|c| c.received = c.received.saturating_add(1));
                None
            }
            Decision::Error(code) => Some(code),
            Decision::Ignore | Decision::RateLimited => continue,
        };
        let reply = response(&boot, request, error, Instant::now().as_millis());
        let Ok(length) = encode(FORMAT, CRC, &reply, &mut output) else {
            counters(|c| c.active[0] = true);
            continue;
        };
        // Enqueue immediately, then bound local draining to 50 ms. This does not
        // wait for a remote ACK. A timeout/link change drops queued responses.
        let sent = if socket.try_send_to(&output[..length], remote).is_ok() {
            matches!(
                with_timeout(
                    Duration::from_millis(50),
                    select(socket.flush(), down(stack))
                )
                .await,
                Ok(Either::First(()))
            )
        } else {
            false
        };
        counters(|c| c.sent(0, sent));
        if !sent {
            socket.close();
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
pub async fn publish(
    stack: Stack<'static>,
    boot: heapless::String<16>,
    kind: i32,
    port: u16,
    period_ms: u64,
) -> ! {
    let mut rx_meta = [];
    let mut rx = [];
    let mut tx_meta = [PacketMetadata::EMPTY; 1];
    let mut tx = [0; DATAGRAM_CAPACITY];
    let mut output = [0; DATAGRAM_CAPACITY];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
    unwrap!(socket.bind(port));
    socket.set_hop_limit(Some(1));
    let mut schedule = Schedule::new(period_ms, Instant::now().as_millis()).unwrap();
    let index = (kind - 20) as usize;
    let mut reported_crc_rejections = 0;
    let mut next_crc_report_ms = 0;
    STARTED.fetch_or(1 << (index + 1), Ordering::Release);
    loop {
        let deadline = Timer::at(Instant::from_millis(schedule.next_ms()));
        if ready(stack) {
            if matches!(select(deadline, down(stack)).await, Either::Second(())) {
                socket.close();
                counters(|c| c.active[index] = true);
                continue;
            }
        } else {
            socket.close();
            deadline.await;
        }
        let now = Instant::now().as_millis();
        let Some(due) = schedule.take_due(now) else {
            continue;
        };
        // CRC-specific counts stay out of the frozen health schema. Emit at most
        // once per second, on the health task, and only when the count changed.
        if kind == 22 && now >= next_crc_report_ms {
            next_crc_report_ms = now.saturating_add(1000);
            let count = counters(|c| c.crc_rejections);
            if count != reported_crc_rejections {
                defmt::warn!("crc_rejections={}", count);
                reported_crc_rejections = count;
            }
        }
        counters(|c| {
            c.skip(due.skipped);
            if due.skipped > 0 {
                c.active[index] = true;
            }
        });
        if !ready(stack) {
            counters(|c| {
                c.skip(1);
                c.active[index] = true;
            });
            continue;
        }
        if !socket.is_open() {
            unwrap!(socket.bind(port));
        }
        let message = publication(&boot, kind, due.sequence, now);
        let Ok(length) = encode(FORMAT, CRC, &message, &mut output) else {
            counters(|c| {
                c.skip(1);
                c.active[index] = true;
            });
            continue;
        };
        // A stalled local queue cannot retain an old sample across the next slot.
        let sent = if socket.try_send_to(&output[..length], (GROUP, port)).is_ok() {
            matches!(
                with_timeout(
                    Duration::from_millis(period_ms.min(50)),
                    select(socket.flush(), down(stack))
                )
                .await,
                Ok(Either::First(()))
            )
        } else {
            false
        };
        counters(|c| {
            c.sent(index, sent);
            if !sent {
                c.skip(1);
            }
            if due.skipped > 0 {
                c.active[index] = true;
            }
        });
        if !sent {
            socket.close();
        }
    }
}
