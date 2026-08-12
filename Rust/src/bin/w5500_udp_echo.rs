//! UDP echo server for the Arduino-compatible WIZnet W5500 shield.
//!
//! The shield is a complete Ethernet MAC/PHY attached over SPI. This binary
//! asks the W5500 to expose raw Ethernet frames, then reuses Embassy's DHCP,
//! IPv4, ICMP, and UDP stack. The Nucleo's native RMII Ethernet peripheral and
//! onboard RJ45 connector are not initialized.

#![no_std]
#![no_main]

#[path = "../network.rs"]
mod network;
#[path = "../udp_echo.rs"]
mod udp_echo;

use core::convert::Infallible;
use core::sync::atomic::{AtomicU32, Ordering};

use defmt::{error, info, unwrap};
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_net_wiznet::chip::W5500;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::mode::Blocking;
use embassy_stm32::spi::Spi;
use embassy_stm32::spi::mode::Master;
use embassy_stm32::time::Hertz;
use embassy_stm32::{Config, spi};
use embassy_time::{Duration, Timer, block_for};
use embedded_hal::digital::{ErrorType as DigitalErrorType, OutputPin};
use embedded_hal::spi::{ErrorType as SpiErrorType, Operation, SpiBus};
use embedded_hal_async::digital::Wait;
use embedded_hal_async::spi::SpiDevice;
use nucleo_h723zg_udp_echo::W5500_MAC_ADDRESS;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

/// Driver queue storage: four raw frames may wait in either direction.
static W5500_STATE: StaticCell<embassy_net_wiznet::State<4, 4>> = StaticCell::new();
/// DHCP and the UDP echo socket need two stack socket slots; one is spare.
static NETWORK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

/// Debugger-readable initialization status.
///
/// `1` means success, `0x100 | version` means the SPI bus returned an
/// unexpected chip version, and `0x200` means an SPI transaction failed.
#[unsafe(no_mangle)]
static W5500_INIT_STATUS: AtomicU32 = AtomicU32::new(0);

type W5500Spi = Spi<'static, Blocking, Master>;
type W5500SpiDevice = BlockingSpiDevice<W5500Spi, Output<'static>>;
type W5500Runner =
    embassy_net_wiznet::Runner<'static, W5500, W5500SpiDevice, PollingInterrupt, BoardReset>;
type W5500NetworkDevice = embassy_net_wiznet::Device<'static>;

/// Wrap a blocking SPI bus as an async `SpiDevice`.
///
/// The W5500 transfers are short and this application has no other SPI users,
/// so a blocking transaction keeps the wiring and interrupt setup simple. The
/// async network tasks still yield whenever they wait for packets or timers.
struct BlockingSpiDevice<SPI, CS> {
    bus: SPI,
    chip_select: CS,
}

impl<SPI, CS> BlockingSpiDevice<SPI, CS> {
    fn new(bus: SPI, chip_select: CS) -> Self {
        Self { bus, chip_select }
    }
}

impl<SPI, CS> SpiErrorType for BlockingSpiDevice<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
{
    type Error = SPI::Error;
}

impl<SPI, CS> SpiDevice<u8> for BlockingSpiDevice<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
{
    async fn transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        unwrap!(self.chip_select.set_low());

        let result = (|| {
            for operation in operations {
                match operation {
                    Operation::Read(data) => self.bus.read(data)?,
                    Operation::Write(data) => self.bus.write(data)?,
                    Operation::Transfer(read, write) => self.bus.transfer(read, write)?,
                    Operation::TransferInPlace(data) => self.bus.transfer_in_place(data)?,
                    Operation::DelayNs(nanoseconds) => {
                        block_for(Duration::from_nanos(u64::from(*nanoseconds)))
                    }
                }
            }
            self.bus.flush()
        })();

        // Chip select must be released even when an SPI operation failed.
        unwrap!(self.chip_select.set_high());
        result
    }
}

/// The shield's reset line is connected to the Nucleo's NRST header pin.
///
/// It has already been reset when this firmware starts. The W5500 driver also
/// performs a software reset over SPI, so no separately controlled GPIO is
/// required here.
struct BoardReset;

impl DigitalErrorType for BoardReset {
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

/// Periodic wake-up used when the shield's optional INT trace is unpopulated.
struct PollingInterrupt;

impl DigitalErrorType for PollingInterrupt {
    type Error = Infallible;
}

impl Wait for PollingInterrupt {
    async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
        Timer::after_millis(1).await;
        Ok(())
    }

    async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
        Timer::after_millis(1).await;
        Ok(())
    }

    async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
        Timer::after_millis(1).await;
        Ok(())
    }

    async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
        Timer::after_millis(1).await;
        Ok(())
    }

    async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
        Timer::after_millis(1).await;
        Ok(())
    }
}

#[embassy_executor::task]
async fn w5500_task(runner: W5500Runner) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(runner: embassy_net::Runner<'static, W5500NetworkDevice>) -> ! {
    let mut runner = runner;
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
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
            // SPI1 uses PLL1-Q as its kernel clock. 800 MHz / 4 = 200 MHz.
            divq: Some(PllDiv::DIV4),
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

    // MCUboot deliberately chain-loads with PRIMASK set. Embassy has now
    // installed its timer and interrupt configuration, so enable interrupts.
    unsafe {
        cortex_m::interrupt::enable();
    }

    info!("W5500 UDP echo server booting");

    // Reuse the same LEDs as the native Ethernet application. No RMII pins or
    // the STM32 Ethernet peripheral are configured by this binary.
    let ready_led = Output::new(peripherals.PE1, Level::Low, Speed::Low);
    let error_led = Output::new(peripherals.PB14, Level::High, Speed::Low);

    // Arduino SPI header mapping on NUCLEO-H723ZG:
    // D13 = PA5/SPI1_SCK, D12 = PA6/SPI1_MISO,
    // D11 = PB5/SPI1_MOSI, D10 = PD14/W5500 chip select.
    let mut spi_config = spi::Config::default();
    spi_config.frequency = Hertz(20_000_000);
    let spi = Spi::new_blocking(
        peripherals.SPI1,
        peripherals.PA5,
        peripherals.PB5,
        peripherals.PA6,
        spi_config,
    );
    let chip_select = Output::new(peripherals.PD14, Level::High, Speed::VeryHigh);
    let spi_device = BlockingSpiDevice::new(spi, chip_select);

    let initialized = embassy_net_wiznet::new::<4, 4, W5500, _, _, _>(
        W5500_MAC_ADDRESS,
        W5500_STATE.init(embassy_net_wiznet::State::new()),
        spi_device,
        PollingInterrupt,
        BoardReset,
    )
    .await;
    let (device, w5500_runner) = match initialized {
        Ok(parts) => {
            W5500_INIT_STATUS.store(1, Ordering::Relaxed);
            parts
        }
        Err(embassy_net_wiznet::InitError::InvalidChipVersion { actual, .. }) => {
            W5500_INIT_STATUS.store(0x100 | u32::from(actual), Ordering::Relaxed);
            error!("W5500 returned unexpected chip version {}", actual);
            core::future::pending().await
        }
        Err(embassy_net_wiznet::InitError::SpiError(_)) => {
            W5500_INIT_STATUS.store(0x200, Ordering::Relaxed);
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

    spawner.spawn(unwrap!(w5500_task(w5500_runner)));
    spawner.spawn(unwrap!(net_task(net_runner)));
    spawner.spawn(unwrap!(network::supervise(stack, ready_led, error_led)));
    spawner.spawn(unwrap!(udp_echo::run(stack)));

    core::future::pending().await
}
