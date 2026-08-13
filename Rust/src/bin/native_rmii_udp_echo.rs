//! Minimal native-RMII UDP echo firmware for the architecture trade study.

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
#[cfg(feature = "profiling")]
#[path = "../servers/embassy_profiling.rs"]
mod profiling_server;
#[path = "../servers/embassy_udp_echo.rs"]
mod udp_server;

use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_stm32::gpio::{Level, Output, Speed};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

static NETWORK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

#[embassy_executor::task]
async fn net_task(runner: embassy_net::Runner<'static, native_rmii::Device>) -> ! {
    let mut runner = runner;
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = board::init(false);
    info!("native RMII UDP echo server booting");

    let ready_led = Output::new(p.PE1, Level::Low, Speed::Low);
    let error_led = Output::new(p.PB14, Level::High, Speed::Low);
    let device = native_rmii::new(
        p.ETH, p.PA1, p.PA7, p.PC4, p.PC5, p.PG13, p.PB13, p.PG11, p.ETH_SMA, p.PA2, p.PC1,
    );
    let (stack, runner) = embassy_net::new(
        device,
        embassy_net::Config::dhcpv4(Default::default()),
        NETWORK_RESOURCES.init(StackResources::new()),
        0x48_37_32_33_5a_47_00_01,
    );

    spawner.spawn(unwrap!(net_task(runner)));
    spawner.spawn(unwrap!(embassy_network::supervise(
        stack, ready_led, error_led
    )));
    spawner.spawn(unwrap!(udp_server::run(stack)));
    #[cfg(feature = "profiling")]
    spawner.spawn(unwrap!(profiling_server::run(stack)));
    core::future::pending().await
}
