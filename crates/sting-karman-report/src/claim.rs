//! Claims registered before a run, and the grading of a run against one.
//!
//! A report may state a verdict only where the thresholds were fixed before the numbers existed
//! (karman/docs/REPORTING.md §3). A threshold chosen after seeing a distribution is a description of
//! that distribution wearing the word "verdict", and it grades nothing.
//!
//! Nothing here can prove that a file was written before a run — that is what `findings/` and
//! the history it lives in are for. What this can do, and does, is refuse to grade a sweep the
//! claim was not registered against, and quote the registered zones into the report beside the
//! verdict so a reader compares the two without taking anyone's word for it.

use serde::{Deserialize, Serialize};

use crate::MetricSummary;

/// Which number out of a metric's distribution a zone is written against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Statistic {
    /// The lowest value any completed run reached.
    Min,
    /// The lower edge of the reported corridor.
    P05,
    /// The lower quartile.
    P25,
    /// The central value.
    Median,
    /// The upper quartile.
    P75,
    /// The upper edge of the reported corridor.
    P95,
    /// The highest value any completed run reached.
    Max,
    /// Median drift from base, in basis points. Absent where the metric has no base.
    DriftBps,
    /// Corridor width relative to the median, in basis points.
    CorridorBps,
    /// The spread about the mean.
    Sigma,
    /// One whole minus the spread over the median, in basis points.
    StabilityBps,
}

/// What a claim says a measurement landing in a zone means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Says {
    /// The claim holds.
    Confirmed,
    /// The claim does not hold.
    Refuted,
}

/// One graded band: a statistic, the range it must land in, and what landing there means.
///
/// Both edges are optional so a zone can be one-sided, which most preregistered thresholds are:
/// a claim is usually that something stays under a bound, not that it sits between two.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Zone {
    /// The metric this zone grades, by the name the report gives it.
    pub metric: String,
    /// Which number out of that metric's distribution.
    pub statistic: Statistic,
    /// Inclusive lower edge, where the zone has one.
    #[serde(default, with = "sting_karman_types::i128_str::opt")]
    pub at_least: Option<i128>,
    /// Inclusive upper edge, where the zone has one.
    #[serde(default, with = "sting_karman_types::i128_str::opt")]
    pub at_most: Option<i128>,
    /// What a measurement inside this zone means for the claim.
    pub says: Says,
}

impl Zone {
    /// Whether a measurement falls inside. A zone with neither edge holds everything, which is
    /// a registration error rather than a universal truth, and is rejected at load.
    #[must_use]
    pub fn holds(&self, measured: i128) -> bool {
        self.at_least.is_none_or(|edge| measured >= edge)
            && self.at_most.is_none_or(|edge| measured <= edge)
    }
}

/// The sweep a claim was registered against.
///
/// A threshold written for one scenario grades that scenario. Letting it grade another would be
/// the same failure as choosing it afterwards, arrived at from a different direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliesTo {
    /// The scenario by name.
    pub scenario: String,
    /// The seed the claim was registered for.
    pub seed: u64,
    /// How many runs it was registered for.
    pub n_runs: u64,
    /// How many ticks each of those runs was registered to be.
    pub n_ticks: u64,
}

/// A claim, its zones, and where its numbers came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    /// The claim's own name, which is how a verdict is referred to afterwards.
    pub name: String,
    /// When it was registered, as the registry writes it.
    pub registered: String,
    /// What is being claimed, in a sentence a reader can disagree with.
    pub statement: String,
    /// Where the thresholds came from. A zone with no basis is a number someone liked.
    pub basis: String,
    /// The sweep this grades, and no other.
    pub applies_to: AppliesTo,
    /// The graded bands, in the order they are tried.
    pub zones: Vec<Zone>,
}

/// A claim that cannot grade anything as registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidClaim {
    /// A claim with no zones grades nothing.
    #[error("claim `{name}` registers no zones: it can never reach a verdict")]
    NoZones {
        /// The claim's name.
        name: String,
    },
    /// A zone with neither edge holds every measurement, so registering it is registering
    /// nothing.
    #[error("claim `{name}` has a zone on `{metric}` with neither edge: it holds everything")]
    UnboundedZone {
        /// The claim's name.
        name: String,
        /// The metric whose zone is unbounded.
        metric: String,
    },
    /// A zone whose edges exclude each other can never hold anything.
    #[error("claim `{name}` has a zone on `{metric}` whose edges cross: it holds nothing")]
    EmptyZone {
        /// The claim's name.
        name: String,
        /// The metric whose zone is empty.
        metric: String,
    },
}

impl Claim {
    /// The checks a claim must pass before it is allowed to grade anything.
    ///
    /// # Errors
    ///
    /// Fails when the claim registers no zones, or a zone that holds everything or nothing.
    pub fn validate(&self) -> Result<(), InvalidClaim> {
        if self.zones.is_empty() {
            return Err(InvalidClaim::NoZones {
                name: self.name.clone(),
            });
        }
        for zone in &self.zones {
            match (zone.at_least, zone.at_most) {
                (None, None) => {
                    return Err(InvalidClaim::UnboundedZone {
                        name: self.name.clone(),
                        metric: zone.metric.clone(),
                    });
                }
                (Some(low), Some(high)) if low > high => {
                    return Err(InvalidClaim::EmptyZone {
                        name: self.name.clone(),
                        metric: zone.metric.clone(),
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// How a sweep graded against a claim, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Graded {
    /// A measurement landed in a zone that says the claim holds.
    Confirmed {
        /// Which metric and statistic decided it, and what it measured.
        by: String,
    },
    /// A measurement landed in a zone that says the claim does not hold.
    Refuted {
        /// Which metric and statistic decided it, and what it measured.
        by: String,
    },
    /// No verdict, and the reason. Not a failure — most sweeps have nothing registered against
    /// them, and saying so is the honest outcome.
    NoVerdict {
        /// Why the claim could not grade this sweep.
        why: String,
    },
}

/// A claim as it appears in a report: the registration, quoted, and what it made of the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradedClaim {
    /// The claim that was registered.
    pub claim: Claim,
    /// What it graded this sweep as.
    pub graded: Graded,
}

/// Grades a sweep against a claim.
///
/// The first zone whose measurement lands decides it, so the order zones are registered in is
/// part of the registration. A sweep the claim was not registered against reaches no verdict,
/// and neither does a metric the sweep did not measure.
#[must_use]
pub fn grade(claim: &Claim, applies_to: &AppliesTo, metrics: &[MetricSummary]) -> Graded {
    if *applies_to != claim.applies_to {
        return Graded::NoVerdict {
            why: format!(
                "registered against {:?} and this sweep is {:?}",
                claim.applies_to, applies_to
            ),
        };
    }
    for zone in &claim.zones {
        let Some(metric) = metrics.iter().find(|m| m.name == zone.metric) else {
            continue;
        };
        let Some(measured) = metric.statistic(zone.statistic) else {
            continue;
        };
        if zone.holds(measured) {
            let by = format!("{} {:?} measured {measured}", zone.metric, zone.statistic);
            return match zone.says {
                Says::Confirmed => Graded::Confirmed { by },
                Says::Refuted => Graded::Refuted { by },
            };
        }
    }
    Graded::NoVerdict {
        why: "no registered zone held the measurements this sweep produced".into(),
    }
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

    fn applies_to() -> AppliesTo {
        AppliesTo {
            scenario: "smoke".into(),
            seed: 42,
            n_runs: 100,
            n_ticks: 500,
        }
    }

    fn zone(
        statistic: Statistic,
        at_least: Option<i128>,
        at_most: Option<i128>,
        says: Says,
    ) -> Zone {
        Zone {
            metric: "token_reserve".into(),
            statistic,
            at_least,
            at_most,
            says,
        }
    }

    fn claim(zones: Vec<Zone>) -> Claim {
        Claim {
            name: "the pool does not drain".into(),
            registered: "2026-08-19".into(),
            statement: "under symmetric uninformed flow the reserve does not drift".into(),
            basis: "the flow is a fair coin, so any drift is the rounding direction".into(),
            applies_to: applies_to(),
            zones,
        }
    }

    fn metrics(drift_base: i128, values: &[i128]) -> Vec<MetricSummary> {
        vec![MetricSummary::from_values("token_reserve", Some(drift_base), values).unwrap()]
    }

    #[test]
    fn a_measurement_in_a_confirming_zone_confirms_and_says_what_decided_it() {
        let claim = claim(vec![zone(
            Statistic::DriftBps,
            Some(-500),
            Some(500),
            Says::Confirmed,
        )]);
        let graded = grade(&claim, &applies_to(), &metrics(1_000, &[999, 1_000, 1_001]));
        let Graded::Confirmed { by } = graded else {
            panic!("a drift of nothing was not confirmed")
        };
        assert!(
            by.contains("token_reserve"),
            "the deciding measurement is missing from: {by}"
        );
    }

    #[test]
    fn a_measurement_in_a_refuting_zone_refutes() {
        let claim = claim(vec![zone(
            Statistic::DriftBps,
            Some(500),
            None,
            Says::Refuted,
        )]);
        let graded = grade(
            &claim,
            &applies_to(),
            &metrics(1_000, &[2_000, 2_000, 2_000]),
        );
        assert!(matches!(graded, Graded::Refuted { .. }));
    }

    /// The failure this exists to prevent, reached from the other direction: a threshold written
    /// for one sweep grading a different one is the same thing as writing it afterwards.
    #[test]
    fn a_claim_registered_against_another_sweep_grades_nothing() {
        let claim = claim(vec![zone(
            Statistic::Median,
            None,
            Some(i128::MAX),
            Says::Confirmed,
        )]);
        let elsewhere = AppliesTo {
            seed: 43,
            ..applies_to()
        };
        let Graded::NoVerdict { why } = grade(&claim, &elsewhere, &metrics(1_000, &[1_000])) else {
            panic!("a claim graded a sweep it was not registered against");
        };
        assert!(why.contains("registered against"), "unclear reason: {why}");
    }

    #[test]
    fn a_metric_the_sweep_did_not_measure_reaches_no_verdict() {
        let mut zone = zone(Statistic::Median, None, Some(10), Says::Confirmed);
        zone.metric = "a metric nobody measured".into();
        let graded = grade(&claim(vec![zone]), &applies_to(), &metrics(1_000, &[1_000]));
        assert!(matches!(graded, Graded::NoVerdict { .. }));
    }

    /// A metric with no base has no drift, and a zone written against a drift it does not have
    /// grades nothing rather than treating absence as zero.
    #[test]
    fn a_statistic_the_metric_does_not_have_reaches_no_verdict() {
        let claim = claim(vec![zone(
            Statistic::DriftBps,
            Some(-1),
            Some(1),
            Says::Confirmed,
        )]);
        let baseless = vec![MetricSummary::from_values("token_reserve", None, &[1, 2, 3]).unwrap()];
        assert!(matches!(
            grade(&claim, &applies_to(), &baseless),
            Graded::NoVerdict { .. }
        ));
    }

    #[test]
    fn a_measurement_in_no_zone_reaches_no_verdict() {
        let claim = claim(vec![zone(
            Statistic::Median,
            Some(5_000),
            None,
            Says::Confirmed,
        )]);
        let graded = grade(&claim, &applies_to(), &metrics(1_000, &[1_000]));
        let Graded::NoVerdict { why } = graded else {
            panic!("a measurement outside every zone graded")
        };
        assert!(why.contains("no registered zone"), "unclear reason: {why}");
    }

    #[test]
    fn the_first_zone_that_holds_is_the_one_that_decides() {
        let claim = claim(vec![
            zone(Statistic::Median, Some(0), None, Says::Refuted),
            zone(Statistic::Median, Some(0), None, Says::Confirmed),
        ]);
        assert!(matches!(
            grade(&claim, &applies_to(), &metrics(1_000, &[1_000])),
            Graded::Refuted { .. }
        ));
    }

    #[test]
    fn a_claim_with_no_zones_is_refused_at_registration() {
        assert!(matches!(
            claim(Vec::new()).validate(),
            Err(InvalidClaim::NoZones { .. })
        ));
    }

    #[test]
    fn a_zone_with_neither_edge_is_refused_at_registration() {
        let claim = claim(vec![zone(Statistic::Median, None, None, Says::Confirmed)]);
        assert!(matches!(
            claim.validate(),
            Err(InvalidClaim::UnboundedZone { .. })
        ));
    }

    #[test]
    fn a_zone_whose_edges_cross_is_refused_at_registration() {
        let claim = claim(vec![zone(
            Statistic::Median,
            Some(10),
            Some(1),
            Says::Confirmed,
        )]);
        assert!(matches!(
            claim.validate(),
            Err(InvalidClaim::EmptyZone { .. })
        ));
    }

    #[test]
    fn a_valid_claim_survives_the_registry_boundary() {
        let claim = claim(vec![zone(
            Statistic::DriftBps,
            Some(-500),
            Some(500),
            Says::Confirmed,
        )]);
        claim.validate().unwrap();
        let json = serde_json::to_string(&claim).unwrap();
        let back: Claim = serde_json::from_str(&json).unwrap();
        assert_eq!(back.applies_to, claim.applies_to);
        assert_eq!(back.zones[0].at_least, Some(-500));
    }
}
