//! A slow terminal/file cannot block a publisher: the bounded queue drops diagnostics.
use crate::{Result, nonce, revision};
use messaging_codec::{Format, model::Envelope};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Clone)]
pub struct Log {
    sender: SyncSender<Value>,
    dropped: Arc<AtomicU64>,
    metadata: Value,
    start: Instant,
}
impl Log {
    pub fn new(
        path: Option<&String>,
        format: Format,
        local: String,
        interface: String,
        device: &str,
        boot: &str,
        start: Instant,
    ) -> Result<Self> {
        let mut output: Box<dyn Write + Send> = match path {
            Some(path) => Box::new(File::create(path)?),
            None => Box::new(io::stdout()),
        };
        let (sender, receiver) = sync_channel::<Value>(256);
        std::thread::Builder::new()
            .name("message-log".into())
            .spawn(move || {
                for event in receiver {
                    let result = serde_json::to_writer(&mut output, &event)
                        .map_err(io::Error::other)
                        .and_then(|_| writeln!(output))
                        .and_then(|_| output.flush());
                    if let Err(error) = result {
                        eprintln!("Logging stopped: {error}");
                        break;
                    }
                    // Human output shares this worker too: a blocked stderr must not
                    // block command processing any more than a slow JSONL consumer.
                    if event["outcome"] == "ready" {
                        eprintln!(
                            "Ready: {} / {} / {} (CRC off)",
                            event["local_endpoint"].as_str().unwrap_or("?"),
                            event["format"].as_str().unwrap_or("?"),
                            event["device_id"].as_str().unwrap_or("?")
                        );
                    } else if event["outcome"] == "sent_response"
                        || (event["outcome"] == "send_error"
                            && event["message"]["request_id"].is_number())
                    {
                        eprintln!(
                            "{}: kind={} request={} peer={} ({} bytes)",
                            event["outcome"].as_str().unwrap_or("?"),
                            event["message"]["kind"],
                            event["message"]["request_id"],
                            event["peer"].as_str().unwrap_or("?"),
                            event["raw_length"]
                        );
                    }
                }
            })?;
        Ok(Self {
            sender,
            dropped: Arc::new(AtomicU64::new(0)),
            start,
            metadata: json!({"run_id":nonce()?,"revision":revision(),"protocol_version":1,"format":format.name(),"crc":"off","role":"responder","local_endpoint":local,"interface":interface,"device_id":device,"boot_id":boot}),
        })
    }
    pub fn publisher(
        &self,
        endpoint: String,
        interface: String,
        stream: &str,
        period_ms: u64,
    ) -> Self {
        let mut log = self.clone();
        log.metadata["local_endpoint"] = json!(endpoint);
        log.metadata["interface"] = json!(interface);
        log.metadata["stream"] = json!(stream);
        log.metadata["period_ms"] = json!(period_ms);
        log.metadata["multicast_ttl"] = json!(1);
        log
    }
    pub fn event(
        &self,
        outcome: &str,
        peer: Option<String>,
        length: usize,
        message: Option<&Envelope>,
        rejected: Option<&[u8]>,
    ) -> io::Result<()> {
        let mut event = self.metadata.clone();
        event["outcome"] = json!(outcome);
        event["peer"] = json!(peer);
        event["raw_length"] = json!(length);
        event["message"] = json!(message);
        event["monotonic_ms"] = json!(self.start.elapsed().as_secs_f64() * 1000.0);
        event["unix_ms"] = json!(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        );
        event["log_dropped"] = json!(self.dropped.load(Ordering::Relaxed));
        if let Some(bytes) = rejected {
            event["raw_prefix_hex"] = json!(
                bytes
                    .iter()
                    .take(64)
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
        }
        if self.sender.try_send(event).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
}
