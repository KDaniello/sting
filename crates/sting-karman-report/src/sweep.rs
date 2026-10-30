//! The comparative report of a parameter sweep: one design per row, judged side by side.
//!
//! A run report answers what one design does. A parameter sweep answers where a design stops
//! working, which is a different question and needs the answers put next to each other rather
//! than in separate files a reader has to hold in their head.
//!
//! Every point carries its own coverage. A sweep whose middle points quietly aborted would
//! otherwise show a clean line through a range it never ran.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::{Coverage, Identity, MetricSummary};

/// One point of a sweep: the value the knob was turned to, and what the design did there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepPoint {
    /// What the varied parameter was set to.
    #[serde(with = "sting_karman_types::u128_str")]
    pub value: u128,
    /// What was asked for at this point against what happened.
    pub coverage: Coverage,
    /// The distributions this point produced.
    pub metrics: Vec<MetricSummary>,
}

/// A sweep across one parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepReport {
    /// Everything needed to run the sweep again, with the run and tick counts of one point.
    pub identity: Identity,
    /// The parameter that was varied, by the name it is asked for by.
    pub knob: String,
    /// The points, in the order they were asked for.
    pub points: Vec<SweepPoint>,
    /// What this sweep cannot answer, filled by the composition root.
    pub gaps: Vec<String>,
}

impl SweepReport {
    /// Renders the sweep as Markdown: one row per point, one table per metric.
    ///
    /// A metric per table rather than a metric per column, because a reader compares a corridor
    /// across points and never a median against a percentile of something else.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        let _ = write!(
            md,
            "# Parameter sweep — {}\n\n",
            self.identity.scenario_name
        );
        let _ = write!(
            md,
            "> Varying `{}`. Every point is its own sweep of {} runs at seed `{}`, and carries \
             its own coverage: a point that aborted is not a point that behaved.\n\n- reproduce: \
             `{}`\n\n",
            self.knob, self.identity.n_runs, self.identity.seed, self.identity.reproduce
        );

        md.push_str("## Coverage by point\n\n");
        let _ = writeln!(
            md,
            "| {} | planned | completed | aborted | first abort |",
            self.knob
        );
        md.push_str("|---|---|---|---|---|\n");
        for point in &self.points {
            let _ = writeln!(
                md,
                "| {} | {} | {} | {} | {} |",
                point.value,
                point.coverage.runs_planned,
                point.coverage.runs_completed,
                point.coverage.runs_aborted,
                point.coverage.first_abort.as_deref().unwrap_or("—")
            );
        }
        md.push('\n');

        for metric in self.metric_names() {
            let _ = write!(md, "## {metric}\n\n");
            let _ = writeln!(
                md,
                "| {} | median | drift | p05 | p95 | corridor |\n|---|---|---|---|---|---|",
                self.knob
            );
            for point in &self.points {
                let Some(summary) = point.metrics.iter().find(|m| m.name == metric) else {
                    let _ = writeln!(md, "| {} | — | — | — | — | — |", point.value);
                    continue;
                };
                let _ = writeln!(
                    md,
                    "| {} | {} | {} | {} | {} | {} |",
                    point.value,
                    summary.median,
                    render_bps(summary.drift_bps()),
                    summary.p05,
                    summary.p95,
                    render_bps(summary.corridor_bps())
                );
            }
            md.push('\n');
        }

        md.push_str("## Gaps — what this sweep cannot answer\n\n");
        if self.gaps.is_empty() {
            md.push_str(
                "None declared. A sweep with no declared gaps claims completeness — treat that \
                 claim with suspicion.\n",
            );
        } else {
            for gap in &self.gaps {
                let _ = writeln!(md, "- {gap}");
            }
        }
        md.push('\n');
        md
    }

    /// Every metric any point measured, in the order the first point that measured it did.
    ///
    /// Taken across points rather than from the first, so a metric that only appears once still
    /// gets a table and a reader sees the gap where the others should have been.
    fn metric_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for point in &self.points {
            for metric in &point.metrics {
                if !names.contains(&metric.name) {
                    names.push(metric.name.clone());
                }
            }
        }
        names
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a basis-point figure printed to two decimals is a percentage, not a quantity"
)]
fn render_bps(value: Option<i128>) -> String {
    value.map_or_else(
        || "—".to_string(),
        |bps| format!("{:+.2}%", bps as f64 / 100.0),
    )
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

    fn point(value: u128, metrics: Vec<MetricSummary>) -> SweepPoint {
        SweepPoint {
            value,
            coverage: Coverage {
                runs_planned: 10,
                runs_completed: 10,
                runs_aborted: 0,
                first_abort: None,
            },
            metrics,
        }
    }

    fn metric(name: &str, values: &[i128]) -> MetricSummary {
        MetricSummary::from_values(name, Some(1_000), values).unwrap()
    }

    fn report(points: Vec<SweepPoint>) -> SweepReport {
        SweepReport {
            identity: Identity {
                scenario_name: "smoke".into(),
                seed: 42,
                n_runs: 10,
                n_ticks: 100,
                reproduce: "karman sweep --scenario s.json --vary n-ticks --values 1,2".into(),
            },
            knob: "n-ticks".into(),
            points,
            gaps: Vec::new(),
        }
    }

    #[test]
    fn every_point_appears_in_the_coverage_table() {
        let md = report(vec![
            point(100, vec![metric("token_reserve", &[1_000])]),
            point(200, vec![metric("token_reserve", &[1_100])]),
        ])
        .to_markdown();
        assert!(md.contains("## Coverage by point"));
        assert!(md.contains("| 100 |") && md.contains("| 200 |"));
    }

    #[test]
    fn a_metric_gets_one_table_and_every_point_a_row_in_it() {
        let md = report(vec![
            point(100, vec![metric("token_reserve", &[1_000])]),
            point(200, vec![metric("token_reserve", &[1_100])]),
        ])
        .to_markdown();
        assert_eq!(md.matches("## token_reserve").count(), 1);
        assert!(md.contains("1100"), "the second point's median is missing");
    }

    /// A point that measured nothing shows the gap rather than being dropped, because a row
    /// missing from a table reads as a design that was never tried.
    #[test]
    fn a_point_that_measured_a_metric_nobody_else_did_still_gets_its_row() {
        let md = report(vec![
            point(100, vec![metric("token_reserve", &[1_000])]),
            point(200, vec![metric("something_else", &[7])]),
        ])
        .to_markdown();
        assert!(md.contains("## token_reserve") && md.contains("## something_else"));
        assert!(
            md.contains("| 200 | — | — | — | — | — |"),
            "the missing measurement is hidden"
        );
    }

    #[test]
    fn an_aborted_point_carries_its_own_coverage() {
        let mut aborted = point(300, Vec::new());
        aborted.coverage.runs_completed = 6;
        aborted.coverage.runs_aborted = 4;
        aborted.coverage.first_abort = Some("a reserve was drained to zero".into());
        let md = report(vec![aborted]).to_markdown();
        assert!(md.contains("| 300 | 10 | 6 | 4 | a reserve was drained to zero |"));
    }

    #[test]
    fn a_sweep_with_no_declared_gaps_says_the_claim_it_is_making() {
        assert!(
            report(Vec::new())
                .to_markdown()
                .contains("claims completeness")
        );
    }
}
