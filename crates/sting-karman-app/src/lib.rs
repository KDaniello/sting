//! The composition: where a scenario becomes a world, and the operations every surface offers
//! over it.
//!
//! Concrete wiring lives here and nowhere else. Two surfaces that each resolved a scenario into
//! a mechanism would be two answers waiting to disagree, and the Python bindings promise the
//! same numbers as the terminal precisely because there is one implementation between them.
//!
//! What stays outside is anything a surface owns rather than shares: the terminal parses
//! arguments and the profiler installs a counting allocator, and neither belongs to a library.

mod lending;
mod measure;
mod showcase;
mod verify;
/// The version of the summaries this crate prints.
///
/// The reports are for people and the summaries are for programs: something reads them out of a
/// pipeline, and a program parsing a field that moved needs to learn it from the answer rather
/// than from a changelog.
pub const SUMMARY_SCHEMA_VERSION: &str = "1.0";

pub mod vouch;

use std::path::Path;

pub use lending::{Losses, Opening, PULLED, Shocked, opening};
use rayon::prelude::*;
pub use showcase::{
    Generated, ISSUER, LANDED, Landed, Market, Opened, Traded, VENUE, WorldSpec, from_census,
    generate, landed, opened, participants_for, world_builder as showcase_world_builder,
};
use sting_karman_agents::{Arbitrageur, MomentumTrader, NoiseTrader};
use sting_karman_core::World;
use sting_karman_data::{read_json_fixture, write_json_fixture, write_text_artifact};
use sting_karman_mech_curve::{Cpamm, CpammState, Quote, Token};
use sting_karman_report::{
    AppliesTo, Claim, GradedClaim, Identity, InitialCondition, RunReport, SweepPoint, SweepReport,
    grade,
};
use sting_karman_scenario::{
    AgentGroup, BehaviourConfig, Knob, MechanismConfig, Population, ScenarioConfig,
};
use sting_karman_types::Amount;
use thiserror::Error;
pub use verify::verify;

/// Reads a scenario and refuses it here if it could not be run.
///
/// Every surface goes through this rather than its own loader: a scenario accepted by one and
/// refused by another would be two schemas wearing one name.
///
/// # Errors
///
/// Fails when the file cannot be read, is not a scenario, or describes a run nobody could make.
pub fn read_scenario(path: &Path) -> Result<ScenarioConfig, AppError> {
    let scenario: ScenarioConfig = read_json_fixture(path)?;
    scenario.validate()?;
    Ok(scenario)
}

/// Everything that can stop an operation before it produces a result. Each variant keeps the
/// error it came from as its source, so the cause survives whichever surface reports it.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AppError {
    /// The scenario file could not be reached or was not a scenario.
    #[error("cannot read scenario: {0}")]
    Scenario(#[from] sting_karman_data::FixtureError),
    /// The scenario describes a run nobody could make.
    #[error(transparent)]
    Invalid(#[from] sting_karman_scenario::InvalidScenario),
    /// The work was done and its account could not be written down.
    #[error("cannot write report: {0}")]
    Report(#[source] sting_karman_data::FixtureError),
    /// The registered claim could not be reached or was not a claim.
    #[error("cannot read claim: {0}")]
    Claim(#[source] sting_karman_data::FixtureError),
    /// The claim could not have graded anything as registered.
    #[error(transparent)]
    UnregistrableClaim(#[from] sting_karman_report::InvalidClaim),
    /// A measurement is too wide for the signed range a report carries.
    #[error("metric `{metric}` does not fit the report's signed range")]
    Unreportable {
        /// Which measurement.
        metric: &'static str,
    },
    /// The census could not be reached or was not a census.
    #[error("cannot read census: {0}")]
    Census(#[source] sting_karman_data::FixtureError),
    /// A census could not be turned into a world.
    #[error("cannot generate a world: {0}")]
    Ungenerable(String),
    /// A pulled market could not be read as one this workspace can open.
    #[error("cannot open the pulled market: {0}")]
    Pulled(#[from] sting_karman_data::PullError),
    /// The mechanism refused the state a fixture describes.
    #[error("the pulled market is not one that can be opened: {0}")]
    Unopenable(#[from] sting_karman_core::MechError),
    /// More holders than a run can give identifiers to.
    #[error("a pull named more holders than a population can carry")]
    TooManyHolders,
    /// An operation was asked of a scenario whose mechanism it does not serve.
    #[error("this operation wants {wanted}, and the scenario describes something else")]
    WrongFamily {
        /// What it wanted.
        wanted: &'static str,
    },
    /// A sweep was asked for with no values to sweep across.
    #[error("a sweep needs at least one value to vary the knob to")]
    NothingToSweep,
    /// The run stopped before it could be measured.
    #[error("the run stopped before it could be verified: {0}")]
    Aborted(#[from] sting_karman_core::RunAborted),
    /// A run's memory grew with the length of the run rather than with its state.
    #[error(
        "a run's peak memory grew {ratio:.2} times when the run got ten times longer: something          accumulates with the tick count instead of with the state"
    )]
    AllocationDiscipline {
        /// How much the peak grew for a run ten times as long.
        ratio: f64,
    },
    /// The run produced too short a series to have a signature.
    #[error("the scenario is too short to have a statistical signature: {0}")]
    NoSignature(#[from] sting_karman_verify::facts::NotMeasurable),
}

/// What a report of this composition has to declare it did not wire.
///
/// By mechanism, because a gap is a fact about what was wired and the two families are wired
/// differently — a list written for one of them is false about the other, and a false gap is
/// worse than none: it spends a reader's suspicion in the wrong place. One function all the
/// same, so a run and a sweep of the same scenario cannot give two accounts of the same missing
/// layer.
#[allow(
    clippy::too_many_lines,
    reason = "one paragraph per family, saying what that family's report cannot answer: the length is the number of families times what each leaves out"
)]
fn gaps(mechanism: &MechanismConfig) -> Vec<String> {
    match mechanism {
        MechanismConfig::Showcase { .. } => vec![
            concat!(
                "this mechanism has no deployed reference to be differenced against, ",
                "so level 0 of the ladder is unreachable rather than unattempted, ",
                "and no episode of it has ever been recorded to replay at level 1",
            )
            .into(),
            concat!(
                "the population holds and buys and states a price; ",
                "nobody here evades the charge, corners a neighbour's price ",
                "or lends against what they hold, so nothing measured is a claim ",
                "about how this mechanism stands up to any of that",
            )
            .into(),
            concat!(
                "what a thing is worth is drawn rather than observed: the level comes from ",
                "the census and the shape is one of three fitted numbers, ",
                "so a result about convergence is a result about a world of that shape",
            )
            .into(),
            concat!(
                "a buyout here is one transition, where a real sale through an escrow ",
                "is two messages and a platform handout is a third; ",
                "transfer counts from this run are not comparable with a chain's",
            )
            .into(),
        ],
        MechanismConfig::Vouch { .. } => vec![
            concat!(
                "this mechanism has no deployed reference to be differenced against, ",
                "so level 0 of the ladder is unreachable rather than unattempted, ",
                "and no episode of it has ever been recorded to replay at level 1",
            )
            .into(),
            concat!(
                "no parameter here is fitted to anything, because no market of this shape ",
                "has been measured: every term is one a venue could post, and a result is ",
                "a result about a market run on those terms",
            )
            .into(),
            concat!(
                "a participant that does not repay keeps what it drew: nothing here pursues ",
                "an unspent balance, so what a venue able to claw one back would recover ",
                "is outside what was wired",
            )
            .into(),
            concat!(
                "the graph is fixed at the start and never rewired: nobody withdraws a ",
                "guarantee on suspicion, and nobody declines to stand behind a stranger ",
                "who already looks overextended",
            )
            .into(),
            concat!(
                "one seat is one identity and costs nothing to open beyond the money it ",
                "must stake: what an identity check or a joining fee would do to the ",
                "arithmetic of a ring is not measured here",
            )
            .into(),
        ],
        MechanismConfig::Cpamm { .. } => vec![
            concat!(
                "this pool charges no swap fee: what a fee would do to the flow, ",
                "and to whoever supplies against it, is outside what was wired",
            )
            .into(),
            concat!(
                "this mechanism has climbed neither level 0 nor level 1 of the ladder: ",
                "nothing here was differenced against a reference implementation ",
                "or replayed against a recording",
            )
            .into(),
        ],
        MechanismConfig::Lending { population, .. } => {
            let mut stated = vec![
                concat!(
                    "the rate curve is not modelled: a run is charged the rate its scenario ",
                    "states, so nothing here says what the market's own model would have ",
                    "charged as utilisation moved (karman/docs/ARITHMETIC.md §6)",
                )
                .into(),
                concat!(
                    "the price is an exogenous path: nothing the market does answers it, ",
                    "so nothing here says how a quote would have moved under the ",
                    "liquidations it caused",
                )
                .into(),
            ];
            if *population == Population::Stated {
                stated.push(
                    concat!(
                        "the positions are this scenario's own: the market was opened for ",
                        "its terms and its size, so nothing here says what would have become ",
                        "of the holders the pull found",
                    )
                    .into(),
                );
            }
            stated
        }
    }
}

/// The one place a scenario becomes a world.
///
/// # Errors
/// [`AppError::WrongFamily`] when the scenario describes something other than a pool, and
/// [`AppError::Unopenable`] when the reserves it names cannot open one.
///
/// The state is built once, here, rather than inside the closure: a scenario that would not
/// build a pool is refused where a refusal is still a value, and what the closure carries is a
/// state rather than a promise of one. Every command builds through here, so a profiled world, a
/// measured world and a verified world can never quietly diverge.
pub fn world_builder(
    scenario: &ScenarioConfig,
) -> Result<impl Fn() -> World<Cpamm> + Sync + '_, AppError> {
    let MechanismConfig::Cpamm {
        token_reserve,
        quote_reserve,
    } = scenario.mechanism
    else {
        return Err(AppError::WrongFamily {
            wanted: "a constant-product pool",
        });
    };
    // The scenario describes reserves as plain base units; attaching the unit each one counts is
    // exactly the resolution a composition root exists to perform.
    let token_reserve = Amount::<Token>::new(token_reserve);
    let quote_reserve = Amount::<Quote>::new(quote_reserve);
    let state = CpammState::new(token_reserve, quote_reserve)?;
    Ok(move || World::new(Cpamm, state.clone(), agents(&scenario.agents)))
}

/// Sweeps the scenario, writes the run report, and returns the summary a surface prints.
///
/// # Errors
///
/// Fails when a measurement does not fit a report, when a registered claim cannot be read or
/// could not have graded anything, or when the report cannot be written.
pub fn run(
    path: &Path,
    scenario: &ScenarioConfig,
    runs: u64,
    seed_override: Option<u64>,
    report_dir: &Path,
    claim_path: Option<&Path>,
) -> Result<String, AppError> {
    let seed = seed_override.unwrap_or(scenario.seed);
    let measured = measure::measure(scenario, runs, seed)?;

    let report = RunReport {
        identity: Identity {
            scenario_name: scenario.name.clone(),
            seed,
            n_runs: runs,
            n_ticks: scenario.n_ticks,
            reproduce: format!(
                "karman run --scenario {} --runs {} --seed {}",
                path.display(),
                runs,
                seed
            ),
        },
        initial_conditions: initial_conditions(scenario)?,
        coverage: measured.coverage,
        metrics: measured.metrics,
        extremes: measured.extremes,
        outcomes: measured.outcomes,
        segments: measured.segments,
        claim: None,
        // The composition root declares the limits of what it wired: these gaps are wiring
        // facts, and hiding them would sell the sweep as more than it measured.
        gaps: gaps(&scenario.mechanism),
    };

    // The claim is loaded and graded after the metrics exist and never before: what it grades
    // has to be the sweep that ran, and what it grades by has to be what was registered.
    let mut report = report;
    if let Some(claim_path) = claim_path {
        let claim: Claim = read_json_fixture(claim_path).map_err(AppError::Claim)?;
        claim.validate()?;
        let applies_to = AppliesTo {
            scenario: scenario.name.clone(),
            seed,
            n_runs: runs,
            n_ticks: scenario.n_ticks,
        };
        let graded = grade(&claim, &applies_to, &report.metrics);
        report.claim = Some(GradedClaim { claim, graded });
    }

    let report_path = report_dir.join(format!("report-{}-seed{}.md", scenario.name, seed));
    write_text_artifact(&report_path, &report.to_markdown()).map_err(AppError::Report)?;
    let report_json = report_dir.join(format!("report-{}-seed{}.json", scenario.name, seed));
    write_json_fixture(&report_json, &report).map_err(AppError::Report)?;

    let verdict = report.claim.as_ref().map(|graded| match &graded.graded {
        sting_karman_report::Graded::Confirmed { .. } => "confirmed",
        sting_karman_report::Graded::Refuted { .. } => "refuted",
        sting_karman_report::Graded::NoVerdict { .. } => "no verdict",
    });
    Ok(serde_json::json!({
        "schema_version": SUMMARY_SCHEMA_VERSION,
        "verdict": verdict,
        "scenario": scenario.name,
        "seed": seed,
        "runs": runs,
        "ticks": scenario.n_ticks,
        "aborted_runs": report.coverage.runs_aborted,
        "token_reserve": {
            "min": measured.token_finals.iter().min().map(i128::to_string),
            "max": measured.token_finals.iter().max().map(i128::to_string),
        },
        "report": report_path.display().to_string(),
    })
    .to_string())
}

/// The final token reserve of every completed run, in run order.
///
/// The sample behind the summary. A report publishes percentiles because a million runs is not a
/// number anyone reads, but a comparison against another engine needs the observations
/// themselves — a test for two distributions being the same cannot be run against seven points
/// out of each.
///
/// # Errors
///
/// Fails when a measurement is too wide for the signed range a report carries.
pub fn finals(scenario: &ScenarioConfig, runs: u64, seed: u64) -> Result<Vec<i128>, AppError> {
    Ok(measure::measure(scenario, runs, seed)?.token_finals)
}

/// Where a sweep of this scenario started, as the report states it.
///
/// # Errors
///
/// Fails when a scenario names a market whose fixture cannot be read. A report cannot state
/// conditions it was unable to look up, and stating the scenario's half alone would leave a
/// reader with the choices and none of the facts.
#[allow(
    clippy::too_many_lines,
    reason = "one block per family, stating the conditions that family opened from: the length is the number of families"
)]
pub fn initial_conditions(scenario: &ScenarioConfig) -> Result<Vec<InitialCondition>, AppError> {
    let agent_total: u64 = scenario
        .agents
        .iter()
        .map(|group| u64::from(group.count))
        .sum();
    let mut stated = match &scenario.mechanism {
        MechanismConfig::Cpamm {
            token_reserve,
            quote_reserve,
        } => vec![
            InitialCondition {
                name: "mechanism".into(),
                value: "cpamm".into(),
            },
            InitialCondition {
                name: "token_reserve".into(),
                value: token_reserve.to_string(),
            },
            InitialCondition {
                name: "quote_reserve".into(),
                value: quote_reserve.to_string(),
            },
        ],
        MechanismConfig::Showcase {
            census,
            covering,
            items,
            charge_bps,
            shape,
            spread_bps,
            capital,
            ..
        } => vec![
            InitialCondition {
                name: "mechanism".into(),
                value: "showcase".into(),
            },
            InitialCondition {
                name: "census".into(),
                value: census.clone(),
            },
            InitialCondition {
                name: "covering".into(),
                value: covering.clone().unwrap_or_else(|| "every class".into()),
            },
            InitialCondition {
                name: "items".into(),
                value: items.to_string(),
            },
            InitialCondition {
                name: "charge_bps_per_year".into(),
                value: charge_bps.to_string(),
            },
            InitialCondition {
                name: "worth_shape".into(),
                value: shape.to_string(),
            },
            InitialCondition {
                name: "taste_spread_bps".into(),
                value: spread_bps.to_string(),
            },
            InitialCondition {
                name: "capital_each".into(),
                value: capital.to_string(),
            },
        ],
        MechanismConfig::Vouch {
            honest,
            bad,
            ring,
            capital,
            fund,
            opening_bps,
            step_bps,
            floor_bps,
            premium_bps,
            venue_share_bps,
            backing,
            ..
        } => vec![
            InitialCondition {
                name: "mechanism".into(),
                value: "vouch".into(),
            },
            InitialCondition {
                name: "honest_seats".into(),
                value: honest.to_string(),
            },
            InitialCondition {
                name: "bad_borrowers".into(),
                value: bad.to_string(),
            },
            InitialCondition {
                name: "ring_seats".into(),
                value: ring.to_string(),
            },
            InitialCondition {
                name: "capital_each".into(),
                value: capital.to_string(),
            },
            InitialCondition {
                name: "fund".into(),
                value: fund.to_string(),
            },
            InitialCondition {
                name: "collateral_opening_bps".into(),
                value: opening_bps.to_string(),
            },
            InitialCondition {
                name: "collateral_step_bps".into(),
                value: step_bps.to_string(),
            },
            InitialCondition {
                name: "collateral_floor_bps".into(),
                value: floor_bps.to_string(),
            },
            InitialCondition {
                name: "premium_bps_per_year".into(),
                value: premium_bps.to_string(),
            },
            InitialCondition {
                name: "venue_share_bps".into(),
                value: venue_share_bps.to_string(),
            },
            InitialCondition {
                name: "backing".into(),
                value: match backing {
                    sting_karman_scenario::BackingConfig::Stake => "stake".into(),
                    sting_karman_scenario::BackingConfig::NetWorth => "net_worth".into(),
                },
            },
        ],
        MechanismConfig::Lending {
            market,
            lltv,
            borrow_rate,
            population,
            seconds_per_tick,
        } => {
            let pulled = lending::listed(scenario)?;
            let terms = lending::listing(&pulled)?;
            let mut lending = vec![
                InitialCondition {
                    name: "mechanism".into(),
                    value: "lending".into(),
                },
                InitialCondition {
                    name: "market".into(),
                    value: market.clone(),
                },
                InitialCondition {
                    name: "block".into(),
                    value: pulled.origin.block.to_string(),
                },
                InitialCondition {
                    name: "pair".into(),
                    value: format!(
                        "{} ({}dp) against {} ({}dp)",
                        terms.collateral_token,
                        terms.collateral_decimals,
                        terms.loan_token,
                        terms.loan_decimals,
                    ),
                },
                InitialCondition {
                    name: "price".into(),
                    value: terms.price.to_string(),
                },
                InitialCondition {
                    name: "lltv".into(),
                    value: lltv.unwrap_or(terms.lltv).to_string(),
                },
            ];
            // A counterfactual threshold names the listing beside it. A reader who saw only the
            // number would take a question this sweep asked for a fact about the market.
            if lltv.is_some() {
                lending.push(InitialCondition {
                    name: "lltv_as_listed".into(),
                    value: terms.lltv.to_string(),
                });
            }
            // Whether the positions are the market's own or the study's is the first thing a
            // reader needs in order to know what the numbers below are about.
            lending.push(InitialCondition {
                name: "population".into(),
                value: match population {
                    Population::Pulled => "the market's own holders".into(),
                    Population::Stated => "stated by this scenario, over the market's size".into(),
                },
            });
            lending.push(InitialCondition {
                name: "borrow_rate".into(),
                value: borrow_rate.to_string(),
            });
            lending.push(InitialCondition {
                name: "seconds_per_tick".into(),
                value: seconds_per_tick.to_string(),
            });
            lending
        }
    };
    // The mixture a sweep divides its runs between is an assumption, and a report that carried
    // the outcome without it would be reporting the assumption as the finding.
    for regime in &scenario.regimes {
        stated.push(InitialCondition {
            name: format!("regime.{}", regime.name),
            value: format!("{} bps", regime.weight_bps),
        });
    }
    stated.push(InitialCondition {
        name: "agents".into(),
        value: agent_total.to_string(),
    });
    Ok(stated)
}

/// Turns one knob across the values asked for and puts the designs side by side.
///
/// Every point is a full sweep at the same seed, so two points differ by the knob and by nothing
/// else — a point that drew a different stream would be a different experiment wearing the same
/// axis.
/// Turns one knob across the values asked for and returns the summary a surface prints.
///
/// # Errors
///
/// Fails when no value was given, when a turned knob produces a scenario nobody could run, or
/// when the report cannot be written.
#[allow(
    clippy::too_many_arguments,
    reason = "a sweep is a scenario, a knob, its values, where the report goes and how the run is seeded: a struct here would be built at each call site and read once"
)]
pub fn sweep(
    path: &Path,
    scenario: &ScenarioConfig,
    knob: Knob,
    values: &[u128],
    runs: u64,
    seed_override: Option<u64>,
    report_dir: &Path,
) -> Result<String, AppError> {
    if values.is_empty() {
        return Err(AppError::NothingToSweep);
    }
    let seed = seed_override.unwrap_or(scenario.seed);

    // The designs run beside one another, not one after the other. A single design's Monte Carlo
    // is already parallel, but a run of a large world spends most of its time waiting for memory
    // rather than for arithmetic, and such a sweep stops gaining threads long before a machine
    // runs out of them — measured on this workspace at a little over three times on sixteen
    // logical processors. What fills the rest is work that shares nothing, and two designs of one
    // sweep share nothing at all. The pool is the same one the runs inside them use, so nothing
    // is oversubscribed: the scheduler simply has more independent work to hand out.
    //
    // Order is by index and never by which design finished first, so a sweep run on any number of
    // threads reports the same table in the same order — the same rule the runs inside it follow.
    let points = values
        .par_iter()
        .map(|&value| {
            // Turning the knob produces a scenario that goes through the same validation as one
            // written by hand, so a point nobody could have run is refused before it is measured.
            let turned = knob.turned_to(scenario, value)?;
            let measured = measure::measure(&turned, runs, seed)?;
            Ok(SweepPoint {
                value,
                coverage: measured.coverage,
                metrics: measured.metrics,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    let report = SweepReport {
        identity: Identity {
            scenario_name: scenario.name.clone(),
            seed,
            n_runs: runs,
            n_ticks: scenario.n_ticks,
            reproduce: format!(
                "karman sweep --scenario {} --vary {knob} --values {} --runs {runs} --seed {seed}",
                path.display(),
                values
                    .iter()
                    .map(u128::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        },
        knob: knob.to_string(),
        points,
        gaps: gaps(&scenario.mechanism),
    };

    let stem = format!(
        "sweep-{}-{}-seed{}",
        scenario.name,
        knob.to_string().replace(':', "-"),
        seed
    );
    let report_path = report_dir.join(format!("{stem}.md"));
    write_text_artifact(&report_path, &report.to_markdown()).map_err(AppError::Report)?;
    write_json_fixture(&report_dir.join(format!("{stem}.json")), &report)
        .map_err(AppError::Report)?;

    Ok(serde_json::json!({
        "schema_version": SUMMARY_SCHEMA_VERSION,
        "scenario": scenario.name,
        "seed": seed,
        "runs_per_point": runs,
        "vary": knob.to_string(),
        "points": values.len(),
        "aborted_runs": report.points.iter().map(|p| p.coverage.runs_aborted).sum::<u64>(),
        "report": report_path.display().to_string(),
    })
    .to_string())
}

pub(crate) fn agents(groups: &[AgentGroup]) -> Vec<Box<dyn sting_karman_core::Agent<Cpamm>>> {
    let mut population: Vec<Box<dyn sting_karman_core::Agent<Cpamm>>> = Vec::new();
    for group in groups {
        for _ in 0..group.count {
            match group.behaviour {
                BehaviourConfig::NoiseTrader { trade_cap } => {
                    population.push(Box::new(NoiseTrader { trade_cap }));
                }
                BehaviourConfig::Arbitrageur {
                    reference_quote,
                    reference_token,
                    volatility_bps,
                    entry_band_bps,
                    aggression_bps,
                } => {
                    population.push(Box::new(Arbitrageur {
                        reference_quote: Amount::new(reference_quote),
                        reference_token: Amount::new(reference_token),
                        volatility_bps,
                        entry_band_bps,
                        aggression_bps,
                    }));
                }
                BehaviourConfig::MomentumTrader {
                    lookback_ticks,
                    entry_band_bps,
                    aggression_bps,
                } => {
                    population.push(Box::new(MomentumTrader::new(
                        lookback_ticks,
                        entry_band_bps,
                        aggression_bps,
                    )));
                }
                // A behaviour of another family joins no pool: a showcase's population is built
                // where a showcase is built, and a guarantee layer's where that is. The schema
                // cannot say which family a behaviour belongs to on its own, so a population is
                // what was described minus whatever could not act here.
                BehaviourConfig::Holder { .. }
                | BehaviourConfig::Member
                | BehaviourConfig::Borrower { .. }
                | BehaviourConfig::Liquidator { .. }
                | BehaviourConfig::Depositor { .. } => {}
            }
        }
    }
    population
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

    fn scenario() -> ScenarioConfig {
        ScenarioConfig {
            name: "wiring".into(),
            seed: 1,
            n_ticks: 10,
            mechanism: MechanismConfig::Cpamm {
                token_reserve: 1_000_000,
                quote_reserve: 2_000_000,
            },
            regimes: Vec::new(),
            agents: vec![
                AgentGroup {
                    count: 3,
                    behaviour: BehaviourConfig::NoiseTrader { trade_cap: 10 },
                },
                AgentGroup {
                    count: 2,
                    behaviour: BehaviourConfig::NoiseTrader { trade_cap: 20 },
                },
            ],
        }
    }

    #[test]
    fn every_declared_agent_reaches_the_population() {
        assert_eq!(agents(&scenario().agents).len(), 5);
    }

    #[test]
    fn the_built_world_starts_from_the_scenarios_reserves() {
        let config = scenario();
        let world = world_builder(&config).unwrap()();
        assert_eq!(world.state().token_reserve, Amount::<Token>::new(1_000_000));
        assert_eq!(world.state().quote_reserve, Amount::<Quote>::new(2_000_000));
    }

    /// The composition root is the only place a scenario becomes a world, so two builds of one
    /// scenario must be the same world — otherwise a profiled run and a measured run diverge.
    #[test]
    fn two_builds_of_one_scenario_start_identically() {
        let config = scenario();
        let build = world_builder(&config).unwrap();
        assert_eq!(build().state(), build().state());
    }
}
