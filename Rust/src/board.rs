//! MCU setup shared by every firmware variant.
//!
//! Board clocks and the MCUboot interrupt handoff are platform costs rather
//! than Ethernet-device or UDP-server costs. Keeping them here makes that
//! boundary explicit for the trade study.

use embassy_stm32::{Config, Peripherals};

/// Initialize the STM32H723 and return its peripheral tokens.
///
/// W5500 variants need a kernel clock for SPI1. Normal firmware derives it
/// from PLL1-Q; the performance build uses an independent 80 MHz PLL2-P so
/// SPI1 can run at 40 MHz while the CPU remains at 520 MHz. Normal firmware
/// remains at 400/200 MHz CPU/AHB.
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
            divq: (needs_spi1_clock && !performance_clock).then_some(PllDiv::Div4),
            divr: None,
        });
        if needs_spi1_clock && performance_clock {
            // 64 MHz HSI / 4 * 40 / 8 = 80 MHz PLL2-P. SPI1's /2 baud
            // divider then produces 40 MHz independently of the 520 MHz
            // system PLL. The shield did not pass its DHCP test at 50 MHz
            // when the MCU was also running at the performance clock.
            config.rcc.pll2 = Some(Pll {
                source: PllSource::Hsi,
                prediv: PllPreDiv::Div4,
                mul: PllMul::Mul40,
                divp: Some(PllDiv::Div8),
                divq: None,
                divr: None,
            });
            config.rcc.mux.spi123sel = mux::Saisel::Pll2P;
        }
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
