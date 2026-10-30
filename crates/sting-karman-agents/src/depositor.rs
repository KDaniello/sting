use serde::{Deserialize, Serialize};
use sting_karman_core::{Agent, PathSource, RunRng};
use sting_karman_mech_lending::{
    BorrowRate, Collateral, Given, Lending, LendingAction, LendingState, Loan, Price,
};
use sting_karman_types::{AgentId, Amount, MarketId, Rounding, Tick};

use crate::{BPS, share};

/// The supply side: puts assets in while the market is paying for them and takes them out while
/// it is not.
///
/// The baseline of this family, and it says so. What it claims is only the direction — deposits
/// arrive where utilisation is high, because that is where the rate is, and leave where it is
/// low. It claims nothing about the shape of that flow, which is a question about depositors
/// rather than about the mechanism, and answering it would need the calibration this behaviour
/// has not had.
///
/// Utilisation is read from the market rather than from a rate, because the rate is behind a port
/// this side cannot reach — and because it is the same thing seen a step earlier. What a
/// depositor watches is whether the market is short of liquidity, and utilisation is that.
///
/// **Calibration.** `target_utilisation_bps` is the point a market's supply stops growing, which
/// is observable per market from the utilisation it settles at. `step` is a deposit's size, and
/// it is a fit — the size distribution of real deposits is what it should be fitted against.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Depositor {
    /// The market it lends into.
    pub market: MarketId,
    /// What it moves in one go, in the loan asset's base units.
    pub step: Amount<Loan>,
    /// The utilisation it is content with, in basis points. Above it the market is paying enough
    /// to attract more; below it, not enough to keep what is there.
    pub target_utilisation_bps: u32,
}

impl Depositor {
    /// A depositor moving `step` at a time, content at `target_utilisation_bps`.
    #[must_use]
    pub fn new(market: MarketId, step: Amount<Loan>, target_utilisation_bps: u32) -> Self {
        Depositor {
            market,
            step,
            target_utilisation_bps,
        }
    }
}

impl<O, R> Agent<Lending<O, R>> for Depositor
where
    O: PathSource<Price<Collateral, Loan>>,
    R: BorrowRate,
{
    fn decide(
        &mut self,
        me: AgentId,
        state: &LendingState,
        _now: Tick,
        _rng: &mut RunRng,
    ) -> Option<LendingAction> {
        let market = state.market(self.market)?;
        if market.total_supply_assets.is_zero() {
            return Some(LendingAction::Supply {
                market: self.market,
                given: Given::Assets(self.step),
            });
        }
        // Utilisation as a cross product, so the comparison needs no scale and no rounding: what
        // is borrowed exceeds the target share of what was supplied exactly when the two
        // products cross.
        let borrowed = market
            .total_borrow_assets
            .wide_mul(Amount::<Loan>::new(BPS));
        let target = market
            .total_supply_assets
            .wide_mul(Amount::<Loan>::new(share(self.target_utilisation_bps)));

        if borrowed > target {
            return Some(LendingAction::Supply {
                market: self.market,
                given: Given::Assets(self.step),
            });
        }
        let held = state.position(self.market, me);
        let leaving = held
            .supply_shares
            .mul_div(self.step, market.total_supply_assets, Rounding::Down)
            .ok()?;
        (!leaving.is_zero()).then_some(LendingAction::Withdraw {
            market: self.market,
            given: Given::Shares(leaving),
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
    use crate::testing::{MARKET, ME, ask, market};

    fn depositor() -> Depositor {
        Depositor::new(MARKET, Amount::new(100), 8_000)
    }

    /// A market short of liquidity is one paying for it, and that is where a deposit goes.
    #[test]
    fn it_lends_into_a_market_that_is_paying_for_it() {
        assert!(matches!(
            ask(&mut depositor(), &market(1_000, 900), Tick(0)),
            Some(LendingAction::Supply { given: Given::Assets(assets), .. })
                if assets == Amount::new(100)
        ));
    }

    /// And leaves one that is not, taking out what its step is worth in shares rather than a
    /// share count it invented.
    #[test]
    fn it_leaves_a_market_that_is_not() {
        let state = market(1_000, 100)
            .with_position(
                MARKET,
                ME,
                Position {
                    supply_shares: Amount::new(1_000_000_000),
                    ..Position::default()
                },
            )
            .unwrap();
        assert!(matches!(
            ask(&mut depositor(), &state, Tick(0)),
            Some(LendingAction::Withdraw { given: Given::Shares(shares), .. })
                if shares == Amount::new(100_000_000)
        ));
    }

    /// With nothing in it, a market is one nobody has lent to yet rather than one nobody wants.
    #[test]
    fn an_empty_market_is_lent_to() {
        assert!(matches!(
            ask(&mut depositor(), &market(0, 0), Tick(0)),
            Some(LendingAction::Supply { .. })
        ));
    }

    /// Holding nothing, there is nothing to take out.
    #[test]
    fn a_depositor_with_nothing_in_takes_nothing_out() {
        assert!(ask(&mut depositor(), &market(1_000, 100), Tick(0)).is_none());
    }
}
