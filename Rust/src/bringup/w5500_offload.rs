//! W5500 hardwired TCP/IP initialization, DHCP, and link supervision.
//!
//! This module ends at a configured hardware UDP socket. The echo algorithm
//! is intentionally kept in `servers/w5500_offload_udp_echo.rs` so its SLOC
//! can be measured separately from network bring-up.

use defmt::{info, unwrap, warn};
use embassy_stm32::gpio::Output;
use embassy_time::{Instant, Timer};
#[cfg(feature = "profiling")]
use nucleo_h723zg_udp_echo::PROFILING_PORT;
use nucleo_h723zg_udp_echo::{UDP_ECHO_PORT, W5500_MAC_ADDRESS};
use w5500_dhcp::hl::{Common, Udp};
use w5500_dhcp::ll::eh1::vdm::W5500;
use w5500_dhcp::ll::net::Eui48Addr;
use w5500_dhcp::ll::{Mode, Registers, Sn, SocketInterruptMask};
use w5500_dhcp::{Client, Hostname};

use crate::w5500_spi;

pub const DHCP_SOCKET: Sn = Sn::Sn0;
pub const ECHO_SOCKET: Sn = Sn::Sn1;
#[cfg(feature = "profiling")]
pub const PROFILING_SOCKET: Sn = Sn::Sn2;
const HOSTNAME: Hostname<'static> = Hostname::new_unwrapped("nucleo-w5500");
const EXPECTED_CHIP_VERSION: u8 = 0x04;

pub type Device = W5500<w5500_spi::Device>;

pub enum InitError<E> {
    Spi(E),
    InvalidVersion(u8),
}

/// Owns all state required to keep the hardware network configured.
pub struct Network {
    device: Device,
    dhcp: Client<'static>,
    echo_is_bound: bool,
    ready_led: Output<'static>,
    error_led: Output<'static>,
}

impl Network {
    pub async fn new(
        spi: w5500_spi::Device,
        ready_led: Output<'static>,
        error_led: Output<'static>,
    ) -> Result<Self, InitError<<w5500_spi::Device as embedded_hal::spi::ErrorType>::Error>> {
        let mut device = W5500::new(spi);
        device.set_mr(Mode::DEFAULT.rst()).map_err(InitError::Spi)?;
        Timer::after_millis(100).await;

        let version = device.version().map_err(InitError::Spi)?;
        if version != EXPECTED_CHIP_VERSION {
            return Err(InitError::InvalidVersion(version));
        }

        let mac = Eui48Addr::new(
            W5500_MAC_ADDRESS[0],
            W5500_MAC_ADDRESS[1],
            W5500_MAC_ADDRESS[2],
            W5500_MAC_ADDRESS[3],
            W5500_MAC_ADDRESS[4],
            W5500_MAC_ADDRESS[5],
        );
        device.set_shar(&mac).map_err(InitError::Spi)?;
        let seed = Instant::now().as_ticks() ^ 0x5500_0723_A55A_5AA5;
        let dhcp = Client::new(DHCP_SOCKET, seed, mac, HOSTNAME);
        dhcp.setup_socket(&mut device).map_err(InitError::Spi)?;

        Ok(Self {
            device,
            dhcp,
            echo_is_bound: false,
            ready_led,
            error_led,
        })
    }

    /// Service DHCP only when its socket interrupted or its timer is due.
    pub fn poll(&mut self, maintenance_due: bool) -> bool {
        let socket_interrupts = unwrap!(self.device.sir());
        if maintenance_due || socket_interrupts & DHCP_SOCKET.bitmask() != 0 {
            let seconds = Instant::now().as_secs() as u32;
            if let Err(cause) = self.dhcp.process(&mut self.device, seconds) {
                warn!("DHCP processing failed: {:?}", cause);
            }
        }

        if self.dhcp.has_lease() && !self.echo_is_bound {
            unwrap!(self.device.udp_bind(ECHO_SOCKET, UDP_ECHO_PORT));
            #[cfg(feature = "profiling")]
            unwrap!(self.device.udp_bind(PROFILING_SOCKET, PROFILING_PORT));
            let sockets = unwrap!(self.device.simr());
            #[cfg(not(feature = "profiling"))]
            unwrap!(self.device.set_simr(sockets | ECHO_SOCKET.bitmask()));
            #[cfg(feature = "profiling")]
            unwrap!(
                self.device
                    .set_simr(sockets | ECHO_SOCKET.bitmask() | PROFILING_SOCKET.bitmask())
            );
            unwrap!(
                self.device
                    .set_sn_imr(ECHO_SOCKET, SocketInterruptMask::ALL_MASKED.unmask_recv())
            );
            #[cfg(feature = "profiling")]
            unwrap!(self.device.set_sn_imr(
                PROFILING_SOCKET,
                SocketInterruptMask::ALL_MASKED.unmask_recv()
            ));
            self.echo_is_bound = true;
            self.ready_led.set_high();
            self.error_led.set_low();
            info!(
                "DHCP address: {}; UDP echo listening on port {}",
                unwrap!(self.dhcp.leased_ip()),
                UDP_ECHO_PORT
            );
        } else if !self.dhcp.has_lease() && self.echo_is_bound {
            let sockets = unwrap!(self.device.simr());
            unwrap!(self.device.set_simr(sockets & !ECHO_SOCKET.bitmask()));
            unwrap!(self.device.close(ECHO_SOCKET));
            self.echo_is_bound = false;
            self.ready_led.set_low();
            self.error_led.set_high();
            warn!("DHCP lease lost; UDP echo stopped");
        }

        self.echo_is_bound
    }

    pub fn device_mut(&mut self) -> &mut Device {
        &mut self.device
    }
}
