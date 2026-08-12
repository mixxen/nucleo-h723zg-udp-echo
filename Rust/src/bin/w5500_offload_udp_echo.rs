//! UDP echo server using the W5500's hardwired TCP/IP offload engine.
//!
//! Unlike `w5500_udp_echo.rs`, this binary does not move raw Ethernet frames
//! over SPI and does not use `embassy-net`. Socket 0 runs DHCP inside the
//! W5500, socket 1 receives decoded UDP payloads, and the STM32 sends those
//! payloads straight back to the reported source address.

#![no_std]
#![no_main]

use core::convert::Infallible;

use defmt::{error, info, unwrap, warn};
use embassy_executor::Spawner;
use embassy_stm32::Config;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::spi::{self, Spi};
use embassy_stm32::time::Hertz;
use embassy_time::{Duration, Instant, Timer, block_for};
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::{ErrorType, Operation, SpiBus, SpiDevice};
use nucleo_h723zg_udp_echo::{MAX_DATAGRAM_SIZE, UDP_ECHO_PORT, W5500_MAC_ADDRESS};
use w5500_dhcp::hl::io::Write;
use w5500_dhcp::hl::{Common, Error as SocketError, Udp};
use w5500_dhcp::ll::eh1::vdm::W5500;
use w5500_dhcp::ll::net::Eui48Addr;
use w5500_dhcp::ll::{Mode, Registers, Sn};
use w5500_dhcp::{Client, Hostname};
use {defmt_rtt as _, panic_probe as _};

const DHCP_SOCKET: Sn = Sn::Sn0;
const ECHO_SOCKET: Sn = Sn::Sn1;
const HOSTNAME: Hostname<'static> = Hostname::new_unwrapped("nucleo-w5500");
const EXPECTED_CHIP_VERSION: u8 = 0x04;

/// Give the W5500 exclusive ownership of SPI1 and its active-low chip select.
struct ExclusiveSpiDevice<SPI, CS> {
    bus: SPI,
    chip_select: CS,
}

impl<SPI, CS> ExclusiveSpiDevice<SPI, CS> {
    fn new(bus: SPI, chip_select: CS) -> Self {
        Self { bus, chip_select }
    }
}

impl<SPI, CS> ErrorType for ExclusiveSpiDevice<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
{
    type Error = SPI::Error;
}

impl<SPI, CS> SpiDevice<u8> for ExclusiveSpiDevice<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin<Error = Infallible>,
{
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
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

        // Always release chip select, including after a failed SPI operation.
        unwrap!(self.chip_select.set_high());
        result
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
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
    // MCUboot enters the application with global interrupts masked.
    unsafe {
        cortex_m::interrupt::enable();
    }

    let mut ready_led = Output::new(peripherals.PE1, Level::Low, Speed::Low);
    let mut error_led = Output::new(peripherals.PB14, Level::High, Speed::Low);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = Hertz(20_000_000);
    let spi = Spi::new_blocking(
        peripherals.SPI1,
        peripherals.PA5, // Arduino D13: SCK
        peripherals.PB5, // Arduino D11: MOSI
        peripherals.PA6, // Arduino D12: MISO
        spi_config,
    );
    let chip_select = Output::new(peripherals.PD14, Level::High, Speed::VeryHigh);
    let mut w5500 = W5500::new(ExclusiveSpiDevice::new(spi, chip_select));

    // Software reset also closes all eight sockets and restores their default
    // 2 KiB transmit and receive buffers.
    unwrap!(w5500.set_mr(Mode::DEFAULT.rst()));
    Timer::after_millis(100).await;

    let version = unwrap!(w5500.version());
    if version != EXPECTED_CHIP_VERSION {
        error!("expected W5500 version 4, received {}", version);
        core::future::pending().await
    }

    let mac = Eui48Addr::new(
        W5500_MAC_ADDRESS[0],
        W5500_MAC_ADDRESS[1],
        W5500_MAC_ADDRESS[2],
        W5500_MAC_ADDRESS[3],
        W5500_MAC_ADDRESS[4],
        W5500_MAC_ADDRESS[5],
    );
    unwrap!(w5500.set_shar(&mac));

    // The seed varies DHCP transaction IDs after every reset without needing
    // a cryptographic RNG. It is not used for a security decision.
    let seed = Instant::now().as_ticks() ^ 0x5500_0723_A55A_5AA5;
    let mut dhcp = Client::new(DHCP_SOCKET, seed, mac, HOSTNAME);
    unwrap!(dhcp.setup_socket(&mut w5500));

    info!("W5500 hardware-offload UDP echo server booting");
    let mut echo_is_bound = false;
    let mut payload = [0_u8; MAX_DATAGRAM_SIZE];
    let mut echo_count = 0_u32;

    loop {
        let monotonic_secs = Instant::now().as_secs() as u32;
        if let Err(cause) = dhcp.process(&mut w5500, monotonic_secs) {
            warn!("DHCP processing failed: {:?}", cause);
        }

        if dhcp.has_lease() && !echo_is_bound {
            unwrap!(w5500.udp_bind(ECHO_SOCKET, UDP_ECHO_PORT));
            echo_is_bound = true;
            ready_led.set_high();
            error_led.set_low();
            info!(
                "DHCP address: {}; UDP echo listening on port {}",
                unwrap!(dhcp.leased_ip()),
                UDP_ECHO_PORT
            );
        } else if !dhcp.has_lease() && echo_is_bound {
            unwrap!(w5500.close(ECHO_SOCKET));
            echo_is_bound = false;
            ready_led.set_low();
            error_led.set_high();
            warn!("DHCP lease lost; UDP echo stopped");
        }

        if echo_is_bound {
            match w5500.udp_recv_from(ECHO_SOCKET, &mut payload) {
                Ok((length, source)) => {
                    // The W5500 hardwired socket engine does not complete a
                    // SEND command whose payload length is zero. Discard that
                    // legal-but-unusual UDP datagram so the socket remains
                    // usable for later traffic.
                    if length == 0 {
                        warn!("W5500 offload cannot echo a zero-byte UDP payload");
                        continue;
                    }

                    let mut reply = unwrap!(w5500.udp_writer(ECHO_SOCKET));
                    unwrap!(reply.write_all(&payload[..usize::from(length)]));
                    unwrap!(reply.udp_send_to(&source));
                    echo_count = echo_count.wrapping_add(1);
                    info!(
                        "echoed {} decoded UDP payload bytes to {}; total={}",
                        length, source, echo_count
                    );
                }
                Err(SocketError::WouldBlock) => {}
                Err(SocketError::OutOfMemory) => {
                    warn!("received UDP datagram exceeds the application buffer");
                }
                Err(SocketError::UnexpectedEof) => warn!("truncated UDP datagram"),
                Err(SocketError::Other(_)) => warn!("W5500 UDP receive failed"),
            }
        }

        Timer::after_millis(1).await;
    }
}
