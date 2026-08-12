//! W5500 MACRAW initialization for use as an Embassy network device.

use core::convert::Infallible;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_net_wiznet::chip::W5500;
use embassy_time::{Duration, Timer};
use embedded_hal::digital::{ErrorType, OutputPin};
use embedded_hal_async::digital::Wait;
use nucleo_h723zg_udp_echo::W5500_MAC_ADDRESS;
use static_cell::StaticCell;

use crate::w5500_spi;

static STATE: StaticCell<embassy_net_wiznet::State<4, 4>> = StaticCell::new();

#[unsafe(no_mangle)]
pub static W5500_INIT_STATUS: AtomicU32 = AtomicU32::new(0);

pub type Device = embassy_net_wiznet::Device<'static>;
pub type Runner =
    embassy_net_wiznet::Runner<'static, W5500, w5500_spi::Device, PollingInterrupt, BoardReset>;

/// Initialize MACRAW mode. The shield reset is already tied to board NRST.
pub async fn new(
    spi: w5500_spi::Device,
) -> Result<
    (Device, Runner),
    embassy_net_wiznet::InitError<<w5500_spi::Device as embedded_hal::spi::ErrorType>::Error>,
> {
    let result = embassy_net_wiznet::new::<4, 4, W5500, _, _, _>(
        W5500_MAC_ADDRESS,
        STATE.init(embassy_net_wiznet::State::new()),
        spi,
        PollingInterrupt,
        BoardReset,
    )
    .await;

    match &result {
        Ok(_) => W5500_INIT_STATUS.store(1, Ordering::Relaxed),
        Err(embassy_net_wiznet::InitError::InvalidChipVersion { actual, .. }) => {
            W5500_INIT_STATUS.store(0x100 | u32::from(*actual), Ordering::Relaxed)
        }
        Err(embassy_net_wiznet::InitError::SpiError(_)) => {
            W5500_INIT_STATUS.store(0x200, Ordering::Relaxed)
        }
    }
    result
}

pub struct BoardReset;

impl ErrorType for BoardReset {
    type Error = Infallible;
}

impl OutputPin for BoardReset {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Timer polling is used because this shield revision leaves INT unpopulated.
pub struct PollingInterrupt;

impl ErrorType for PollingInterrupt {
    type Error = Infallible;
}

impl Wait for PollingInterrupt {
    async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
        wait().await
    }
    async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
        wait().await
    }
    async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
        wait().await
    }
    async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
        wait().await
    }
    async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
        wait().await
    }
}

async fn wait() -> Result<(), Infallible> {
    Timer::after(Duration::from_millis(1)).await;
    Ok(())
}
