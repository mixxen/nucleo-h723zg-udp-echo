//! Fixed-per-run UDP framing. Verify integrity before constructing a message.
use crate::{CodecError, Format, MAX_BODY, model::Envelope};
pub const TRAILER_BYTES: usize = 8;
pub const MAX_DATAGRAM: usize = MAX_BODY + TRAILER_BYTES;
const CRC32C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
const HEX: &[u8; 16] = b"0123456789ABCDEF";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrcMode {
    Off,
    On,
}
impl CrcMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "on" => Some(Self::On),
            _ => None,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
        }
    }
}
pub fn checksum(body: &[u8]) -> u32 {
    CRC32C.checksum(body)
}

/// Append a trailer to the already encoded body in-place. This deliberately
/// accepts arbitrary bodies for independent framing tests and fault fixtures.
pub fn seal(mode: CrcMode, buffer: &mut [u8], body_len: usize) -> Result<usize, CodecError> {
    let extra = if mode == CrcMode::On {
        TRAILER_BYTES
    } else {
        0
    };
    if body_len > MAX_BODY || buffer.len() < body_len + extra {
        return Err(CodecError::Size);
    }
    if mode == CrcMode::On {
        let value = checksum(&buffer[..body_len]);
        for index in 0..TRAILER_BYTES {
            buffer[body_len + index] = HEX[((value >> (28 - index * 4)) & 15) as usize];
        }
    }
    Ok(body_len + extra)
}
/// Size rejection precedes CRC work; CRC rejection precedes decode/validation.
/// An empty, correctly framed body passes integrity and then fails decoding.
pub fn open(mode: CrcMode, datagram: &[u8]) -> Result<&[u8], CodecError> {
    let extra = if mode == CrcMode::On {
        TRAILER_BYTES
    } else {
        0
    };
    if datagram.len() > MAX_BODY + extra {
        return Err(CodecError::Size);
    }
    if mode == CrcMode::Off {
        return Ok(datagram);
    }
    let body_len = datagram
        .len()
        .checked_sub(TRAILER_BYTES)
        .ok_or(CodecError::Crc)?;
    let (body, trailer) = datagram.split_at(body_len);
    let mut expected = 0u32;
    for &digit in trailer {
        expected = (expected << 4)
            | match digit {
                b'0'..=b'9' => (digit - b'0') as u32,
                b'A'..=b'F' => (digit - b'A' + 10) as u32,
                _ => return Err(CodecError::Crc),
            };
    }
    if checksum(body) != expected {
        return Err(CodecError::Crc);
    }
    Ok(body)
}
pub fn encode(
    format: Format,
    mode: CrcMode,
    message: &Envelope,
    out: &mut [u8],
) -> Result<usize, CodecError> {
    let body_len = crate::encode(format, message, out)?;
    seal(mode, out, body_len)
}
pub fn parse(format: Format, mode: CrcMode, bytes: &[u8]) -> Result<Envelope, CodecError> {
    crate::parse(format, open(mode, bytes)?)
}
pub fn decode(format: Format, mode: CrcMode, bytes: &[u8]) -> Result<Envelope, CodecError> {
    crate::decode(format, open(mode, bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_check_and_rfc3720_vectors() {
        // RFC 3720 Appendix B.4 presents digest bytes least-significant first.
        // Our trailer prints the conventional numeric value most-significant first.
        assert_eq!(checksum(b""), 0);
        assert_eq!(checksum(b"123456789"), 0xE3069283);
        assert_eq!(checksum(&[0; 32]), 0x8A9136AA);
        assert_eq!(checksum(&[0xff; 32]), 0x62A8AB43);
        assert_eq!(
            checksum(&core::array::from_fn::<_, 32, _>(|i| i as u8)),
            0x46DD794E
        );
        assert_eq!(
            checksum(&core::array::from_fn::<_, 32, _>(|i| 31 - i as u8)),
            0x113FDB5C
        );
        assert_eq!(
            open(CrcMode::On, b"123456789E3069283"),
            Ok(&b"123456789"[..])
        );
    }
    #[test]
    fn framing_limits_and_error_order() {
        let mut bytes = [b'x'; MAX_DATAGRAM];
        assert_eq!(seal(CrcMode::On, &mut bytes, MAX_BODY), Ok(MAX_DATAGRAM));
        assert_eq!(open(CrcMode::On, &bytes).unwrap().len(), MAX_BODY);
        assert_eq!(open(CrcMode::Off, &bytes), Err(CodecError::Size));
        assert_eq!(
            seal(CrcMode::On, &mut bytes[..MAX_DATAGRAM - 1], MAX_BODY),
            Err(CodecError::Size)
        );
        assert_eq!(
            seal(CrcMode::Off, &mut bytes, usize::MAX),
            Err(CodecError::Size)
        );
        assert_eq!(
            open(CrcMode::On, &[0; MAX_DATAGRAM + 1]),
            Err(CodecError::Size)
        );
        assert_eq!(open(CrcMode::On, b"123"), Err(CodecError::Crc));
        assert_eq!(
            open(CrcMode::On, b"123456789e3069283"),
            Err(CodecError::Crc)
        );
        assert_eq!(
            decode(Format::Json, CrcMode::On, b"00000000"),
            Err(CodecError::Decode)
        );
        assert_eq!(
            decode(Format::Json, CrcMode::On, b"garbage00000000"),
            Err(CodecError::Crc)
        );
    }
    #[test]
    fn every_single_bit_mutation_is_rejected() {
        let original = *b"123456789E3069283";
        for byte in 0..original.len() {
            for bit in 0..8 {
                let mut damaged = original;
                damaged[byte] ^= 1 << bit;
                assert_eq!(open(CrcMode::On, &damaged), Err(CodecError::Crc));
            }
        }
        assert_eq!(open(CrcMode::Off, b"a\r\n"), Ok(&b"a\r\n"[..]));
        let mut bytes = [0; 11];
        bytes[..3].copy_from_slice(b"a\r\n");
        seal(CrcMode::On, &mut bytes, 3).unwrap();
        bytes[2] = b' ';
        assert_eq!(open(CrcMode::On, &bytes), Err(CodecError::Crc));
    }
}
