//! UDP telemetry endpoint enabled only in profiling benchmark images.

use embassy_net::Stack;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use nucleo_h723zg_udp_echo::{PROFILING_MAGIC, PROFILING_PORT, PROFILING_WIRE_SIZE};

#[embassy_executor::task]
pub async fn run(stack: Stack<'static>) -> ! {
    // One packet slot is enough: this control endpoint receives only a reset
    // or snapshot request before/after a benchmark trial, never stream data.
    let mut rx_metadata = [PacketMetadata::EMPTY; 1];
    let mut tx_metadata = [PacketMetadata::EMPTY; 1];
    let mut rx_buffer = [0u8; 16];
    let mut tx_buffer = [0u8; PROFILING_WIRE_SIZE];
    let mut request = [0u8; 16];
    let mut response = [0u8; PROFILING_WIRE_SIZE];

    stack.wait_config_up().await;
    let mut socket = UdpSocket::new(
        stack,
        &mut rx_metadata,
        &mut rx_buffer,
        &mut tx_metadata,
        &mut tx_buffer,
    );
    socket.bind(PROFILING_PORT).unwrap();

    loop {
        let Ok((length, remote)) = socket.recv_from(&mut request).await else {
            continue;
        };
        if length < 9 || request[..8] != PROFILING_MAGIC {
            continue;
        }
        // The nine-byte request is "STRMPRF1" followed by R (reset) or S
        // (snapshot). Both operations reply with the same fixed-size record.
        if request[8] == b'R' {
            crate::profiling::reset();
        }
        crate::profiling::encode_snapshot(&mut response);
        let _ = socket.send_to(&response, remote).await;
    }
}
