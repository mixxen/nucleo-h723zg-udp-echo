//! W5500 MACRAW initialization for use as an Embassy network device.

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_net_wiznet::chip::W5500;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::mode::Async;
use embedded_hal::digital::{ErrorType, OutputPin};
use nucleo_h723zg_udp_echo::W5500_MAC_ADDRESS;
use static_cell::StaticCell;

use crate::w5500_spi;

static STATE: StaticCell<embassy_net_wiznet::State<4, 4>> = StaticCell::new();

#[unsafe(no_mangle)]
pub static W5500_INIT_STATUS: AtomicU32 = AtomicU32::new(0);

pub type Device = embassy_net_wiznet::Device<'static>;
pub type Runner =
    embassy_net_wiznet::Runner<'static, W5500, w5500_spi::Device, Interrupt, BoardReset>;
pub type Interrupt = ExtiInput<'static, Async>;

/// Initialize MACRAW mode. The shield reset is already tied to board NRST.
pub async fn new(
    spi: w5500_spi::Device,
    interrupt: Interrupt,
) -> Result<
    (Device, Runner),
    embassy_net_wiznet::InitError<<w5500_spi::Device as embedded_hal::spi::ErrorType>::Error>,
> {
    let result = embassy_net_wiznet::new::<4, 4, W5500, _, _, _>(
        W5500_MAC_ADDRESS,
        STATE.init(embassy_net_wiznet::State::new()),
        spi,
        interrupt,
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
    type Error = core::convert::Infallible;
}

impl OutputPin for BoardReset {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
