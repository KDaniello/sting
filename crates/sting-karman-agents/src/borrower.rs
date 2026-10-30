use serde::{Deserialize, Serialize};
use sting_karman_core::{Agent, PathSource, RunRng};
use sting_karman_mech_lending::{
    BorrowRate, BorrowShare, Collateral, Given, Lending, LendingAction, LendingState, Loan, Price,
};
use sting_karman_types::{AgentId, Amount, MarketId, Rounding, Tick};

use crate::{BPS, share};

/// A leveraged position: pledge collateral, borrow against it, and put it back together after a
/// liquidation takes part of it away.
///
/// Everything this behaviour decides is decided from quantities it can see. A borrower in a
/// lending market cannot read the oracle the market reads, and one that could would be a
/// borrower with the venue's own information; what it knows is what it pledged and what it owes,
/// which is what a position is opened against in the first place. Somebody who puts up one unit
/// of collateral and draws fifteen hundred against it has made a decision about quantities at the
/// price of that moment, and the price moving afterwards is what happens to them rather than what
/// they respond to.
///
/// Which is why this behaviour never asks how healthy it is. It opens once, and afterwards its
/// only response is to what it can observe directly: collateral that is no longer there was taken
/// from it, and it repays a share of the debt that collateral was carrying.
///
/// **Calibration.** The pair of `collateral` and `debt` is the leverage of a real position, and
/// it is measurable one market at a time — the distribution of it across a market's holders is
/// exactly what a pulled fixture carries. `collateral` is the size of a position, measurable from
/// the same place. `repair_bps` is the only fitted parameter, and what it should be fitted against
/// is the share of liquidated borrowers who pay down rather than walk away.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Borrower {
    /// The market it borrows in.
    pub market: MarketId,
    /// What it pledges, once, in the collateral's base units.
    pub collateral: Amount<Collateral>,
    /// What it draws against that collateral, in the loan asset's base units.
    ///
    /// A quantity beside a quantity rather than a ratio between them. The two assets are counted
    /// at scales of their own, so a ratio between them would need a price to mean anything — and
    /// the price is not this behaviour's to see. A position is opened as a pair, which is how
    /// one is opened.
    pub debt: Amount<Loan>,
    /// The share of what it still owes that it pays down after collateral is taken from it, in
    /// basis points.
    pub repair_bps: u32,
    /// What it last saw itself holding, which is how it notices a liquidation without being told.
    pub last_collateral: Amount<Collateral>,
}

impl Borrower {
    /// A position of `collateral` against `debt`, not yet opened.
    #[must_use]
    pub fn new(
        market: MarketId,
        collateral: Amount<Collateral>,
        debt: Amount<Loan>,
        repair_bps: u32,
    ) -> Self {
        Borrower {
            market,
            collateral,
            debt,
            repair_bps,
            last_collateral: Amount::ZERO,
        }
    }
}

impl<O, R> Agent<Lending<O, R>> for Borrower
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
        let held = state.position(self.market, me);

        if held.collateral.is_zero() && self.last_collateral.is_zero() {
            self.last_collateral = self.collateral;
            return Some(LendingAction::SupplyCollateral {
                market: self.market,
                assets: self.collateral,
            });
        }

        if held.borrow_shares.is_zero() && !held.collateral.is_zero() {
            self.last_collateral = held.collateral;
            return Some(LendingAction::Borrow {
                market: self.market,
                given: Given::Assets(self.debt),
            });
        }

        // Collateral that is no longer there was seized. The debt it carried is the part this
        // pays down, scaled by how much of it the borrower is willing to cover at all.
        if held.collateral < self.last_collateral {
            self.last_collateral = held.collateral;
            let repaid = held
                .borrow_shares
                .mul_div(
                    Amount::<BorrowShare>::new(share(self.repair_bps)),
                    Amount::<BorrowShare>::new(BPS),
                    Rounding::Down,
                )
                .ok()?;
            if !repaid.is_zero() {
                return Some(LendingAction::Repay {
                    market: self.market,
                    given: Given::Shares(repaid),
                });
            }
        }
        self.last_collateral = held.collateral;
        None
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

    fn borrower() -> Borrower {
        Borrower::new(MARKET, Amount::new(1_000), Amount::new(700), 5_000)
    }

    /// The order a position is opened in: collateral first, because there is nothing to borrow
    /// against until there is.
    #[test]
    fn a_position_is_opened_collateral_first() {
        let mut agent = borrower();
        assert!(matches!(
            ask(&mut agent, &market(0, 0), Tick(0)),
            Some(LendingAction::SupplyCollateral { assets, .. }) if assets == Amount::new(1_000)
        ));
    }

    /// Collateral first, then the debt it was pledged for: the pair the position was decided as.
    #[test]
    fn it_draws_the_debt_it_pledged_for() {
        let mut agent = borrower();
        let state = market(0, 0)
            .with_position(
                MARKET,
                ME,
                Position {
                    collateral: Amount::new(1_000),
                    ..Position::default()
                },
            )
            .unwrap();
        assert!(matches!(
            ask(&mut agent, &state, Tick(0)),
            Some(LendingAction::Borrow { given: Given::Assets(assets), .. })
                if assets == Amount::new(700)
        ));
    }

    /// Collateral that is no longer there was taken, and that is the only thing this behaviour
    /// responds to. It never asks a price it could not see.
    #[test]
    fn it_pays_down_after_collateral_is_taken_and_not_before() {
        let held = |collateral| Position {
            collateral: Amount::new(collateral),
            borrow_shares: Amount::new(800),
            ..Position::default()
        };
        let mut agent = borrower();

        agent.last_collateral = Amount::new(1_000);
        let standing = market(10_000, 5_000)
            .with_position(MARKET, ME, held(1_000))
            .unwrap();
        assert!(ask(&mut agent, &standing, Tick(1)).is_none());

        agent.last_collateral = Amount::new(1_000);
        let seized = market(10_000, 5_000)
            .with_position(MARKET, ME, held(600))
            .unwrap();
        assert!(matches!(
            ask(&mut agent, &seized, Tick(2)),
            Some(LendingAction::Repay { given: Given::Shares(shares), .. })
                if shares == Amount::new(400)
        ));
    }
}
