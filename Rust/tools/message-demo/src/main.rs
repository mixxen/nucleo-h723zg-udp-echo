//! Host transport and diagnostics. The bounded wire implementation lives in messaging-codec.
use messaging_codec::framing::{self, CrcMode, MAX_DATAGRAM};
use messaging_codec::service::{CommandPolicy, Decision};
use messaging_codec::{CodecError, Format, model::*, validation};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    error::Error,
    io::{self, BufRead},
    net::UdpSocket,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

mod benchmark;
mod log;
mod streams;
use log::Log;

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
#[derive(Default)]
struct Counters {
    received: u32,
    rejected: u32,
    send_errors: u32,
    active_send_error: bool,
    active_stream_error: [bool; 2],
    skipped: u32,
}
impl Counters {
    fn health(&self) -> Health {
        Health {
            state: Some(
                if self.active_send_error || self.active_stream_error.iter().any(|v| *v) {
                    2
                } else {
                    1
                },
            ),
            received_requests: Some(self.received),
            rejected_datagrams: Some(self.rejected),
            send_errors: Some(self.send_errors),
            skipped_publications: Some(self.skipped),
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
    let crc = CrcMode::parse(options.get("crc").map(String::as_str).unwrap_or("off"))
        .ok_or("--crc must be off|on")?;
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
    socket.set_write_timeout(Some(Duration::from_millis(50)))?;
    let local = socket.local_addr()?;
    let start = Instant::now();
    let publishers = streams::Publisher::configure(&options, local.port())?;
    let log = Log::new(
        options.get("log"),
        (format, crc),
        local.to_string(),
        local.ip().to_string(),
        device,
        &boot,
        start,
    )?;
    let counters = Arc::new(Mutex::new(Counters::default()));
    for publisher in publishers {
        publisher.start(
            (format, crc),
            template.clone(),
            status.clone(),
            counters.clone(),
            log.clone(),
            start,
        )?;
    }
    log.event("ready", None, 0, None, None)?;
    let mut policy = CommandPolicy::new(0);
    // Full UDP receive buffer lets logs record the actual oversize length on hosts.
    let mut input = [0; 65535];
    let mut output = [0; MAX_DATAGRAM];
    loop {
        let (length, peer) = match socket.recv_from(&mut input) {
            Ok(received) => received,
            Err(error) => {
                log.event("local_io_error", None, 0, None, None)?;
                return Err(error.into());
            }
        };
        let request = match framing::parse(format, crc, &input[..length]) {
            Ok(message) => message,
            Err(error) => {
                {
                    let mut shared = counters.lock().unwrap();
                    shared.rejected = shared.rejected.saturating_add(1);
                }
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
        let assessment =
            policy.assess(&request, format, device, start.elapsed().as_millis() as u64);
        let accepted = assessment.decision == Decision::Accept;
        if let Some(rejection) = assessment.rejection {
            {
                let mut shared = counters.lock().unwrap();
                shared.rejected = shared.rejected.saturating_add(1);
            }
            log.event(
                rejection.outcome(),
                Some(peer.to_string()),
                length,
                Some(&request),
                Some(&input[..length]),
            )?;
        }
        let error = match assessment.decision {
            Decision::Accept => None,
            Decision::Error(code) => Some(code),
            Decision::Ignore => continue,
            Decision::RateLimited => {
                log.event(
                    "error_rate_limited",
                    Some(peer.to_string()),
                    length,
                    Some(&request),
                    None,
                )?;
                continue;
            }
        };
        if accepted {
            {
                let mut shared = counters.lock().unwrap();
                shared.received = shared.received.saturating_add(1);
            }
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
            13 => response.health = Some(counters.lock().unwrap().health()),
            14 => response.error = Some(messaging_codec::model::Error { code: error }),
            _ => unreachable!(),
        }
        let length =
            framing::encode(format, crc, &response, &mut output).map_err(|e| e.outcome())?;
        let outcome = match socket.send_to(&output[..length], peer) {
            Ok(n) if n == length => "sent_response",
            _ => {
                {
                    let mut shared = counters.lock().unwrap();
                    shared.send_errors = shared.send_errors.saturating_add(1);
                }
                "send_error"
            }
        };
        counters.lock().unwrap().active_send_error = outcome == "send_error";
        log.event(
            outcome,
            Some(peer.to_string()),
            length,
            Some(&response),
            None,
        )?;
    }
}
fn codec_operation(input: Value) -> std::result::Result<Value, &'static str> {
    let format =
        Format::parse(input["format"].as_str().ok_or("decode_error")?).ok_or("decode_error")?;
    let crc = match input.get("crc") {
        None => CrcMode::Off,
        Some(value) => {
            CrcMode::parse(value.as_str().ok_or("validation_error")?).ok_or("validation_error")?
        }
    };
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
            let message = framing::decode(format, crc, &bytes).map_err(CodecError::outcome)?;
            Ok(json!({"message":message}))
        }
        Some("encode") => {
            let message: Envelope =
                serde_json::from_value(input["message"].clone()).map_err(|_| "decode_error")?;
            let mut output = [0; MAX_DATAGRAM];
            let length =
                framing::encode(format, crc, &message, &mut output).map_err(CodecError::outcome)?;
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
        Some("bench") => benchmark::run(args.collect()),
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
                let key=flag.strip_prefix("--").filter(|key|matches!(*key,"format"|"crc"|"bind"|"device"|"sample"|"log"|"multicast-interface"|"group"|"status-port"|"health-port"|"status-hz"|"health-hz")).ok_or("unknown flag")?;
                if options.insert(key.to_owned(),args.next().ok_or("missing flag value")?).is_some(){return Err("duplicate flag".into());}
            }
            serve(options)
        },
        _=>Err("Usage: message-demo serve --format csv|json|protobuf [--crc off|on] [--bind 127.0.0.1:42000] [--device board-01] [--sample valid|unavailable|fault] [--log run.jsonl] [--multicast-interface IPv4 --group 239.255.42.1 --status-port 42001 --health-port 42002 --status-hz 10 --health-hz 1]\n       message-demo codec  (JSONL fixture runner on stdin/stdout)\n       message-demo bench ITERATIONS BATCHES  (JSONL fixed fixtures; batch timing)".into()),
    }
}
