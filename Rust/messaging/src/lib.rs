//! Portable, bounded codecs shared by the host demo and future firmware.
#![no_std]
pub mod cells;
mod csv;
pub mod schedule;
pub mod validation;
#[allow(nonstandard_style, unused, clippy::all)]
mod protobuf_generated {
    include!("generated/protobuf.rs");
}
pub use protobuf_generated::messaging_::v1_ as pb;
// Generated conversions deliberately assign fields one at a time.
#[allow(clippy::field_reassign_with_default, clippy::let_and_return)]
pub mod model {
    include!("generated/model.rs");
}
use micropb::{MessageDecode, MessageEncode, PbEncoder};
use model::Envelope;
use serde::Deserialize;

pub use model::MAX_BODY;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    Decode,
    Validation,
    Version,
    Size,
}
impl CodecError {
    pub fn outcome(self) -> &'static str {
        match self {
            Self::Decode => "decode_error",
            Self::Validation => "validation_error",
            Self::Version => "version_error",
            Self::Size => "size_error",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Json,
    Protobuf,
}
impl Format {
    pub fn id(self) -> i32 {
        match self {
            Self::Csv => 1,
            Self::Json => 2,
            Self::Protobuf => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Protobuf => "protobuf",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            "protobuf" => Some(Self::Protobuf),
            _ => None,
        }
    }
}
// A missing field uses default None; a present null must fail to decode T.
pub fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
pub fn parse(format: Format, bytes: &[u8]) -> Result<Envelope, CodecError> {
    if bytes.is_empty() {
        return Err(CodecError::Decode);
    }
    if bytes.len() > MAX_BODY {
        return Err(CodecError::Size);
    }
    match format {
        Format::Csv => csv::decode(bytes),
        Format::Json => {
            // serde-json-core accepts literal controls inside strings; JSON requires escaping.
            let (mut in_string, mut escaped) = (false, false);
            for &byte in bytes {
                if in_string && byte < 0x20 {
                    return Err(CodecError::Decode);
                }
                if escaped {
                    escaped = false;
                    continue;
                }
                if in_string && byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = !in_string;
                }
            }
            let mut scratch = [0; MAX_BODY];
            let (message, used) =
                serde_json_core::from_slice_escaped::<Envelope>(bytes, &mut scratch)
                    .map_err(|_| CodecError::Decode)?;
            if bytes[used..]
                .iter()
                .any(|b| !matches!(b, b' ' | b'\r' | b'\n' | b'\t'))
            {
                return Err(CodecError::Decode);
            }
            Ok(message)
        }
        Format::Protobuf => {
            let mut message = pb::Envelope::default();
            message
                .decode_from_bytes(bytes)
                .map_err(|_| CodecError::Decode)?;
            Ok(Envelope::from_pb(message))
        }
    }
}
pub fn decode(format: Format, bytes: &[u8]) -> Result<Envelope, CodecError> {
    let mut message = parse(format, bytes)?;
    validation::validate(&message, format)?;
    normalize(&mut message);
    Ok(message)
}
fn normalize(message: &mut Envelope) {
    if let Some(status) = &mut message.status
        && status.temperature_celsius == Some(0.0)
    {
        status.temperature_celsius = Some(0.0);
    }
}
pub fn encode(format: Format, message: &Envelope, out: &mut [u8]) -> Result<usize, CodecError> {
    validation::validate(message, format)?;
    let mut normalized = message.clone();
    normalize(&mut normalized);
    let capacity = out.len().min(MAX_BODY);
    let out = &mut out[..capacity];
    match format {
        Format::Csv => csv::encode(&normalized, out),
        Format::Json => serde_json_core::to_slice(&normalized, out).map_err(|_| CodecError::Size),
        Format::Protobuf => {
            let proto = normalized.to_pb();
            let mut encoder = PbEncoder::new(heapless::Vec::<u8, MAX_BODY>::new());
            proto.encode(&mut encoder).map_err(|_| CodecError::Size)?;
            let bytes = encoder.as_writer().as_slice();
            if bytes.len() > out.len() {
                return Err(CodecError::Size);
            }
            out[..bytes.len()].copy_from_slice(bytes);
            Ok(bytes.len())
        }
    }
}
