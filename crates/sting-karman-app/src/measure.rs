//! What a sweep of one scenario measures, in one place.
//!
//! Both `run` and `sweep` need the same numbers out of the same worlds, and a second
//! implementation of "what a sweep measures" would be a second answer waiting to disagree with
//! the first.

use sting_karman_core::{RunStats, monte_carlo, monte_carlo_watched};
use sting_karman_mech_curve::CpammState;
use sting_karman_mech_lending::LendingState;
use sting_karman_report::{Coverage, ExtremeRuns, MetricSummary, Outcome, Segment};
use sting_karman_scenario::{MechanismConfig, ScenarioConfig};

use crate::{AppError, lending, showcase, vouch, world_builder};

/// The sweep cut back into the futures it was divided between.
///
/// Each part is a contiguous stretch of the sweep, in the order the futures were run, so the
/// stretches are found by walking the spans rather than by asking a run which one it belonged to.
/// A sweep that was one future has no parts: the distributions above it are already about it.
fn parts(
    spans: &[(String, u64)],
    landings: &[Option<Landed>],
    named: [&str; 2],
    watching: bool,
) -> Vec<Segment> {
    if spans.len() < 2 {
        return Vec::new();
    }
    let mut cut = Vec::with_capacity(spans.len());
    let mut from = 0usize;
    for (name, runs) in spans {
        let to = (from + usize::try_from(*runs).unwrap_or_default()).min(landings.len());
        let landed: Vec<Landed> = landings[from..to].iter().flatten().copied().collect();
        from = to;

        let mut liquidated = Outcome::named("a position was taken over");
        let mut realised = Outcome::named("a loss was realised against the supply side");
        for (at, one) in landed.iter().enumerate() {
            let run = at as u64;
            liquidated.saw(run, one.liquidated);
            realised.saw(run, one.realised);
        }
        let column = |pick: fn(&Landed) -> i128| -> Vec<i128> { landed.iter().map(pick).collect() };
        cut.push(Segment {
            name: name.clone(),
            runs: u64::try_from(landed.len()).unwrap_or(*runs),
            metrics: [
                MetricSummary::from_values(named[0], None, &column(|one| one.token)),
                MetricSummary::from_values(named[1], None, &column(|one| one.quote)),
                MetricSummary::from_values(
                    "actions_applied_per_run",
                    None,
                    &column(|one| one.applied),
                ),
                MetricSummary::from_values(
                    "actions_rejected_per_run",
                    None,
                    &column(|one| one.rejected),
                ),
            ]
            .into_iter()
            .flatten()
            .collect(),
            outcomes: if watching {
                vec![liquidated, realised]
            } else {
                Vec::new()
            },
        });
    }
    cut
}

/// The outcome of sweeping one scenario: what was measured, and what was lost measuring it.
pub(crate) struct Measured {
    pub(crate) coverage: Coverage,
    pub(crate) metrics: Vec<MetricSummary>,
    pub(crate) extremes: Vec<ExtremeRuns>,
    pub(crate) outcomes: Vec<Outcome>,
    pub(crate) segments: Vec<Segment>,
    pub(crate) token_finals: Vec<i128>,
}

/// What one run left behind, kept per run so a sweep can be read whole and part by part.
///
/// A run that aborted leaves nothing, and the place it would have occupied stays, because the
/// parts of a mixture are contiguous stretches of the sweep and a missing run must not shift the
/// ones after it into the wrong stretch.
#[derive(Clone, Copy)]
struct Landed {
    token: i128,
    quote: i128,
    applied: i128,
    rejected: i128,
    liquidated: bool,
    realised: bool,
}

/// The showcase's own sweep, which is a different fold because it measures different things.
///
/// Six columns and two outcomes rather than two columns and a loss, and every one of them is a
/// number a gate is stated in. Folding it through the shape a pool is read by would have meant
/// either dropping four of them or pretending a pool has them.
fn showcase_sweep(scenario: &ScenarioConfig, runs: u64, seed: u64) -> Result<Measured, AppError> {
    let opened = showcase::opened(scenario)?;
    let items = opened.market.state.items.len();
    let build = showcase::world_builder(scenario)?;
    let market = opened.market.clone();
    let worth = opened.market.worth.clone();
    let results = monte_carlo_watched(
        runs,
        scenario.n_ticks,
        seed,
        build,
        move || showcase::Traded::watching(items),
        move |state, stats, watched| (showcase::landed(state, &worth, &watched, &market), stats),
    );

    let mut aborted = 0u64;
    let mut first_abort = None;
    let mut columns: Vec<Vec<i128>> = vec![Vec::new(); showcase::LANDED.len()];
    let (mut listed, mut applied, mut rejected) = (Vec::new(), Vec::new(), Vec::new());
    let mut moved = Vec::new();
    let mut priced = Outcome::named("the run settled enough sales to have quartiles");
    let mut ordered = Outcome::named("enough was on the showcase to score its order");
    let mut extremes: Option<ExtremeRuns> = None;

    for (at, result) in results.into_iter().enumerate() {
        let run = at as u64;
        match result {
            Ok((one, stats)) => {
                priced.saw(run, one.priced_enough);
                ordered.saw(run, one.ordered_enough);
                for (column, value) in columns.iter_mut().zip([
                    one.turnover_bps,
                    one.priced_bps,
                    one.paid_dispersion,
                    one.asked_over_paid,
                    one.narrowest_bps,
                    one.widest_bps,
                    one.convergence_bps,
                ]) {
                    column.push(value);
                }
                listed.push(i128::from(one.listed));
                moved.push(i128::from(one.moved));
                applied.push(i128::from(stats.actions_applied));
                rejected.push(i128::from(stats.actions_rejected));
                let extreme = extremes.get_or_insert_with(|| ExtremeRuns {
                    metric: "convergence_bps".into(),
                    lowest_run: run,
                    lowest_value: one.convergence_bps,
                    highest_run: run,
                    highest_value: one.convergence_bps,
                });
                if one.convergence_bps < extreme.lowest_value {
                    extreme.lowest_value = one.convergence_bps;
                    extreme.lowest_run = run;
                }
                if one.convergence_bps > extreme.highest_value {
                    extreme.highest_value = one.convergence_bps;
                    extreme.highest_run = run;
                }
            }
            Err(abort) => {
                aborted += 1;
                first_abort.get_or_insert_with(|| abort.to_string());
            }
        }
    }

    let mut metrics: Vec<MetricSummary> = showcase::LANDED
        .iter()
        .zip(&columns)
        .filter_map(|(name, values)| MetricSummary::from_values(*name, None, values))
        .collect();
    metrics.extend(
        [
            MetricSummary::from_values("items_listed", None, &listed),
            MetricSummary::from_values("items_moved", None, &moved),
            MetricSummary::from_values("actions_applied_per_run", None, &applied),
            MetricSummary::from_values("actions_rejected_per_run", None, &rejected),
        ]
        .into_iter()
        .flatten(),
    );

    Ok(Measured {
        coverage: Coverage {
            runs_planned: runs,
            runs_completed: runs - aborted,
            runs_aborted: aborted,
            first_abort,
        },
        metrics,
        extremes: extremes.into_iter().collect(),
        outcomes: vec![priced, ordered],
        segments: Vec::new(),
        token_finals: columns.into_iter().next().unwrap_or_default(),
    })
}

/// The guarantee layer's own sweep, folded the way a claim about a ring is stated.
///
/// Seven columns and one outcome. Six of the seven are readings of the final state alone, because
/// a guarantee layer keeps its history: what was drawn, what was written off and what everybody is
/// left holding are all still there when the last tick lands. The seventh — whether the ring ever
/// drew anything — is not a number but a condition, and it is reported apart because a ring that
/// never got started produces a flattering net result and says nothing at all about whether
/// attacking pays.
fn vouch_sweep(scenario: &ScenarioConfig, runs: u64, seed: u64) -> Result<Measured, AppError> {
    let opened = vouch::opened(scenario)?;
    let build = vouch::world_builder(scenario)?;
    let results = monte_carlo(runs, scenario.n_ticks, seed, build, |state, stats| {
        (vouch::landed(state, &opened), stats)
    });

    let mut aborted = 0u64;
    let mut first_abort = None;
    let mut columns: Vec<Vec<i128>> = vec![Vec::new(); vouch::LANDED.len()];
    let (mut applied, mut rejected) = (Vec::new(), Vec::new());
    let mut acted = Outcome::named("the ring drew credit at all");
    let mut extremes: Option<ExtremeRuns> = None;

    for (at, result) in results.into_iter().enumerate() {
        let run = at as u64;
        match result {
            Ok((one, stats)) => {
                acted.saw(run, one.ring_acted);
                for (column, value) in columns.iter_mut().zip([
                    one.ring_net_bps,
                    one.honest_net_bps,
                    one.drawn_bps,
                    one.written_off_bps,
                    one.venue_take_bps,
                    one.leverage_bps,
                    one.ring_settled,
                ]) {
                    column.push(value);
                }
                applied.push(i128::from(stats.actions_applied));
                rejected.push(i128::from(stats.actions_rejected));
                let extreme = extremes.get_or_insert_with(|| ExtremeRuns {
                    metric: "ring_net_bps".into(),
                    lowest_run: run,
                    lowest_value: one.ring_net_bps,
                    highest_run: run,
                    highest_value: one.ring_net_bps,
                });
                if one.ring_net_bps < extreme.lowest_value {
                    extreme.lowest_value = one.ring_net_bps;
                    extreme.lowest_run = run;
                }
                if one.ring_net_bps > extreme.highest_value {
                    extreme.highest_value = one.ring_net_bps;
                    extreme.highest_run = run;
                }
            }
            Err(abort) => {
                aborted += 1;
                first_abort.get_or_insert_with(|| abort.to_string());
            }
        }
    }

    let mut metrics: Vec<MetricSummary> = vouch::LANDED
        .iter()
        .zip(&columns)
        .filter_map(|(name, values)| MetricSummary::from_values(*name, None, values))
        .collect();
    metrics.extend(
        [
            MetricSummary::from_values("actions_applied_per_run", None, &applied),
            MetricSummary::from_values("actions_rejected_per_run", None, &rejected),
        ]
        .into_iter()
        .flatten(),
    );

    Ok(Measured {
        coverage: Coverage {
            runs_planned: runs,
            runs_completed: runs - aborted,
            runs_aborted: aborted,
            first_abort,
        },
        metrics,
        extremes: extremes.into_iter().collect(),
        outcomes: vec![acted],
        segments: Vec::new(),
        token_finals: columns.into_iter().next().unwrap_or_default(),
    })
}

/// Runs `runs` independent worlds of `scenario` at `seed` and reduces them to a report's numbers.
///
/// # Errors
///
/// Fails when a measurement is too wide for the signed range a report carries.
#[allow(
    clippy::too_many_lines,
    reason = "one branch per mechanism family, each measuring what that family reports: the length is the number of families"
)]
pub(crate) fn measure(
    scenario: &ScenarioConfig,
    runs: u64,
    seed: u64,
) -> Result<Measured, AppError> {
    // The one place a family is named. What follows is the same fold whatever answered it: a
    // sweep measures two numbers, counts what it did and says what it lost, and none of that is a
    // property of which mechanism produced them.
    if matches!(scenario.mechanism, MechanismConfig::Showcase { .. }) {
        return showcase_sweep(scenario, runs, seed);
    }
    if matches!(scenario.mechanism, MechanismConfig::Vouch { .. }) {
        return vouch_sweep(scenario, runs, seed);
    }
    let mut spans: Vec<(String, u64)> = Vec::new();
    let (named, opening, results) = match scenario.mechanism {
        MechanismConfig::Cpamm {
            token_reserve,
            quote_reserve,
        } => (
            ["token_reserve", "quote_reserve"],
            [token_reserve, quote_reserve],
            monte_carlo(
                runs,
                scenario.n_ticks,
                seed,
                world_builder(scenario)?,
                curve,
            ),
        ),
        MechanismConfig::Showcase { .. } => unreachable!("a showcase is folded above"),
        MechanismConfig::Vouch { .. } => unreachable!("a guarantee layer is folded above"),
        MechanismConfig::Lending { .. } => {
            let opened = lending::opening_market(scenario)?;
            // One sweep per future, concatenated in the order the scenario states them. Every
            // future draws the same streams, so two of them differ by their shock and by nothing
            // else: the comparison is paired rather than two independent samples of noise.
            let mut gathered = Vec::with_capacity(usize::try_from(runs).unwrap_or_default());
            for future in lending::futures(scenario, runs) {
                let build = lending::world_builder(scenario, future.shock)?;
                spans.push((future.name, future.runs));
                gathered.extend(monte_carlo_watched(
                    future.runs,
                    scenario.n_ticks,
                    seed,
                    build,
                    lending::Losses::default,
                    credit,
                ));
            }
            (
                ["supply_assets", "written_off"],
                [opened.total_supply_assets.base_units(), 0],
                gathered,
            )
        }
    };
    let (token_reserve, quote_reserve) = (opening[0], opening[1]);

    let mut aborted = 0u64;
    let mut first_abort = None;
    let mut token_finals = Vec::with_capacity(results.len());
    let mut quote_finals = Vec::with_capacity(results.len());
    let mut applied = Vec::with_capacity(results.len());
    let mut rejected = Vec::with_capacity(results.len());
    let mut token_extremes: Option<ExtremeRuns> = None;
    let mut landings: Vec<Option<Landed>> = Vec::with_capacity(results.len());
    let watching = matches!(scenario.mechanism, MechanismConfig::Lending { .. });
    let mut liquidated = Outcome::named("a position was taken over");
    let mut realised = Outcome::named("a loss was realised against the supply side");

    for (run_index, result) in results.into_iter().enumerate() {
        match result {
            Ok((token, quote, stats, watched)) => {
                let run = run_index as u64;
                liquidated.saw(run, watched.liquidations > 0);
                realised.saw(run, watched.written_off > 0);
                let token = reportable(named[0], token)?;
                let quote = reportable(named[1], quote)?;
                landings.push(Some(Landed {
                    token,
                    quote,
                    applied: i128::from(stats.actions_applied),
                    rejected: i128::from(stats.actions_rejected),
                    liquidated: watched.liquidations > 0,
                    realised: watched.written_off > 0,
                }));
                token_finals.push(token);
                quote_finals.push(quote);
                applied.push(i128::from(stats.actions_applied));
                rejected.push(i128::from(stats.actions_rejected));
                let extremes = token_extremes.get_or_insert_with(|| ExtremeRuns {
                    metric: named[0].into(),
                    lowest_run: run_index as u64,
                    lowest_value: token,
                    highest_run: run_index as u64,
                    highest_value: token,
                });
                if token < extremes.lowest_value {
                    extremes.lowest_value = token;
                    extremes.lowest_run = run_index as u64;
                }
                if token > extremes.highest_value {
                    extremes.highest_value = token;
                    extremes.highest_run = run_index as u64;
                }
            }
            Err(abort) => {
                aborted += 1;
                landings.push(None);
                first_abort.get_or_insert_with(|| abort.to_string());
            }
        }
    }

    Ok(Measured {
        coverage: Coverage {
            runs_planned: runs,
            runs_completed: runs - aborted,
            runs_aborted: aborted,
            first_abort,
        },
        metrics: [
            MetricSummary::from_values(
                named[0],
                Some(reportable(named[0], token_reserve)?),
                &token_finals,
            ),
            MetricSummary::from_values(
                named[1],
                Some(reportable(named[1], quote_reserve)?),
                &quote_finals,
            ),
            MetricSummary::from_values("actions_applied_per_run", None, &applied),
            MetricSummary::from_values("actions_rejected_per_run", None, &rejected),
        ]
        .into_iter()
        .flatten()
        .collect(),
        extremes: token_extremes.into_iter().collect(),
        outcomes: if watching {
            vec![liquidated, realised]
        } else {
            Vec::new()
        },
        segments: parts(&spans, &landings, named, watching),
        token_finals,
    })
}

fn curve(state: &CpammState, counted: RunStats) -> (u128, u128, RunStats, lending::Losses) {
    (
        state.token_reserve.base_units(),
        state.quote_reserve.base_units(),
        counted,
        lending::Losses::default(),
    )
}

/// The two numbers a credit market's sweep is read by: what the supply side is left holding, and
/// what was taken out of it by a debt nobody was left to owe.
fn credit(
    state: &LendingState,
    counted: RunStats,
    watched: lending::Losses,
) -> (u128, u128, RunStats, lending::Losses) {
    let supplied = state
        .market(lending::PULLED)
        .map_or(0, |market| market.total_supply_assets.base_units());
    (supplied, watched.written_off, counted, watched)
}

/// Reports carry signed values, because a metric may be a change. A quantity wider than the
/// signed range has no honest place in one, and inventing a clamped number for it would put a
/// wrong figure where a reader expects a measured one.
fn reportable(metric: &'static str, value: u128) -> Result<i128, AppError> {
    i128::try_from(value).map_err(|_| AppError::Unreportable { metric })
}
