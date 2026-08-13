//! Client for benchmark-only MCU profiling telemetry.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use crate::model::ProfileMetrics;

const PORT: u16 = 5001;
const MAGIC: &[u8; 8] = b"STRMPRF1";
const RESPONSE_SIZE: usize = 48;

pub struct ProfileClient {
    socket: UdpSocket,
}

impl ProfileClient {
    pub fn connect(mut target: SocketAddr) -> io::Result<Self> {
        target.set_port(PORT);
        let bind = if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(bind)?;
        socket.connect(target)?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;
        Ok(Self { socket })
    }

    pub fn reset(&self) -> io::Result<()> {
        self.query(b'R', 0).map(|_| ())
    }

    pub fn snapshot(&self, valid_packets: u64) -> io::Result<ProfileMetrics> {
        self.query(b'S', valid_packets)
    }

    fn query(&self, operation: u8, valid_packets: u64) -> io::Result<ProfileMetrics> {
        let mut request = [0u8; 9];
        request[..8].copy_from_slice(MAGIC);
        request[8] = operation;
        let mut response = [0u8; RESPONSE_SIZE];
        let mut last_error = None;

        for _ in 0..3 {
            self.socket.send(&request)?;
            match self.socket.recv(&mut response) {
                Ok(RESPONSE_SIZE) if &response[..8] == MAGIC => {
                    return decode(&response, valid_packets);
                }
                Ok(_) => last_error = Some(io::Error::other("invalid profiling response")),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| io::Error::other("profiling query failed")))
    }
}

fn decode(bytes: &[u8; RESPONSE_SIZE], valid_packets: u64) -> io::Result<ProfileMetrics> {
    let cpu_hz = u32_at(bytes, 8);
    let time_ticks_hz = u32_at(bytes, 12);
    let busy_cycles = u64_at(bytes, 16);
    let elapsed_ticks = u64_at(bytes, 24);
    let executor_polls = u64_at(bytes, 32);
    let stack_high_water_bytes = u32_at(bytes, 40);
    let stack_capacity_bytes = u32_at(bytes, 44);
    if cpu_hz == 0 || time_ticks_hz == 0 || stack_capacity_bytes > 128 * 1024 {
        return Err(io::Error::other("invalid profiling metric values"));
    }
    let elapsed_seconds = elapsed_ticks as f64 / time_ticks_hz as f64;
    Ok(ProfileMetrics {
        cpu_hz,
        time_ticks_hz,
        busy_cycles,
        elapsed_ticks,
        executor_polls,
        executor_cpu_percent: if elapsed_seconds == 0.0 {
            0.0
        } else {
            busy_cycles as f64 / (cpu_hz as f64 * elapsed_seconds) * 100.0
        },
        cycles_per_valid_packet: if valid_packets == 0 {
            0.0
        } else {
            busy_cycles as f64 / valid_packets as f64
        },
        stack_high_water_bytes,
        stack_capacity_bytes,
        static_ram_bytes: 128 * 1024 - stack_capacity_bytes,
    })
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_decode_calculates_cpu_and_memory() {
        let mut bytes = [0u8; RESPONSE_SIZE];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&400_000_000u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&32_768u32.to_le_bytes());
        bytes[16..24].copy_from_slice(&200_000_000u64.to_le_bytes());
        bytes[24..32].copy_from_slice(&32_768u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&1_000u64.to_le_bytes());
        bytes[40..44].copy_from_slice(&2_048u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&100_000u32.to_le_bytes());
        let metrics = decode(&bytes, 1_000).unwrap();
        assert_eq!(metrics.executor_cpu_percent, 50.0);
        assert_eq!(metrics.cycles_per_valid_packet, 200_000.0);
        assert_eq!(metrics.static_ram_bytes, 31_072);
    }

    #[test]
    fn reset_reply_may_have_zero_elapsed_time() {
        let mut bytes = [0u8; RESPONSE_SIZE];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&400_000_000u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&32_768u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&100_000u32.to_le_bytes());

        let metrics = decode(&bytes, 0).unwrap();
        assert_eq!(metrics.executor_cpu_percent, 0.0);
    }
}
