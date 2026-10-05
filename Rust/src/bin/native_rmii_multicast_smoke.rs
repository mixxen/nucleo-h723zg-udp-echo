//! Phase 3 bench smoke image: two Protobuf multicast streams on native Ethernet.
//! Uses the existing MCUboot application layout. No command service or updater yet.
#![no_std]
#![no_main]
#[path = "../board.rs"]
mod board;
#[path = "../bringup/embassy_network.rs"]
mod embassy_network;
#[path = "../bringup/native_rmii.rs"]
mod native_rmii;
#[cfg(feature = "profiling")]
#[path = "../profiling.rs"]
mod profiling;

use core::{
    fmt::Write,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};
use defmt::{error, info, unwrap};
use embassy_executor::Spawner;
use embassy_net::{
    Ipv4Address, Stack, StackResources,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_stm32::{
    bind_interrupts,
    gpio::{Level, Output, Speed},
    peripherals::RNG,
    rng::{self, Rng},
};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use messaging_codec::{
    Format, MAX_BODY, encode,
    model::{Envelope, Health, Status},
    schedule::Schedule,
};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// One DHCP socket and two TX-only publication sockets. No command/profiling socket.
static NETWORK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static SEND_ERRORS: AtomicU32 = AtomicU32::new(0);
static SKIPPED: AtomicU32 = AtomicU32::new(0);
static ACTIVE_ERRORS: AtomicU8 = AtomicU8::new(0);
const GROUP: Ipv4Address = Ipv4Address::new(239, 255, 42, 1);
const DEVICE_ID: &str = "board-01";
bind_interrupts!(struct RngInterrupts {RNG => rng::InterruptHandler<RNG>;});
fn add(counter: &AtomicU32, amount: u64) {
    let increment = amount.min(u32::MAX as u64) as u32;
    let mut value = counter.load(Ordering::Relaxed);
    loop {
        match counter.compare_exchange_weak(
            value,
            value.saturating_add(increment),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(current) => value = current,
        }
    }
}
#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, native_rmii::Device>) -> ! {
    runner.run().await
}

#[embassy_executor::task(pool_size = 2)]
async fn publish(
    stack: Stack<'static>,
    boot: heapless::String<16>,
    kind: i32,
    port: u16,
    period_ms: u64,
) -> ! {
    let mut rx_metadata = [];
    let mut rx_bytes = [];
    let mut tx_metadata = [PacketMetadata::EMPTY; 1];
    let mut tx_bytes = [0; MAX_BODY];
    let mut bytes = [0; MAX_BODY];
    let mut socket = UdpSocket::new(
        stack,
        &mut rx_metadata,
        &mut rx_bytes,
        &mut tx_metadata,
        &mut tx_bytes,
    );
    socket.set_hop_limit(Some(1));
    let mut schedule = Schedule::new(period_ms, Instant::now().as_millis()).unwrap();
    let error_bit = 1 << (kind - 21);
    loop {
        Timer::at(Instant::from_millis(schedule.next_ms())).await;
        let now = Instant::now().as_millis();
        let Some(due) = schedule.take_due(now) else {
            continue;
        };
        add(&SKIPPED, due.skipped);
        if due.skipped != 0 {
            ACTIVE_ERRORS.fetch_or(error_bit, Ordering::Relaxed);
        }
        if !stack.is_link_up() || !stack.is_config_up() {
            // Discard the single pending TX packet during an outage, not a backlog at reconnect.
            socket.close();
            add(&SKIPPED, 1);
            ACTIVE_ERRORS.fetch_or(error_bit, Ordering::Relaxed);
            continue;
        }
        if !socket.is_open() {
            unwrap!(socket.bind(port));
        }
        let mut message = Envelope {
            protocol_version: Some(1),
            kind: Some(kind),
            device_id: Some(DEVICE_ID.try_into().unwrap()),
            boot_id: Some(boot.clone()),
            sequence: Some(due.sequence),
            uptime_ms: Some(now as u32),
            ..Default::default()
        };
        if kind == 21 {
            message.status = Some(Status {
                temperature_celsius: Some(25.5),
                validity: Some(1),
                synthetic: Some(true),
            });
        } else {
            message.health = Some(Health {
                state: Some(if ACTIVE_ERRORS.load(Ordering::Relaxed) == 0 {
                    1
                } else {
                    2
                }),
                received_requests: Some(0),
                rejected_datagrams: Some(0),
                send_errors: Some(SEND_ERRORS.load(Ordering::Relaxed)),
                skipped_publications: Some(SKIPPED.load(Ordering::Relaxed)),
            });
        }
        let length = match encode(Format::Protobuf, &message, &mut bytes) {
            Ok(length) => length,
            Err(_) => {
                error!("smoke encode failed");
                add(&SKIPPED, 1);
                ACTIVE_ERRORS.fetch_or(error_bit, Ordering::Relaxed);
                continue;
            }
        };
        // This pinned API enqueues immediately or returns an error; no cancellable send future.
        match socket.try_send_to(&bytes[..length], (GROUP, port)) {
            Ok(()) => {
                if due.skipped == 0 {
                    ACTIVE_ERRORS.fetch_and(!error_bit, Ordering::Relaxed);
                } else {
                    ACTIVE_ERRORS.fetch_or(error_bit, Ordering::Relaxed);
                }
                if kind == 22 {
                    info!(
                        "health queued: seq={} skipped={} send_errors={}",
                        due.sequence,
                        SKIPPED.load(Ordering::Relaxed),
                        SEND_ERRORS.load(Ordering::Relaxed)
                    );
                }
            }
            Err(_) => {
                add(&SEND_ERRORS, 1);
                add(&SKIPPED, 1);
                ACTIVE_ERRORS.fetch_or(error_bit, Ordering::Relaxed);
            }
        }
    }
}
#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = board::init(false);
    let ready_led = Output::new(p.PE1, Level::Low, Speed::Low);
    let error_led = Output::new(p.PB14, Level::High, Speed::Low);
    let mut rng = Rng::new(p.RNG, RngInterrupts);
    let mut random = [0; 8];
    match with_timeout(Duration::from_secs(1), rng.async_fill_bytes(&mut random)).await {
        Ok(Ok(())) if random != [0; 8] => {}
        _ => {
            error!("boot nonce failed; multicast service stopped");
            core::future::pending::<()>().await;
            unreachable!();
        }
    }
    let mut boot = heapless::String::<16>::new();
    for byte in random {
        write!(&mut boot, "{byte:02x}").unwrap();
    }
    info!(
        "multicast smoke: device={} boot={} protobuf group=239.255.42.1 ttl=1",
        DEVICE_ID,
        boot.as_str()
    );
    let device = native_rmii::new(
        p.ETH, p.PA1, p.PA7, p.PC4, p.PC5, p.PG13, p.PB13, p.PG11, p.ETH_SMA, p.PA2, p.PC1,
    );
    let (stack, runner) = embassy_net::new(
        device,
        embassy_net::Config::dhcpv4(Default::default()),
        NETWORK_RESOURCES.init(StackResources::new()),
        u64::from_le_bytes(random),
    );
    spawner.spawn(unwrap!(net_task(runner)));
    spawner.spawn(unwrap!(embassy_network::supervise(
        stack, ready_led, error_led
    )));
    spawner.spawn(unwrap!(publish(stack, boot.clone(), 21, 42001, 100)));
    spawner.spawn(unwrap!(publish(stack, boot, 22, 42002, 1000)));
    core::future::pending().await
}
