//! SPI1 transport shared by both W5500 architectures.
//!
//! The two W5500 drivers use different embedded-hal traits: MACRAW is async,
//! while hardware offload is blocking. One device implements both interfaces
//! so pin mapping and chip-select behavior count as shared bring-up code.

use core::convert::Infallible;

use defmt::unwrap;
use embassy_stm32::Peri;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::mode::Blocking;
use embassy_stm32::peripherals::{PA5, PA6, PB5, PD14, SPI1};
use embassy_stm32::spi::mode::Master;
use embassy_stm32::spi::{self, Spi};
use embassy_stm32::time::Hertz;
use embassy_time::{Duration, block_for};
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::{ErrorType, Operation, SpiBus, SpiDevice};

pub type BoardSpi = Spi<'static, Blocking, Master>;
pub type Device = ExclusiveDevice<BoardSpi, Output<'static>>;

/// Configure the Arduino SPI pins and the W5500's D10 chip select.
pub fn new(
    spi1: Peri<'static, SPI1>,
    sck: Peri<'static, PA5>,
    mosi: Peri<'static, PB5>,
    miso: Peri<'static, PA6>,
    chip_select: Peri<'static, PD14>,
) -> Device {
    let mut config = spi::Config::default();
    config.frequency = Hertz(20_000_000);
    let bus = Spi::new_blocking(spi1, sck, mosi, miso, config);
    let chip_select = Output::new(chip_select, Level::High, Speed::VeryHigh);
    ExclusiveDevice { bus, chip_select }
}

pub struct ExclusiveDevice<SPI, CS> {
    bus: SPI,
    chip_select: CS,
}

impl<SPI, CS> ErrorType for ExclusiveDevice<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
{
    type Error = SPI::Error;
}

impl<SPI, CS> ExclusiveDevice<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
{
    fn blocking_transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> Result<(), SPI::Error> {
        unwrap!(self.chip_select.set_low());
        let result = (|| {
            for operation in operations {
                match operation {
                    Operation::Read(data) => self.bus.read(data)?,
                    Operation::Write(data) => self.bus.write(data)?,
                    Operation::Transfer(read, write) => self.bus.transfer(read, write)?,
                    Operation::TransferInPlace(data) => self.bus.transfer_in_place(data)?,
                    Operation::DelayNs(ns) => block_for(Duration::from_nanos(u64::from(*ns))),
                }
            }
            self.bus.flush()
        })();
        unwrap!(self.chip_select.set_high());
        result
    }
}

impl<SPI, CS> SpiDevice<u8> for ExclusiveDevice<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
{
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.blocking_transaction(operations)
    }
}

impl<SPI, CS> embedded_hal_async::spi::SpiDevice<u8> for ExclusiveDevice<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
{
    async fn transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        self.blocking_transaction(operations)
    }
}
