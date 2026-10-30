//! Run reports: the structured account every sweep must give of itself.
//!
//! A report answers the reader's questions without its author in the room, which fixes the
//! shape of its content (karman/docs/REPORTING.md): identity that reproduces the run, initial
//! conditions the sweep started from, coverage that admits what was lost, distributions with
//! their base and drift instead of point estimates, pointers to the extreme runs, and a gaps
//! section that names what the run cannot say. A report missing any of these is not a shorter
//! report — it is a different, less honest document.
//!
//! A seventh section appears only where something was registered to appear in it: a verdict,
//! against thresholds fixed before the run existed. A report with nothing registered against it
//! says so, which is the honest answer and the usual one.
//!
//! The crate is pure: it turns values into values (a report struct, then Markdown or JSON).
//! Reading results and writing files stay with the caller, on the other side of the I/O
//! boundary.

mod claim;
mod rank;
mod sweep;

pub use claim::{
    AppliesTo, Claim, Graded, GradedClaim, InvalidClaim, Says, Statistic, Zone, grade,
};
pub use rank::rank_correlation;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
pub use sweep::{SweepPoint, SweepReport};

/// Everything needed to reproduce the run this report describes. The header of every report:
/// a report whose numbers cannot be reproduced is an opinion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// The scenario this sweep ran, by the name the scenario gives itself.
    pub scenario_name: String,
    /// The seed actually used, which may be an override rather than the scenario's own.
    pub seed: u64,
    /// How many runs were asked for. What was achieved is in [`Coverage`].
    pub n_runs: u64,
    /// Length of each run, in logical steps.
    pub n_ticks: u64,
    /// The exact command that reruns this sweep.
    pub reproduce: String,
}

/// One named fact about where the sweep started: a reserve, a population size, a parameter.
/// The report states its starting point so every drift in it has a visible base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialCondition {
    /// What the fact is about.
    pub name: String,
    /// Its value at the start, rendered — a base for every drift reported later.
    pub value: String,
}

/// What was asked for versus what actually happened. Aborted runs are named, not netted out:
/// a sweep that quietly lost a third of its runs reports a different reality than it measured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    /// How many runs were asked for.
    pub runs_planned: u64,
    /// How many produced a result the distributions were computed from.
    pub runs_completed: u64,
    /// How many stopped instead. Never netted out of the planned figure.
    pub runs_aborted: u64,
    /// The first abort's reason, verbatim, when any run aborted — the start of the diagnosis.
    pub first_abort: Option<String>,
}

/// Fewer runs than this in a hundred, and a thing that happened is stated as a count rather
/// than as a rate.
///
/// Not a significance test and not pretending to be one. It is a rule about language: a reader
/// handed "0.03%" carries away an expectation, and a reader handed "three runs in ten thousand"
/// carries away three runs. The second is what was measured.
const RARELY: u64 = 10;

/// What a sweep reached, counted rather than distributed.
///
/// A distribution answers how much; this answers how often, and whether it ever happened at all —
/// and the second question is one no single run can answer. It is also the half a reader is most
/// likely to be denied: a sweep that never once reached the thing it was run to study produces no
/// distribution to argue with and no finding to publish, and the silence reads like absence of
/// risk rather than absence of evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// What was watched for.
    pub name: String,
    /// Runs that could have reached it.
    pub runs: u64,
    /// Runs that did.
    pub reached: u64,
    /// The first run that did, so a reproduction has a number to start from.
    pub first: Option<u64>,
}

impl Outcome {
    /// Nothing seen yet, over no runs.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Outcome {
            name: name.into(),
            runs: 0,
            reached: 0,
            first: None,
        }
    }

    /// Records one run, and whether it reached this.
    pub fn saw(&mut self, run: u64, reached: bool) {
        self.runs += 1;
        if reached {
            self.reached += 1;
            self.first = Some(self.first.map_or(run, |first| first.min(run)));
        }
    }

    /// Two counts of the same thing, from two pieces of one sweep, as one.
    ///
    /// Addition, and the earlier of the two firsts. A sweep is cut into pieces and run on however
    /// many workers there are, so what folds it back has to give one answer whatever order the
    /// pieces arrive in — which is the same promise the sweep itself makes.
    #[must_use]
    pub fn merged(mut self, other: &Outcome) -> Self {
        self.runs += other.runs;
        self.reached += other.reached;
        self.first = match (self.first, other.first) {
            (Some(mine), Some(theirs)) => Some(mine.min(theirs)),
            (mine, theirs) => mine.or(theirs),
        };
        self
    }

    /// How a report states it.
    ///
    /// Below one run in ten it is a count and never a rate.
    #[must_use]
    pub fn stated(&self) -> String {
        if self.runs == 0 {
            return String::from("no run could have reached it");
        }
        if self.reached == 0 {
            return format!("never, in {} runs", self.runs);
        }
        if self.reached * 100 < self.runs * RARELY {
            return format!("{} of {} runs", self.reached, self.runs);
        }
        let hundredths = self.reached * 10_000 / self.runs;
        format!(
            "{}.{:02}% of runs ({} of {})",
            hundredths / 100,
            hundredths % 100,
            self.reached,
            self.runs
        )
    }
}

/// A basis-point figure as a percentage, truncated rather than rounded.
///
/// Truncated because every one of these is a share of something, and a share rounded up reads
/// as more of it than was measured. Integer throughout, like the numbers it renders.
#[allow(
    clippy::cast_precision_loss,
    reason = "a basis-point figure printed to two decimals is a percentage, not a quantity"
)]
fn signed_percent(bps: i128) -> String {
    format!("{:+.2}%", bps as f64 / 100.0)
}

fn percent(bps: i128) -> String {
    format!("{}.{:02}%", bps / 100, (bps % 100).abs())
}

/// Fewer runs than this in one part, and it states its middle without a corridor around it.
///
/// Derived rather than chosen. By nearest rank the fifth percentile is the value at rank
/// `ceil(n/20)`, which is the first rank — the minimum itself — until the twenty-first run. Below
/// that the corridor's lower edge is the extreme wearing a percentile's name, and printing it
/// beside corridors drawn from hundreds invites a comparison the numbers cannot carry.
const THIN: u64 = 21;

/// One named part of a sweep, summarised on its own.
///
/// A sweep of a mixture is several experiments sharing a report, and collapsing them into one
/// distribution answers a question nobody asked — what the average of futures that never happen
/// together is — while hiding the only thing a mixture is run to show, which of them produced
/// what.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// The part, as the scenario names it and at the depth it was swept at.
    pub name: String,
    /// Runs of it that produced values.
    ///
    /// What the numbers beside it are about, which is why a corridor is judged against this and
    /// not against what was planned. A sweep that lost runs says how many and why under Coverage.
    pub runs: u64,
    /// Its own distributions.
    pub metrics: Vec<MetricSummary>,
    /// What it reached.
    pub outcomes: Vec<Outcome>,
}

impl Segment {
    /// Whether this part has too few runs for its corridor to be anything but its extremes.
    #[must_use]
    pub fn is_thin(&self) -> bool {
        self.runs < THIN
    }
}

/// The distribution of one metric across completed runs, by nearest-rank percentiles over
/// integer values — no interpolation, so the summary is exact and platform-independent.
/// When the metric has a base (its value before the sweep), the report derives drift from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSummary {
    /// What was measured, as the report names it in its table.
    pub name: String,
    /// The metric's value before the sweep, when it has one; drift is computed against it.
    #[serde(with = "sting_karman_types::i128_str::opt")]
    pub base: Option<i128>,
    /// The lowest value any completed run reached.
    #[serde(with = "sting_karman_types::i128_str")]
    pub min: i128,
    /// The lower end of the corridor a reader should quote beside the median.
    #[serde(with = "sting_karman_types::i128_str")]
    pub p05: i128,
    /// The lower quartile.
    #[serde(with = "sting_karman_types::i128_str")]
    pub p25: i128,
    /// The central value, and the one drift is measured from.
    #[serde(with = "sting_karman_types::i128_str")]
    pub median: i128,
    /// The upper quartile.
    #[serde(with = "sting_karman_types::i128_str")]
    pub p75: i128,
    /// The upper end of the corridor.
    #[serde(with = "sting_karman_types::i128_str")]
    pub p95: i128,
    /// The highest value any completed run reached.
    #[serde(with = "sting_karman_types::i128_str")]
    pub max: i128,
    /// The spread about the mean, over the runs themselves rather than a sample of them.
    ///
    /// Integer, floored, and computed as deviations from the integer mean, so it is exact in the
    /// arithmetic the rest of this report is exact in. A corridor says where the middle nine
    /// tenths landed; this says how tightly, and the two disagree exactly when the tails are
    /// long — which is when a reader most needs to be told.
    ///
    /// A deviation whose square leaves this width saturates rather than wrapping, and the
    /// stability derived from it then reads as none at all. That takes values around `1e19` in a
    /// metric measured in base units, which no market this reads produces; the direction is the
    /// safe one either way.
    #[serde(default, with = "sting_karman_types::i128_str")]
    pub sigma: i128,
}

impl MetricSummary {
    /// Summarises a metric's values by nearest-rank percentiles. Returns `None` on an empty
    /// set: an empty distribution has no summary, and inventing one would hide a coverage
    /// failure the report exists to expose.
    #[must_use]
    pub fn from_values(
        name: impl Into<String>,
        base: Option<i128>,
        values: &[i128],
    ) -> Option<MetricSummary> {
        if values.is_empty() {
            return None;
        }
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let rank = |q_num: usize, q_den: usize| -> i128 {
            let n = sorted.len();
            let idx = (n * q_num).div_ceil(q_den).max(1) - 1;
            sorted[idx.min(n - 1)]
        };
        let count = i128::try_from(sorted.len()).unwrap_or(i128::MAX);
        let mean = sorted.iter().sum::<i128>() / count;
        // Deviations from the integer mean rather than the textbook `n·Σv² − (Σv)²`, which
        // overflows this width at run counts a sweep actually reaches.
        let variance = sorted
            .iter()
            .map(|value| {
                let apart = value - mean;
                apart.saturating_mul(apart)
            })
            .fold(0i128, i128::saturating_add)
            / count;
        Some(MetricSummary {
            name: name.into(),
            base,
            min: sorted[0],
            p05: rank(5, 100),
            p25: rank(25, 100),
            median: rank(50, 100),
            p75: rank(75, 100),
            p95: rank(95, 100),
            max: sorted[sorted.len() - 1],
            sigma: i128::try_from(u128::try_from(variance).unwrap_or(0).isqrt()).unwrap_or(0),
        })
    }

    /// How tightly this landed, as a share of where it landed, in basis points.
    ///
    /// One whole minus the spread over the median, floored at zero, which is the shape a reader
    /// compares two designs by. Absent where the median is zero: a spread cannot be expressed as
    /// a share of nothing, and answering zero there would call a metric that never moved the
    /// least stable thing in the report.
    #[must_use]
    pub fn stability_bps(&self) -> Option<i128> {
        if self.median == 0 {
            return None;
        }
        Some((10_000 - self.sigma.saturating_mul(10_000) / self.median.abs()).max(0))
    }

    /// One number out of this distribution, by name.
    ///
    /// Returns nothing where the statistic does not exist for this metric — a drift needs a
    /// base, and a metric without one has no drift rather than a drift of zero.
    #[must_use]
    pub fn statistic(&self, which: Statistic) -> Option<i128> {
        match which {
            Statistic::Min => Some(self.min),
            Statistic::P05 => Some(self.p05),
            Statistic::P25 => Some(self.p25),
            Statistic::Median => Some(self.median),
            Statistic::P75 => Some(self.p75),
            Statistic::P95 => Some(self.p95),
            Statistic::Max => Some(self.max),
            Statistic::DriftBps => self.drift_bps(),
            Statistic::CorridorBps => self.corridor_bps(),
            Statistic::Sigma => Some(self.sigma),
            Statistic::StabilityBps => self.stability_bps(),
        }
    }

    /// Median drift from base in basis points, when a nonzero base exists.
    #[must_use]
    pub fn drift_bps(&self) -> Option<i128> {
        let base = self.base?;
        if base == 0 {
            return None;
        }
        Some((self.median - base).saturating_mul(10_000) / base)
    }

    /// Width of the p05–p95 corridor relative to the median, in basis points — the spread the
    /// reader needs next to every central value.
    #[must_use]
    pub fn corridor_bps(&self) -> Option<i128> {
        if self.median == 0 {
            return None;
        }
        Some((self.p95 - self.p05).saturating_mul(10_000) / self.median)
    }
}

/// Reproduction pointers to the runs at a metric's two tails. A distribution says how wide the
/// outcome space is; these say exactly which runs to replay to watch the best and worst of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtremeRuns {
    /// Which metric these two runs are the tails of.
    pub metric: String,
    /// Run index (= RNG stream) of the lowest final value.
    pub lowest_run: u64,
    /// What that run ended at.
    #[serde(with = "sting_karman_types::i128_str")]
    pub lowest_value: i128,
    /// Run index (= RNG stream) of the highest final value.
    pub highest_run: u64,
    /// What that run ended at.
    #[serde(with = "sting_karman_types::i128_str")]
    pub highest_value: i128,
}

/// The full report of one sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    /// Everything needed to run this sweep again.
    pub identity: Identity,
    /// Where the sweep started, so every drift in it has a visible base.
    pub initial_conditions: Vec<InitialCondition>,
    /// What was asked for against what happened.
    pub coverage: Coverage,
    /// One distribution per measured quantity.
    pub metrics: Vec<MetricSummary>,
    /// Pointers to the runs at each metric's two tails.
    pub extremes: Vec<ExtremeRuns>,
    /// What the sweep reached, and what it never did.
    #[serde(default)]
    pub outcomes: Vec<Outcome>,
    /// The same, part by part, where the sweep was a mixture of futures.
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// What this run cannot answer: unimplemented layers, excluded data, known biases — each
    /// with its direction. Filled by the composition root, which knows what it wired and what
    /// it left out. An empty list is a claim of completeness, and it is almost always false.
    pub gaps: Vec<String>,
    /// The claim this sweep was graded against, where one was registered before it ran.
    #[serde(default)]
    pub claim: Option<GradedClaim>,
}

impl RunReport {
    /// Renders the report as Markdown. Every section renders even when empty-handed —
    /// "no runs aborted" is information, an absent coverage section is a question.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "one block per section, and a report missing a section is a different, less honest document"
    )]
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        let _ = write!(md, "# Run report — {}\n\n", self.identity.scenario_name);
        md.push_str(
            "> How to read this report: every central value carries its base, its drift and \
             its p05–p95 corridor; Coverage says what was lost; Extreme runs are reproducible \
             by their run index; Gaps bound what these numbers may be used to claim.\n\n",
        );

        md.push_str("## Identity\n\n");
        let _ = write!(
            md,
            "- seed: `{}` · runs: {} · ticks: {}\n- reproduce: `{}`\n\n",
            self.identity.seed,
            self.identity.n_runs,
            self.identity.n_ticks,
            self.identity.reproduce
        );

        md.push_str("## Initial conditions\n\n");
        if self.initial_conditions.is_empty() {
            md.push_str(
                "None declared — the sweep claims to start from nothing, which is \
                         itself worth questioning.\n\n",
            );
        } else {
            for c in &self.initial_conditions {
                let _ = writeln!(md, "- {}: {}", c.name, c.value);
            }
            md.push('\n');
        }

        md.push_str("## Coverage\n\n");
        let _ = writeln!(
            md,
            "- planned: {} · completed: {} · aborted: {}",
            self.coverage.runs_planned, self.coverage.runs_completed, self.coverage.runs_aborted
        );
        match &self.coverage.first_abort {
            Some(reason) => {
                let _ = write!(md, "- first abort: {reason}\n\n");
            }
            None => md.push_str("- no runs aborted\n\n"),
        }

        md.push_str("## Outcomes\n\n");
        if self.outcomes.is_empty() {
            md.push_str(
                "Nothing was watched for. A sweep that counted no outcome cannot say whether it \
                 ever reached the thing it was run to study.\n\n",
            );
        } else {
            for outcome in &self.outcomes {
                let first = outcome
                    .first
                    .map_or_else(String::new, |run| format!(" · first at run {run}"));
                let _ = writeln!(md, "- {}: {}{}", outcome.name, outcome.stated(), first);
            }
            md.push('\n');
        }

        md.push_str("## Distributions\n\n");
        if self.metrics.is_empty() {
            md.push_str("No metric produced values: see Coverage.\n\n");
        } else {
            md.push_str(
                "| metric | base | median | drift | min | p05 | p25 | p75 | p95 | max | σ | stability | corridor (p05–p95)/median |\n",
            );
            md.push_str("|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
            for m in &self.metrics {
                let base = m.base.map_or_else(|| "—".to_string(), |b| b.to_string());
                let drift = m
                    .drift_bps()
                    .map_or_else(|| "—".to_string(), signed_percent);
                let corridor = m.corridor_bps().map_or_else(|| "—".to_string(), percent);
                let stability = m.stability_bps().map_or_else(|| "—".to_string(), percent);
                let _ = writeln!(
                    md,
                    "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                    m.name,
                    base,
                    m.median,
                    drift,
                    m.min,
                    m.p05,
                    m.p25,
                    m.p75,
                    m.p95,
                    m.max,
                    m.sigma,
                    stability,
                    corridor
                );
            }
            md.push('\n');
        }

        md.push_str("## By regime\n\n");
        if self.segments.is_empty() {
            md.push_str(
                "This sweep is one future, so the distributions above are already about it. A \
                 mixture would be reported part by part here.\n\n",
            );
        } else {
            md.push_str(
                "> One row per future the sweep was divided between, at the depth it was swept \
                 at. A mixture collapsed into a single distribution reports the average of \
                 futures that never happen together, which is a number nobody asked for.\n\n",
            );
            let thin = self.segments.iter().any(Segment::is_thin);
            for name in self.metrics.iter().map(|m| &m.name) {
                let _ = write!(md, "### {name}\n\n");
                md.push_str("| regime | runs | median | p05 | p95 | σ | stability |\n");
                md.push_str("|---|---|---|---|---|---|---|\n");
                for part in &self.segments {
                    let Some(m) = part.metrics.iter().find(|m| &m.name == name) else {
                        continue;
                    };
                    if part.is_thin() {
                        let _ = writeln!(
                            md,
                            "| {} | {} | {} | — | — | — | — |",
                            part.name, part.runs, m.median,
                        );
                        continue;
                    }
                    let stability = m.stability_bps().map_or_else(|| "—".to_string(), percent);
                    let _ = writeln!(
                        md,
                        "| {} | {} | {} | {} | {} | {} | {} |",
                        part.name, part.runs, m.median, m.p05, m.p95, m.sigma, stability,
                    );
                }
                md.push('\n');
                if thin {
                    md.push_str(
                        "A part of fewer than 21 runs states its middle and no corridor: at that \
                         count the fifth and ninety-fifth percentiles are the extremes \
                         themselves.\n\n",
                    );
                }
            }

            let watched: Vec<&String> = self
                .segments
                .first()
                .map(|part| part.outcomes.iter().map(|o| &o.name).collect())
                .unwrap_or_default();
            if !watched.is_empty() {
                md.push_str("### Outcomes by regime\n\n");
                md.push_str("| regime | runs |");
                for name in &watched {
                    let _ = write!(md, " {name} |");
                }
                md.push_str("\n|---|---|");
                for _ in &watched {
                    md.push_str("---|");
                }
                md.push('\n');
                for part in &self.segments {
                    let _ = write!(md, "| {} | {} |", part.name, part.runs);
                    for name in &watched {
                        let stated = part
                            .outcomes
                            .iter()
                            .find(|o| &&o.name == name)
                            .map_or_else(|| "—".to_string(), Outcome::stated);
                        let _ = write!(md, " {stated} |");
                    }
                    md.push('\n');
                }
                md.push('\n');
            }
        }

        md.push_str("## Extreme runs\n\n");
        if self.extremes.is_empty() {
            md.push_str("Not tracked for this sweep.\n\n");
        } else {
            md.push_str(
                "Replay any of these exactly: same scenario, same seed — the run index is the \
                 RNG stream.\n\n",
            );
            md.push_str("| metric | lowest | at run | highest | at run |\n");
            md.push_str("|---|---|---|---|---|\n");
            for e in &self.extremes {
                let _ = writeln!(
                    md,
                    "| {} | {} | #{} | {} | #{} |",
                    e.metric, e.lowest_value, e.lowest_run, e.highest_value, e.highest_run
                );
            }
            md.push('\n');
        }

        md.push_str("## Verdict\n\n");
        match &self.claim {
            None => md.push_str(
                "No claim was registered against this sweep, so it states no verdict. A \
                 threshold chosen after these numbers existed would describe them rather than \
                 grade them.\n\n",
            ),
            Some(graded) => {
                let _ = write!(
                    md,
                    "**{}** — registered {}\n\n> {}\n\nBasis: {}\n\n",
                    graded.claim.name,
                    graded.claim.registered,
                    graded.claim.statement,
                    graded.claim.basis
                );
                md.push_str("| metric | statistic | at least | at most | says |\n");
                md.push_str("|---|---|---|---|---|\n");
                for zone in &graded.claim.zones {
                    let _ = writeln!(
                        md,
                        "| {} | {:?} | {} | {} | {:?} |",
                        zone.metric,
                        zone.statistic,
                        zone.at_least.map_or_else(|| "—".into(), |v| v.to_string()),
                        zone.at_most.map_or_else(|| "—".into(), |v| v.to_string()),
                        zone.says
                    );
                }
                let outcome = match &graded.graded {
                    Graded::Confirmed { by } => format!("**confirmed** — {by}"),
                    Graded::Refuted { by } => format!("**refuted** — {by}"),
                    Graded::NoVerdict { why } => format!("**no verdict** — {why}"),
                };
                let _ = write!(md, "\n{outcome}\n\n");
            }
        }

        md.push_str("## Gaps — what this run cannot answer\n\n");
        if self.gaps.is_empty() {
            md.push_str(
                "None declared. A sweep with no declared gaps claims completeness — \
                 treat that claim with suspicion.\n",
            );
        } else {
            for gap in &self.gaps {
                let _ = writeln!(md, "- {gap}");
            }
        }
        md.push('\n');
        md
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod outcome_tests {
    use super::*;

    /// A thing seen three times in ten thousand runs is three runs, and saying "0.03%" would hand
    /// a reader an expectation where there is a count.
    #[test]
    fn something_rare_is_stated_as_a_count_and_never_as_a_rate() {
        let mut rare = Outcome::named("a loss");
        for run in 0..10_000 {
            rare.saw(run, run == 17 || run == 4_000 || run == 9_999);
        }
        assert_eq!(rare.stated(), "3 of 10000 runs");
        assert_eq!(rare.first, Some(17));
    }

    /// Above the threshold it is a rate, with the count beside it so the rate can be checked.
    #[test]
    fn something_common_is_stated_as_a_rate_with_its_count() {
        let mut common = Outcome::named("a liquidation");
        for run in 0..1_000 {
            common.saw(run, run % 4 == 0);
        }
        assert_eq!(common.stated(), "25.00% of runs (250 of 1000)");
    }

    /// Never is information, and a sweep that reached nothing has to say so rather than fall
    /// silent — the silence reads as absence of risk.
    #[test]
    fn something_never_reached_says_how_many_runs_did_not_reach_it() {
        let mut never = Outcome::named("a cascade");
        for run in 0..500 {
            never.saw(run, false);
        }
        assert_eq!(never.stated(), "never, in 500 runs");
        assert_eq!(never.first, None);
    }

    /// A sweep is cut into pieces and folded back on however many workers there are, so the fold
    /// has to give one answer whatever order the pieces arrive in.
    #[test]
    fn pieces_of_one_sweep_fold_to_the_same_answer_in_any_order() {
        let piece = |runs: std::ops::Range<u64>, hit: fn(u64) -> bool| {
            let mut out = Outcome::named("a loss");
            for run in runs {
                out.saw(run, hit(run));
            }
            out
        };
        let first = piece(0..50, |run| run == 7);
        let second = piece(50..100, |run| run == 80);
        let third = piece(100..150, |_| false);

        let forwards = first.clone().merged(&second).merged(&third);
        let backwards = third.merged(&second).merged(&first);
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.runs, 150);
        assert_eq!(forwards.reached, 2);
        assert_eq!(forwards.first, Some(7));
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

    #[test]
    fn percentiles_are_nearest_rank() {
        let values: Vec<i128> = (1..=100).collect();
        let s = MetricSummary::from_values("m", None, &values).unwrap();
        assert_eq!(s.min, 1);
        assert_eq!(s.p05, 5);
        assert_eq!(s.median, 50);
        assert_eq!(s.p95, 95);
        assert_eq!(s.max, 100);
    }

    #[test]
    fn drift_and_corridor_are_in_basis_points() {
        let values: Vec<i128> = (900..=1100).collect();
        let s = MetricSummary::from_values("m", Some(1_000), &values).unwrap();
        assert_eq!(s.median, 1_000);
        assert_eq!(s.drift_bps(), Some(0));
        // Corridor: p05 = 910, p95 = 1090 -> width 180 over median 1000 = 1800 bps.
        assert_eq!(s.corridor_bps(), Some(1_800));
    }

    #[test]
    fn zero_base_yields_no_drift() {
        let s = MetricSummary::from_values("m", Some(0), &[1, 2, 3]).unwrap();
        assert_eq!(s.drift_bps(), None);
    }

    #[test]
    fn empty_values_yield_no_summary() {
        assert!(MetricSummary::from_values("m", None, &[]).is_none());
    }

    /// The rendering rule the crate exists to enforce: a section with nothing to say still
    /// appears and says so, because an absent section reads as a question rather than a fact.
    #[test]
    fn an_empty_handed_report_still_renders_every_section() {
        let report = RunReport {
            identity: Identity {
                scenario_name: "empty".into(),
                seed: 0,
                n_runs: 0,
                n_ticks: 0,
                reproduce: "karman run --scenario s.json --runs 0".into(),
            },
            initial_conditions: Vec::new(),
            coverage: Coverage {
                runs_planned: 0,
                runs_completed: 0,
                runs_aborted: 0,
                first_abort: None,
            },
            metrics: Vec::new(),
            extremes: Vec::new(),
            outcomes: Vec::new(),
            segments: Vec::new(),
            gaps: Vec::new(),
            claim: None,
        };
        let md = report.to_markdown();
        for section in [
            "## Identity",
            "## Initial conditions",
            "## Coverage",
            "## Outcomes",
            "## Distributions",
            "## By regime",
            "## Extreme runs",
            "## Gaps",
        ] {
            assert!(md.contains(section), "missing section {section}");
        }
        assert!(md.contains("no runs aborted"));
        assert!(
            md.contains("None declared"),
            "an undeclared gaps list must say so out loud"
        );
    }

    /// The spread is the population one, over deviations from the integer mean, and it is
    /// floored like every other number here rather than carried at a precision it does not have.
    #[test]
    fn the_spread_is_measured_over_the_runs_themselves() {
        let steady = MetricSummary::from_values("m", None, &[10, 10, 10, 10]).unwrap();
        assert_eq!(steady.sigma, 0);
        assert_eq!(steady.stability_bps(), Some(10_000));

        // Deviations 2, 1, 1, 2 give a variance of ten halved to two, whose root is one and a
        // half and floors to one; the median is the second of four by nearest rank.
        let spread = MetricSummary::from_values("m", None, &[8, 9, 11, 12]).unwrap();
        assert_eq!(spread.sigma, 1);
        assert_eq!(spread.median, 9);
        assert_eq!(spread.stability_bps(), Some(8_889));
    }

    /// A spread wider than the middle is not negative stability, and a middle of nothing has no
    /// stability at all — answering zero there would call a metric that never moved the least
    /// stable thing in the report.
    #[test]
    fn stability_has_a_floor_and_an_absence() {
        let wild = MetricSummary::from_values("m", None, &[1, 1, 1, 1_000]).unwrap();
        assert_eq!(wild.stability_bps(), Some(0));

        let still = MetricSummary::from_values("m", None, &[0, 0, 0, 0]).unwrap();
        assert_eq!(still.sigma, 0);
        assert_eq!(still.stability_bps(), None);
    }

    /// Twenty runs put the fifth percentile on the minimum itself, which is a lower edge in name
    /// only; twenty-one is where it steps inside.
    #[test]
    fn a_corridor_needs_enough_runs_to_have_edges() {
        let of = |runs: u64| Segment {
            name: "part".into(),
            runs,
            metrics: Vec::new(),
            outcomes: Vec::new(),
        };
        assert!(of(20).is_thin());
        assert!(!of(21).is_thin());

        let twenty: Vec<i128> = (1..=20).collect();
        let short = MetricSummary::from_values("m", None, &twenty).unwrap();
        assert_eq!(short.p05, short.min);
        let twenty_one: Vec<i128> = (1..=21).collect();
        let long = MetricSummary::from_values("m", None, &twenty_one).unwrap();
        assert!(long.p05 > long.min);
        assert!(long.p95 < long.max);
    }

    /// A mixture is reported part by part, and a part too small for a corridor says so instead
    /// of printing its extremes as one.
    #[test]
    fn a_mixture_is_reported_part_by_part() {
        let part = |name: &str, runs: u64, values: &[i128]| Segment {
            name: name.into(),
            runs,
            metrics: MetricSummary::from_values("written_off", None, values)
                .into_iter()
                .collect(),
            outcomes: vec![Outcome {
                name: "a loss".into(),
                runs,
                reached: 0,
                first: None,
            }],
        };
        let mut report = report();
        report.metrics = MetricSummary::from_values("written_off", None, &[0])
            .into_iter()
            .collect();
        report.segments = vec![
            part("held @ 8500 bps", 30, &vec![0; 30]),
            part("past @ 7900 bps", 5, &[1, 2, 3, 4, 5]),
        ];

        let md = report.to_markdown();
        assert!(md.contains("## By regime"));
        assert!(md.contains("| held @ 8500 bps | 30 | 0 | 0 | 0 | 0 | — |"));
        assert!(md.contains("| past @ 7900 bps | 5 | 3 | — | — | — | — |"));
        assert!(md.contains("A part of fewer than 21 runs"));
        assert!(md.contains("### Outcomes by regime"));
    }

    /// A sweep of one future is already about that future, and says so rather than repeating the
    /// distributions above under a heading.
    #[test]
    fn one_future_is_not_reported_twice() {
        let md = report().to_markdown();
        assert!(md.contains("## By regime"));
        assert!(md.contains("This sweep is one future"));
    }

    fn report() -> RunReport {
        RunReport {
            identity: Identity {
                scenario_name: "smoke".into(),
                seed: 42,
                n_runs: 10,
                n_ticks: 100,
                reproduce: "karman run --scenario s.json --runs 10 --seed 42".into(),
            },
            initial_conditions: vec![InitialCondition {
                name: "token_reserve".into(),
                value: "1000".into(),
            }],
            coverage: Coverage {
                runs_planned: 10,
                runs_completed: 10,
                runs_aborted: 0,
                first_abort: None,
            },
            metrics: vec![
                MetricSummary::from_values("token_reserve", Some(2), &[1, 2, 3]).unwrap(),
            ],
            outcomes: Vec::new(),
            segments: Vec::new(),
            extremes: vec![ExtremeRuns {
                metric: "token_reserve".into(),
                lowest_run: 3,
                lowest_value: 1,
                highest_run: 7,
                highest_value: 3,
            }],
            gaps: vec!["no fee layer".into()],
            claim: None,
        }
    }

    #[test]
    fn markdown_carries_every_mandatory_section() {
        let md = report().to_markdown();
        for section in [
            "## Identity",
            "## Initial conditions",
            "## Coverage",
            "## Outcomes",
            "## Distributions",
            "## By regime",
            "## Extreme runs",
            "## Verdict",
            "## Gaps",
        ] {
            assert!(md.contains(section), "missing section {section}");
        }
        assert!(
            md.contains("No claim was registered"),
            "an ungraded sweep hid its own silence"
        );
    }

    /// A graded sweep renders the registration beside the outcome, so the reader compares the
    /// thresholds against the numbers without taking anyone's word for which came first.
    #[test]
    fn a_graded_sweep_quotes_the_registration_it_was_graded_by() {
        let applies_to = AppliesTo {
            scenario: "smoke".into(),
            seed: 42,
            n_runs: 10,
            n_ticks: 100,
        };
        let claim = Claim {
            name: "the pool does not drain".into(),
            registered: "2026-08-19".into(),
            statement: "symmetric flow leaves the reserve where it found it".into(),
            basis: "the flow is a fair coin, so a drift is the rounding direction".into(),
            applies_to: applies_to.clone(),
            zones: vec![Zone {
                metric: "token_reserve".into(),
                statistic: Statistic::DriftBps,
                at_least: Some(-500),
                at_most: Some(500),
                says: Says::Confirmed,
            }],
        };
        let metrics =
            vec![MetricSummary::from_values("token_reserve", Some(1_000), &[1_000]).unwrap()];
        let graded = grade(&claim, &applies_to, &metrics);
        let mut report = report();
        report.metrics = metrics;
        report.claim = Some(GradedClaim { claim, graded });

        let md = report.to_markdown();
        assert!(
            md.contains("the pool does not drain"),
            "the claim is missing"
        );
        assert!(
            md.contains("2026-08-19"),
            "the registration date is missing"
        );
        assert!(
            md.contains("Basis:"),
            "the basis of the thresholds is missing"
        );
        assert!(md.contains("**confirmed**"), "the outcome is missing");
        assert!(md.contains("-500"), "the registered zone is missing");
    }
}
