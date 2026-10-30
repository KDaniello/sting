//! Level 2: stylized facts. Ask whether the synthetic market's returns look like a real
//! market's, by the statistics the empirical literature agrees a return series has.
//!
//! A simulator that reproduces a recorded episode has shown it can follow. A simulator whose own
//! output carries the same statistical signature as the market it models has shown it can
//! generate, and that is the claim a scenario nobody recorded depends on.
//!
//! **Returns are simple, not logarithmic.** The difference between the two is negligible at the
//! moves these statistics describe, and the reason is not convenience: a logarithm is not
//! required to give the same bits on every platform, while addition, subtraction, multiplication,
//! division and square root are. A verdict that depends on which machine rendered it is not a
//! verdict.
//!
//! Floating point appears here and nowhere below it. Analysis and reporting may use it; a
//! mechanism transition may not (karman/docs/ARCHITECTURE.md §8).

use crate::Verdict;

/// A series too short, or too still, to have the structure being asked about.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotMeasurable {
    /// Fewer observations than the statistic needs.
    #[error("{needed} observations are needed and {given} were given")]
    TooShort {
        /// How many the request implied.
        needed: usize,
        /// How many arrived.
        given: usize,
    },
    /// A series that never moved has no correlation structure and no shape, and reporting zero
    /// for either would be inventing a measurement.
    #[error("the series never moved: it has no distribution to describe")]
    Motionless,
}

/// What to measure, so that a measurement is chosen before it is taken rather than after.
#[derive(Debug, Clone)]
pub struct Request {
    /// How far the autocorrelations reach, in observations.
    pub max_lag: usize,
    /// The horizons aggregational gaussianity is checked over, in observations. A distribution
    /// that is heavy-tailed at one step and closer to normal at many is the fact being asked
    /// about, so these should rise.
    pub horizons: Vec<usize>,
}

impl Default for Request {
    /// Lags out to twenty and horizons doubling to sixteen: enough for a decay to be visible
    /// without claiming resolution a simulated series of ordinary length can support.
    fn default() -> Self {
        Request {
            max_lag: 20,
            horizons: vec![1, 2, 4, 8, 16],
        }
    }
}

/// The measured signature of one return series.
#[derive(Debug, Clone, PartialEq)]
pub struct StylizedFacts {
    /// How many returns the statistics were taken over.
    pub returns: usize,
    /// Fourth moment over squared second moment, less three. Positive is the heavy tail every
    /// real return series has; zero is what a normal distribution would give.
    pub excess_kurtosis: f64,
    /// Autocorrelation of returns at lag one and upward. Real returns have none to speak of
    /// beyond the shortest intraday scales, which is what makes a price hard to predict from
    /// its own past.
    ///
    /// A lag the series is too short to reach, or a series with no variation at that lag, is
    /// absent rather than zero. A not-a-number would compare unequal to itself and turn a
    /// reproducibility check into a coin toss.
    pub return_autocorrelation: Vec<Option<f64>>,
    /// Autocorrelation of absolute returns at the same lags. Real markets have this positive
    /// and slow to decay — large moves follow large moves — and it is the fact a model is
    /// least likely to reproduce by accident.
    pub absolute_return_autocorrelation: Vec<Option<f64>>,
    /// Excess kurtosis measured over each requested horizon. Falling with the horizon is
    /// aggregational gaussianity; a horizon over which the series does not vary has no shape to
    /// report and is absent.
    pub excess_kurtosis_by_horizon: Vec<(usize, Option<f64>)>,
}

/// The range a statistic has to land in to be called agreement, registered before the run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    /// Lowest value that still counts as agreement.
    pub low: f64,
    /// Highest value that still counts as agreement.
    pub high: f64,
}

impl Band {
    /// Whether a measurement falls inside. A value on either edge is inside: a band is a
    /// preregistration, and moving its edge after seeing the number is the thing preregistration
    /// exists to prevent.
    #[must_use]
    pub fn holds(self, measured: f64) -> bool {
        measured >= self.low && measured <= self.high
    }
}

/// One fact, its measurement, and whether the measurement landed where it was said it would.
#[derive(Debug, Clone, PartialEq)]
pub struct FactCheck {
    /// Which fact this is, named as the preregistration names it.
    pub fact: String,
    /// What was measured.
    pub measured: f64,
    /// Where it was said the measurement would land.
    pub band: Band,
    /// Whether it did.
    pub verdict: Verdict,
}

/// Measures the signature of a price series.
///
/// Sums run in index order and never in parallel, so the answer is a function of the series and
/// not of how the work was divided.
///
/// # Errors
///
/// Fails when the series is shorter than the request needs, or when it never moved.
pub fn measure(prices: &[f64], request: &Request) -> Result<StylizedFacts, NotMeasurable> {
    let longest = request.horizons.iter().copied().max().unwrap_or(1).max(1);
    let needed = longest + request.max_lag + 2;
    if prices.len() < needed {
        return Err(NotMeasurable::TooShort {
            needed,
            given: prices.len(),
        });
    }

    let returns = simple_returns(prices, 1);
    let (mean, variance) = moments(&returns);
    if variance == 0.0 {
        return Err(NotMeasurable::Motionless);
    }

    let absolute: Vec<f64> = returns.iter().map(|r| r.abs()).collect();
    let mut by_horizon = Vec::with_capacity(request.horizons.len());
    for &horizon in &request.horizons {
        let aggregated = simple_returns(prices, horizon.max(1));
        by_horizon.push((horizon, excess_kurtosis(&aggregated)));
    }

    Ok(StylizedFacts {
        returns: returns.len(),
        excess_kurtosis: excess_kurtosis(&returns).ok_or(NotMeasurable::Motionless)?,
        return_autocorrelation: autocorrelation(&returns, mean, variance, request.max_lag),
        absolute_return_autocorrelation: {
            let (mean, variance) = moments(&absolute);
            autocorrelation(&absolute, mean, variance, request.max_lag)
        },
        excess_kurtosis_by_horizon: by_horizon,
    })
}

/// Judges one measurement against the band registered for it.
#[must_use]
pub fn check(fact: impl Into<String>, measured: f64, band: Band) -> FactCheck {
    let fact = fact.into();
    let verdict = if band.holds(measured) {
        Verdict::Confirmed
    } else {
        Verdict::Diverged {
            detail: format!(
                "{fact}: measured {measured}, registered band {}..={}",
                band.low, band.high
            ),
        }
    };
    FactCheck {
        fact,
        measured,
        band,
        verdict,
    }
}

/// Returns over `horizon` observations, as fractions of where the price started.
///
/// A price at or below zero ends the series: a fraction of nothing is not a return, and
/// extending past it would mix a real series with an invented one.
fn simple_returns(prices: &[f64], horizon: usize) -> Vec<f64> {
    let mut returns = Vec::with_capacity(prices.len().saturating_sub(horizon));
    for window in prices.windows(horizon + 1) {
        let (from, to) = (window[0], window[horizon]);
        if from <= 0.0 || !from.is_finite() || !to.is_finite() {
            break;
        }
        returns.push((to - from) / from);
    }
    returns
}

/// Mean and population variance, in one pass over the series and a second over its deviations.
#[allow(
    clippy::cast_precision_loss,
    reason = "a count of samples in one run is far inside a double's exact range"
)]
fn moments(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let count = values.len() as f64;
    let mut sum = 0.0;
    for value in values {
        sum += value;
    }
    let mean = sum / count;
    let mut squares = 0.0;
    for value in values {
        let deviation = value - mean;
        squares += deviation * deviation;
    }
    (mean, squares / count)
}

/// Fourth moment over squared variance, less the three a normal distribution would give.
#[allow(
    clippy::cast_precision_loss,
    reason = "a count of samples in one run is far inside a double's exact range"
)]
fn excess_kurtosis(values: &[f64]) -> Option<f64> {
    let (mean, variance) = moments(values);
    if variance == 0.0 {
        return None;
    }
    let count = values.len() as f64;
    let mut fourth = 0.0;
    for value in values {
        let deviation = value - mean;
        let squared = deviation * deviation;
        fourth += squared * squared;
    }
    Some((fourth / count) / (variance * variance) - 3.0)
}

/// Autocorrelation at each lag from one to `max_lag`, against a mean and variance measured once
/// over the whole series.
#[allow(
    clippy::cast_precision_loss,
    reason = "a count of samples in one run is far inside a double's exact range"
)]
fn autocorrelation(values: &[f64], mean: f64, variance: f64, max_lag: usize) -> Vec<Option<f64>> {
    let count = values.len() as f64;
    let mut correlations = Vec::with_capacity(max_lag);
    for lag in 1..=max_lag {
        if lag >= values.len() || variance == 0.0 {
            correlations.push(None);
            continue;
        }
        let mut products = 0.0;
        for index in 0..values.len() - lag {
            products += (values[index] - mean) * (values[index + lag] - mean);
        }
        correlations.push(Some(products / count / variance));
    }
    correlations
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use super::*;

    /// A price series whose returns alternate by a fixed fraction: its autocorrelation at lag
    /// one is minus one by construction, which is what pins the estimator.
    fn alternating(count: usize) -> Vec<f64> {
        let mut prices = vec![1_000.0];
        for step in 0..count {
            let previous = *prices.last().unwrap();
            prices.push(if step % 2 == 0 {
                previous * 1.01
            } else {
                previous / 1.01
            });
        }
        prices
    }

    /// Doubling every step, so every return is exactly one and the series has a price that moves
    /// while its returns do not.
    fn doubling(count: usize) -> Vec<f64> {
        let mut prices = vec![1.0];
        for _ in 0..count {
            prices.push(prices.last().unwrap() * 2.0);
        }
        prices
    }

    #[test]
    fn a_series_shorter_than_the_request_is_refused_with_both_numbers() {
        let error = measure(&[1.0, 2.0], &Request::default()).unwrap_err();
        assert!(matches!(error, NotMeasurable::TooShort { given: 2, .. }));
        assert!(error.to_string().contains("2 were given"));
    }

    #[test]
    fn a_series_that_never_moved_has_nothing_to_describe() {
        let flat = vec![1_000.0; 64];
        assert_eq!(
            measure(&flat, &Request::default()),
            Err(NotMeasurable::Motionless)
        );
    }

    /// A constant growth rate is a constant return, which is a series with no variance in it
    /// even though the price moves every step. Doubling is used because its return is exactly
    /// representable: a growth rate that is not would leave rounding in the variance and make
    /// this test about floating point rather than about the guard.
    #[test]
    fn a_constant_return_is_motionless_in_the_sense_that_matters() {
        assert_eq!(
            measure(&doubling(64), &Request::default()),
            Err(NotMeasurable::Motionless)
        );
    }

    #[test]
    fn returns_are_fractions_of_where_the_price_started() {
        let returns = simple_returns(&[100.0, 110.0, 99.0], 1);
        assert!((returns[0] - 0.1).abs() < 1e-12);
        assert!((returns[1] - -0.1).abs() < 1e-12);
    }

    #[test]
    fn a_horizon_measures_across_that_many_steps() {
        let returns = simple_returns(&[100.0, 110.0, 121.0], 2);
        assert_eq!(returns.len(), 1);
        assert!((returns[0] - 0.21).abs() < 1e-12);
    }

    #[test]
    fn a_price_that_reaches_zero_ends_the_series_rather_than_dividing_by_it() {
        let returns = simple_returns(&[100.0, 0.0, 50.0], 1);
        assert_eq!(
            returns.len(),
            1,
            "the series continued past a price of nothing"
        );
    }

    /// The estimator against a series whose answer is known: perfectly alternating returns are
    /// anticorrelated at lag one and correlated at lag two.
    #[test]
    fn autocorrelation_recovers_a_series_it_is_given() {
        let facts = measure(
            &alternating(400),
            &Request {
                max_lag: 4,
                horizons: vec![1],
            },
        )
        .unwrap();
        assert!(
            facts.return_autocorrelation[0].unwrap() < -0.9,
            "lag one is not near minus one"
        );
        assert!(
            facts.return_autocorrelation[1].unwrap() > 0.9,
            "lag two is not near plus one"
        );
    }

    /// A series with no variation has no correlation structure, and the harness reports the
    /// absence rather than a number standing in for it.
    #[test]
    fn a_series_with_no_variation_reports_absence_and_not_a_number() {
        assert_eq!(
            autocorrelation(&[1.0, 1.0, 1.0, 1.0], 1.0, 0.0, 2),
            vec![None, None]
        );
    }

    /// A lag the series cannot reach is absent for the same reason.
    #[test]
    fn a_lag_beyond_the_series_reports_absence() {
        let reached = autocorrelation(&[1.0, 2.0, 3.0], 2.0, 1.0, 4);
        assert!(reached[0].is_some() && reached[1].is_some());
        assert_eq!(reached[2], None);
        assert_eq!(reached[3], None);
    }

    /// A two-valued return series is as far from a bell as a series gets: its excess kurtosis is
    /// minus two, which is the floor.
    #[test]
    fn excess_kurtosis_of_a_two_valued_series_is_at_the_floor() {
        let facts = measure(
            &alternating(400),
            &Request {
                max_lag: 2,
                horizons: vec![1],
            },
        )
        .unwrap();
        assert!(
            (facts.excess_kurtosis - -2.0).abs() < 0.05,
            "{}",
            facts.excess_kurtosis
        );
    }

    #[test]
    fn a_band_holds_its_own_edges() {
        let band = Band {
            low: -0.1,
            high: 0.1,
        };
        assert!(band.holds(-0.1) && band.holds(0.1) && band.holds(0.0));
        assert!(!band.holds(0.100_001));
    }

    #[test]
    fn a_measurement_outside_its_band_diverges_and_quotes_both() {
        let checked = check(
            "absence of autocorrelation",
            0.42,
            Band {
                low: -0.1,
                high: 0.1,
            },
        );
        let Verdict::Diverged { detail } = checked.verdict else {
            panic!("a measurement outside its band was confirmed");
        };
        assert!(
            detail.contains("0.42") && detail.contains("0.1"),
            "unclear detail: {detail}"
        );
    }

    #[test]
    fn a_measurement_inside_its_band_is_confirmed() {
        let checked = check(
            "absence of autocorrelation",
            0.01,
            Band {
                low: -0.1,
                high: 0.1,
            },
        );
        assert_eq!(checked.verdict, Verdict::Confirmed);
        assert_eq!(checked.fact, "absence of autocorrelation");
    }

    /// The whole point of taking sums in index order: the same series gives the same bits, so a
    /// verdict does not depend on the machine that rendered it.
    #[test]
    fn the_same_series_measures_to_the_same_bits() {
        let prices = alternating(500);
        let request = Request::default();
        assert_eq!(measure(&prices, &request), measure(&prices, &request));
    }
}
