//! MCU setup shared by every firmware variant.
//!
//! Board clocks and the MCUboot interrupt handoff are platform costs rather
//! than Ethernet-device or UDP-server costs. Keeping them here makes that
//! boundary explicit for the trade study.

use embassy_stm32::{Config, Peripherals};

/// Initialize the STM32H723 at 400 MHz and return its peripheral tokens.
///
/// W5500 variants request PLL1-Q because SPI1 uses it as a 200 MHz kernel
/// clock. The native RMII application does not need that clock output.
pub fn init(needs_spi1_clock: bool) -> Peripherals {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;

        config.rcc.hsi = Some(HSIPrescaler::DIV1);
        config.rcc.csi = true;
        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSI,
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL50,
            divp: Some(PllDiv::DIV2),
            divq: needs_spi1_clock.then_some(PllDiv::DIV4),
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV2;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.apb3_pre = APBPrescaler::DIV2;
        config.rcc.apb4_pre = APBPrescaler::DIV2;
        config.rcc.voltage_scale = VoltageScale::Scale1;
    }

    let peripherals = embassy_stm32::init(config);

    // MCUboot chain-loads with PRIMASK set. Embassy has now installed the
    // application's clocks, timer, vector table, and interrupt handlers.
    unsafe {
        cortex_m::interrupt::enable();
    }

    #[cfg(feature = "profiling")]
    crate::profiling::init();

    peripherals
}
