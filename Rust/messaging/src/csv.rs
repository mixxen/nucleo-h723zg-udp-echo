use crate::{
    CodecError, MAX_BODY,
    cells::{Cells, Text},
    model::Envelope,
};
use csv_core::{ReadRecordResult, Reader, Terminator, WriteResult, WriterBuilder};

// csv-core intentionally tolerates malformed quoting; this small grammar check
// enforces the single-record contract before asking it to decode field values.
fn check_record(bytes: &[u8]) -> Result<(), CodecError> {
    if !bytes.ends_with(b"\r\n") {
        return Err(CodecError::Decode);
    }
    let mut state = 0; // start, unquoted, quoted, closing quote
    for &byte in &bytes[..bytes.len() - 2] {
        if matches!(byte, b'\r' | b'\n') {
            return Err(CodecError::Decode);
        }
        state = match (state, byte) {
            (0, b'"') => 2,
            (0, b',') => 0,
            (0, _) => 1,
            (1, b',') => 0,
            (1, b'"') => return Err(CodecError::Decode),
            (1, _) => 1,
            (2, b'"') => 3,
            (2, _) => 2,
            (3, b'"') => 2,
            (3, b',') => 0,
            (3, _) => return Err(CodecError::Decode),
            _ => unreachable!(),
        };
    }
    if state == 2 {
        return Err(CodecError::Decode);
    }
    Ok(())
}
pub fn decode(bytes: &[u8]) -> Result<Envelope, CodecError> {
    check_record(bytes)?;
    let mut data = [0; MAX_BODY];
    let mut ends = [0; 16];
    let (result, used, written, count) = Reader::new().read_record(bytes, &mut data, &mut ends);
    // CRLF is a single terminator, but csv-core may return after consuming CR.
    if result != ReadRecordResult::Record
        || !(used == bytes.len() || (used + 1 == bytes.len() && bytes[used] == b'\n'))
    {
        return Err(CodecError::Decode);
    }
    let mut cells = Cells::new();
    let mut start = 0;
    for end in &ends[..count] {
        let value = core::str::from_utf8(&data[start..*end]).map_err(|_| CodecError::Decode)?;
        cells
            .push(Text::try_from(value).map_err(|_| CodecError::Decode)?)
            .map_err(|_| CodecError::Size)?;
        start = *end;
    }
    if start != written {
        return Err(CodecError::Decode);
    }
    Envelope::from_cells(&cells)
}
pub fn encode(message: &Envelope, out: &mut [u8]) -> Result<usize, CodecError> {
    let cells = message.to_cells()?;
    let mut writer = WriterBuilder::new().terminator(Terminator::CRLF).build();
    let mut used = 0;
    for (index, field) in cells.iter().enumerate() {
        let (result, consumed, written) = writer.field(field.as_bytes(), &mut out[used..]);
        used += written;
        if result == WriteResult::OutputFull || consumed != field.len() {
            return Err(CodecError::Size);
        }
        if index + 1 < cells.len() {
            let (result, n) = writer.delimiter(&mut out[used..]);
            used += n;
            if result == WriteResult::OutputFull {
                return Err(CodecError::Size);
            }
        }
    }
    let (result, n) = writer.terminator(&mut out[used..]);
    used += n;
    if result == WriteResult::OutputFull {
        return Err(CodecError::Size);
    }
    Ok(used)
}
