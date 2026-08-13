//! Reproducible host-side benchmark for the NUCLEO UDP echo firmware.

mod model;
mod protocol;
mod runner;
mod stats;

use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand};
use model::{
    CapacityResult, Metadata, PacketCounters, StreamSweepPoint, StreamSweepResult, SuiteResult,
};
use runner::BenchSocket;
use serde::Serialize;

const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(version, about = "Benchmark a UDP echo server from this computer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Measure stop-and-wait application round-trip latency.
    Latency(LatencyArgs),
    /// Find sustained zero-loss throughput and the saturation region.
    Throughput(ThroughputArgs),
    /// Measure loss and latency for back-to-back packet bursts.
    Burst(BurstArgs),
    /// Run mixed-size traffic for an extended reliability interval.
    Soak(SoakArgs),
    /// Run a repeatable fixed-size, fixed-rate datagram stream.
    Stream(StreamArgs),
    /// Sweep the stream rate from 1 through 20 kHz.
    StreamSweep(StreamSweepArgs),
    /// Run the functional, latency, throughput, burst, and soak phases.
    Suite(SuiteArgs),
    /// Combine suite JSON files into one Markdown trade-study report.
    Compare(CompareArgs),
}

#[derive(Clone, Args)]
struct TargetArgs {
    /// Board address, optionally including a port. Port 7 is the default.
    #[arg(long)]
    board: String,
    #[arg(long, default_value_t = 7)]
    port: u16,
}

#[derive(Args)]
struct LatencyArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long, default_value = "1,32,64,256,512,1024,1472")]
    sizes: String,
    #[arg(long, default_value_t = 10_000)]
    samples: u64,
    #[arg(long, default_value_t = 2_000)]
    warmup_ms: u64,
    #[arg(long, default_value_t = 250)]
    timeout_ms: u64,
    #[arg(long, default_value = "benchmark-results/latency")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 0x7230_0001)]
    run_id: u64,
}

#[derive(Args)]
struct ThroughputArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long, default_value = "64,256,512,1024,1472")]
    sizes: String,
    #[arg(long, default_value = "0.25,0.5,1,2,4,8,16,32,64,80")]
    rates_mbps: String,
    #[arg(long, default_value_t = 10)]
    duration_seconds: u64,
    #[arg(long, default_value_t = 2_000)]
    warmup_ms: u64,
    #[arg(long, default_value_t = 2_000)]
    drain_ms: u64,
    #[arg(long, default_value_t = 250)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 256)]
    window: usize,
    #[arg(long, default_value_t = 2)]
    refinements: usize,
    #[arg(long, default_value = "benchmark-results/throughput")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 0x7230_0002)]
    run_id: u64,
}

#[derive(Args)]
struct BurstArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long, default_value = "64,512,1472")]
    sizes: String,
    #[arg(long, default_value = "1,4,8,16,32,64,128")]
    bursts: String,
    #[arg(long, default_value_t = 100)]
    repetitions: u64,
    #[arg(long, default_value_t = 250)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 2)]
    idle_ms: u64,
    #[arg(long, default_value = "benchmark-results/burst")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 0x7230_0003)]
    run_id: u64,
}

#[derive(Args)]
struct SoakArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long, default_value = "64,256,512,1024,1472")]
    sizes: String,
    #[arg(long)]
    rate_mbps: f64,
    #[arg(long, default_value_t = 900)]
    duration_seconds: u64,
    #[arg(long, default_value_t = 60)]
    interval_seconds: u64,
    #[arg(long, default_value_t = 2_000)]
    drain_ms: u64,
    #[arg(long, default_value_t = 250)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 256)]
    window: usize,
    #[arg(long, default_value_t = 0x5eed_0723)]
    seed: u64,
    #[arg(long, default_value = "benchmark-results/soak")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 0x7230_0004)]
    run_id: u64,
}

#[derive(Args)]
struct StreamArgs {
    #[command(flatten)]
    target: TargetArgs,
    /// Command datagram size, including the benchmark sequence header.
    #[arg(long, default_value_t = 100)]
    payload_bytes: usize,
    /// Commands sent per second.
    #[arg(long, default_value_t = 1_000.0)]
    rate_hz: f64,
    #[arg(long, default_value_t = 900)]
    duration_seconds: u64,
    #[arg(long, default_value_t = 60)]
    interval_seconds: u64,
    #[arg(long, default_value_t = 2_000)]
    drain_ms: u64,
    #[arg(long, default_value_t = 50)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 256)]
    window: usize,
    #[arg(long, default_value = "benchmark-results/stream")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 0x4653_4d07_2300_0001)]
    run_id: u64,
}

#[derive(Args)]
struct StreamSweepArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long, default_value_t = 100)]
    payload_bytes: usize,
    /// Comma-separated command rates in Hz.
    #[arg(
        long,
        default_value = "1000,2000,3000,4000,5000,6000,7000,8000,9000,10000,11000,12000,13000,14000,15000,16000,17000,18000,19000,20000"
    )]
    rates_hz: String,
    #[arg(long, default_value_t = 10)]
    duration_seconds: u64,
    #[arg(long, default_value_t = 2_000)]
    drain_ms: u64,
    #[arg(long, default_value_t = 50)]
    timeout_ms: u64,
    /// Large enough that loss recovery does not throttle high-rate trials.
    #[arg(long, default_value_t = 8_192)]
    window: usize,
    #[arg(long, default_value = "benchmark-results/stream-sweep")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 0x4653_4d07_2300_1000)]
    run_id: u64,
}

#[derive(Args)]
struct SuiteArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long)]
    variant: String,
    #[arg(long)]
    mac: String,
    #[arg(long)]
    firmware_version: String,
    #[arg(long)]
    spi_hz: Option<u32>,
    /// Set this for W5500 offload, whose SEND command cannot echo zero bytes.
    #[arg(long)]
    no_zero_byte: bool,
    #[arg(long, default_value_t = 0x5eed_0723)]
    seed: u64,
    #[arg(long, default_value_t = 10_000)]
    latency_samples: u64,
    #[arg(long, default_value_t = 10)]
    throughput_seconds: u64,
    #[arg(long, default_value_t = 100)]
    burst_repetitions: u64,
    #[arg(long, default_value_t = 900)]
    soak_seconds: u64,
    #[arg(long, default_value_t = 60)]
    soak_interval_seconds: u64,
    #[arg(long, default_value_t = 256)]
    window: usize,
    #[arg(long, default_value = "benchmark-results/suite")]
    output_dir: PathBuf,
}

#[derive(Args)]
struct CompareArgs {
    /// Suite JSON files, one per firmware variant.
    #[arg(long, required = true, num_args = 1..)]
    inputs: Vec<PathBuf>,
    #[arg(long, default_value = "BENCHMARK_REPORT.md")]
    output: PathBuf,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Latency(args) => run_latency(args)?,
        Commands::Throughput(args) => run_throughput(args)?,
        Commands::Burst(args) => run_burst(args)?,
        Commands::Soak(args) => run_soak(args)?,
        Commands::Stream(args) => run_stream(args)?,
        Commands::StreamSweep(args) => run_stream_sweep(args)?,
        Commands::Suite(args) => run_suite(args)?,
        Commands::Compare(args) => run_compare(args)?,
    }
    Ok(())
}

fn run_latency(args: LatencyArgs) -> Result<(), Box<dyn Error>> {
    let socket = BenchSocket::connect(target_address(&args.target)?)?;
    let results = socket.latency(
        &parse_list(&args.sizes)?,
        args.samples,
        Duration::from_millis(args.warmup_ms),
        Duration::from_millis(args.timeout_ms),
        args.run_id,
    )?;
    prepare_output(&args.output_dir)?;
    write_json(args.output_dir.join("latency.json"), &results)?;
    write_latency_csv(args.output_dir.join("latency.csv"), &results)?;
    for result in results {
        println!(
            "latency size={} samples={} p50={:.3} ms p99={:.3} ms max={:.3} ms timeouts={}",
            result.payload_bytes,
            result.statistics.samples,
            ns_ms(result.statistics.p50_ns),
            ns_ms(result.statistics.p99_ns),
            ns_ms(result.statistics.max_ns),
            result.timeouts
        );
    }
    Ok(())
}

fn run_throughput(args: ThroughputArgs) -> Result<(), Box<dyn Error>> {
    let socket = BenchSocket::connect(target_address(&args.target)?)?;
    let results = socket.capacity(
        &parse_list(&args.sizes)?,
        &parse_float_list(&args.rates_mbps)?,
        Duration::from_secs(args.duration_seconds),
        Duration::from_millis(args.warmup_ms),
        Duration::from_millis(args.drain_ms),
        Duration::from_millis(args.timeout_ms),
        args.window,
        args.refinements,
        args.run_id,
    )?;
    prepare_output(&args.output_dir)?;
    write_json(args.output_dir.join("throughput.json"), &results)?;
    write_capacity_csv(args.output_dir.join("throughput.csv"), &results)?;
    Ok(())
}

fn run_burst(args: BurstArgs) -> Result<(), Box<dyn Error>> {
    let socket = BenchSocket::connect(target_address(&args.target)?)?;
    let results = socket.bursts(
        &parse_list(&args.sizes)?,
        &parse_list(&args.bursts)?,
        args.repetitions,
        Duration::from_millis(args.timeout_ms),
        Duration::from_millis(args.idle_ms),
        args.run_id,
    )?;
    prepare_output(&args.output_dir)?;
    write_json(args.output_dir.join("burst.json"), &results)?;
    write_burst_csv(args.output_dir.join("burst.csv"), &results)?;
    Ok(())
}

fn run_soak(args: SoakArgs) -> Result<(), Box<dyn Error>> {
    let socket = BenchSocket::connect(target_address(&args.target)?)?;
    let result = socket.soak(
        &parse_list(&args.sizes)?,
        args.rate_mbps,
        Duration::from_secs(args.duration_seconds),
        Duration::from_secs(args.interval_seconds),
        Duration::from_millis(args.drain_ms),
        Duration::from_millis(args.timeout_ms),
        args.window,
        args.seed,
        args.run_id,
    )?;
    prepare_output(&args.output_dir)?;
    write_json(args.output_dir.join("soak.json"), &result)?;
    write_load_csv(args.output_dir.join("soak.csv"), &result.intervals)?;
    Ok(())
}

fn run_stream(args: StreamArgs) -> Result<(), Box<dyn Error>> {
    let socket = BenchSocket::connect(target_address(&args.target)?)?;
    let result = socket.fixed_rate(
        args.payload_bytes,
        args.rate_hz,
        Duration::from_secs(args.duration_seconds),
        Duration::from_secs(args.interval_seconds),
        Duration::from_millis(args.drain_ms),
        Duration::from_millis(args.timeout_ms),
        args.window,
        args.run_id,
    )?;
    prepare_output(&args.output_dir)?;
    write_json(args.output_dir.join("stream.json"), &result)?;
    write_load_csv(args.output_dir.join("stream.csv"), &result.intervals)?;
    fs::write(
        args.output_dir.join("summary.md"),
        stream_markdown(args.payload_bytes, args.rate_hz, &result),
    )?;
    println!(
        "stream payload={} bytes target={:.3} Hz achieved={:.3} Hz valid={}/{} missing={} late={} p99={:.3} ms",
        args.payload_bytes,
        args.rate_hz,
        result.counters.sent as f64 / args.duration_seconds as f64,
        result.counters.valid_replies,
        result.counters.sent,
        result.counters.missing,
        result.counters.late,
        ns_ms(result.latency.p99_ns),
    );
    Ok(())
}

fn run_stream_sweep(args: StreamSweepArgs) -> Result<(), Box<dyn Error>> {
    if args.duration_seconds == 0 {
        return Err("duration must be greater than zero".into());
    }
    let rates = parse_float_list(&args.rates_hz)?;
    let socket = BenchSocket::connect(target_address(&args.target)?)?;
    let mut points = Vec::with_capacity(rates.len());

    for (index, target_hz) in rates.into_iter().enumerate() {
        let result = socket.fixed_rate(
            args.payload_bytes,
            target_hz,
            Duration::from_secs(args.duration_seconds),
            Duration::from_secs(args.duration_seconds),
            Duration::from_millis(args.drain_ms),
            Duration::from_millis(args.timeout_ms),
            args.window,
            args.run_id.wrapping_add(index as u64),
        )?;
        let achieved_hz = result.counters.sent as f64 / args.duration_seconds as f64;
        let reliable = stream_reliable(&result, target_hz, achieved_hz);
        let error_events = stream_error_events(&result.counters);
        println!(
            "stream sweep target={:.3} kHz achieved={:.3} kHz reliable={} errors={} valid={}/{} missing={} late={} duplicate={} reordered={} corrupt={} foreign={} send_errors={} p99={:.3} ms",
            target_hz / 1_000.0,
            achieved_hz / 1_000.0,
            reliable,
            error_events,
            result.counters.valid_replies,
            result.counters.sent,
            result.counters.missing,
            result.counters.late,
            result.counters.duplicates,
            result.counters.reordered,
            result.counters.corrupt,
            result.counters.foreign,
            result.counters.send_errors,
            ns_ms(result.latency.p99_ns),
        );
        points.push(StreamSweepPoint {
            target_hz,
            achieved_hz,
            reliable,
            error_events,
            result,
        });
    }

    let sweep = StreamSweepResult {
        payload_bytes: args.payload_bytes,
        duration_seconds_per_rate: args.duration_seconds,
        highest_reliable_hz: points
            .iter()
            .take_while(|point| point.reliable)
            .last()
            .map(|point| point.target_hz),
        first_unreliable_hz: points
            .iter()
            .find(|point| !point.reliable)
            .map(|point| point.target_hz),
        points,
    };
    prepare_output(&args.output_dir)?;
    write_json(args.output_dir.join("stream-sweep.json"), &sweep)?;
    write_stream_sweep_csv(args.output_dir.join("stream-sweep.csv"), &sweep)?;
    fs::write(
        args.output_dir.join("summary.md"),
        stream_sweep_markdown(&sweep),
    )?;
    println!(
        "stream reliable range: 1 kHz -> {}; first unreliable: {}",
        format_rate(sweep.highest_reliable_hz),
        format_rate(sweep.first_unreliable_hz),
    );
    Ok(())
}

fn run_suite(args: SuiteArgs) -> Result<(), Box<dyn Error>> {
    prepare_output(&args.output_dir)?;
    let target = target_address(&args.target)?;
    let socket = BenchSocket::connect(target)?;
    let base_run = args.seed ^ unix_timestamp_seconds();
    let timeout = Duration::from_millis(250);
    let drain = Duration::from_secs(2);

    println!("functional gate: {}", args.variant);
    let functional_sizes = if args.no_zero_byte {
        vec![1, 32, 256, 1472]
    } else {
        vec![0, 1, 32, 256, 1472]
    };
    let functional = socket.functional(&functional_sizes, 5, timeout, base_run)?;
    if functional
        .iter()
        .any(|result| result.valid_replies != result.attempts || result.corrupt != 0)
    {
        return Err("functional gate failed; refusing to record performance".into());
    }

    println!("latency phase: {}", args.variant);
    let latency = socket.latency(
        &[1, 32, 64, 256, 512, 1024, 1472],
        args.latency_samples,
        Duration::from_secs(2),
        timeout,
        base_run ^ 0x1000,
    )?;
    if latency
        .iter()
        .any(|result| result.statistics.samples < args.latency_samples)
    {
        return Err("latency phase did not collect the requested successful samples".into());
    }

    println!("throughput phase: {}", args.variant);
    let capacity = socket.capacity(
        &[64, 256, 512, 1024, 1472],
        &[0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 80.0],
        Duration::from_secs(args.throughput_seconds),
        Duration::from_secs(2),
        drain,
        timeout,
        args.window,
        2,
        base_run ^ 0x2000,
    )?;
    if capacity.iter().any(|result| !result.saturation_observed) {
        return Err("throughput phase did not reach saturation for every payload size".into());
    }

    println!("burst phase: {}", args.variant);
    let bursts = socket.bursts(
        &[64, 512, 1472],
        &[1, 4, 8, 16, 32, 64, 128],
        args.burst_repetitions,
        timeout,
        Duration::from_millis(2),
        base_run ^ 0x3000,
    )?;

    let soak_rate = capacity
        .iter()
        .map(|result| result.zero_loss_mbps)
        .filter(|rate| *rate > 0.0)
        .fold(f64::INFINITY, f64::min)
        * 0.70;
    if !soak_rate.is_finite() || soak_rate <= 0.0 {
        return Err("no positive zero-loss rate was available for the soak test".into());
    }
    println!(
        "soak phase: {} at {:.3} Mbps for {} seconds",
        args.variant, soak_rate, args.soak_seconds
    );
    let soak = socket.soak(
        &[64, 256, 512, 1024, 1472],
        soak_rate,
        Duration::from_secs(args.soak_seconds),
        Duration::from_secs(args.soak_interval_seconds),
        drain,
        timeout,
        args.window,
        args.seed,
        base_run ^ 0x4000,
    )?;

    let suite = SuiteResult {
        metadata: metadata(&args, target),
        functional,
        latency,
        capacity,
        bursts,
        soak,
    };
    write_json(args.output_dir.join("suite.json"), &suite)?;
    write_latency_csv(args.output_dir.join("latency.csv"), &suite.latency)?;
    write_capacity_csv(args.output_dir.join("throughput.csv"), &suite.capacity)?;
    write_burst_csv(args.output_dir.join("burst.csv"), &suite.bursts)?;
    write_load_csv(args.output_dir.join("soak.csv"), &suite.soak.intervals)?;
    fs::write(args.output_dir.join("summary.md"), suite_markdown(&suite))?;
    println!("suite complete: {}", args.output_dir.display());
    Ok(())
}

fn run_compare(args: CompareArgs) -> Result<(), Box<dyn Error>> {
    let mut suites = Vec::new();
    for input in &args.inputs {
        suites.push(serde_json::from_reader::<_, SuiteResult>(File::open(
            input,
        )?)?);
    }
    if suites.len() < 2 {
        return Err("comparison requires at least two suite files".into());
    }
    if let Some(parent) = args
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, comparison_markdown(&suites))?;
    println!("comparison report: {}", args.output.display());
    Ok(())
}

fn metadata(args: &SuiteArgs, target: SocketAddr) -> Metadata {
    Metadata {
        schema_version: 1,
        tool_version: TOOL_VERSION.to_owned(),
        unix_timestamp_seconds: unix_timestamp_seconds(),
        host_name: std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "unknown".to_owned()),
        operating_system: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        git_commit: command_output("git", &["rev-parse", "HEAD"]),
        git_dirty: !command_output("git", &["status", "--porcelain"]).is_empty(),
        firmware_variant: args.variant.clone(),
        firmware_version: args.firmware_version.clone(),
        board_ip: target.ip().to_string(),
        board_mac: args.mac.clone(),
        udp_port: target.port(),
        spi_hz: args.spi_hz,
        random_seed: args.seed,
    }
}

fn target_address(target: &TargetArgs) -> Result<SocketAddr, Box<dyn Error>> {
    if let Ok(address) = target.board.parse::<SocketAddr>() {
        return Ok(address);
    }
    Ok(format!("{}:{}", target.board, target.port).parse()?)
}

fn parse_list(value: &str) -> Result<Vec<usize>, Box<dyn Error>> {
    let values = value
        .split(',')
        .map(|part| part.trim().parse::<usize>())
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err("list must not be empty".into());
    }
    Ok(values)
}

fn parse_float_list(value: &str) -> Result<Vec<f64>, Box<dyn Error>> {
    let values = value
        .split(',')
        .map(|part| part.trim().parse::<f64>())
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty()
        || values
            .iter()
            .any(|number| !number.is_finite() || *number <= 0.0)
    {
        return Err("rates must be finite positive numbers".into());
    }
    Ok(values)
}

fn prepare_output(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(path)?;
    Ok(())
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<(), Box<dyn Error>> {
    let writer = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(writer, value)?;
    Ok(())
}

fn write_latency_csv(path: PathBuf, results: &[model::LatencyResult]) -> io_result::Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    writeln!(output, "payload_bytes,sample_index,rtt_ns")?;
    for result in results {
        for (index, sample) in result.samples_ns.iter().enumerate() {
            writeln!(output, "{},{index},{sample}", result.payload_bytes)?;
        }
    }
    Ok(())
}

fn write_capacity_csv(path: PathBuf, results: &[CapacityResult]) -> io_result::Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    writeln!(
        output,
        "payload_bytes,target_mbps,offered_mbps,goodput_mbps,loss_percent,sent,valid,missing,late,duplicates,reordered,corrupt,p50_ns,p99_ns"
    )?;
    for capacity in results {
        for trial in &capacity.trials {
            writeln!(
                output,
                "{},{:.6},{:.6},{:.6},{:.9},{},{},{},{},{},{},{},{},{}",
                capacity.payload_bytes,
                trial.offered_target_mbps,
                trial.offered_mbps,
                trial.goodput_mbps,
                trial.loss_percent,
                trial.counters.sent,
                trial.counters.valid_replies,
                trial.counters.missing,
                trial.counters.late,
                trial.counters.duplicates,
                trial.counters.reordered,
                trial.counters.corrupt,
                trial.latency.p50_ns,
                trial.latency.p99_ns
            )?;
        }
    }
    Ok(())
}

fn write_burst_csv(path: PathBuf, results: &[model::BurstResult]) -> io_result::Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    writeln!(
        output,
        "payload_bytes,burst_packets,repetitions,complete_percent,sent,valid,missing,duplicates,corrupt,p50_ns,p99_ns"
    )?;
    for result in results {
        writeln!(
            output,
            "{},{},{},{:.6},{},{},{},{},{},{},{}",
            result.payload_bytes,
            result.burst_packets,
            result.repetitions,
            result.complete_percent,
            result.counters.sent,
            result.counters.valid_replies,
            result.counters.missing,
            result.counters.duplicates,
            result.counters.corrupt,
            result.latency.p50_ns,
            result.latency.p99_ns
        )?;
    }
    Ok(())
}

fn write_load_csv(path: PathBuf, results: &[model::LoadResult]) -> io_result::Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    writeln!(
        output,
        "interval,target_mbps,offered_mbps,goodput_mbps,loss_percent,sent,valid,missing,late,duplicates,reordered,corrupt,p50_ns,p99_ns"
    )?;
    for (index, result) in results.iter().enumerate() {
        writeln!(
            output,
            "{index},{:.6},{:.6},{:.6},{:.9},{},{},{},{},{},{},{},{},{}",
            result.offered_target_mbps,
            result.offered_mbps,
            result.goodput_mbps,
            result.loss_percent,
            result.counters.sent,
            result.counters.valid_replies,
            result.counters.missing,
            result.counters.late,
            result.counters.duplicates,
            result.counters.reordered,
            result.counters.corrupt,
            result.latency.p50_ns,
            result.latency.p99_ns
        )?;
    }
    Ok(())
}

fn write_stream_sweep_csv(path: PathBuf, sweep: &StreamSweepResult) -> io_result::Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    writeln!(
        output,
        "target_hz,achieved_hz,reliable,error_events,sent,valid,missing,late,duplicates,reordered,corrupt,foreign,send_errors,p50_ns,p99_ns,max_ns"
    )?;
    for point in &sweep.points {
        let counters = &point.result.counters;
        let latency = &point.result.latency;
        writeln!(
            output,
            "{:.3},{:.3},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            point.target_hz,
            point.achieved_hz,
            point.reliable,
            point.error_events,
            counters.sent,
            counters.valid_replies,
            counters.missing,
            counters.late,
            counters.duplicates,
            counters.reordered,
            counters.corrupt,
            counters.foreign,
            counters.send_errors,
            latency.p50_ns,
            latency.p99_ns,
            latency.max_ns,
        )?;
    }
    Ok(())
}

fn suite_markdown(suite: &SuiteResult) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "# {} benchmark summary\n\n",
        suite.metadata.firmware_variant
    ));
    output.push_str(&format!(
        "- Firmware: `{}`\n- Board: `{}` / `{}`\n- Host: `{}` ({})\n- Git commit: `{}`{}\n\n",
        suite.metadata.firmware_version,
        suite.metadata.board_ip,
        suite.metadata.board_mac,
        suite.metadata.host_name,
        suite.metadata.operating_system,
        suite.metadata.git_commit,
        if suite.metadata.git_dirty {
            " (dirty worktree)"
        } else {
            ""
        }
    ));
    output.push_str("## Latency\n\n| Bytes | Samples | p50 ms | p95 ms | p99 ms | Max ms | Timeouts |\n|---:|---:|---:|---:|---:|---:|---:|\n");
    for result in &suite.latency {
        output.push_str(&format!(
            "| {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {} |\n",
            result.payload_bytes,
            result.statistics.samples,
            ns_ms(result.statistics.p50_ns),
            ns_ms(result.statistics.p95_ns),
            ns_ms(result.statistics.p99_ns),
            ns_ms(result.statistics.max_ns),
            result.timeouts
        ));
    }
    output.push_str("\n## Capacity\n\n| Bytes | Zero-loss Mbps | Peak goodput Mbps | Saturation observed |\n|---:|---:|---:|:---:|\n");
    for result in &suite.capacity {
        output.push_str(&format!(
            "| {} | {:.3} | {:.3} | {} |\n",
            result.payload_bytes,
            result.zero_loss_mbps,
            result.peak_goodput_mbps,
            if result.saturation_observed {
                "yes"
            } else {
                "no"
            }
        ));
    }
    output.push_str(&format!(
        "\n## Reliability soak\n\nTarget: {:.3} Mbps for {} seconds.\n\n{}\n",
        suite.soak.target_mbps,
        suite.soak.duration_seconds,
        counters_markdown(&suite.soak.counters)
    ));
    output
}

fn comparison_markdown(suites: &[SuiteResult]) -> String {
    let mut output = String::from(
        "# NUCLEO-H723ZG UDP benchmark report\n\nThis report compares UDP-only release firmware using the same Windows host, LAN, packet format, and benchmark procedure. Latency is application round-trip time; throughput is correctly echoed UDP payload goodput.\n\n## Test subjects\n\n| Variant | Firmware | Board IP | MAC | SPI | Host |\n|---|---|---|---|---:|---|\n",
    );
    for suite in suites {
        output.push_str(&format!(
            "| {} | {} | {} | `{}` | {} | {} |\n",
            suite.metadata.firmware_variant,
            suite.metadata.firmware_version,
            suite.metadata.board_ip,
            suite.metadata.board_mac,
            suite
                .metadata
                .spi_hz
                .map(|value| format!("{} MHz", value / 1_000_000))
                .unwrap_or_else(|| "n/a".to_owned()),
            suite.metadata.host_name
        ));
    }

    output.push_str("\n## Latency\n\n| Variant | Bytes | Samples | Min ms | p50 ms | p95 ms | p99 ms | p99.9 ms | Max ms | Timeouts |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for suite in suites {
        for result in &suite.latency {
            let stats = &result.statistics;
            output.push_str(&format!(
                "| {} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {} |\n",
                suite.metadata.firmware_variant,
                result.payload_bytes,
                stats.samples,
                ns_ms(stats.min_ns),
                ns_ms(stats.p50_ns),
                ns_ms(stats.p95_ns),
                ns_ms(stats.p99_ns),
                ns_ms(stats.p999_ns),
                ns_ms(stats.max_ns),
                result.timeouts
            ));
        }
    }

    output.push_str("\n## Sustained bandwidth\n\n| Variant | Bytes | Zero-loss Mbps | Peak goodput Mbps | Saturation observed |\n|---|---:|---:|---:|:---:|\n");
    for suite in suites {
        for result in &suite.capacity {
            output.push_str(&format!(
                "| {} | {} | {:.3} | {:.3} | {} |\n",
                suite.metadata.firmware_variant,
                result.payload_bytes,
                result.zero_loss_mbps,
                result.peak_goodput_mbps,
                if result.saturation_observed {
                    "yes"
                } else {
                    "no"
                }
            ));
        }
    }

    output.push_str("\n## Burst reliability\n\n| Variant | Bytes | Burst | Complete bursts | Missing packets | p99 RTT ms |\n|---|---:|---:|---:|---:|---:|\n");
    for suite in suites {
        for result in &suite.bursts {
            output.push_str(&format!(
                "| {} | {} | {} | {:.1}% | {} | {:.3} |\n",
                suite.metadata.firmware_variant,
                result.payload_bytes,
                result.burst_packets,
                result.complete_percent,
                result.counters.missing,
                ns_ms(result.latency.p99_ns)
            ));
        }
    }

    output.push_str("\n## Reliability soak\n\n| Variant | Duration | Target Mbps | Sent | Valid | Missing | Late | Duplicate | Reordered | Corrupt | p99 RTT ms |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for suite in suites {
        let soak = &suite.soak;
        output.push_str(&format!(
            "| {} | {} s | {:.3} | {} | {} | {} | {} | {} | {} | {} | {:.3} |\n",
            suite.metadata.firmware_variant,
            soak.duration_seconds,
            soak.target_mbps,
            soak.counters.sent,
            soak.counters.valid_replies,
            soak.counters.missing,
            soak.counters.late,
            soak.counters.duplicates,
            soak.counters.reordered,
            soak.counters.corrupt,
            ns_ms(soak.latency.p99_ns)
        ));
    }

    output.push_str("\n## Interpretation notes\n\n- Results include the Windows UDP stack, LAN equipment, firmware scheduling, device driver, and active Ethernet interface.\n- A zero-loss rate is the highest tested target with complete valid replies and at least 98% of requested offered load.\n- Saturation means at least 0.1% loss, less than 95% of requested offered load, or less than 90% returned goodput.\n- Each soak runs at 70% of that variant's lowest measured zero-loss rate across the tested payload sizes.\n- W5500 hardware offload excludes zero-byte functional echo because the chip does not complete a zero-length SEND command.\n- Raw JSON and CSV files are the authoritative measurements behind these rounded tables.\n");
    output
}

fn counters_markdown(counters: &PacketCounters) -> String {
    format!(
        "Sent: {}; valid: {}; missing: {}; late: {}; duplicates: {}; reordered: {}; corrupt: {}.",
        counters.sent,
        counters.valid_replies,
        counters.missing,
        counters.late,
        counters.duplicates,
        counters.reordered,
        counters.corrupt
    )
}

fn stream_markdown(payload_bytes: usize, rate_hz: f64, result: &model::SoakResult) -> String {
    format!(
        "# Stream benchmark\n\n- Payload: {payload_bytes} bytes\n- Target: {rate_hz:.3} datagrams/s ({:.3} Mbit/s one way)\n- Duration: {} seconds\n- Sent: {}\n- Valid replies: {}\n- Missing: {}\n- Late: {}\n- Duplicate: {}\n- Reordered: {}\n- Corrupt: {}\n- RTT p50: {:.3} ms\n- RTT p99: {:.3} ms\n- RTT max: {:.3} ms\n",
        payload_bytes as f64 * rate_hz * 8.0 / 1_000_000.0,
        result.duration_seconds,
        result.counters.sent,
        result.counters.valid_replies,
        result.counters.missing,
        result.counters.late,
        result.counters.duplicates,
        result.counters.reordered,
        result.counters.corrupt,
        ns_ms(result.latency.p50_ns),
        ns_ms(result.latency.p99_ns),
        ns_ms(result.latency.max_ns),
    )
}

fn stream_reliable(result: &model::SoakResult, target_hz: f64, achieved_hz: f64) -> bool {
    let counters = &result.counters;
    achieved_hz >= target_hz * 0.98
        && counters.sent == counters.planned
        && counters.valid_replies == counters.sent
        && counters.missing == 0
        && counters.late == 0
        && counters.duplicates == 0
        && counters.reordered == 0
        && counters.corrupt == 0
        && counters.foreign == 0
        && counters.send_errors == 0
}

fn stream_error_events(counters: &PacketCounters) -> u64 {
    counters.missing
        + counters.late
        + counters.duplicates
        + counters.reordered
        + counters.corrupt
        + counters.foreign
        + counters.send_errors
}

fn stream_sweep_markdown(sweep: &StreamSweepResult) -> String {
    let mut output = format!(
        "# Stream rate sweep\n\n- Payload: {} bytes\n- Duration per rate: {} seconds\n- Reliable range: 1 kHz -> {}\n- First unreliable rate: {}\n\n| Target kHz | Achieved kHz | Reliable | Error events | Valid / sent | Missing | Late | Duplicate | Reordered | Corrupt | Foreign | Send errors | p50 ms | p99 ms | Max ms |\n|---:|---:|:---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        sweep.payload_bytes,
        sweep.duration_seconds_per_rate,
        format_rate(sweep.highest_reliable_hz),
        format_rate(sweep.first_unreliable_hz),
    );
    for point in &sweep.points {
        let counters = &point.result.counters;
        let latency = &point.result.latency;
        output.push_str(&format!(
            "| {:.3} | {:.3} | {} | {} | {} / {} | {} | {} | {} | {} | {} | {} | {} | {:.3} | {:.3} | {:.3} |\n",
            point.target_hz / 1_000.0,
            point.achieved_hz / 1_000.0,
            if point.reliable { "yes" } else { "no" },
            point.error_events,
            counters.valid_replies,
            counters.sent,
            counters.missing,
            counters.late,
            counters.duplicates,
            counters.reordered,
            counters.corrupt,
            counters.foreign,
            counters.send_errors,
            ns_ms(latency.p50_ns),
            ns_ms(latency.p99_ns),
            ns_ms(latency.max_ns),
        ));
    }
    output.push_str("\nA point is reliable only when at least 98% of the requested rate is offered and every planned packet is sent and returned exactly once, in order, on time, and uncorrupted. Error events are the sum of missing, late, duplicate, reordered, corrupt, foreign, and send-error counters; one packet can contribute more than one event.\n");
    output
}

fn format_rate(rate_hz: Option<f64>) -> String {
    rate_hz
        .map(|rate| format!("{:.3} kHz", rate / 1_000.0))
        .unwrap_or_else(|| "none observed".to_owned())
}

fn ns_ms(value: u64) -> f64 {
    value as f64 / 1_000_000.0
}

fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn command_output(program: &str, arguments: &[&str]) -> String {
    Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

// Avoid confusing this module with the crate-level `Result` alias in function
// signatures that return only filesystem I/O errors.
mod io_result {
    pub type Result<T> = std::io::Result<T>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SoakResult;
    use crate::stats::LatencyStats;

    fn stream_result(counters: PacketCounters) -> SoakResult {
        SoakResult {
            target_mbps: 0.8,
            payload_sizes: vec![100],
            duration_seconds: 1,
            interval_seconds: 1,
            intervals: Vec::new(),
            counters,
            latency: LatencyStats::default(),
        }
    }

    #[test]
    fn stream_reliability_requires_complete_exact_delivery() {
        let perfect = PacketCounters {
            planned: 1_000,
            sent: 1_000,
            valid_replies: 1_000,
            ..PacketCounters::default()
        };
        assert!(stream_reliable(
            &stream_result(perfect.clone()),
            1_000.0,
            1_000.0
        ));

        let mut loss = perfect.clone();
        loss.valid_replies -= 1;
        loss.missing = 1;
        assert!(!stream_reliable(&stream_result(loss), 1_000.0, 1_000.0));

        let mut late = perfect;
        late.late = 1;
        assert!(!stream_reliable(&stream_result(late), 1_000.0, 1_000.0));
    }

    #[test]
    fn stream_reliability_rejects_an_unachieved_offer() {
        let counters = PacketCounters {
            planned: 960,
            sent: 960,
            valid_replies: 960,
            ..PacketCounters::default()
        };
        assert!(!stream_reliable(&stream_result(counters), 1_000.0, 960.0));
    }

    #[test]
    fn stream_error_events_sums_the_logged_error_categories() {
        let counters = PacketCounters {
            missing: 1,
            late: 2,
            duplicates: 3,
            reordered: 4,
            corrupt: 5,
            foreign: 6,
            send_errors: 7,
            ..PacketCounters::default()
        };
        assert_eq!(stream_error_events(&counters), 28);
    }
}
