//! Opt-in benchmark codec. No allocator; unchanged normal schema and framing.
use crate::{
    CodecError, Format, MAX_BODY,
    framing::{self, CrcMode},
};
#[cfg(feature = "protobuf")]
#[allow(nonstandard_style, unused, clippy::all)]
mod wire {
    include!("generated/benchmark_protobuf.rs");
}
#[cfg(feature = "protobuf")]
use wire::messaging_::benchmark_::v1_ as pb;
#[allow(clippy::field_reassign_with_default)]
pub mod model {
    include!("generated/benchmark_model.rs");
}
use model::Envelope;

fn require(ok: bool) -> Result<(), CodecError> {
    if ok {
        Ok(())
    } else {
        Err(CodecError::Validation)
    }
}
fn nonce(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        v.len() == 16
            && v != "0000000000000000"
            && v.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub fn configuration_valid(c: &model::Configuration) -> bool {
    c.payload_bytes.is_some_and(|v| v <= 512)
        && c.pattern_seed.is_some()
        && c.status_period_us
            .is_some_and(|v| v == 0 || (1000..=10000000).contains(&v))
        && c.health_period_us
            .is_some_and(|v| v == 0 || (10000..=10000000).contains(&v))
        && c.lease_ms.is_some_and(|v| (2000..=30000).contains(&v))
}
pub fn validate(m: &Envelope, format: Format, crc: CrcMode) -> Result<(), CodecError> {
    if m.protocol_version != Some(1) {
        return Err(if m.protocol_version.is_some() {
            CodecError::Version
        } else {
            CodecError::Validation
        });
    }
    require(m.device_id.as_ref().is_some_and(|v| {
        !v.is_empty()
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    }))?;
    require(nonce(m.session_id.as_deref()))?;
    let kind = m.kind.ok_or(CodecError::Validation)?;
    require(
        [
            m.capabilities.is_some(),
            m.configure.is_some(),
            m.renew.is_some(),
            m.stop.is_some(),
            m.probe.is_some(),
            m.limits.is_some(),
            m.result.is_some(),
            m.probe_response.is_some(),
            m.status.is_some(),
            m.health.is_some(),
        ]
        .iter()
        .filter(|v| **v)
        .count()
            == 1,
    )?;
    require(match kind {
        1 => m.capabilities.is_some(),
        2 => m.configure.is_some(),
        3 => m.renew.is_some(),
        4 => m.stop.is_some(),
        5 => m.probe.is_some(),
        11 => m.limits.is_some(),
        12 => m.result.is_some(),
        15 => m.probe_response.is_some(),
        21 => m.status.is_some(),
        22 => m.health.is_some(),
        _ => false,
    })?;
    let request = matches!(kind, 1..=5);
    let publication = matches!(kind, 21 | 22);
    require(
        m.uptime_ms.is_some() != request
            && m.sequence.is_some() == publication
            && m.request_id.is_some() != publication,
    )?;
    if !publication {
        require(m.request_id.is_some_and(|v| v > 0))?;
    }
    if kind == 1 {
        require(m.boot_id.is_none() && m.run_epoch.is_none())?;
    } else {
        require(nonce(m.boot_id.as_deref()) && m.run_epoch.is_some_and(|v| v > 0))?;
    }
    require(m.payload.is_some() == matches!(kind, 5 | 15 | 21 | 22))?;
    if let Some(payload) = &m.payload {
        require(payload.bytes().all(|b| b.is_ascii_alphanumeric()))?;
    }
    if let Some(c) = &m.configure {
        // Bounds are a control result, not a structural error. The service must
        // call configuration_valid before atomically applying any values.
        require(
            c.payload_bytes.is_some()
                && c.pattern_seed.is_some()
                && c.status_period_us.is_some()
                && c.health_period_us.is_some()
                && c.lease_ms.is_some(),
        )?;
    }
    if let Some(l) = &m.limits {
        require(
            l.encoding == Some(format.id())
                && l.crc_enabled == Some(crc == CrcMode::On)
                && l.max_payload_bytes == Some(512)
                && l.max_datagram_bytes == Some(1024)
                && l.min_status_period_us == Some(1000)
                && l.min_health_period_us == Some(10000)
                && l.max_period_us == Some(10000000)
                && l.min_lease_ms == Some(2000)
                && l.max_lease_ms == Some(30000)
                && l.available.is_some(),
        )?;
    }
    if let Some(r) = &m.result {
        require(
            r.code.is_some_and(|v| (1..=10).contains(&v))
                && r.lease_remaining_ms.is_some_and(|v| v <= 30000),
        )?;
    }
    // Reuse ordinary status/health semantics without accepting normal envelopes.
    if m.status.is_some() || m.health.is_some() {
        let status = m.status.as_ref().map(|s| crate::model::Status {
            temperature_celsius: s.temperature_celsius,
            validity: s.validity,
            synthetic: s.synthetic,
        });
        let health = m.health.as_ref().map(|h| crate::model::Health {
            state: h.state,
            received_requests: h.received_requests,
            rejected_datagrams: h.rejected_datagrams,
            send_errors: h.send_errors,
            skipped_publications: h.skipped_publications,
        });
        crate::validation::validate(
            &crate::model::Envelope {
                protocol_version: Some(1),
                kind: Some(kind),
                device_id: m.device_id.clone(),
                boot_id: m.boot_id.clone(),
                sequence: m.sequence,
                uptime_ms: m.uptime_ms,
                status,
                health,
                ..Default::default()
            },
            format,
        )?;
    }
    Ok(())
}

pub fn decode(format: Format, crc: CrcMode, bytes: &[u8]) -> Result<Envelope, CodecError> {
    let bytes = framing::open(crc, bytes)?;
    if bytes.is_empty() {
        return Err(CodecError::Decode);
    }
    #[allow(unreachable_patterns)]
    let mut m = match format {
        #[cfg(feature = "json")]
        Format::Json => {
            let (mut quoted, mut escaped) = (false, false);
            for &b in bytes {
                if quoted && b < 32 {
                    return Err(CodecError::Decode);
                }
                if escaped {
                    escaped = false;
                    continue;
                }
                if quoted && b == b'\\' {
                    escaped = true;
                } else if b == b'"' {
                    quoted = !quoted;
                }
            }
            let mut scratch = [0; MAX_BODY];
            let (m, used) = serde_json_core::from_slice_escaped::<Envelope>(bytes, &mut scratch)
                .map_err(|_| CodecError::Decode)?;
            if bytes[used..].iter().any(|b| !b" \r\n\t".contains(b)) {
                return Err(CodecError::Decode);
            }
            m
        }
        #[cfg(feature = "csv")]
        Format::Csv => {
            crate::csv::check_record(bytes)?;
            let mut scratch = [0; MAX_BODY];
            let mut ends = [0; 20];
            let (result, used, written, count) =
                csv_core::Reader::new().read_record(bytes, &mut scratch, &mut ends);
            if result != csv_core::ReadRecordResult::Record
                || !(used == bytes.len() || used + 1 == bytes.len() && bytes[used] == b'\n')
            {
                return Err(CodecError::Decode);
            }
            let mut fields = heapless::Vec::<&str, 20>::new();
            let mut start = 0;
            for &end in &ends[..count] {
                fields
                    .push(
                        core::str::from_utf8(&scratch[start..end])
                            .map_err(|_| CodecError::Decode)?,
                    )
                    .map_err(|_| CodecError::Size)?;
                start = end;
            }
            if start != written {
                return Err(CodecError::Decode);
            }
            Envelope::csv_read(&fields)?
        }
        #[cfg(feature = "protobuf")]
        Format::Protobuf => {
            use micropb::MessageDecode;
            let mut message = pb::Envelope::default();
            message
                .decode_from_bytes(bytes)
                .map_err(|_| CodecError::Decode)?;
            Envelope::from_pb(message)
        }
        _ => return Err(CodecError::Decode),
    };
    validate(&m, format, crc)?;
    if let Some(s) = &mut m.status
        && s.temperature_celsius == Some(0.0)
    {
        s.temperature_celsius = Some(0.0);
    }
    Ok(m)
}

pub fn encode(
    format: Format,
    crc: CrcMode,
    m: &Envelope,
    out: &mut [u8],
) -> Result<usize, CodecError> {
    validate(m, format, crc)?;
    let capacity = out.len().min(MAX_BODY);
    #[allow(unreachable_patterns)]
    let size = match format {
        #[cfg(feature = "json")]
        Format::Json => {
            serde_json_core::to_slice(m, &mut out[..capacity]).map_err(|_| CodecError::Size)?
        }
        #[cfg(feature = "csv")]
        Format::Csv => m.csv_write(&mut out[..capacity])?,
        #[cfg(feature = "protobuf")]
        Format::Protobuf => {
            use micropb::{MessageEncode, PbEncoder};
            let mut encoder = PbEncoder::new(heapless::Vec::<u8, MAX_BODY>::new());
            m.to_pb()
                .encode(&mut encoder)
                .map_err(|_| CodecError::Size)?;
            let bytes = encoder.as_writer().as_slice();
            if bytes.len() > capacity {
                return Err(CodecError::Size);
            }
            out[..bytes.len()].copy_from_slice(bytes);
            bytes.len()
        }
        _ => return Err(CodecError::Validation),
    };
    framing::seal(crc, out, size)
}

#[cfg(feature = "csv")]
trait Field {
    fn text(&self, out: &mut heapless::String<MAX_BODY>) -> Result<(), CodecError>;
}
#[cfg(feature = "csv")]
macro_rules! scalar_field {
    ($($t:ty),*) => {$ (impl Field for $t {
        fn text(&self,out:&mut heapless::String<MAX_BODY>)->Result<(),CodecError> {
            let mut short=crate::cells::Text::new();crate::cells::Cell::write(self,&mut short)?;
            out.push_str(&short).map_err(|_|CodecError::Size)
        }
    })*};
}
#[cfg(feature = "csv")]
scalar_field!(u32, i32, f32, bool);
#[cfg(feature = "csv")]
impl<const N: usize> Field for heapless::String<N> {
    fn text(&self, out: &mut heapless::String<MAX_BODY>) -> Result<(), CodecError> {
        out.push_str(self).map_err(|_| CodecError::Size)
    }
}
// CSV payload strings bypass the normal 128-byte scalar cell formatter.
#[cfg(feature = "csv")]
struct CsvOutput<'a> {
    out: &'a mut [u8],
    used: usize,
    count: usize,
}
#[cfg(feature = "csv")]
impl<'a> CsvOutput<'a> {
    fn new(out: &'a mut [u8]) -> Self {
        Self {
            out,
            used: 0,
            count: 0,
        }
    }
    fn append(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        if bytes.len() > self.out.len() - self.used {
            return Err(CodecError::Size);
        }
        self.out[self.used..self.used + bytes.len()].copy_from_slice(bytes);
        self.used += bytes.len();
        Ok(())
    }
    fn field<T: Field>(&mut self, value: Option<&T>) -> Result<(), CodecError> {
        if self.count > 0 {
            self.append(b",")?;
        }
        self.count += 1;
        if let Some(value) = value {
            let mut text = heapless::String::<MAX_BODY>::new();
            value.text(&mut text)?;
            self.append(text.as_bytes())?;
        }
        Ok(())
    }
    fn finish(mut self) -> Result<usize, CodecError> {
        self.append(b"\r\n")?;
        Ok(self.used)
    }
}
