//! Host-only ABI for Python orchestration; the codec itself remains no_std.
use messaging_codec::{Format, benchmark, framing::CrcMode};

/// Translate canonical JSON to/from a benchmark datagram using the portable codec.
/// Returns bytes written, or a negative CodecError category. No retained state.
///
/// # Safety
/// Input must reference input_len readable bytes; output must reference capacity
/// writable bytes. Regions must not overlap and must remain valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn benchmark_translate(
    encoding: u32,
    crc_on: bool,
    encode: bool,
    input: *const u8,
    input_len: usize,
    output: *mut u8,
    capacity: usize,
) -> isize {
    if input.is_null() || output.is_null() || input_len > 4096 || capacity > 4096 {
        return -1;
    }
    let format = match encoding {
        1 => Format::Csv,
        2 => Format::Json,
        3 => Format::Protobuf,
        _ => return -1,
    };
    let crc = if crc_on { CrcMode::On } else { CrcMode::Off };
    // SAFETY: the caller guarantees lifetime, sizes, and disjoint allocations.
    let input = unsafe { core::slice::from_raw_parts(input, input_len) };
    let output = unsafe { core::slice::from_raw_parts_mut(output, capacity) };
    let result = if encode {
        serde_json::from_slice::<benchmark::model::Envelope>(input)
            .map_err(|_| messaging_codec::CodecError::Decode)
            .and_then(|m| benchmark::encode(format, crc, &m, output))
    } else {
        benchmark::decode(format, crc, input).and_then(|m| {
            let bytes = serde_json::to_vec(&m).map_err(|_| messaging_codec::CodecError::Decode)?;
            if bytes.len() > output.len() {
                return Err(messaging_codec::CodecError::Size);
            }
            output[..bytes.len()].copy_from_slice(&bytes);
            Ok(bytes.len())
        })
    };
    match result {
        Ok(n) => n as isize,
        Err(e) => match e {
            messaging_codec::CodecError::Crc => -2,
            messaging_codec::CodecError::Decode => -3,
            messaging_codec::CodecError::Validation => -4,
            messaging_codec::CodecError::Version => -5,
            messaging_codec::CodecError::Size => -6,
        },
    }
}
