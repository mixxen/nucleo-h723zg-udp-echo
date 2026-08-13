//! Profiling telemetry over a dedicated W5500 hardware UDP socket.

use nucleo_h723zg_udp_echo::{PROFILING_MAGIC, PROFILING_WIRE_SIZE};
use w5500_dhcp::hl::io::Write;
use w5500_dhcp::hl::{Error, Udp};
use w5500_dhcp::ll::Registers;

use crate::w5500_offload::{Device, PROFILING_SOCKET};

pub fn poll(device: &mut Device) {
    // Hardware-offload firmware has no Embassy IP stack. Socket 2 provides
    // the same tiny telemetry protocol directly through the W5500 UDP engine.
    let interrupt = device.sn_ir(PROFILING_SOCKET).unwrap();
    if interrupt.any_raised() {
        device.set_sn_ir(PROFILING_SOCKET, interrupt).unwrap();
    }

    let mut request = [0u8; 16];
    match device.udp_recv_from(PROFILING_SOCKET, &mut request) {
        Ok((length, remote)) if length >= 9 && request[..8] == PROFILING_MAGIC => {
            if request[8] == b'R' {
                crate::profiling::reset();
            }
            let mut response = [0u8; PROFILING_WIRE_SIZE];
            crate::profiling::encode_snapshot(&mut response);
            let mut writer = device.udp_writer(PROFILING_SOCKET).unwrap();
            writer.write_all(&response).unwrap();
            writer.udp_send_to(&remote).unwrap();
        }
        Ok(_) | Err(Error::WouldBlock) => {}
        Err(_) => {}
    }
}
