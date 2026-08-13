//! W5500 hardwired TCP/IP offload UDP echo firmware.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[cfg(feature = "profiling")]
#[path = "../profiling.rs"]
mod profiling;
#[cfg(feature = "profiling")]
#[path = "../servers/w5500_offload_profiling.rs"]
mod profiling_server;
#[path = "../servers/w5500_offload_udp_echo.rs"]
mod udp_server;
#[path = "../bringup/w5500_offload.rs"]
mod w5500_offload;
#[path = "../bringup/w5500_spi.rs"]
mod w5500_spi;

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_stm32::bind_interrupts;
use embassy_stm32::exti::{ExtiInput, InterruptHandler};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::interrupt::typelevel::EXTI15_10;
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    EXTI15_10 => InterruptHandler<EXTI15_10>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = board::init(true);
    info!("W5500 hardware-offload UDP echo server booting");

    let ready_led = Output::new(p.PE1, Level::Low, Speed::Low);
    let error_led = Output::new(p.PB14, Level::High, Speed::Low);
    // W5500 supports an 80 MHz SPI clock. Use 50 MHz here as a conservative
    // first step for the longer Arduino-shield traces; MACRAW remains at its
    // measured 20 MHz baseline so the offload experiment stays isolated.
    let spi = w5500_spi::new(p.SPI1, p.PA5, p.PB5, p.PA6, p.PD14, 50_000_000);
    let mut interrupt = ExtiInput::new(p.PG14, p.EXTI14, Pull::Up, Irqs);
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
    let mut maintenance_due = true;

    loop {
        if network.poll(maintenance_due) {
            server.poll(network.device_mut());
            #[cfg(feature = "profiling")]
            profiling_server::poll(network.device_mut());
        }

        // Packets normally wake us immediately through W5500 INTn. The
        // one-second maintenance wakeup advances DHCP and recovers safely if
        // an interrupt is ever missed or the cable state changes.
        maintenance_due = matches!(
            select(interrupt.wait_for_low(), Timer::after_secs(1)).await,
            Either::Second(_)
        );
    }
}
