//! Time the actual portable codec in-process, outside UDP, JSONL and subprocess I/O.
//! Each JSONL input is a fixed semantic fixture. Each output is one batch mean.
use crate::Result;
use messaging_codec::{
    Format,
    framing::{self, CrcMode, MAX_DATAGRAM},
    model::Envelope,
};
use serde_json::{Value, json};
use std::{
    hint::black_box,
    io::{self, BufRead},
    time::Instant,
};

pub fn run(args: Vec<String>) -> Result<()> {
    if args.len() != 2 {
        return Err(
            "Usage: message-demo bench ITERATIONS BATCHES (JSONL fixtures on stdin)".into(),
        );
    }
    let iterations: u32 = args[0].parse()?;
    let batches: u32 = args[1].parse()?;
    if !(1..=1_000_000).contains(&iterations) || !(1..=100).contains(&batches) {
        return Err("iterations must be 1..1000000 and batches 1..100".into());
    }
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line?)?;
        let format = Format::parse(input["format"].as_str().ok_or("missing format")?)
            .ok_or("invalid format")?;
        let crc =
            CrcMode::parse(input["crc"].as_str().ok_or("missing crc")?).ok_or("invalid crc")?;
        let message: Envelope = serde_json::from_value(input["message"].clone())?;
        let mut bytes = [0; MAX_DATAGRAM];
        let length = framing::encode(format, crc, &message, &mut bytes).map_err(|e| e.outcome())?;
        let decoded = framing::decode(format, crc, &bytes[..length]).map_err(|e| e.outcome())?;
        if serde_json::to_value(&decoded)? != serde_json::to_value(&message)? {
            return Err("benchmark fixture must already be normalized".into());
        }
        // Untimed warm-up touches both paths. Black-box the input AND output so
        // the compiler cannot replace repeated work with a constant or hoist it.
        for _ in 0..iterations.min(1000) {
            black_box(
                framing::encode(
                    black_box(format),
                    black_box(crc),
                    black_box(&message),
                    black_box(&mut bytes),
                )
                .unwrap(),
            );
            black_box(
                framing::decode(
                    black_box(format),
                    black_box(crc),
                    black_box(&bytes[..length]),
                )
                .unwrap(),
            );
        }
        for batch in 0..batches {
            // Alternate operation order to avoid always timing decode second.
            for encode in if batch % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            } {
                let started = Instant::now();
                for _ in 0..iterations {
                    if encode {
                        black_box(
                            framing::encode(
                                black_box(format),
                                black_box(crc),
                                black_box(&message),
                                black_box(&mut bytes),
                            )
                            .unwrap(),
                        );
                    } else {
                        black_box(
                            framing::decode(
                                black_box(format),
                                black_box(crc),
                                black_box(&bytes[..length]),
                            )
                            .unwrap(),
                        );
                    }
                }
                let elapsed = started.elapsed().as_nanos();
                // Nothing is printed/allocated inside the timed loop. The portable
                // functions include validation, copies/conversion and CRC framing.
                println!(
                    "{}",
                    json!({
                        "format":format.name(), "crc":crc.name(), "fixture":input["fixture"],
                        "kind":message.kind, "operation":if encode {"encode"} else {"decode"},
                        "batch":batch, "iterations":iterations, "elapsed_ns":elapsed as u64,
                        "ns_per_operation":elapsed as f64 / f64::from(iterations),
                        "body_bytes":length - if crc == CrcMode::On {8} else {0},
                        "datagram_bytes":length
                    })
                );
            }
        }
    }
    Ok(())
}
