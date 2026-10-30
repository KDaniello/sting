//! How a wallet is spaced in time, which is the strongest single signal there is.
//!
//! What is measured is how *anomalous* the spacing is, not how regular. Automation shows up as
//! both — a metronome and a burst are equally unlike a person — so a feature that pinned one
//! direction would be blind to the other half of it.

/// What the gaps between a wallet's transactions look like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TemporalFeatures {
    /// Mean gap between events, in seconds.
    pub iet_mean: f64,
    /// Standard deviation of those gaps.
    pub iet_std: f64,
    /// The deviation over the mean, which is the shape rather than the pace.
    pub iet_cv: f64,
    /// Inverse entropy of the gap distribution: higher is more metronome-like.
    pub iet_regularity: f64,
    /// From first event to last, in seconds.
    pub active_duration: f64,
}

/// Temporal features from timestamps that are already in order.
///
/// One pass per wallet. Fewer than two timestamps is not an error and not a panic — it is a
/// wallet with no gap to measure, and every figure is zero.
#[must_use]
pub fn extract_temporal(timestamps: &[u32]) -> TemporalFeatures {
    if timestamps.len() < 2 {
        return TemporalFeatures {
            iet_mean: 0.0,
            iet_std: 0.0,
            iet_cv: 0.0,
            iet_regularity: 0.0,
            active_duration: 0.0,
        };
    }
    let iets: Vec<f64> = timestamps
        .windows(2)
        .map(|pair| f64::from(pair[1] - pair[0]))
        .collect();
    let iet_mean = mean(&iets);
    let iet_std = std_dev(&iets, iet_mean);
    let iet_cv = if iet_mean > 0.0 {
        iet_std / iet_mean
    } else {
        0.0
    };
    let iet_regularity = 1.0 / (1.0 + shannon_entropy_binned(&iets, 10));
    let active_duration = f64::from(*timestamps.last().unwrap_or(&0)) - f64::from(timestamps[0]);
    TemporalFeatures {
        iet_mean,
        iet_std,
        iet_cv,
        iet_regularity,
        active_duration,
    }
}

/// How many runs of unusually close events there were, and how long the longest one lasted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BurstFeatures {
    /// How many runs.
    pub burst_count: u32,
    /// The longest one, in seconds of wall clock rather than in events.
    pub longest_burst: f64,
}

/// The longest run of consecutive gaps below `burst_factor` times the mean gap.
///
/// The factor is the caller's rather than a constant here: a threshold chosen inside an algorithm
/// is a threshold fitted to whichever dataset it was written against.
#[must_use]
pub fn extract_burst(timestamps: &[u32], burst_factor: f64) -> BurstFeatures {
    if timestamps.len() < 2 {
        return BurstFeatures::default();
    }
    let iets: Vec<f64> = timestamps
        .windows(2)
        .map(|pair| f64::from(pair[1] - pair[0]))
        .collect();
    let threshold = mean(&iets) * burst_factor;

    let mut burst_count = 0u32;
    let mut longest_burst = 0.0f64;
    let mut run_start: Option<usize> = None;

    let close_run = |run_start: usize, end_idx: usize, longest_burst: &mut f64| {
        let span = f64::from(timestamps[end_idx]) - f64::from(timestamps[run_start]);
        *longest_burst = longest_burst.max(span);
    };

    for (i, &iet) in iets.iter().enumerate() {
        if iet < threshold {
            run_start.get_or_insert(i);
        } else if let Some(start) = run_start.take() {
            burst_count += 1;
            close_run(start, i, &mut longest_burst);
        }
    }
    if let Some(start) = run_start {
        burst_count += 1;
        close_run(start, timestamps.len() - 1, &mut longest_burst);
    }

    BurstFeatures {
        burst_count,
        longest_burst,
    }
}

/// The entropy of activity across the hours of a day, in UTC.
///
/// Twenty-four is a fact about a day rather than a threshold fitted to data, so it is not a
/// parameter the way the bin count of the gap histogram is. There is no notion of a wallet's own
/// time zone anywhere in this contour, and inventing one would be inventing evidence.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "a count of one wallet's events is exact in a double"
)]
pub fn extract_active_hours_entropy(timestamps: &[u32]) -> f64 {
    if timestamps.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 24];
    for &ts in timestamps {
        let hour = ((ts % 86_400) / 3_600) as usize;
        counts[hour] += 1;
    }
    shannon_entropy_of_counts(&counts, timestamps.len() as f64)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a count of one wallet's gaps is exact in a double"
)]
fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a count of one wallet's gaps is exact in a double"
)]
fn std_dev(xs: &[f64], mean: f64) -> f64 {
    (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt()
}

/// Entropy over a histogram of values, with the bin count given rather than fixed.
///
/// Gaps that are all identical fall in one bin, which is zero entropy and therefore the highest
/// possible regularity — which is the answer that shape deserves.
#[allow(
    clippy::cast_precision_loss,
    reason = "bin counts and a gap count are exact in a double, and the result is an entropy"
)]
fn shannon_entropy_binned(xs: &[f64], bins: usize) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let max = xs.iter().copied().fold(f64::MIN, f64::max);
    let min = xs.iter().copied().fold(f64::MAX, f64::min);
    if (max - min).abs() < f64::EPSILON {
        return 0.0;
    }
    let bin_width = (max - min) / bins as f64;
    let mut counts = vec![0usize; bins];
    for &x in xs {
        // `x` is inside `[min, max]` and the width is positive, so the quotient is inside
        // `[0, bins]` before the clamp: the conversion cannot go negative or overflow.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the quotient is bounded by the bin count by construction"
        )]
        let idx = (((x - min) / bin_width) as usize).min(bins - 1);
        counts[idx] += 1;
    }
    shannon_entropy_of_counts(&counts, xs.len() as f64)
}

/// Entropy over counts that have already been made, shared by the two callers above so that
/// binning by value and binning by hour cannot drift apart.
#[allow(
    clippy::cast_precision_loss,
    reason = "a histogram bucket count is exact in a double, and the result is a probability"
)]
fn shannon_entropy_of_counts(counts: &[usize], total: f64) -> f64 {
    if total <= 0.0 {
        return 0.0;
    }
    -counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = count as f64 / total;
            p * p.log2()
        })
        .sum::<f64>()
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "these are exact values a feature computes from small integers, not the result of accumulated arithmetic"
)]
mod tests {
    use super::*;

    #[test]
    fn one_run_of_close_events_is_one_burst() {
        // Gaps of 100, 100, 5, 5, 5, 285: the mean is about 83, a fifth of it about 17, and only
        // the three middle gaps fall under that — one unbroken run.
        let timestamps = [0u32, 100, 200, 205, 210, 215, 500];
        let burst = extract_burst(&timestamps, 0.2);
        assert_eq!(burst.burst_count, 1);
        assert_eq!(burst.longest_burst, 15.0);
    }

    #[test]
    fn perfectly_even_spacing_is_no_burst_at_all() {
        let timestamps = [0u32, 60, 120, 180];
        let burst = extract_burst(&timestamps, 0.5);
        assert_eq!(burst.burst_count, 0);
        assert_eq!(burst.longest_burst, 0.0);
    }

    #[test]
    fn fewer_than_two_events_have_no_gaps_to_measure() {
        assert_eq!(extract_burst(&[], 0.5), BurstFeatures::default());
        assert_eq!(extract_burst(&[42], 0.5), BurstFeatures::default());
    }

    #[test]
    fn activity_inside_one_hour_has_no_entropy() {
        let timestamps = [0u32, 100, 200];
        assert_eq!(extract_active_hours_entropy(&timestamps), 0.0);
    }

    #[test]
    fn activity_spread_over_every_hour_is_at_the_maximum() {
        let timestamps: Vec<u32> = (0..24).map(|hour| hour * 3_600).collect();
        let entropy = extract_active_hours_entropy(&timestamps);
        assert!((entropy - 24.0f64.log2()).abs() < 1e-9);
    }

    #[test]
    fn no_events_is_no_entropy() {
        assert_eq!(extract_active_hours_entropy(&[]), 0.0);
    }
}
