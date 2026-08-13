//! W5500 MACRAW plus Embassy UDP echo firmware.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../bringup/embassy_network.rs"]
mod embassy_network;
#[path = "../servers/embassy_udp_echo.rs"]
mod udp_server;
#[path = "../bringup/w5500_macraw.rs"]
mod w5500_macraw;
#[path = "../bringup/w5500_spi.rs"]
mod w5500_spi;

use defmt::{error, info, unwrap};
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_stm32::bind_interrupts;
use embassy_stm32::exti::{ExtiInput, InterruptHandler};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::interrupt::typelevel::EXTI15_10;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

static NETWORK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

bind_interrupts!(struct Irqs {
    EXTI15_10 => InterruptHandler<EXTI15_10>;
});

#[embassy_executor::task]
async fn driver_task(runner: w5500_macraw::Runner) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(runner: embassy_net::Runner<'static, w5500_macraw::Device>) -> ! {
    let mut runner = runner;
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = board::init(true);
    info!("W5500 MACRAW UDP echo server booting");

    let ready_led = Output::new(p.PE1, Level::Low, Speed::Low);
    let error_led = Output::new(p.PB14, Level::High, Speed::Low);
    let spi = w5500_spi::new(p.SPI1, p.PA5, p.PB5, p.PA6, p.PD14);
    // W5500 INTn is active-low. The WIZnet shield routes it through Arduino
    // D2, which is PG14/EXTI14 on the NUCLEO-H723ZG.
    let interrupt = ExtiInput::new(p.PG14, p.EXTI14, Pull::Up, Irqs);
    let (device, driver_runner) = match w5500_macraw::new(spi, interrupt).await {
        Ok(parts) => parts,
        Err(embassy_net_wiznet::InitError::InvalidChipVersion { actual, .. }) => {
            error!("W5500 returned unexpected chip version {}", actual);
            core::future::pending().await
        }
        Err(embassy_net_wiznet::InitError::SpiError(_)) => {
            error!("W5500 SPI transaction failed");
            core::future::pending().await
        }
    };
    let (stack, net_runner) = embassy_net::new(
        device,
        embassy_net::Config::dhcpv4(Default::default()),
        NETWORK_RESOURCES.init(StackResources::new()),
        0x5500_0723,
    );

    spawner.spawn(unwrap!(driver_task(driver_runner)));
    spawner.spawn(unwrap!(net_task(net_runner)));
    spawner.spawn(unwrap!(embassy_network::supervise(
        stack, ready_led, error_led
    )));
    spawner.spawn(unwrap!(udp_server::run(stack)));
    core::future::pending().await
}
