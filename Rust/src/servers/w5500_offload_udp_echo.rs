//! UDP echo algorithm for a W5500 hardwired socket.
//!
//! Hardware setup, DHCP, LEDs, and socket binding deliberately live outside
//! this file. Its SLOC is therefore the offload server metric.

#[cfg(not(feature = "benchmark"))]
use defmt::info;
use defmt::{unwrap, warn};
use nucleo_h723zg_udp_echo::MAX_DATAGRAM_SIZE;
use w5500_dhcp::hl::io::Write;
use w5500_dhcp::hl::{Error, Udp};
use w5500_dhcp::ll::Registers;

use crate::w5500_offload::{Device, ECHO_SOCKET};

pub struct Server {
    payload: [u8; MAX_DATAGRAM_SIZE],
    #[cfg(not(feature = "benchmark"))]
    echo_count: u32,
}

impl Server {
    pub const fn new() -> Self {
        Self {
            payload: [0; MAX_DATAGRAM_SIZE],
            #[cfg(not(feature = "benchmark"))]
            echo_count: 0,
        }
    }

    /// Clear the active-low interrupt and drain every queued datagram.
    ///
    /// Clearing before draining avoids losing a packet that arrives between
    /// the last receive and the clear operation: a later arrival will assert
    /// INTn again and wake the executor.
    pub fn poll(&mut self, device: &mut Device) {
        let interrupt = unwrap!(device.sn_ir(ECHO_SOCKET));
        if interrupt.any_raised() {
            unwrap!(device.set_sn_ir(ECHO_SOCKET, interrupt));
        }

        loop {
            match device.udp_recv_from(ECHO_SOCKET, &mut self.payload) {
                Ok((length, source)) => {
                    // The W5500 never completes SEND for an empty payload. Drop
                    // it so this hardware limitation cannot wedge the socket.
                    if length == 0 {
                        warn!("W5500 offload cannot echo a zero-byte UDP payload");
                        continue;
                    }

                    let mut reply = unwrap!(device.udp_writer(ECHO_SOCKET));
                    unwrap!(reply.write_all(&self.payload[..usize::from(length)]));
                    unwrap!(reply.udp_send_to(&source));
                    #[cfg(not(feature = "benchmark"))]
                    {
                        self.echo_count = self.echo_count.wrapping_add(1);
                        info!(
                            "echoed {} decoded UDP payload bytes to {}; total={}",
                            length, source, self.echo_count
                        );
                    }
                }
                Err(Error::WouldBlock) => break,
                Err(Error::OutOfMemory) => warn!("UDP datagram exceeds application buffer"),
                Err(Error::UnexpectedEof) => warn!("truncated UDP datagram"),
                Err(Error::Other(_)) => warn!("W5500 UDP receive failed"),
            }
        }
    }
}
