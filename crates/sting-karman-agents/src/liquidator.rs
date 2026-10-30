use serde::{Deserialize, Serialize};
use sting_karman_core::{Agent, PathSource, RunRng};
use sting_karman_mech_lending::{
    BorrowRate, Collateral, Given, Lending, LendingAction, LendingState, Loan, Price,
};
use sting_karman_types::{AgentId, Amount, MarketId, Rounding, Tick};

use crate::BPS;

/// Whoever takes an uncovered position over, and the two things that decide whether they get
/// there in time.
///
/// A liquidator is not a rule the market enforces. It is somebody who has to notice, decide it is
/// worth the gas, and land a transaction — and a market's losses are almost entirely a question
/// of whether that happened fast enough and with enough capital behind it. Those are the two
/// parameters here, and they are the two axes a search over this mechanism has to run along.
///
/// It judges nothing. A position that still covers its debt refuses it, which is what a real bot
/// discovers by being refused, and it is why a run reaches a write-off because the arithmetic
/// allowed one rather than because the population was told when to act. Which borrower it reaches
/// first comes off the run stream, because in a market with several of these the order is not
/// something anybody can predict.
///
/// **Calibration.** `every` is a reaction time, measurable directly: the gap between the block a
/// position crossed its threshold in and the block it was liquidated in, across a market's
/// history. `capacity_bps` is how much of a position one liquidator can pay for at once,
/// measurable from the size distribution of real liquidations against the positions they hit.
/// Neither is a fit, which matters — a finding about how fast liquidators must be is worth
/// nothing if how fast they are was a knob.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Liquidator {
    /// The market it watches.
    pub market: MarketId,
    /// How many ticks pass between one attempt and the next. One means every tick.
    pub every: u64,
    /// The most of a position it takes in one go, as a share of that position's collateral, in
    /// basis points.
    ///
    /// A whole is a whole here, and that is not a detail. A loss is realised when the last unit
    /// of collateral goes, so a capacity that could only ever reach all-but-one would leave every
    /// position insolvent and none of them written off — a market carrying debt against dust
    /// forever, which is not what a liquidator with the capital to clear a position does.
    pub capacity_bps: u32,
    /// How many holders it will consider, which is the population it is watching.
    pub watching: u32,
}

impl Liquidator {
    /// A liquidator that acts every `every` ticks and can pay for `capacity_bps` of a position.
    #[must_use]
    pub fn new(market: MarketId, every: u64, capacity_bps: u32, watching: u32) -> Self {
        Liquidator {
            market,
            every: every.max(1),
            capacity_bps,
            watching,
        }
    }
}

impl<O, R> Agent<Lending<O, R>> for Liquidator
where
    O: PathSource<Price<Collateral, Loan>>,
    R: BorrowRate,
{
    fn decide(
        &mut self,
        _me: AgentId,
        state: &LendingState,
        now: Tick,
        rng: &mut RunRng,
    ) -> Option<LendingAction> {
        if !now.0.is_multiple_of(self.every) || self.watching == 0 {
            return None;
        }
        let borrower = AgentId(u32::try_from(rng.below(u64::from(self.watching))).ok()?);
        let held = state.position(self.market, borrower);
        let taken = held
            .collateral
            .mul_div(
                Amount::<Collateral>::new(u128::from(self.capacity_bps).min(BPS)),
                Amount::<Collateral>::new(BPS),
                Rounding::Down,
            )
            .ok()?;
        (!taken.is_zero()).then_some(LendingAction::Liquidate {
            market: self.market,
            borrower,
            given: Given::Assets(taken),
        })
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
    use sting_karman_mech_lending::Position;

    use super::*;
    use crate::testing::{MARKET, ask, market};

    fn watched() -> LendingState {
        market(10_000, 5_000)
            .with_position(
                MARKET,
                AgentId(0),
                Position {
                    collateral: Amount::new(1_000),
                    ..Position::default()
                },
            )
            .unwrap()
    }

    /// A reaction time is the point of this behaviour, so a tick it is not watching produces
    /// nothing at all.
    #[test]
    fn it_is_silent_between_its_attempts() {
        let mut agent = Liquidator::new(MARKET, 10, 5_000, 1);
        let state = watched();
        for tick in 1..10 {
            assert!(ask(&mut agent, &state, Tick(tick)).is_none());
        }
        assert!(ask(&mut agent, &state, Tick(10)).is_some());
    }

    /// It takes what it can pay for and no more, which is the second of the two parameters a
    /// market's losses turn on.
    #[test]
    fn it_takes_what_its_capacity_allows() {
        let mut agent = Liquidator::new(MARKET, 1, 2_500, 1);
        assert!(matches!(
            ask(&mut agent, &watched(), Tick(0)),
            Some(LendingAction::Liquidate { given: Given::Assets(assets), .. })
                if assets == Amount::new(250)
        ));
    }

    /// A whole capacity takes the whole position, down to the last unit. Anything less leaves a
    /// position insolvent and never written off, which is a different market entirely.
    #[test]
    fn a_whole_capacity_takes_the_last_unit() {
        let mut agent = Liquidator::new(MARKET, 1, 10_000, 1);
        assert!(matches!(
            ask(&mut agent, &watched(), Tick(0)),
            Some(LendingAction::Liquidate { given: Given::Assets(assets), .. })
                if assets == Amount::new(1_000)
        ));
    }

    /// A holder with nothing pledged is nothing to take, and the attempt is not made.
    #[test]
    fn a_position_with_no_collateral_is_not_attempted() {
        let mut agent = Liquidator::new(MARKET, 1, 5_000, 1);
        assert!(ask(&mut agent, &market(10_000, 5_000), Tick(0)).is_none());
    }
}
