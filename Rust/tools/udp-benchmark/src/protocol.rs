//! On-wire benchmark packet format.
//!
//! The embedded application remains an ordinary echo server. Only this host
//! tool knows that the echoed bytes contain a run ID, sequence, timestamp, and
//! deterministic validation pattern.

pub const HEADER_LEN: usize = 32;
const MAGIC: [u8; 4] = *b"NUBE";
const VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    pub run_id: u64,
    pub sequence: u64,
    pub sent_ns: u64,
}

/// Create one self-validating datagram of exactly `size` bytes.
pub fn encode(size: usize, header: Header) -> Result<Vec<u8>, String> {
    if size < HEADER_LEN {
        return Err(format!(
            "instrumented packets require at least {HEADER_LEN} bytes"
        ));
    }

    let mut packet = vec![0; size];
    packet[0..4].copy_from_slice(&MAGIC);
    packet[4] = VERSION;
    packet[5] = 0;
    packet[6..8].copy_from_slice(&(HEADER_LEN as u16).to_be_bytes());
    packet[8..16].copy_from_slice(&header.run_id.to_be_bytes());
    packet[16..24].copy_from_slice(&header.sequence.to_be_bytes());
    packet[24..32].copy_from_slice(&header.sent_ns.to_be_bytes());

    for (index, byte) in packet[HEADER_LEN..].iter_mut().enumerate() {
        *byte = pattern_byte(header.run_id, header.sequence, index);
    }
    Ok(packet)
}

/// Decode and validate the fixed header without yet checking payload bytes.
pub fn decode(packet: &[u8]) -> Result<Header, &'static str> {
    if packet.len() < HEADER_LEN {
        return Err("packet is shorter than the benchmark header");
    }
    if packet[0..4] != MAGIC {
        return Err("wrong benchmark magic");
    }
    if packet[4] != VERSION {
        return Err("unsupported benchmark protocol version");
    }
    if u16::from_be_bytes([packet[6], packet[7]]) as usize != HEADER_LEN {
        return Err("wrong benchmark header length");
    }

    Ok(Header {
        run_id: u64::from_be_bytes(packet[8..16].try_into().expect("fixed slice")),
        sequence: u64::from_be_bytes(packet[16..24].try_into().expect("fixed slice")),
        sent_ns: u64::from_be_bytes(packet[24..32].try_into().expect("fixed slice")),
    })
}

/// Verify every byte after the header, detecting payload corruption.
pub fn validate_payload(packet: &[u8], header: Header) -> bool {
    packet[HEADER_LEN..]
        .iter()
        .enumerate()
        .all(|(index, byte)| *byte == pattern_byte(header.run_id, header.sequence, index))
}

/// Generate reproducible nontrivial data without pulling in a random crate.
fn pattern_byte(run_id: u64, sequence: u64, index: usize) -> u8 {
    let mut value =
        run_id ^ sequence.rotate_left(17) ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    (value ^ (value >> 31)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_round_trips_and_validates() {
        let expected = Header {
            run_id: 0x1122_3344_5566_7788,
            sequence: 42,
            sent_ns: 987_654_321,
        };
        let packet = encode(1472, expected).unwrap();

        assert_eq!(decode(&packet), Ok(expected));
        assert!(validate_payload(&packet, expected));
    }

    #[test]
    fn corruption_is_detected() {
        let header = Header {
            run_id: 1,
            sequence: 2,
            sent_ns: 3,
        };
        let mut packet = encode(64, header).unwrap();
        packet[47] ^= 0x80;

        assert!(!validate_payload(&packet, header));
    }

    #[test]
    fn short_instrumented_packet_is_rejected() {
        assert!(
            encode(
                HEADER_LEN - 1,
                Header {
                    run_id: 0,
                    sequence: 0,
                    sent_ns: 0
                }
            )
            .is_err()
        );
    }
}
