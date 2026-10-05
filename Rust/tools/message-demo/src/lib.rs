//! Host-only ABI for Python orchestration; the codec itself remains no_std.
use benchmark::service::{Peer, Service};
use messaging_codec::{Format, benchmark, framing::CrcMode};

/// Construct host-owned portable service state from JSON device/boot/format/CRC.
/// # Safety
/// Input references input_len readable bytes, at most 4096. Returned state must
/// be freed exactly once by benchmark_service_free, with no concurrent calls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn benchmark_service_new(input: *const u8, input_len: usize) -> *mut Service {
    if input.is_null() || input_len > 4096 {
        return core::ptr::null_mut();
    }
    // SAFETY: caller supplies readable input of the stated length.
    let bytes = unsafe { core::slice::from_raw_parts(input, input_len) };
    let make = || -> Option<Service> {
        let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        Some(Service::new(
            v["device"].as_str()?.try_into().ok()?,
            v["boot"].as_str()?.try_into().ok()?,
            Format::parse(v["format"].as_str()?)?,
            CrcMode::parse(v["crc"].as_str()?)?,
            0,
        ))
    };
    make().map_or(core::ptr::null_mut(), |s| Box::into_raw(Box::new(s)))
}
/// # Safety
/// State is a live pointer returned by benchmark_service_new, used exclusively.
/// Input/output have their declared lengths, are disjoint, and do not alias state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn benchmark_service_call(
    state: *mut Service,
    input: *const u8,
    input_len: usize,
    output: *mut u8,
    capacity: usize,
) -> isize {
    if state.is_null() || input.is_null() || output.is_null() || input_len > 4096 || capacity > 4096
    {
        return -1;
    }
    // SAFETY: exclusive state and disjoint readable/writable buffers are required.
    let (s, bytes, out) = unsafe {
        (
            &mut *state,
            core::slice::from_raw_parts(input, input_len),
            core::slice::from_raw_parts_mut(output, capacity),
        )
    };
    let call = || -> Option<serde_json::Value> {
        let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
        let now = v["now"].as_u64()?;
        let reply = match v["op"].as_str()? {
            "handle" => {
                let m = serde_json::from_value(v["message"].clone()).ok()?;
                let address = v["address"].as_array()?;
                if address.len() != 4 {
                    return None;
                }
                let mut ip = [0; 4];
                for (index, value) in address.iter().enumerate() {
                    ip[index] = u8::try_from(value.as_u64()?).ok()?;
                }
                s.handle(
                    &m,
                    Peer {
                        address: ip,
                        port: u16::try_from(v["port"].as_u64()?).ok()?,
                    },
                    now,
                )
            }
            "publication" => s.publication(usize::try_from(v["index"].as_u64()?).ok()?, now),
            "tick" => {
                s.tick(now);
                None
            }
            "end" => {
                s.end();
                None
            }
            "reject" => {
                s.reject(v["crc"].as_bool()?);
                None
            }
            "sent" => {
                let index = usize::try_from(v["index"].as_u64()?).ok()?;
                if index > 2 {
                    return None;
                }
                s.counters.sent(index, v["success"].as_bool()?);
                None
            }
            "skip" => {
                s.counters.skip(v["amount"].as_u64()?);
                None
            }
            "info" => None,
            _ => return None,
        };
        Some(serde_json::json!({"reply":reply,"epoch":s.epoch(),"active":s.active()}))
    };
    let mut call = call;
    let Some(value) = call() else {
        return -1;
    };
    let Ok(encoded) = serde_json::to_vec(&value) else {
        return -1;
    };
    if encoded.len() > out.len() {
        return -1;
    }
    out[..encoded.len()].copy_from_slice(&encoded);
    encoded.len() as isize
}
/// # Safety
/// State is null or a live pointer returned by benchmark_service_new; there must
/// be no other references/calls, and this function must only free it once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn benchmark_service_free(state: *mut Service) {
    if !state.is_null() {
        // SAFETY: ownership is transferred back exactly once by the caller.
        drop(unsafe { Box::from_raw(state) });
    }
}

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
