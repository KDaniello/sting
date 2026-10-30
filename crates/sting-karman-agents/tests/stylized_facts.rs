//! The level-2 harness against a real mechanism and a real population, rather than a series
//! written to make it pass.
//!
//! What this proves is that the harness measures: it takes a price series a run actually
//! produced, through the port a run reports by, and returns a signature. What it does not prove
//! is realism. A band belongs to a preregistration and a preregistration belongs to a market
//! being modelled, and neither exists until a mechanism climbs the ladder against one.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use sting_karman_agents::{Arbitrageur, MomentumTrader, NoiseTrader};
use sting_karman_core::{Agent, MechError, NullObserver, Observer, RunRng, World};
use sting_karman_mech_curve::{Cpamm, CpammState, Swap};
use sting_karman_types::{AgentId, Amount, Tick};
use sting_karman_verify::{
    Verdict,
    facts::{Band, NotMeasurable, Request, check, measure},
};

/// Collects what the pool was worth at the close of every tick.
///
/// The price is a ratio of two integer reserves and is turned into a float here, at the edge
/// where analysis begins, and nowhere earlier.
#[derive(Default)]
struct PriceTape {
    prices: Vec<f64>,
}

impl Observer<Cpamm> for PriceTape {
    fn on_events(&mut self, _now: Tick, _events: &[Swap]) {}

    fn on_rejected(&mut self, _now: Tick, _actor: AgentId, _reason: &MechError) {}

    #[allow(
        clippy::cast_precision_loss,
        reason = "a pool's reserves at the sizes this test opens with are exact in a double, and what it measures is a ratio"
    )]
    fn on_tick_end(&mut self, _now: Tick, state: &CpammState) {
        let quote = state.quote_reserve.base_units() as f64;
        let token = state.token_reserve.base_units() as f64;
        self.prices.push(quote / token);
    }
}

fn population() -> Vec<Box<dyn Agent<Cpamm>>> {
    let mut agents: Vec<Box<dyn Agent<Cpamm>>> = Vec::new();
    for _ in 0..5 {
        agents.push(Box::new(NoiseTrader {
            trade_cap: 1_000_000,
        }));
    }
    for _ in 0..2 {
        agents.push(Box::new(Arbitrageur {
            reference_quote: Amount::new(1_000_000),
            reference_token: Amount::new(1_000_000),
            volatility_bps: 30,
            entry_band_bps: 30,
            aggression_bps: 50,
        }));
    }
    for _ in 0..3 {
        agents.push(Box::new(MomentumTrader::new(20, 50, 20)));
    }
    agents
}

fn world() -> World<Cpamm> {
    let state = CpammState::new(Amount::new(1_000_000_000), Amount::new(1_000_000_000))
        .expect("a live pool");
    World::new(Cpamm, state, population())
}

fn tape(seed: u64, ticks: u64) -> Vec<f64> {
    let mut world = world();
    let mut observer = PriceTape::default();
    world
        .run(ticks, &mut RunRng::new(seed, 0), &mut observer)
        .expect("the run completes");
    observer.prices
}

#[test]
fn the_harness_measures_a_series_a_real_run_produced() {
    let facts = measure(&tape(42, 2_000), &Request::default()).expect("2000 ticks is enough");
    assert_eq!(facts.returns, 1_999);
    assert!(
        facts.excess_kurtosis.is_finite(),
        "the shape of the distribution is not a number"
    );
    assert!(
        facts.return_autocorrelation.iter().all(Option::is_some),
        "a lag inside a two-thousand-tick series went unmeasured"
    );
    assert!(
        facts
            .absolute_return_autocorrelation
            .iter()
            .all(Option::is_some)
    );
    assert_eq!(
        facts.excess_kurtosis_by_horizon.len(),
        Request::default().horizons.len()
    );
}

/// A verdict that changed between two runs of one seed would be a verdict about the machine.
#[test]
fn the_same_seed_measures_to_the_same_signature() {
    let request = Request::default();
    assert_eq!(
        measure(&tape(42, 2_000), &request),
        measure(&tape(42, 2_000), &request)
    );
}

#[test]
fn a_different_seed_is_a_different_market() {
    let request = Request::default();
    assert_ne!(
        measure(&tape(42, 2_000), &request),
        measure(&tape(43, 2_000), &request)
    );
}

/// The harness refuses a run too short to say anything, rather than reporting a signature
/// measured over a handful of observations.
#[test]
fn a_run_too_short_to_have_a_signature_is_refused() {
    let error = measure(&tape(42, 8), &Request::default()).unwrap_err();
    assert!(matches!(error, NotMeasurable::TooShort { given: 8, .. }));
}

/// The judgement half, driven by a measurement this population actually produced: a band that
/// contains it confirms, and the same band moved off it diverges with both numbers quoted.
#[test]
fn a_band_judges_the_measurement_it_was_registered_against() {
    let facts = measure(&tape(42, 2_000), &Request::default()).unwrap();
    let lag_one = facts.return_autocorrelation[0].expect("lag one is measurable here");

    let wide = check(
        "absence of autocorrelation",
        lag_one,
        Band {
            low: -1.0,
            high: 1.0,
        },
    );
    assert_eq!(wide.verdict, Verdict::Confirmed);

    let elsewhere = check(
        "absence of autocorrelation",
        lag_one,
        Band {
            low: 5.0,
            high: 6.0,
        },
    );
    assert!(matches!(elsewhere.verdict, Verdict::Diverged { .. }));
}

/// A population that never trades leaves a price that never moves, and a signature taken from it
/// would be a signature of nothing.
#[test]
fn a_market_that_never_traded_has_no_signature() {
    let mut world = World::new(
        Cpamm,
        CpammState::new(Amount::new(1_000_000_000), Amount::new(1_000_000_000)).unwrap(),
        Vec::new(),
    );
    let mut observer = PriceTape::default();
    world
        .run(2_000, &mut RunRng::new(42, 0), &mut observer)
        .unwrap();
    assert_eq!(
        measure(&observer.prices, &Request::default()),
        Err(NotMeasurable::Motionless)
    );
}

/// The tape and the engine must agree about how long a run was, or every statistic below is
/// measured over a series that is not the one the run produced.
#[test]
fn the_tape_carries_one_price_per_tick() {
    let mut world = world();
    let mut observer = PriceTape::default();
    world
        .run(500, &mut RunRng::new(1, 0), &mut NullObserver)
        .unwrap();
    let mut second = self::world();
    second
        .run(500, &mut RunRng::new(1, 0), &mut observer)
        .unwrap();
    assert_eq!(observer.prices.len(), 500);
    assert_eq!(
        second.state(),
        world.state(),
        "the observer changed the run it observed"
    );
}
