//! Hardware-independent rules shared by the firmware and host unit tests.
//!
//! Keeping this module free of STM32 and Embassy types lets `cargo test` build
//! it for Windows or Linux with Rust's normal test harness. The embedded binary
//! consumes the same constants and payload-boundary function, so the tests
//! exercise production code rather than a separate simulation.

#![no_std]

/// Locally administered, unicast Ethernet address used by this example.
pub const MAC_ADDRESS: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];

/// UDP port assigned to the traditional echo protocol.
pub const UDP_ECHO_PORT: u16 = 7;

/// Storage reserved for one received UDP datagram.
pub const MAX_DATAGRAM_SIZE: usize = 1536;

/// IPv4 configuration used when DHCP does not respond.
pub const FALLBACK_ADDRESS: [u8; 4] = [192, 168, 0, 10];
pub const FALLBACK_GATEWAY: [u8; 4] = [192, 168, 0, 1];
pub const FALLBACK_PREFIX_LENGTH: u8 = 24;

/// Return exactly the initialized portion of a receive buffer.
///
/// Embassy guarantees that a successful receive length fits its destination
/// buffer. Returning `Option` keeps this boundary checked if the function is
/// reused with another packet source in the future.
pub fn echo_payload(buffer: &[u8], received_length: usize) -> Option<&[u8]> {
    buffer.get(..received_length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_datagram_produces_an_empty_reply() {
        assert_eq!(echo_payload(&[0xaa; 8], 0), Some(&[][..]));
    }

    #[test]
    fn reply_contains_only_received_bytes() {
        let buffer = [1, 2, 3, 4, 0, 0];
        assert_eq!(echo_payload(&buffer, 4), Some(&[1, 2, 3, 4][..]));
    }

    #[test]
    fn full_receive_buffer_is_valid() {
        let buffer = [0x5a; MAX_DATAGRAM_SIZE];
        assert_eq!(echo_payload(&buffer, MAX_DATAGRAM_SIZE), Some(&buffer[..]));
    }

    #[test]
    fn length_beyond_buffer_is_rejected() {
        assert_eq!(echo_payload(&[1, 2, 3], 4), None);
    }

    #[test]
    fn mac_address_is_local_and_unicast() {
        assert_eq!(MAC_ADDRESS[0] & 0b0000_0010, 0b0000_0010);
        assert_eq!(MAC_ADDRESS[0] & 0b0000_0001, 0);
    }

    #[test]
    fn fallback_gateway_is_in_the_same_24_bit_subnet() {
        assert_eq!(FALLBACK_PREFIX_LENGTH, 24);
        assert_eq!(FALLBACK_ADDRESS[..3], FALLBACK_GATEWAY[..3]);
        assert_ne!(FALLBACK_ADDRESS, FALLBACK_GATEWAY);
    }

    #[test]
    fn buffer_accepts_a_standard_unfragmented_udp_payload() {
        const ETHERNET_MTU: usize = 1500;
        const IPV4_HEADER: usize = 20;
        const UDP_HEADER: usize = 8;

        assert!(MAX_DATAGRAM_SIZE >= ETHERNET_MTU - IPV4_HEADER - UDP_HEADER);
    }
}
