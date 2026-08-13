//! MCU setup shared by every firmware variant.
//!
//! Board clocks and the MCUboot interrupt handoff are platform costs rather
//! than Ethernet-device or UDP-server costs. Keeping them here makes that
//! boundary explicit for the trade study.

use embassy_stm32::{Config, Peripherals};

/// Initialize the STM32H723 and return its peripheral tokens.
///
/// W5500 variants request PLL1-Q because SPI1 uses it as a 200 MHz kernel
/// clock. The native RMII performance build instead matches the C sample's
/// 520 MHz CPU and 260 MHz AHB clocks; normal firmware remains at 400/200 MHz.
pub fn init(needs_spi1_clock: bool) -> Peripherals {
    let mut config = Config::default();
    {
        use embassy_stm32::rcc::*;

        let performance_clock = cfg!(feature = "performance");

        config.rcc.hsi = Some(HSIPrescaler::Div1);
        config.rcc.csi = true;
        config.rcc.pll1 = Some(Pll {
            source: PllSource::Hsi,
            prediv: if performance_clock {
                PllPreDiv::Div8
            } else {
                PllPreDiv::Div4
            },
            mul: if performance_clock {
                PllMul::Mul65
            } else {
                PllMul::Mul50
            },
            divp: Some(if performance_clock {
                PllDiv::Div1
            } else {
                PllDiv::Div2
            }),
            // At 520 MHz, DIV4 would clock SPI1 at 65 MHz after its minimum
            // /2 baud divider. That proved unreliable through the stacked
            // Arduino headers, so keep performance-mode SPI1 near 32.5 MHz.
            divq: needs_spi1_clock.then_some(if performance_clock {
                PllDiv::Div8
            } else {
                PllDiv::Div4
            }),
            divr: None,
        });
        config.rcc.sys = Sysclk::Pll1P;
        config.rcc.ahb_pre = AHBPrescaler::Div2;
        config.rcc.apb1_pre = APBPrescaler::Div2;
        config.rcc.apb2_pre = APBPrescaler::Div2;
        config.rcc.apb3_pre = APBPrescaler::Div2;
        config.rcc.apb4_pre = APBPrescaler::Div2;
        config.rcc.voltage_scale = if performance_clock {
            VoltageScale::Scale0
        } else {
            VoltageScale::Scale1
        };
    }

    let peripherals = embassy_stm32::init(config);

    // The Cortex-M7 does not enable its instruction cache after reset, and
    // Embassy intentionally leaves that policy to the application. Fetching
    // every instruction directly from flash is needlessly expensive in the
    // packet-processing hot path, so match the STM32Cube C startup here.
    //
    // Do not also enable the data cache: Ethernet DMA reads and writes the
    // packet buffers behind the CPU's back, and this Embassy driver version
    // does not perform the cache maintenance needed to keep them coherent.
    let mut core = unsafe { cortex_m::Peripherals::steal() };
    core.SCB.enable_icache();

    // MCUboot chain-loads with PRIMASK set. Embassy has now installed the
    // application's clocks, timer, vector table, and interrupt handlers.
    unsafe {
        cortex_m::interrupt::enable();
    }

    #[cfg(feature = "profiling")]
    crate::profiling::init();

    peripherals
}
