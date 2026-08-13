//! Statistical summaries shared by every benchmark mode.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LatencyStats {
    pub samples: u64,
    pub min_ns: u64,
    pub mean_ns: f64,
    pub p50_ns: u64,
    pub p90_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub p999_ns: u64,
    pub max_ns: u64,
    pub stddev_ns: f64,
}

pub fn summarize(samples: &[u64]) -> LatencyStats {
    if samples.is_empty() {
        return LatencyStats::default();
    }

    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let sum = sorted.iter().map(|value| *value as f64).sum::<f64>();
    let mean = sum / sorted.len() as f64;
    let variance = sorted
        .iter()
        .map(|value| {
            let difference = *value as f64 - mean;
            difference * difference
        })
        .sum::<f64>()
        / sorted.len() as f64;

    LatencyStats {
        samples: sorted.len() as u64,
        min_ns: sorted[0],
        mean_ns: mean,
        p50_ns: percentile(&sorted, 0.50),
        p90_ns: percentile(&sorted, 0.90),
        p95_ns: percentile(&sorted, 0.95),
        p99_ns: percentile(&sorted, 0.99),
        p999_ns: percentile(&sorted, 0.999),
        max_ns: *sorted.last().expect("nonempty samples"),
        stddev_ns: variance.sqrt(),
    }
}

fn percentile(sorted: &[u64], fraction: f64) -> u64 {
    let index = ((sorted.len() - 1) as f64 * fraction).ceil() as usize;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_summary_is_zeroed() {
        assert_eq!(summarize(&[]).samples, 0);
    }

    #[test]
    fn percentiles_and_mean_are_deterministic() {
        let result = summarize(&[50, 10, 40, 20, 30]);
        assert_eq!(result.min_ns, 10);
        assert_eq!(result.p50_ns, 30);
        assert_eq!(result.p90_ns, 50);
        assert_eq!(result.max_ns, 50);
        assert_eq!(result.mean_ns, 30.0);
    }
}
