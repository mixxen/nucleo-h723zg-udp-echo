use crate::{CodecError, Format, model::Envelope};
fn require(condition: bool) -> Result<(), CodecError> {
    if condition {
        Ok(())
    } else {
        Err(CodecError::Validation)
    }
}
fn session(value: Option<&str>) -> bool {
    value.is_some_and(|s| {
        s.len() == 16
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && s != "0000000000000000"
    })
}
pub fn correlation(message: &Envelope) -> bool {
    message.protocol_version == Some(1)
        && message.device_id.as_ref().is_some_and(|id| {
            !id.is_empty()
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        })
        && session(message.client_session.as_deref())
        && message.request_id.is_some_and(|v| v != 0)
}
pub fn validate(m: &Envelope, format: Format) -> Result<(), CodecError> {
    if m.protocol_version != Some(1) {
        return Err(if m.protocol_version.is_none() {
            CodecError::Validation
        } else {
            CodecError::Version
        });
    }
    require(m.device_id.as_ref().is_some_and(|id| {
        !id.is_empty()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    }))?;
    let kind = m.kind.ok_or(CodecError::Validation)?;
    let request = matches!(kind, 1..=3);
    let response = matches!(kind, 11..=14);
    let publication = matches!(kind, 21 | 22);
    require(request || response || publication)?;
    require(
        [
            m.get_device_info.is_some(),
            m.get_status.is_some(),
            m.get_health.is_some(),
            m.device_info.is_some(),
            m.status.is_some(),
            m.health.is_some(),
            m.error.is_some(),
        ]
        .iter()
        .filter(|v| **v)
        .count()
            == 1,
    )?;
    require(match kind {
        1 => m.get_device_info.is_some(),
        2 => m.get_status.is_some(),
        3 => m.get_health.is_some(),
        11 => m.device_info.is_some(),
        12 | 21 => m.status.is_some(),
        13 | 22 => m.health.is_some(),
        14 => m.error.is_some(),
        _ => false,
    })?;
    if request {
        require(
            m.boot_id.is_none() && m.uptime_ms.is_none() && m.sequence.is_none() && correlation(m),
        )?;
    }
    if response {
        require(
            session(m.boot_id.as_deref())
                && m.uptime_ms.is_some()
                && m.sequence.is_none()
                && correlation(m),
        )?;
    }
    if publication {
        require(
            session(m.boot_id.as_deref())
                && m.uptime_ms.is_some()
                && m.sequence.is_some()
                && m.client_session.is_none()
                && m.request_id.is_none(),
        )?;
    }
    if let Some(info) = &m.device_info {
        require(
            info.encoding == Some(format.id())
                && info.firmware_version.as_ref().is_some_and(|v| {
                    !v.is_empty() && v.bytes().all(|b| (0x20..=0x7e).contains(&b))
                }),
        )?;
    }
    if let Some(status) = &m.status {
        require(status.synthetic == Some(true))?;
        match status.validity {
            Some(1) => require(
                status
                    .temperature_celsius
                    .is_some_and(|v| v.is_finite() && (-100.0..=200.0).contains(&v)),
            )?,
            Some(2 | 3) => require(status.temperature_celsius.is_none())?,
            _ => return Err(CodecError::Validation),
        }
    }
    if let Some(health) = &m.health {
        require(
            matches!(health.state, Some(1 | 2))
                && health.received_requests.is_some()
                && health.rejected_datagrams.is_some()
                && health.send_errors.is_some()
                && health.skipped_publications.is_some(),
        )?;
    }
    if let Some(error) = &m.error {
        require(matches!(error.code, Some(1 | 2)))?;
    }
    Ok(())
}
