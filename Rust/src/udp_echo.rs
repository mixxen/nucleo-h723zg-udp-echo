//! UDP echo service.
//!
//! A UDP server receives independent datagrams rather than a byte stream.
//! Every received payload is sent back unchanged to the endpoint that sent it.

use defmt::{info, unwrap, warn};
use embassy_net::Stack;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use nucleo_h723zg_udp_echo::{MAX_DATAGRAM_SIZE, UDP_ECHO_PORT, echo_payload};

// Metadata entries form small fixed-size receive/transmit packet queues.
const PACKET_SLOTS: usize = 4;

#[embassy_executor::task]
pub async fn run(stack: Stack<'static>) -> ! {
    // Embassy sockets borrow caller-provided storage. This avoids `Vec`,
    // `Box`, and a global allocator. Although these arrays look local, this
    // async task lives in static task storage and never returns, so the buffers
    // remain alive as long as the socket borrows them.
    let mut rx_metadata = [PacketMetadata::EMPTY; PACKET_SLOTS];
    let mut tx_metadata = [PacketMetadata::EMPTY; PACKET_SLOTS];
    let mut rx_buffer = [0u8; MAX_DATAGRAM_SIZE];
    let mut tx_buffer = [0u8; MAX_DATAGRAM_SIZE];
    // `payload` is separate working memory into which one received datagram is
    // copied before it is sent back.
    let mut payload = [0u8; MAX_DATAGRAM_SIZE];

    // Do not bind the application socket until DHCP or the static fallback has
    // supplied a usable IPv4 address.
    stack.wait_config_up().await;

    // Each `&mut` borrow gives the socket exclusive access to its storage.
    // Rust enforces that no other code can modify those buffers concurrently.
    let mut socket = UdpSocket::new(
        stack,
        &mut rx_metadata,
        &mut rx_buffer,
        &mut tx_metadata,
        &mut tx_buffer,
    );
    // Binding reserves local port 7. Failure here is a startup/configuration
    // bug rather than a transient network condition, so stopping with a
    // diagnostic panic is appropriate.
    unwrap!(socket.bind(UDP_ECHO_PORT));

    info!("UDP echo server listening on port {}", UDP_ECHO_PORT);
    let mut echoed = 0u32;

    loop {
        // `recv_from` waits without busy-spinning. On success it returns the
        // byte count and the sender's IP address/port. The `match` handles a
        // recoverable receive error and immediately starts the next iteration.
        let (length, remote) = match socket.recv_from(&mut payload).await {
            Ok(datagram) => datagram,
            Err(error) => {
                warn!("UDP receive error: {:?}", error);
                continue;
            }
        };

        // `echo_payload` returns a borrowed slice containing exactly the
        // received bytes, not the unused remainder of the 1536-byte array.
        let Some(reply) = echo_payload(&payload, length) else {
            warn!("UDP receive length exceeds payload buffer");
            continue;
        };

        // Replying to `remote` means clients may use any source port.
        if let Err(error) = socket.send_to(reply, remote).await {
            warn!("UDP send error: {:?}", error);
            continue;
        }

        // Wrapping arithmetic avoids a panic after 2^32 successful packets;
        // this diagnostic counter simply rolls back to zero.
        echoed = echoed.wrapping_add(1);
        info!("echoed {} bytes to {:?}; total={}", length, remote, echoed);
    }
}
