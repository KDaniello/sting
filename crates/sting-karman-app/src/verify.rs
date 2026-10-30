//! The `karman verify` command: what the wired mechanism can and cannot claim on the ladder.
//!
//! The ladder has four levels and this reports on all four, including the ones it cannot reach.
//! A verification report that only listed what was measured would read as though the rest had
//! been passed, which is the failure the whole document exists to prevent
//! (karman/docs/VERIFICATION.md).

use std::fmt::Write as _;

use std::path::Path;

use sting_karman_core::{MechError, Observer, RunRng};
use sting_karman_data::write_text_artifact;
use sting_karman_mech_curve::{Cpamm, CpammState, Swap};
use sting_karman_scenario::ScenarioConfig;
use sting_karman_types::{AgentId, Tick};
use sting_karman_verify::facts::{Request, StylizedFacts, measure};

use crate::{AppError, world_builder};

/// Collects the pool's price at the close of every tick.
///
/// The price is a ratio of two integer reserves and becomes a float here, at the edge where
/// analysis begins and nowhere earlier (karman/docs/ARCHITECTURE.md §8).
#[derive(Debug, Default)]
struct PriceTape {
    prices: Vec<f64>,
}

impl Observer<Cpamm> for PriceTape {
    fn on_events(&mut self, _now: Tick, _events: &[Swap]) {}

    fn on_rejected(&mut self, _now: Tick, _actor: AgentId, _reason: &MechError) {}

    #[allow(
        clippy::cast_precision_loss,
        reason = "a pool's reserves at the sizes a scenario opens with are exact in a double, and what this keeps is a ratio"
    )]
    fn on_tick_end(&mut self, _now: Tick, state: &CpammState) {
        let quote = state.quote_reserve.base_units() as f64;
        let token = state.token_reserve.base_units() as f64;
        self.prices.push(quote / token);
    }
}

/// Drives the scenario through every executable level of the ladder and writes the report.
///
/// # Errors
///
/// Fails when the run aborts, when the series is too short to have a signature, or when the
/// report cannot be written.
pub fn verify(
    path: &Path,
    scenario: &ScenarioConfig,
    seed_override: Option<u64>,
    report_dir: &Path,
) -> Result<String, AppError> {
    let seed = seed_override.unwrap_or(scenario.seed);
    let mut world = world_builder(scenario)?();
    let mut tape = PriceTape::default();
    world.run(scenario.n_ticks, &mut RunRng::new(seed, 0), &mut tape)?;

    let request = Request::default();
    let facts = measure(&tape.prices, &request)?;

    let mut md = String::new();
    let _ = write!(md, "# Verification — {}\n\n", scenario.name);
    let _ = write!(
        md,
        "> The ladder has four levels and every one of them is listed, including those this \
         mechanism cannot reach yet. seed `{seed}` · {} ticks · one run.\n- reproduce: `karman \
         verify --scenario {} --seed {seed}`\n\n",
        scenario.n_ticks,
        path.display()
    );

    md.push_str("## Level 0 — differential against a reference\n\n");
    md.push_str(
        "**Not reached.** The constant-product mechanism has no reference implementation \
         registered in this workspace, so nothing was differenced. The harness exists and is \
         covered; what is missing is the other side of the comparison, which is gate 1.3.\n\n",
    );

    md.push_str("## Level 1 — replay of a recorded episode\n\n");
    md.push_str(
        "**Not reached.** No recorded episode is registered for this scenario. The harness \
         exists and is covered; what is missing is a recording, which is gate 1.5.\n\n",
    );

    md.push_str("## Level 2 — the statistical signature of the synthetic market\n\n");
    md.push_str(&facts_table(&facts, &request));
    md.push_str(
        "\n**Measured, not judged.** These numbers are compared against nothing, because no \
         band has been registered for this scenario in `findings/`. A band chosen now would \
         describe this table rather than grade it (karman/docs/REPORTING.md §3).\n\n",
    );

    md.push_str("## Level 3 — predictions registered before reality answers\n\n");
    md.push_str(
        "**Not reached, and not reachable by a program.** This level is a public track record: \
         claims and their verdict zones published before the outcome exists, graded afterwards \
         by what happened. It is a process, and `findings/` is where it lives.\n\n",
    );

    let report_path = report_dir.join(format!("verify-{}-seed{}.md", scenario.name, seed));
    write_text_artifact(&report_path, &md).map_err(AppError::Report)?;

    Ok(serde_json::json!({
        "schema_version": crate::SUMMARY_SCHEMA_VERSION,
        "scenario": scenario.name,
        "seed": seed,
        "ticks": scenario.n_ticks,
        "level_reached": 2,
        "level_2_measured_not_judged": true,
        "returns": facts.returns,
        "excess_kurtosis": facts.excess_kurtosis,
        "report": report_path.display().to_string(),
    })
    .to_string())
}

/// The measured signature, as a table a reader can compare against a published one.
fn facts_table(facts: &StylizedFacts, request: &Request) -> String {
    let mut md = String::new();
    let _ = write!(md, "Measured over {} returns.\n\n", facts.returns);
    let _ = writeln!(
        md,
        "| fact | measurement |\n|---|---|\n| heavy tails — excess kurtosis of returns | {:.4} |",
        facts.excess_kurtosis
    );
    for (lag, value) in [1usize, 5, 10].into_iter().filter_map(|lag| {
        facts
            .return_autocorrelation
            .get(lag - 1)
            .map(|value| (lag, value))
    }) {
        let _ = writeln!(
            md,
            "| absence of autocorrelation — returns at lag {lag} | {} |",
            render(*value)
        );
    }
    for (lag, value) in [1usize, 5, 10].into_iter().filter_map(|lag| {
        facts
            .absolute_return_autocorrelation
            .get(lag - 1)
            .map(|value| (lag, value))
    }) {
        let _ = writeln!(
            md,
            "| volatility clustering — absolute returns at lag {lag} | {} |",
            render(*value)
        );
    }
    for (horizon, value) in &facts.excess_kurtosis_by_horizon {
        let _ = writeln!(
            md,
            "| aggregational gaussianity — excess kurtosis at horizon {horizon} | {} |",
            render(*value)
        );
    }
    let _ = writeln!(
        md,
        "\nAutocorrelations reach lag {}; horizons are {:?}.",
        request.max_lag, request.horizons
    );
    md
}

/// A statistic the series could not support is rendered as absent, never as a number standing
/// in for one.
fn render(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_string(), |value| format!("{value:.4}"))
}
