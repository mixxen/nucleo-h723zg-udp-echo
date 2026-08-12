//! W5500 hardwired TCP/IP offload UDP echo firmware.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../servers/w5500_offload_udp_echo.rs"]
mod udp_server;
#[path = "../bringup/w5500_offload.rs"]
mod w5500_offload;
#[path = "../bringup/w5500_spi.rs"]
mod w5500_spi;

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = board::init(true);
    info!("W5500 hardware-offload UDP echo server booting");

    let ready_led = Output::new(p.PE1, Level::Low, Speed::Low);
    let error_led = Output::new(p.PB14, Level::High, Speed::Low);
    let spi = w5500_spi::new(p.SPI1, p.PA5, p.PB5, p.PA6, p.PD14);
    let mut network = match w5500_offload::Network::new(spi, ready_led, error_led).await {
        Ok(network) => network,
        Err(w5500_offload::InitError::InvalidVersion(version)) => {
            error!("expected W5500 version 4, received {}", version);
            core::future::pending().await
        }
        Err(w5500_offload::InitError::Spi(_)) => {
            error!("W5500 SPI initialization failed");
            core::future::pending().await
        }
    };
    let mut server = udp_server::Server::new();

    loop {
        if network.poll() {
            server.poll(network.device_mut());
        }
        Timer::after_millis(1).await;
    }
}
