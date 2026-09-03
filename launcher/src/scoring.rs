use crate::protocol::{CategoryScores, MetricSummary, TelemetrySample};
use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

pub fn calculate(samples: &[TelemetrySample]) -> Result<(MetricSummary, CategoryScores, f64)> {
    let frame_times = samples
        .iter()
        .filter(|sample| sample.test == "graphics_static")
        .filter_map(|sample| sample.frame_time_ns)
        .collect::<Vec<_>>();
    let tick_times = samples
        .iter()
        .filter(|sample| sample.test == "simulation")
        .filter_map(|sample| sample.tick_time_ns)
        .collect::<Vec<_>>();
    let chunk_times = samples
        .iter()
        .filter(|sample| sample.test == "chunk_generation")
        .filter_map(|sample| sample.chunk_time_ns)
        .collect::<Vec<_>>();
    if frame_times.is_empty() || tick_times.is_empty() || chunk_times.is_empty() {
        bail!("telemetry does not contain all required mock test samples");
    }

    let average_frame_ns = mean(&frame_times);
    let average_fps = 1_000_000_000.0 / average_frame_ns;
    let one_percent_low_fps = 1_000_000_000.0 / percentile(&frame_times, 0.99);
    let median_mspt = percentile(&tick_times, 0.50) / 1_000_000.0;
    let p99_mspt = percentile(&tick_times, 0.99) / 1_000_000.0;
    let chunks_per_second = 1_000_000_000.0 / mean(&chunk_times);

    let metrics = MetricSummary {
        average_fps,
        one_percent_low_fps,
        median_mspt,
        p99_mspt,
        chunks_per_second,
    };
    let scores = CategoryScores {
        graphics: positive_score(average_fps / 120.0),
        simulation: positive_score(16.67 / median_mspt),
        world_generation: positive_score(chunks_per_second / 20.0),
    };
    let overall_score = geometric_mean(&[
        (scores.graphics, 30.0),
        (scores.simulation, 25.0),
        (scores.world_generation, 20.0),
    ]);
    Ok((metrics, scores, overall_score))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn percentile(values: &[u64], quantile: f64) -> f64 {
    assert!(!values.is_empty());
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let position = quantile.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        return sorted[lower] as f64;
    }
    let fraction = position - lower as f64;
    sorted[lower] as f64 + (sorted[upper] - sorted[lower]) as f64 * fraction
}

fn mean(values: &[u64]) -> f64 {
    values.iter().map(|value| *value as f64).sum::<f64>() / values.len() as f64
}

fn positive_score(normalized: f64) -> f64 {
    (normalized.max(0.0) * 10_000.0).clamp(0.0, 100_000.0)
}

fn geometric_mean(values: &[(f64, f64)]) -> f64 {
    let total_weight = values.iter().map(|(_, weight)| weight).sum::<f64>();
    let log_sum = values
        .iter()
        .map(|(value, weight)| value.max(f64::MIN_POSITIVE).ln() * weight)
        .sum::<f64>();
    (log_sum / total_weight).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(
        test: &str,
        frame: Option<u64>,
        tick: Option<u64>,
        chunk: Option<u64>,
    ) -> TelemetrySample {
        TelemetrySample {
            schema: 1,
            sequence: 0,
            test: test.to_string(),
            kind: "sample".to_string(),
            monotonic_ns: 1,
            frame_time_ns: frame,
            tick_time_ns: tick,
            chunk_time_ns: chunk,
        }
    }

    #[test]
    fn percentile_and_score_are_deterministic() {
        assert_eq!(percentile(&[10, 20, 30], 0.5), 20.0);
        let samples = vec![
            sample("graphics_static", Some(10_000_000), None, None),
            sample("graphics_static", Some(10_000_000), None, None),
            sample("simulation", None, Some(10_000_000), None),
            sample("simulation", None, Some(20_000_000), None),
            sample("chunk_generation", None, None, Some(50_000_000)),
            sample("chunk_generation", None, None, Some(50_000_000)),
        ];
        let (metrics, scores, overall) = calculate(&samples).unwrap();
        assert_eq!(metrics.average_fps, 100.0);
        assert_eq!(metrics.median_mspt, 15.0);
        assert_eq!(metrics.chunks_per_second, 20.0);
        assert!(scores.graphics > 0.0);
        assert!(overall > 0.0);
    }

    #[test]
    fn incomplete_samples_are_rejected() {
        let error = calculate(&[]).unwrap_err().to_string();
        assert!(error.contains("required mock test samples"));
    }
}
