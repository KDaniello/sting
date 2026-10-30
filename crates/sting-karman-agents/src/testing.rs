//! What the lending behaviours are exercised against: one market, built by hand.
//!
//! A behaviour is a function of what it can see, so what it is shown is the whole of the test.
//!
//! Test scaffolding compiled into the library because the behaviours' own tests are in other
//! modules of it. `karman/docs/ARCHITECTURE.md` §3.2 scopes the no-unwrap policy to library code, and a
//! builder that cannot build the world it exists to build must fail where it is called.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test scaffolding, exported to this crate's own tests"
)]

use sting_karman_core::{Agent, PathSource, RunRng, SourceError};
use sting_karman_mech_lending::{
    Collateral, FixedRate, Lending, LendingAction, LendingState, Loan, Market, ORACLE_PRICE_SCALE,
    Price,
};
use sting_karman_types::{AgentId, Amount, MarketId, Tick};

pub(crate) const MARKET: MarketId = MarketId(0);
pub(crate) const ME: AgentId = AgentId(3);

/// A price that never moves. These behaviours never read one, which is the point of them, but
/// the mechanism they act on is typed by the one it reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Steady;

impl PathSource<Price<Collateral, Loan>> for Steady {
    fn value_at(&self, _now: Tick) -> Result<Price<Collateral, Loan>, SourceError> {
        Ok(Price::new(ORACLE_PRICE_SCALE))
    }
}

/// The mechanism these behaviours are typed against.
pub(crate) type Priced = Lending<Steady, FixedRate>;

/// What one behaviour decides, shown one state.
pub(crate) fn ask<A: Agent<Priced>>(
    agent: &mut A,
    state: &LendingState,
    now: Tick,
) -> Option<LendingAction> {
    agent.decide(ME, state, now, &mut RunRng::new(42, 0))
}

/// One market holding `supplied` with `borrowed` of it out.
pub(crate) fn market(supplied: u128, borrowed: u128) -> LendingState {
    let opened = Market {
        total_supply_assets: Amount::new(supplied),
        total_supply_shares: Amount::new(supplied * 1_000_000),
        total_borrow_assets: Amount::new(borrowed),
        total_borrow_shares: Amount::new(borrowed * 1_000_000),
        last_update: Tick(0),
        fee: Amount::ZERO,
    };
    LendingState::new().with_market(MARKET, opened).unwrap()
}
