use serde::{Deserialize, Serialize};
use sting_karman_types::{AgentId, Amount, MarketId, Unit};

use crate::{BorrowShare, Collateral, Loan, SupplyShare};

/// Which side of a conversion an action states.
///
/// The reference takes an asset amount and a share amount together and requires exactly one of
/// them to be zero; here the choice is the type, so the state that requirement guards against
/// does not exist. The duality itself is not an accident of that interface — a market's holders
/// think in assets and its accounting thinks in shares, and a transaction has to be replayable
/// as whichever one it was submitted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""), rename_all = "snake_case")]
pub enum Given<A: Unit, S: Unit> {
    /// Stated as an amount of the asset.
    Assets(Amount<A>),
    /// Stated as a number of shares.
    Shares(Amount<S>),
}

impl<A: Unit, S: Unit> Given<A, S> {
    /// Whether the side this states amounts to anything.
    pub(crate) fn is_zero(&self) -> bool {
        match self {
            Given::Assets(assets) => assets.is_zero(),
            Given::Shares(shares) => shares.is_zero(),
        }
    }
}

/// What an agent can do to a lending market.
///
/// Every action names the market it acts on, as the reference's do: one deployment hosts many
/// markets, and an action without that name is not the action a recorded transaction was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LendingAction {
    /// Put loan assets into the market to earn interest on them.
    Supply {
        /// Where.
        market: MarketId,
        /// How much, stated either way.
        given: Given<Loan, SupplyShare>,
    },
    /// Take supplied assets back out, as far as the market's idle liquidity allows.
    Withdraw {
        /// Where.
        market: MarketId,
        /// How much, stated either way.
        given: Given<Loan, SupplyShare>,
    },
    /// Take on debt against collateral already pledged.
    Borrow {
        /// Where.
        market: MarketId,
        /// How much, stated either way.
        given: Given<Loan, BorrowShare>,
    },
    /// Pay debt down.
    Repay {
        /// Where.
        market: MarketId,
        /// How much, stated either way.
        given: Given<Loan, BorrowShare>,
    },
    /// Pledge collateral. Earns nothing and is never lent out.
    SupplyCollateral {
        /// Where.
        market: MarketId,
        /// How much.
        assets: Amount<Collateral>,
    },
    /// Take collateral back, as far as the position stays healthy without it.
    WithdrawCollateral {
        /// Where.
        market: MarketId,
        /// How much.
        assets: Amount<Collateral>,
    },
    /// Clear part of an unhealthy position's debt and take its collateral at a discount.
    ///
    /// How much may be taken at once is bounded by the position and by nothing else: the
    /// reference has no close factor, so a liquidator able to pay for the whole of a position
    /// may have the whole of it.
    Liquidate {
        /// Where.
        market: MarketId,
        /// Whose position.
        borrower: AgentId,
        /// How much, stated as the collateral to seize or as the debt shares to clear.
        given: Given<Collateral, BorrowShare>,
    },
}

impl LendingAction {
    /// The market this action acts on.
    #[must_use]
    pub fn market(&self) -> MarketId {
        match *self {
            LendingAction::Supply { market, .. }
            | LendingAction::Withdraw { market, .. }
            | LendingAction::Borrow { market, .. }
            | LendingAction::Repay { market, .. }
            | LendingAction::SupplyCollateral { market, .. }
            | LendingAction::WithdrawCollateral { market, .. }
            | LendingAction::Liquidate { market, .. } => market,
        }
    }

    /// Whose position this action rewrites.
    ///
    /// The actor's, except for a liquidation, which rewrites the borrower's. The liquidator's
    /// own books are not this mechanism's to keep — no agent holds a balance here — so a
    /// liquidation touches one position, not two.
    pub(crate) fn subject(&self, actor: AgentId) -> AgentId {
        match *self {
            LendingAction::Liquidate { borrower, .. } => borrower,
            _ => actor,
        }
    }

    /// Whether this action has to see the market's interest brought up to date first.
    ///
    /// Only pledging collateral does not: interest never touches a collateral balance, so
    /// accruing before one would cost a rate the transition has no use for.
    pub(crate) fn accrues_first(&self) -> bool {
        !matches!(self, LendingAction::SupplyCollateral { .. })
    }
}

/// What a lending market reports happened.
///
/// Both sides of every conversion are carried. A divergence between two implementations of this
/// arithmetic is one unit wide and shows up on the pair, where a total that has already absorbed
/// it no longer distinguishes it from the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LendingEvent {
    /// Assets entered the supply side.
    Supplied {
        /// Where.
        market: MarketId,
        /// Who.
        holder: AgentId,
        /// What went in.
        assets: Amount<Loan>,
        /// What was credited for it.
        shares: Amount<SupplyShare>,
    },
    /// Assets left it.
    Withdrawn {
        /// Where.
        market: MarketId,
        /// Who.
        holder: AgentId,
        /// What came out.
        assets: Amount<Loan>,
        /// What was charged for it.
        shares: Amount<SupplyShare>,
    },
    /// Debt was taken on.
    Borrowed {
        /// Where.
        market: MarketId,
        /// Who.
        holder: AgentId,
        /// What was drawn.
        assets: Amount<Loan>,
        /// What was owed for it.
        shares: Amount<BorrowShare>,
    },
    /// Debt was paid down.
    Repaid {
        /// Where.
        market: MarketId,
        /// Who.
        holder: AgentId,
        /// What was paid.
        assets: Amount<Loan>,
        /// What it cleared.
        shares: Amount<BorrowShare>,
    },
    /// Collateral was pledged.
    CollateralAdded {
        /// Where.
        market: MarketId,
        /// Who.
        holder: AgentId,
        /// How much.
        assets: Amount<Collateral>,
    },
    /// Collateral was taken back.
    CollateralRemoved {
        /// Where.
        market: MarketId,
        /// Who.
        holder: AgentId,
        /// How much.
        assets: Amount<Collateral>,
    },
    /// An unhealthy position was taken over, in part or in whole.
    Liquidated {
        /// Where.
        market: MarketId,
        /// Who acted.
        liquidator: AgentId,
        /// Whose position.
        borrower: AgentId,
        /// What the liquidator paid.
        repaid: Amount<Loan>,
        /// What it cleared.
        repaid_shares: Amount<BorrowShare>,
        /// What they took for it.
        seized: Amount<Collateral>,
        /// What was written off because no collateral was left to cover it.
        bad_debt: Amount<Loan>,
        /// The shares that debt was owed by.
        bad_debt_shares: Amount<BorrowShare>,
    },
    /// A market was brought up to date.
    InterestAccrued {
        /// Where.
        market: MarketId,
        /// What the debt grew by, which is what the supply side gained.
        interest: Amount<Loan>,
        /// What was minted for the fee recipient out of it.
        fee_shares: Amount<SupplyShare>,
    },
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

    /// A recording has to carry an action back in exactly as it was submitted, including which
    /// side of the conversion it named.
    #[test]
    fn an_action_survives_the_serialisation_boundary() {
        let actions = [
            LendingAction::Supply {
                market: MarketId(1),
                given: Given::Assets(Amount::new(u128::MAX)),
            },
            LendingAction::Borrow {
                market: MarketId(0),
                given: Given::Shares(Amount::new(7)),
            },
            LendingAction::SupplyCollateral {
                market: MarketId(2),
                assets: Amount::new(3),
            },
        ];
        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            assert_eq!(
                serde_json::from_str::<LendingAction>(&json).unwrap(),
                action
            );
        }
    }

    #[test]
    fn an_event_survives_the_serialisation_boundary() {
        let event = LendingEvent::Repaid {
            market: MarketId(1),
            holder: AgentId(4),
            assets: Amount::new(100),
            shares: Amount::new(99),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<LendingEvent>(&json).unwrap(), event);
    }

    /// A liquidation rewrites the borrower's position and not the liquidator's, because no
    /// agent holds a balance in this mechanism for a liquidator to be paid into.
    #[test]
    fn only_a_liquidation_acts_on_someone_else() {
        let (market, borrower) = (MarketId(0), AgentId(3));
        let liquidate = LendingAction::Liquidate {
            market,
            borrower,
            given: Given::Assets(Amount::new(1)),
        };
        assert_eq!(liquidate.subject(AgentId(1)), borrower);
        let supply = LendingAction::Supply {
            market,
            given: Given::Assets(Amount::new(1)),
        };
        assert_eq!(supply.subject(AgentId(1)), AgentId(1));
    }

    /// Pledging collateral is the one transition a rate has nothing to say about.
    #[test]
    fn only_a_collateral_deposit_skips_the_rate() {
        let market = MarketId(0);
        assert!(
            !LendingAction::SupplyCollateral {
                market,
                assets: Amount::new(1)
            }
            .accrues_first()
        );
        assert!(
            LendingAction::WithdrawCollateral {
                market,
                assets: Amount::new(1)
            }
            .accrues_first()
        );
        assert!(
            LendingAction::Supply {
                market,
                given: Given::Assets(Amount::new(1))
            }
            .accrues_first()
        );
    }
}
