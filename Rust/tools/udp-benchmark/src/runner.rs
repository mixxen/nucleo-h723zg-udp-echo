//! Network workload implementations.

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, Socket, Type};

use crate::model::{
    BurstResult, CapacityResult, FunctionalResult, LatencyResult, LoadResult, PacketCounters,
    SoakResult,
};
use crate::protocol::{self, HEADER_LEN, Header};
use crate::stats::summarize;

const RECEIVE_BUFFER_BYTES: usize = 4 * 1024 * 1024;
const SOCKET_POLL: Duration = Duration::from_millis(10);

pub struct BenchSocket {
    socket: UdpSocket,
}

impl BenchSocket {
    pub fn connect(target: SocketAddr) -> io::Result<Self> {
        let domain = if target.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        };
        let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_recv_buffer_size(RECEIVE_BUFFER_BYTES)?;
        socket.set_send_buffer_size(RECEIVE_BUFFER_BYTES)?;
        let bind_address = match target.ip() {
            IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            IpAddr::V6(_) => "[::]:0".parse().expect("valid IPv6 wildcard"),
        };
        socket.bind(&bind_address.into())?;
        socket.connect(&target.into())?;
        socket.set_read_timeout(Some(SOCKET_POLL))?;
        Ok(Self {
            socket: socket.into(),
        })
    }

    pub fn functional(
        &self,
        payload_sizes: &[usize],
        repetitions: u64,
        timeout: Duration,
        run_id: u64,
    ) -> io::Result<Vec<FunctionalResult>> {
        payload_sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                let result = self.stop_and_wait(
                    *size,
                    repetitions,
                    Duration::ZERO,
                    timeout,
                    run_id.wrapping_add(index as u64),
                    false,
                )?;
                Ok(FunctionalResult {
                    payload_bytes: *size,
                    attempts: result.attempts,
                    valid_replies: result.statistics.samples,
                    timeouts: result.timeouts,
                    corrupt: result.corrupt,
                })
            })
            .collect()
    }

    pub fn latency(
        &self,
        payload_sizes: &[usize],
        successful_samples: u64,
        warmup: Duration,
        timeout: Duration,
        run_id: u64,
    ) -> io::Result<Vec<LatencyResult>> {
        payload_sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                self.stop_and_wait(
                    *size,
                    successful_samples,
                    warmup,
                    timeout,
                    run_id.wrapping_add(index as u64),
                    true,
                )
            })
            .collect()
    }

    fn stop_and_wait(
        &self,
        size: usize,
        target_successes: u64,
        warmup: Duration,
        timeout: Duration,
        run_id: u64,
        retain_samples: bool,
    ) -> io::Result<LatencyResult> {
        self.socket.set_read_timeout(Some(timeout))?;
        flush(&self.socket)?;

        let mut sequence = 0u64;
        if !warmup.is_zero() {
            let deadline = Instant::now() + warmup;
            while Instant::now() < deadline {
                let packet = raw_packet(size, run_id, sequence);
                let _ = self.socket.send(&packet);
                let mut reply = vec![0; size.max(1) + 64];
                let _ = self.socket.recv(&mut reply);
                sequence = sequence.wrapping_add(1);
            }
            flush(&self.socket)?;
        }

        let mut samples = Vec::with_capacity(target_successes as usize);
        let mut attempts = 0u64;
        let mut timeouts = 0u64;
        let mut corrupt = 0u64;
        // A broken interface must terminate instead of retrying forever.
        let maximum_attempts = target_successes.saturating_mul(2).max(10);

        while samples.len() < target_successes as usize && attempts < maximum_attempts {
            let packet = raw_packet(size, run_id, sequence);
            attempts += 1;
            let sent_at = Instant::now();
            self.socket.send(&packet)?;

            let mut reply = vec![0; size.max(1) + 64];
            match self.socket.recv(&mut reply) {
                Ok(length) if reply[..length] == packet => {
                    samples.push(elapsed_ns(sent_at));
                }
                Ok(_) => corrupt += 1,
                Err(error) if is_timeout(&error) => timeouts += 1,
                Err(error) => return Err(error),
            }
            sequence = sequence.wrapping_add(1);
        }

        let statistics = summarize(&samples);
        Ok(LatencyResult {
            payload_bytes: size,
            attempts,
            timeouts,
            corrupt,
            statistics,
            samples_ns: if retain_samples { samples } else { Vec::new() },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn capacity(
        &self,
        payload_sizes: &[usize],
        coarse_rates_mbps: &[f64],
        measurement: Duration,
        warmup: Duration,
        drain: Duration,
        timeout: Duration,
        window: usize,
        refinements: usize,
        run_id: u64,
    ) -> io::Result<Vec<CapacityResult>> {
        let mut results = Vec::new();
        for (size_index, size) in payload_sizes.iter().copied().enumerate() {
            let mut trials = Vec::new();
            let mut lower_zero = 0.0f64;
            let mut upper_saturated = None;

            for rate in coarse_rates_mbps.iter().copied() {
                self.warm_load(size, rate, warmup, timeout, window, run_id)?;
                let raw = self.load(
                    PayloadPlan::fixed(size, rate, measurement),
                    rate,
                    measurement,
                    drain,
                    timeout,
                    window,
                    run_id
                        .wrapping_add((size_index as u64) << 32)
                        .wrapping_add(trials.len() as u64),
                )?;
                let saturated = is_saturated(&raw.result);
                if is_zero_loss(&raw.result) {
                    lower_zero = lower_zero.max(rate);
                }
                println!(
                    "  throughput size={size} target={rate:.3} Mbps offered={:.3} goodput={:.3} loss={:.4}%",
                    raw.result.offered_mbps, raw.result.goodput_mbps, raw.result.loss_percent
                );
                trials.push(raw.result);
                if saturated {
                    upper_saturated = Some(rate);
                    break;
                }
            }

            if let Some(mut upper) = upper_saturated {
                let mut lower = lower_zero;
                for refinement in 0..refinements {
                    if lower <= 0.0 || upper - lower < 0.05 {
                        break;
                    }
                    let rate = (lower + upper) / 2.0;
                    self.warm_load(size, rate, warmup, timeout, window, run_id)?;
                    let raw = self.load(
                        PayloadPlan::fixed(size, rate, measurement),
                        rate,
                        measurement,
                        drain,
                        timeout,
                        window,
                        run_id
                            .wrapping_add((size_index as u64) << 32)
                            .wrapping_add(0x1000 + refinement as u64),
                    )?;
                    if is_zero_loss(&raw.result) {
                        lower = rate;
                        lower_zero = lower_zero.max(rate);
                    } else {
                        upper = rate;
                    }
                    println!(
                        "  refine size={size} target={rate:.3} Mbps offered={:.3} goodput={:.3} loss={:.4}%",
                        raw.result.offered_mbps, raw.result.goodput_mbps, raw.result.loss_percent
                    );
                    trials.push(raw.result);
                }
            }

            let peak_goodput = trials
                .iter()
                .map(|trial| trial.goodput_mbps)
                .fold(0.0f64, f64::max);
            results.push(CapacityResult {
                payload_bytes: size,
                zero_loss_mbps: lower_zero,
                peak_goodput_mbps: peak_goodput,
                saturation_observed: upper_saturated.is_some(),
                trials,
            });
        }
        Ok(results)
    }

    fn warm_load(
        &self,
        size: usize,
        rate_mbps: f64,
        duration: Duration,
        timeout: Duration,
        window: usize,
        run_id: u64,
    ) -> io::Result<()> {
        if duration.is_zero() {
            return Ok(());
        }
        let _ = self.load(
            PayloadPlan::fixed(size, rate_mbps, duration),
            rate_mbps,
            duration,
            Duration::from_millis(100),
            timeout,
            window,
            run_id ^ 0xfeed_face_dead_beef,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bursts(
        &self,
        payload_sizes: &[usize],
        burst_sizes: &[usize],
        repetitions: u64,
        timeout: Duration,
        idle: Duration,
        run_id: u64,
    ) -> io::Result<Vec<BurstResult>> {
        self.socket.set_read_timeout(Some(SOCKET_POLL))?;
        flush(&self.socket)?;
        let start = Instant::now();
        let mut results = Vec::new();
        let mut global_sequence = 0u64;

        for size in payload_sizes.iter().copied() {
            if size < HEADER_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("burst payload must be at least {HEADER_LEN} bytes"),
                ));
            }
            for burst_packets in burst_sizes.iter().copied() {
                let mut counters = PacketCounters::default();
                let mut complete_bursts = 0u64;
                let mut latency_samples = Vec::new();
                for _ in 0..repetitions {
                    thread::sleep(idle);
                    flush(&self.socket)?;
                    let first_sequence = global_sequence;
                    let mut sent_times = Vec::with_capacity(burst_packets);
                    for offset in 0..burst_packets {
                        let sequence = first_sequence + offset as u64;
                        let sent_ns = elapsed_ns(start);
                        let packet = protocol::encode(
                            size,
                            Header {
                                run_id,
                                sequence,
                                sent_ns,
                            },
                        )
                        .map_err(invalid_input)?;
                        match self.socket.send(&packet) {
                            Ok(_) => {
                                counters.sent += 1;
                                sent_times.push(sent_ns);
                            }
                            Err(_) => {
                                counters.send_errors += 1;
                                sent_times.push(0);
                            }
                        }
                    }
                    counters.planned += burst_packets as u64;
                    global_sequence += burst_packets as u64;

                    let deadline = Instant::now() + timeout;
                    let mut seen = vec![false; burst_packets];
                    let mut received_this_burst = 0usize;
                    let mut maximum_offset = None::<usize>;
                    while Instant::now() < deadline && received_this_burst < burst_packets {
                        let mut reply = vec![0; size + 64];
                        match self.socket.recv(&mut reply) {
                            Ok(length) => {
                                let packet = &reply[..length];
                                let Ok(header) = protocol::decode(packet) else {
                                    counters.corrupt += 1;
                                    continue;
                                };
                                if header.run_id != run_id
                                    || header.sequence < first_sequence
                                    || header.sequence >= global_sequence
                                {
                                    counters.foreign += 1;
                                    continue;
                                }
                                let offset = (header.sequence - first_sequence) as usize;
                                if length != size || !protocol::validate_payload(packet, header) {
                                    counters.corrupt += 1;
                                } else if seen[offset] {
                                    counters.duplicates += 1;
                                } else {
                                    seen[offset] = true;
                                    received_this_burst += 1;
                                    counters.valid_replies += 1;
                                    if maximum_offset.is_some_and(|maximum| offset < maximum) {
                                        counters.reordered += 1;
                                    }
                                    maximum_offset = Some(
                                        maximum_offset
                                            .map_or(offset, |maximum| maximum.max(offset)),
                                    );
                                    latency_samples
                                        .push(elapsed_ns(start).saturating_sub(sent_times[offset]));
                                }
                            }
                            Err(error) if is_timeout(&error) => {}
                            Err(error) => return Err(error),
                        }
                    }
                    if received_this_burst == burst_packets {
                        complete_bursts += 1;
                    }
                }
                counters.missing = counters.sent.saturating_sub(counters.valid_replies);
                results.push(BurstResult {
                    payload_bytes: size,
                    burst_packets,
                    repetitions,
                    complete_bursts,
                    complete_percent: percentage(complete_bursts, repetitions),
                    counters,
                    latency: summarize(&latency_samples),
                });
            }
        }
        Ok(results)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn soak(
        &self,
        payload_sizes: &[usize],
        target_mbps: f64,
        duration: Duration,
        interval: Duration,
        drain: Duration,
        timeout: Duration,
        window: usize,
        seed: u64,
        run_id: u64,
    ) -> io::Result<SoakResult> {
        let mut results = Vec::new();
        let mut all_samples = Vec::new();
        let mut totals = PacketCounters::default();
        let mut remaining = duration;
        let mut interval_index = 0u64;

        while !remaining.is_zero() {
            let this_interval = remaining.min(interval);
            let plan = PayloadPlan::mixed(
                payload_sizes,
                target_mbps,
                this_interval,
                seed ^ interval_index,
            );
            let raw = self.load(
                plan,
                target_mbps,
                this_interval,
                drain,
                timeout,
                window,
                run_id.wrapping_add(interval_index),
            )?;
            add_counters(&mut totals, &raw.result.counters);
            all_samples.extend(raw.samples);
            println!(
                "  soak interval={}s sent={} valid={} missing={} corrupt={}",
                this_interval.as_secs(),
                raw.result.counters.sent,
                raw.result.counters.valid_replies,
                raw.result.counters.missing,
                raw.result.counters.corrupt
            );
            results.push(raw.result);
            remaining = remaining.saturating_sub(this_interval);
            interval_index += 1;
        }

        Ok(SoakResult {
            target_mbps,
            payload_sizes: payload_sizes.to_vec(),
            duration_seconds: duration.as_secs(),
            interval_seconds: interval.as_secs(),
            intervals: results,
            counters: totals,
            latency: summarize(&all_samples),
        })
    }

    /// Run a fixed-size, fixed-frequency datagram stream.
    #[allow(clippy::too_many_arguments)]
    pub fn fixed_rate(
        &self,
        payload_bytes: usize,
        rate_hz: f64,
        duration: Duration,
        interval: Duration,
        drain: Duration,
        timeout: Duration,
        window: usize,
        run_id: u64,
    ) -> io::Result<SoakResult> {
        if payload_bytes < HEADER_LEN || !rate_hz.is_finite() || rate_hz <= 0.0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("payload must be at least {HEADER_LEN} bytes and rate must be positive"),
            ));
        }

        let target_mbps = payload_bytes as f64 * rate_hz * 8.0 / 1_000_000.0;
        self.soak(
            &[payload_bytes],
            target_mbps,
            duration,
            interval,
            drain,
            timeout,
            window,
            run_id,
            run_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn load(
        &self,
        plan: PayloadPlan,
        target_mbps: f64,
        measurement: Duration,
        drain: Duration,
        timeout: Duration,
        window: usize,
        run_id: u64,
    ) -> io::Result<RawLoad> {
        if window == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "window must be greater than zero",
            ));
        }
        let packet_sizes = Arc::new(plan.sizes);
        let count = packet_sizes.len();
        let states = Arc::new((0..count).map(|_| AtomicU8::new(0)).collect::<Vec<_>>());
        let sent_times = Arc::new((0..count).map(|_| AtomicU64::new(0)).collect::<Vec<_>>());
        let outstanding = Arc::new(AtomicUsize::new(0));
        let sent_count = Arc::new(AtomicU64::new(0));
        let send_errors = Arc::new(AtomicU64::new(0));
        let receiver_socket = self.socket.try_clone()?;
        let sender_socket = self.socket.try_clone()?;
        receiver_socket.set_read_timeout(Some(SOCKET_POLL))?;
        flush(&receiver_socket)?;

        let start = Instant::now() + Duration::from_millis(50);
        let receive_deadline = start + measurement + drain;
        let timeout_ns = timeout.as_nanos().min(u64::MAX as u128) as u64;
        let receiver_states = Arc::clone(&states);
        let receiver_sizes = Arc::clone(&packet_sizes);
        let receiver_times = Arc::clone(&sent_times);
        let receiver_outstanding = Arc::clone(&outstanding);

        let receiver = thread::spawn(move || -> io::Result<ReceiveResult> {
            let mut counters = PacketCounters::default();
            let mut samples = Vec::new();
            let mut maximum_sequence = None::<u64>;
            let mut buffer = vec![0; 65_535];

            wait_until(start);
            while Instant::now() < receive_deadline {
                match receiver_socket.recv(&mut buffer) {
                    Ok(length) => {
                        let packet = &buffer[..length];
                        let Ok(header) = protocol::decode(packet) else {
                            counters.corrupt += 1;
                            continue;
                        };
                        if header.run_id != run_id
                            || header.sequence as usize >= receiver_states.len()
                        {
                            counters.foreign += 1;
                            continue;
                        }
                        let index = header.sequence as usize;
                        if length != receiver_sizes[index] as usize
                            || !protocol::validate_payload(packet, header)
                        {
                            counters.corrupt += 1;
                            continue;
                        }

                        let sent_ns = receiver_times[index].load(Ordering::Acquire);
                        let rtt_ns = elapsed_ns(start).saturating_sub(sent_ns);
                        match classify_receipt(&receiver_states[index], &receiver_outstanding) {
                            Receipt::Valid => {
                                counters.valid_replies += 1;
                                if rtt_ns > timeout_ns {
                                    counters.late += 1;
                                }
                            }
                            Receipt::Late => {
                                counters.valid_replies += 1;
                                counters.late += 1;
                            }
                            Receipt::Duplicate => {
                                counters.duplicates += 1;
                                continue;
                            }
                            Receipt::Foreign => {
                                counters.foreign += 1;
                                continue;
                            }
                        }
                        if maximum_sequence.is_some_and(|maximum| header.sequence < maximum) {
                            counters.reordered += 1;
                        }
                        maximum_sequence = Some(
                            maximum_sequence
                                .map_or(header.sequence, |value| value.max(header.sequence)),
                        );
                        samples.push(rtt_ns);
                    }
                    Err(error) if is_timeout(&error) => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(ReceiveResult { counters, samples })
        });

        wait_until(start);
        let rate_bits_per_second = target_mbps * 1_000_000.0;
        let mut cumulative_bits = 0u64;
        let mut expiry_cursor = 0usize;

        for sequence in 0..count {
            let scheduled_ns = ((cumulative_bits as f64 / rate_bits_per_second) * 1e9) as u64;
            wait_until(start + Duration::from_nanos(scheduled_ns));

            // If backpressure prevented the requested schedule, stop at the
            // measurement boundary. Continuing afterward would make the
            // reported offered rate and receiver deadline misleading.
            if Instant::now() >= start + measurement {
                break;
            }

            while outstanding.load(Ordering::Acquire) >= window {
                expire_old(
                    &states,
                    &sent_times,
                    &outstanding,
                    &mut expiry_cursor,
                    sequence,
                    elapsed_ns(start),
                    timeout_ns,
                );
                if outstanding.load(Ordering::Acquire) >= window {
                    thread::yield_now();
                }
            }

            let sent_ns = elapsed_ns(start);
            let size = packet_sizes[sequence] as usize;
            let packet = protocol::encode(
                size,
                Header {
                    run_id,
                    sequence: sequence as u64,
                    sent_ns,
                },
            )
            .map_err(invalid_input)?;
            sent_times[sequence].store(sent_ns, Ordering::Release);
            // Publish the outstanding state before sending. A localhost or
            // very fast LAN reply can otherwise race ahead of this store and
            // be misclassified as foreign traffic.
            states[sequence].store(1, Ordering::Release);
            outstanding.fetch_add(1, Ordering::AcqRel);
            match sender_socket.send(&packet) {
                Ok(_) => {
                    sent_count.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    if states[sequence].swap(4, Ordering::AcqRel) == 1 {
                        outstanding.fetch_sub(1, Ordering::AcqRel);
                    }
                    send_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            cumulative_bits += size as u64 * 8;
        }

        let receive = receiver
            .join()
            .map_err(|_| io::Error::other("receiver thread panicked"))??;
        let sent = sent_count.load(Ordering::Relaxed);
        let mut counters = receive.counters;
        counters.planned = count as u64;
        counters.sent = sent;
        counters.send_errors = send_errors.load(Ordering::Relaxed);
        counters.missing = sent.saturating_sub(counters.valid_replies);

        let sent_payload_bytes = states
            .iter()
            .zip(packet_sizes.iter())
            .filter(|(state, _)| state.load(Ordering::Acquire) != 4)
            .map(|(_, size)| u64::from(*size))
            .sum::<u64>();
        let valid_payload_bytes = states
            .iter()
            .zip(packet_sizes.iter())
            .filter(|(state, _)| state.load(Ordering::Acquire) == 2)
            .map(|(_, size)| u64::from(*size))
            .sum::<u64>();
        let seconds = measurement.as_secs_f64();

        Ok(RawLoad {
            result: LoadResult {
                payload_bytes: plan.fixed_size,
                offered_target_mbps: target_mbps,
                measurement_seconds: seconds,
                drain_seconds: drain.as_secs_f64(),
                window,
                offered_mbps: sent_payload_bytes as f64 * 8.0 / seconds / 1_000_000.0,
                goodput_mbps: valid_payload_bytes as f64 * 8.0 / seconds / 1_000_000.0,
                sent_packets_per_second: sent as f64 / seconds,
                valid_packets_per_second: counters.valid_replies as f64 / seconds,
                loss_percent: percentage(counters.missing, sent),
                latency: summarize(&receive.samples),
                counters,
            },
            samples: receive.samples,
        })
    }
}

struct PayloadPlan {
    sizes: Vec<u16>,
    fixed_size: Option<usize>,
}

impl PayloadPlan {
    fn fixed(size: usize, rate_mbps: f64, duration: Duration) -> Self {
        assert!(size >= HEADER_LEN && size <= u16::MAX as usize);
        let target_bits = rate_mbps * 1_000_000.0 * duration.as_secs_f64();
        let count = (target_bits / (size as f64 * 8.0)).ceil().max(1.0) as usize;
        Self {
            sizes: vec![size as u16; count],
            fixed_size: Some(size),
        }
    }

    fn mixed(sizes: &[usize], rate_mbps: f64, duration: Duration, seed: u64) -> Self {
        assert!(!sizes.is_empty());
        assert!(
            sizes
                .iter()
                .all(|size| *size >= HEADER_LEN && *size <= u16::MAX as usize)
        );
        let target_bits = rate_mbps * 1_000_000.0 * duration.as_secs_f64();
        let mut generated = Vec::new();
        let mut bits = 0u64;
        let mut state = seed.max(1);
        while (bits as f64) < target_bits {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let size = sizes[state as usize % sizes.len()];
            generated.push(size as u16);
            bits += size as u64 * 8;
        }
        Self {
            sizes: generated,
            fixed_size: None,
        }
    }
}

struct RawLoad {
    result: LoadResult,
    samples: Vec<u64>,
}

struct ReceiveResult {
    counters: PacketCounters,
    samples: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Receipt {
    Valid,
    Late,
    Duplicate,
    Foreign,
}

fn classify_receipt(state: &AtomicU8, outstanding: &AtomicUsize) -> Receipt {
    match state.swap(2, Ordering::AcqRel) {
        1 => {
            outstanding.fetch_sub(1, Ordering::AcqRel);
            Receipt::Valid
        }
        3 => Receipt::Late,
        2 => Receipt::Duplicate,
        _ => Receipt::Foreign,
    }
}

fn raw_packet(size: usize, run_id: u64, sequence: u64) -> Vec<u8> {
    if size >= HEADER_LEN {
        return protocol::encode(
            size,
            Header {
                run_id,
                sequence,
                sent_ns: 0,
            },
        )
        .expect("validated packet size");
    }
    (0..size)
        .map(|index| {
            let mut value = run_id
                ^ sequence.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (index as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value ^= value >> 29;
            value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
            (value ^ (value >> 31)) as u8
        })
        .collect()
}

fn expire_old(
    states: &[AtomicU8],
    sent_times: &[AtomicU64],
    outstanding: &AtomicUsize,
    cursor: &mut usize,
    sent_up_to: usize,
    now_ns: u64,
    timeout_ns: u64,
) {
    while *cursor < sent_up_to {
        let state = states[*cursor].load(Ordering::Acquire);
        if state == 1 {
            let sent_ns = sent_times[*cursor].load(Ordering::Acquire);
            if now_ns.saturating_sub(sent_ns) < timeout_ns {
                break;
            }
            if states[*cursor]
                .compare_exchange(1, 3, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                outstanding.fetch_sub(1, Ordering::AcqRel);
            }
        }
        *cursor += 1;
    }
}

fn wait_until(deadline: Instant) {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline - now;
        if remaining > Duration::from_millis(1) {
            thread::sleep(remaining - Duration::from_micros(500));
        } else {
            std::hint::spin_loop();
        }
    }
}

fn flush(socket: &UdpSocket) -> io::Result<()> {
    socket.set_nonblocking(true)?;
    let mut buffer = [0u8; 2048];
    loop {
        match socket.recv(&mut buffer) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => {
                socket.set_nonblocking(false)?;
                return Err(error);
            }
        }
    }
    socket.set_nonblocking(false)
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::ConnectionReset
    ) || (cfg!(windows) && error.raw_os_error() == Some(997))
}

fn is_zero_loss(result: &LoadResult) -> bool {
    result.counters.missing == 0
        && result.counters.corrupt == 0
        && result.counters.send_errors == 0
        && result.offered_mbps >= result.offered_target_mbps * 0.98
}

fn is_saturated(result: &LoadResult) -> bool {
    result.loss_percent >= 0.1
        || result.offered_mbps < result.offered_target_mbps * 0.95
        || result.goodput_mbps < result.offered_target_mbps * 0.90
}

fn percentage(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 * 100.0 / denominator as f64
    }
}

fn elapsed_ns(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn add_counters(total: &mut PacketCounters, value: &PacketCounters) {
    total.planned += value.planned;
    total.sent += value.sent;
    total.valid_replies += value.valid_replies;
    total.missing += value.missing;
    total.late += value.late;
    total.duplicates += value.duplicates;
    total.reordered += value.reordered;
    total.corrupt += value.corrupt;
    total.foreign += value.foreign;
    total.send_errors += value.send_errors;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;

    #[test]
    fn deterministic_small_packets_change_with_sequence() {
        assert_ne!(raw_packet(8, 1, 1), raw_packet(8, 1, 2));
        assert_eq!(raw_packet(8, 1, 1), raw_packet(8, 1, 1));
    }

    #[test]
    fn payload_plan_hits_requested_bit_budget() {
        let plan = PayloadPlan::mixed(&[64, 512, 1472], 1.0, Duration::from_secs(1), 7);
        let bits = plan
            .sizes
            .iter()
            .map(|size| u64::from(*size) * 8)
            .sum::<u64>();
        assert!(bits >= 1_000_000);
        assert!(bits < 1_000_000 + 1472 * 8);
    }

    #[test]
    fn saturation_detects_pacing_failure() {
        let result = LoadResult {
            payload_bytes: Some(1472),
            offered_target_mbps: 10.0,
            measurement_seconds: 1.0,
            drain_seconds: 1.0,
            window: 64,
            offered_mbps: 9.0,
            goodput_mbps: 9.0,
            sent_packets_per_second: 0.0,
            valid_packets_per_second: 0.0,
            loss_percent: 0.0,
            counters: PacketCounters::default(),
            latency: crate::stats::LatencyStats::default(),
        };
        assert!(is_saturated(&result));
        assert!(!is_zero_loss(&result));
    }

    #[test]
    fn sequence_accounting_classifies_valid_late_duplicate_and_foreign() {
        let outstanding = AtomicUsize::new(1);
        let valid = AtomicU8::new(1);
        assert_eq!(classify_receipt(&valid, &outstanding), Receipt::Valid);
        assert_eq!(outstanding.load(Ordering::Acquire), 0);
        assert_eq!(classify_receipt(&valid, &outstanding), Receipt::Duplicate);

        let late = AtomicU8::new(3);
        assert_eq!(classify_receipt(&late, &outstanding), Receipt::Late);
        let unsent = AtomicU8::new(0);
        assert_eq!(classify_receipt(&unsent, &outstanding), Receipt::Foreign);
    }

    #[test]
    fn functional_mode_works_against_local_echo_server() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let echo = thread::spawn(move || {
            let mut buffer = [0u8; 2048];
            for _ in 0..6 {
                let (length, remote) = server.recv_from(&mut buffer).unwrap();
                server.send_to(&buffer[..length], remote).unwrap();
            }
        });

        let client = BenchSocket::connect(address).unwrap();
        let results = client
            .functional(&[0, 1, 1472], 2, Duration::from_millis(100), 77)
            .unwrap();
        echo.join().unwrap();

        assert!(results.iter().all(|result| result.valid_replies == 2));
        assert!(results.iter().all(|result| result.timeouts == 0));
        assert!(results.iter().all(|result| result.corrupt == 0));
    }

    #[test]
    fn load_mode_detects_injected_network_faults() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let echo = thread::spawn(move || {
            let mut buffer = [0u8; 2048];
            let mut held = None;
            for _ in 0..20 {
                let (length, remote) = server.recv_from(&mut buffer).unwrap();
                let packet = buffer[..length].to_vec();
                let sequence = protocol::decode(&packet).unwrap().sequence;
                match sequence {
                    1 => {} // loss
                    2 => {
                        server.send_to(&packet, remote).unwrap();
                        server.send_to(&packet, remote).unwrap();
                    }
                    3 => {
                        let mut corrupt = packet;
                        corrupt[HEADER_LEN] ^= 1;
                        server.send_to(&corrupt, remote).unwrap();
                    }
                    4 => held = Some((packet, remote)),
                    5 => {
                        server.send_to(&packet, remote).unwrap();
                        let (older, older_remote) = held.take().unwrap();
                        server.send_to(&older, older_remote).unwrap();
                    }
                    6 => {
                        thread::sleep(Duration::from_millis(40));
                        server.send_to(&packet, remote).unwrap();
                    }
                    _ => {
                        server.send_to(&packet, remote).unwrap();
                    }
                }
            }
        });

        let client = BenchSocket::connect(address).unwrap();
        let result = client
            .load(
                PayloadPlan {
                    sizes: vec![64; 20],
                    fixed_size: Some(64),
                },
                0.0512,
                Duration::from_millis(200),
                Duration::from_millis(150),
                Duration::from_millis(20),
                256,
                1234,
            )
            .unwrap()
            .result;
        echo.join().unwrap();

        assert_eq!(result.counters.sent, 20);
        assert_eq!(result.counters.valid_replies, 18);
        assert_eq!(result.counters.missing, 2);
        assert_eq!(result.counters.duplicates, 1);
        assert_eq!(result.counters.reordered, 1);
        assert_eq!(result.counters.corrupt, 1);
        // The intentionally delayed packet also queues later replies behind
        // it in this single-threaded synthetic server.
        assert!(result.counters.late >= 1);
    }
}
