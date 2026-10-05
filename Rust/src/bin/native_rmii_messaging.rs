//! Phase 4: one wire format, read commands, and two independent multicast streams.
#![no_std]
#![no_main]
#[cfg(feature = "messaging-benchmark")]
#[path = "../servers/embassy_benchmark.rs"]
mod benchmark;
#[path = "../board.rs"]
mod board;
#[cfg(not(feature = "rollback-test"))]
#[path = "../boot_confirmation.rs"]
mod boot_confirmation;
#[path = "../bringup/embassy_network.rs"]
mod embassy_network;
#[path = "../servers/embassy_messaging.rs"]
mod messaging;
#[path = "../servers/messaging_config.rs"]
mod messaging_config;
#[path = "../bringup/native_rmii.rs"]
mod native_rmii;
#[cfg(feature = "profiling")]
#[path = "../profiling.rs"]
mod profiling;
#[cfg(feature = "profiling")]
#[path = "../servers/embassy_profiling.rs"]
mod profiling_server;

use core::{fmt::Write, sync::atomic::Ordering};
use defmt::{error, info, unwrap};
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_stm32::{
    bind_interrupts,
    gpio::{Level, Output, Speed},
    peripherals::RNG,
    rng::{self, Rng},
};
use embassy_time::{Duration, Timer, with_timeout};
use messaging_config::*;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// DHCP + command + status + health + optional profiling. No echo/SSH socket.
#[cfg(not(feature = "messaging-benchmark"))]
static NETWORK_RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
#[cfg(feature = "messaging-benchmark")]
static NETWORK_RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();
bind_interrupts!(struct RngInterrupts { RNG => rng::InterruptHandler<RNG>; });
#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, native_rmii::Device>) -> ! {
    messaging::STARTED.fetch_or(1, Ordering::Release);
    runner.run().await
}
#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = board::init(false);
    let ready_led = Output::new(p.PE1, Level::Low, Speed::Low);
    let error_led = Output::new(p.PB14, Level::High, Speed::Low);
    let mut rng = Rng::new(p.RNG, RngInterrupts);
    let mut random = [0; 8];
    if !matches!(
        with_timeout(Duration::from_secs(1), rng.async_fill_bytes(&mut random)).await,
        Ok(Ok(()))
    ) || random == [0; 8]
    {
        error!("boot nonce failed; service stopped; trial remains unconfirmed");
        core::future::pending::<()>().await;
    }
    let mut boot = heapless::String::<16>::new();
    for byte in random {
        write!(&mut boot, "{byte:02x}").unwrap();
    }
    if !messaging::self_check(&boot) {
        error!("messaging codec checkpoint failed; trial remains unconfirmed");
        core::future::pending::<()>().await;
    }
    #[cfg(feature = "messaging-benchmark")]
    if !benchmark::self_check(&boot) {
        error!("benchmark codec/service checkpoint failed; trial remains unconfirmed");
        core::future::pending::<()>().await;
    }
    info!(
        "messaging: device={} boot={} format={} crc={}",
        DEVICE_ID,
        boot.as_str(),
        FORMAT.name(),
        CRC.name()
    );
    info!(
        "ports command={} status={} health={} group={} ttl=1",
        COMMAND_PORT, STATUS_PORT, HEALTH_PORT, GROUP
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
    spawner.spawn(unwrap!(messaging::commands(stack, boot.clone())));
    #[cfg(feature = "messaging-benchmark")]
    spawner.spawn(unwrap!(benchmark::run(stack, boot.clone())));
    spawner.spawn(unwrap!(messaging::publish(
        stack,
        boot.clone(),
        21,
        STATUS_PORT,
        STATUS_PERIOD_MS
    )));
    spawner.spawn(unwrap!(messaging::publish(
        stack,
        boot,
        22,
        HEALTH_PORT,
        HEALTH_PERIOD_MS
    )));
    #[cfg(feature = "profiling")]
    spawner.spawn(unwrap!(profiling_server::run(stack)));
    // Require a working timer and all service tasks to initialize. Cable, DHCP,
    // remote clients and multicast receivers are deliberately not prerequisites.
    let initialized = with_timeout(Duration::from_secs(2), async {
        loop {
            Timer::after_millis(100).await;
            if messaging::STARTED.load(Ordering::Acquire) == messaging::ALL_STARTED {
                break;
            }
        }
    })
    .await
    .is_ok();
    if !initialized {
        error!("messaging task checkpoint failed; trial remains unconfirmed");
        core::future::pending::<()>().await;
    }
    #[cfg(not(feature = "rollback-test"))]
    {
        let mut flash = embassy_stm32::flash::Flash::new_blocking(p.FLASH);
        match boot_confirmation::confirm_running_trial(&mut flash) {
            Ok(result) => info!("messaging startup checkpoint: {:?}", result),
            Err(reason) => error!(
                "MCUboot confirmation failed: {:?}; reset can roll back",
                reason
            ),
        }
    }
    #[cfg(feature = "rollback-test")]
    defmt::warn!("rollback-test: messaging checkpoint passed; confirmation deliberately withheld");
    core::future::pending().await
}
