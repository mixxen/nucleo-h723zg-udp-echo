//! Host transport and diagnostics. The bounded wire implementation lives in messaging-codec.
use messaging_codec::{CodecError, Format, MAX_BODY, decode, encode, model::*, parse, validation};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    error::Error,
    fs::File,
    io::{self, BufRead, Write},
    net::UdpSocket,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn nonce() -> Result<String> {
    loop {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
        if bytes != [0; 8] {
            return Ok(bytes.iter().map(|b| format!("{b:02x}")).collect());
        }
    }
}
fn revision() -> String {
    std::process::Command::new("git")
        .args(["describe", "--always", "--dirty"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into())
}
struct Log {
    output: Box<dyn Write>,
    metadata: Value,
    start: Instant,
}
impl Log {
    fn event(
        &mut self,
        outcome: &str,
        peer: Option<String>,
        length: usize,
        message: Option<&Envelope>,
        rejected_bytes: Option<&[u8]>,
    ) -> io::Result<()> {
        let mut event = self.metadata.clone();
        event["outcome"] = json!(outcome);
        event["peer"] = json!(peer);
        event["raw_length"] = json!(length);
        event["monotonic_ms"] = json!(self.start.elapsed().as_secs_f64() * 1000.0);
        event["unix_ms"] = json!(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        );
        event["message"] = json!(message);
        if let Some(bytes) = rejected_bytes {
            let sample: String = bytes
                .iter()
                .take(64)
                .map(|byte| format!("{byte:02x}"))
                .collect();
            event["raw_prefix_hex"] = json!(sample);
        }
        serde_json::to_writer(&mut self.output, &event)?;
        writeln!(self.output)?;
        self.output.flush()
    }
}
#[derive(Default)]
struct Counters {
    received: u32,
    rejected: u32,
    send_errors: u32,
    active_send_error: bool,
}
// Token bucket limits error replies only; read commands are never retried by the service.
struct ErrorBudget {
    tokens: f64,
    updated: Instant,
}
impl ErrorBudget {
    fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens =
            (self.tokens + now.duration_since(self.updated).as_secs_f64() * 10.0).min(10.0);
        self.updated = now;
        if self.tokens < 1.0 {
            false
        } else {
            self.tokens -= 1.0;
            true
        }
    }
}
fn serve(options: BTreeMap<String, String>) -> Result<()> {
    let format = Format::parse(
        options
            .get("format")
            .ok_or("--format csv|json|protobuf is required")?,
    )
    .ok_or("invalid format")?;
    let bind = options
        .get("bind")
        .map(String::as_str)
        .unwrap_or("127.0.0.1:42000");
    let device = options
        .get("device")
        .map(String::as_str)
        .unwrap_or("board-01");
    let sample = options.get("sample").map(String::as_str).unwrap_or("valid");
    if !matches!(sample, "valid" | "unavailable" | "fault") {
        return Err("--sample must be valid, unavailable, or fault".into());
    }
    let boot = nonce()?;
    let template = Envelope {
        protocol_version: Some(1),
        device_id: Some(device.try_into().map_err(|_| "device too long")?),
        boot_id: Some(boot.as_str().try_into().unwrap()),
        ..Default::default()
    };
    let status = Status {
        temperature_celsius: if sample == "valid" { Some(25.5) } else { None },
        validity: Some(match sample {
            "valid" => 1,
            "unavailable" => 2,
            _ => 3,
        }),
        synthetic: Some(true),
    };
    // Validate configured identity before binding (a publication is a convenient complete probe).
    let probe = Envelope {
        kind: Some(21),
        sequence: Some(0),
        uptime_ms: Some(0),
        status: Some(status.clone()),
        ..template.clone()
    };
    validation::validate(&probe, format).map_err(|e| e.outcome())?;
    let socket = UdpSocket::bind(bind)?;
    let local = socket.local_addr()?;
    let start = Instant::now();
    let output: Box<dyn Write> = match options.get("log") {
        Some(path) => Box::new(File::create(path)?),
        None => Box::new(io::stdout()),
    };
    let mut log = Log {
        output,
        start,
        metadata: json!({"run_id":nonce()?,"revision":revision(),"protocol_version":1,"format":format.name(),"crc":"off","role":"responder","local_endpoint":local.to_string(),"interface":local.ip().to_string(),"device_id":device,"boot_id":boot}),
    };
    log.event("ready", None, 0, None, None)?;
    eprintln!(
        "Ready: {local} / {} / {device} (synthetic {sample}; CRC off)",
        format.name()
    );
    let mut counters = Counters::default();
    let mut budget = ErrorBudget {
        tokens: 10.0,
        updated: Instant::now(),
    };
    // Full UDP receive buffer lets logs record the actual oversize length on hosts.
    let mut input = [0; 65535];
    let mut output = [0; MAX_BODY];
    loop {
        let (length, peer) = match socket.recv_from(&mut input) {
            Ok(received) => received,
            Err(error) => {
                log.event("local_io_error", None, 0, None, None)?;
                return Err(error.into());
            }
        };
        let request = match parse(format, &input[..length]) {
            Ok(message) => message,
            Err(error) => {
                counters.rejected = counters.rejected.saturating_add(1);
                log.event(
                    error.outcome(),
                    Some(peer.to_string()),
                    length,
                    None,
                    Some(&input[..length]),
                )?;
                continue;
            }
        };
        let validity = validation::validate(&request, format);
        let is_request = matches!(request.kind, Some(1..=3));
        let target = request.device_id == template.device_id;
        let accepted = validity.is_ok() && is_request && target;
        let error = if accepted {
            None
        } else {
            counters.rejected = counters.rejected.saturating_add(1);
            let outcome = validity.err().unwrap_or(CodecError::Validation).outcome();
            log.event(
                outcome,
                Some(peer.to_string()),
                length,
                Some(&request),
                Some(&input[..length]),
            )?;
            // Never answer a response/publication, bad version, wrong target, or uncorrelatable input.
            if !target
                || !validation::correlation(&request)
                || matches!(request.kind, Some(11..=14 | 21 | 22))
            {
                continue;
            }
            if !budget.take() {
                log.event(
                    "error_rate_limited",
                    Some(peer.to_string()),
                    length,
                    Some(&request),
                    None,
                )?;
                continue;
            }
            Some(if request.kind.is_some() && !is_request {
                1
            } else {
                2
            })
        };
        if accepted {
            counters.received = counters.received.saturating_add(1);
            log.event(
                "accepted_request",
                Some(peer.to_string()),
                length,
                Some(&request),
                None,
            )?;
        }
        let mut response = Envelope {
            kind: Some(if error.is_some() {
                14
            } else {
                request.kind.unwrap() + 10
            }),
            client_session: request.client_session,
            request_id: request.request_id,
            uptime_ms: Some(start.elapsed().as_millis() as u32),
            ..template.clone()
        };
        match response.kind.unwrap() {
            11 => {
                response.device_info = Some(DeviceInfo {
                    firmware_version: Some("host-demo-0.1.0".try_into().unwrap()),
                    encoding: Some(format.id()),
                })
            }
            12 => response.status = Some(status.clone()),
            13 => {
                response.health = Some(Health {
                    state: Some(if counters.active_send_error { 2 } else { 1 }),
                    received_requests: Some(counters.received),
                    rejected_datagrams: Some(counters.rejected),
                    send_errors: Some(counters.send_errors),
                    skipped_publications: Some(0),
                })
            }
            14 => response.error = Some(messaging_codec::model::Error { code: error }),
            _ => unreachable!(),
        }
        let length = encode(format, &response, &mut output).map_err(|e| e.outcome())?;
        let outcome = match socket.send_to(&output[..length], peer) {
            Ok(n) if n == length => "sent_response",
            _ => {
                counters.send_errors = counters.send_errors.saturating_add(1);
                "send_error"
            }
        };
        counters.active_send_error = outcome == "send_error";
        log.event(
            outcome,
            Some(peer.to_string()),
            length,
            Some(&response),
            None,
        )?;
        eprintln!(
            "{outcome}: kind={} request={} peer={peer} ({length} bytes)",
            response.kind.unwrap(),
            response.request_id.unwrap()
        );
    }
}
fn codec_operation(input: Value) -> std::result::Result<Value, &'static str> {
    let format =
        Format::parse(input["format"].as_str().ok_or("decode_error")?).ok_or("decode_error")?;
    match input["operation"].as_str() {
        Some("decode") => {
            let text = input["hex"].as_str().ok_or("decode_error")?;
            if !text.is_ascii() || text.len() % 2 != 0 {
                return Err("decode_error");
            }
            let bytes = (0..text.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&text[i..i + 2], 16))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| "decode_error")?;
            let message = decode(format, &bytes).map_err(CodecError::outcome)?;
            Ok(json!({"message":message}))
        }
        Some("encode") => {
            let message: Envelope =
                serde_json::from_value(input["message"].clone()).map_err(|_| "decode_error")?;
            let mut output = [0; MAX_BODY];
            let length = encode(format, &message, &mut output).map_err(CodecError::outcome)?;
            let hex: String = output[..length]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            Ok(json!({"hex":hex}))
        }
        _ => Err("decode_error"),
    }
}
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("codec")=>{
            for line in io::stdin().lock().lines() {
                let result=serde_json::from_str(&line?).map_err(|_|"decode_error").and_then(codec_operation);
                println!("{}",result.unwrap_or_else(|e|json!({"error":e})));
            }
            Ok(())
        },
        Some("serve")=>{
            let mut options=BTreeMap::new();
            while let Some(flag)=args.next() {
                let key=flag.strip_prefix("--").filter(|key|matches!(*key,"format"|"bind"|"device"|"sample"|"log")).ok_or("unknown flag")?;
                if options.insert(key.to_owned(),args.next().ok_or("missing flag value")?).is_some(){return Err("duplicate flag".into());}
            }
            serve(options)
        },
        _=>Err("Usage: message-demo serve --format csv|json|protobuf [--bind 127.0.0.1:42000] [--device board-01] [--sample valid|unavailable|fault] [--log run.jsonl]\n       message-demo codec  (JSONL fixture runner on stdin/stdout)".into()),
    }
}
