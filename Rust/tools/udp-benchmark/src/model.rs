//! Serializable result schema used by JSON, CSV, and Markdown reporting.

use serde::{Deserialize, Serialize};

use crate::stats::LatencyStats;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub schema_version: u32,
    pub tool_version: String,
    pub unix_timestamp_seconds: u64,
    pub host_name: String,
    pub operating_system: String,
    pub git_commit: String,
    pub git_dirty: bool,
    pub firmware_variant: String,
    pub firmware_version: String,
    pub board_ip: String,
    pub board_mac: String,
    pub udp_port: u16,
    pub spi_hz: Option<u32>,
    pub random_seed: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionalResult {
    pub payload_bytes: usize,
    pub attempts: u64,
    pub valid_replies: u64,
    pub timeouts: u64,
    pub corrupt: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LatencyResult {
    pub payload_bytes: usize,
    pub attempts: u64,
    pub timeouts: u64,
    pub corrupt: u64,
    pub statistics: LatencyStats,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub samples_ns: Vec<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PacketCounters {
    pub planned: u64,
    pub sent: u64,
    pub valid_replies: u64,
    pub missing: u64,
    pub late: u64,
    pub duplicates: u64,
    pub reordered: u64,
    pub corrupt: u64,
    pub foreign: u64,
    pub send_errors: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoadResult {
    pub payload_bytes: Option<usize>,
    pub offered_target_mbps: f64,
    pub measurement_seconds: f64,
    pub drain_seconds: f64,
    pub window: usize,
    pub offered_mbps: f64,
    pub goodput_mbps: f64,
    pub sent_packets_per_second: f64,
    pub valid_packets_per_second: f64,
    pub loss_percent: f64,
    pub counters: PacketCounters,
    pub latency: LatencyStats,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapacityResult {
    pub payload_bytes: usize,
    pub zero_loss_mbps: f64,
    pub peak_goodput_mbps: f64,
    pub saturation_observed: bool,
    pub trials: Vec<LoadResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BurstResult {
    pub payload_bytes: usize,
    pub burst_packets: usize,
    pub repetitions: u64,
    pub complete_bursts: u64,
    pub complete_percent: f64,
    pub counters: PacketCounters,
    pub latency: LatencyStats,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoakResult {
    pub target_mbps: f64,
    pub payload_sizes: Vec<usize>,
    pub duration_seconds: u64,
    pub interval_seconds: u64,
    pub intervals: Vec<LoadResult>,
    pub counters: PacketCounters,
    pub latency: LatencyStats,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SuiteResult {
    pub metadata: Metadata,
    pub functional: Vec<FunctionalResult>,
    pub latency: Vec<LatencyResult>,
    pub capacity: Vec<CapacityResult>,
    pub bursts: Vec<BurstResult>,
    pub soak: SoakResult,
}
