//! What an unhealthy position costs to take over, and what a market writes off when a
//! borrower's collateral runs out before their debt does.
//!
//! There is no close factor here, and its absence is the reference's design rather than an
//! omission: a liquidator may take as much of a position as they can pay for, bounded by the
//! collateral it holds and the debt it owes. What limits a liquidation is therefore the position
//! itself, and the parameter that decides whether the trade is worth making is the incentive.

use sting_karman_types::{Amount, ArithError, Rounding};

use crate::{
    BorrowShare, Collateral, Loan, WAD, Wad,
    action::Given,
    market::Market,
    price::Price,
    shares::{Flow, to_assets, to_shares},
};

/// The share of the room below full collateral value that is paid out as incentive.
const LIQUIDATION_CURSOR: Amount<Wad> = Amount::new(300_000_000_000_000_000);

/// The most a liquidator may be paid, whatever the threshold says.
const MAX_INCENTIVE: Amount<Wad> = Amount::new(1_150_000_000_000_000_000);

/// One liquidation resolved: what leaves the borrower's collateral, and what it clears of the
/// debt behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Seizure {
    pub(crate) seized: Amount<Collateral>,
    pub(crate) repaid_shares: Amount<BorrowShare>,
}

/// What a liquidator is paid for taking a position over, as a factor on what they repay.
///
/// `min(1.15, 1 / (1 - cursor · (1 - lltv)))`. A threshold further below the full value of the
/// collateral leaves more of that value above the debt, and a discount paid out of the surplus
/// can be larger without pushing what remains underwater — which is why the factor rises as the
/// threshold falls. The cap is there because the formula keeps rising past the point where the
/// surplus could cover it.
pub(crate) fn incentive(lltv: Amount<Wad>) -> Result<Amount<Wad>, ArithError> {
    let below_full_value = WAD.checked_sub(lltv)?;
    let discount = LIQUIDATION_CURSOR.mul_div(below_full_value, WAD, Rounding::Down)?;
    let factor = WAD.mul_div(WAD, WAD.checked_sub(discount)?, Rounding::Down)?;
    Ok(factor.min(MAX_INCENTIVE))
}

/// Resolves whichever side of the trade the liquidator named into the other.
///
/// Both directions round against the liquidator: naming collateral values it up and charges the
/// debt up, naming debt values it down and pays the collateral down. A liquidator who needs the
/// other side exact has to be the one who names it.
pub(crate) fn seizure(
    given: Given<Collateral, BorrowShare>,
    market: &Market,
    price: Price<Collateral, Loan>,
    incentive: Amount<Wad>,
) -> Result<Seizure, ArithError> {
    match given {
        Given::Assets(seized) => {
            let worth = price.quote(seized, Rounding::Up)?;
            let repaid = worth.mul_div(WAD, incentive, Rounding::Up)?;
            let repaid_shares = to_shares(
                repaid,
                market.total_borrow_assets,
                market.total_borrow_shares,
                Flow::Out,
            )?;
            Ok(Seizure {
                seized,
                repaid_shares,
            })
        }
        Given::Shares(repaid_shares) => {
            let repaid = to_assets(
                repaid_shares,
                market.total_borrow_assets,
                market.total_borrow_shares,
                Flow::Out,
            )?;
            let earned = repaid.mul_div(incentive, WAD, Rounding::Down)?;
            Ok(Seizure {
                seized: price.buys(earned, Rounding::Down)?,
                repaid_shares,
            })
        }
    }
}

/// What a market writes off for a borrower whose collateral is gone.
///
/// Everything still owed, valued as the borrower would have had to repay it, and capped at what
/// the market records as owed in total — the two can differ by a unit, because the valuation
/// rounds up and the total does not.
pub(crate) fn bad_debt(
    market: &Market,
    shares: Amount<BorrowShare>,
) -> Result<Amount<Loan>, ArithError> {
    let owed = to_assets(
        shares,
        market.total_borrow_assets,
        market.total_borrow_shares,
        Flow::In,
    )?;
    Ok(owed.min(market.total_borrow_assets))
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use sting_karman_types::Tick;

    use super::*;
    use crate::ORACLE_PRICE_SCALE;

    const HALF: Amount<Wad> = Amount::new(500_000_000_000_000_000);
    const EIGHTY: Amount<Wad> = Amount::new(800_000_000_000_000_000);
    const NINETY: Amount<Wad> = Amount::new(900_000_000_000_000_000);

    fn market(borrowed: u128) -> Market {
        Market {
            total_supply_assets: Amount::new(borrowed * 2),
            total_supply_shares: Amount::new(borrowed * 2_000_000),
            total_borrow_assets: Amount::new(borrowed),
            total_borrow_shares: Amount::new(borrowed * 1_000_000),
            last_update: Tick(0),
            fee: Amount::ZERO,
        }
    }

    /// One collateral unit is worth one loan unit.
    fn parity() -> Price<Collateral, Loan> {
        Price::new(ORACLE_PRICE_SCALE)
    }

    /// The lower the threshold, the further a price may fall before a debt stops being covered,
    /// and the larger the payment for stepping in — until the cap.
    #[test]
    fn the_incentive_rises_as_the_threshold_falls_and_then_stops() {
        assert_eq!(
            incentive(NINETY).unwrap(),
            Amount::new(1_030_927_835_051_546_391)
        );
        assert_eq!(
            incentive(EIGHTY).unwrap(),
            Amount::new(1_063_829_787_234_042_553)
        );
        assert_eq!(incentive(HALF).unwrap(), MAX_INCENTIVE);
        assert_eq!(incentive(Amount::ZERO).unwrap(), MAX_INCENTIVE);
        assert!(incentive(NINETY).unwrap() < incentive(EIGHTY).unwrap());
    }

    /// A threshold at the collateral's full value leaves nothing to pay an incentive out of, and
    /// the factor is then exactly one rather than undefined.
    #[test]
    fn a_threshold_at_full_value_pays_nothing() {
        assert_eq!(incentive(WAD).unwrap(), WAD);
    }

    /// The liquidator is paid in collateral worth more than the debt they clear. That difference
    /// is the whole reason anyone does this, so it is checked rather than assumed.
    #[test]
    fn the_collateral_taken_is_worth_more_than_the_debt_cleared() {
        let market = market(1_000_000);
        let factor = incentive(EIGHTY).unwrap();
        let taken = seizure(
            Given::Shares(Amount::new(500_000_000_000)),
            &market,
            parity(),
            factor,
        )
        .unwrap();
        let repaid = to_assets(
            taken.repaid_shares,
            market.total_borrow_assets,
            market.total_borrow_shares,
            Flow::In,
        )
        .unwrap();
        let worth = parity().quote(taken.seized, Rounding::Down).unwrap();
        assert!(worth.base_units() > repaid.base_units());
        assert_eq!(taken.seized, Amount::new(531_914));
    }

    /// Naming one side or the other is the same trade approached from opposite ends, and each
    /// end rounds away from the liquidator — so asking for the collateral a repayment earns and
    /// then asking what that collateral costs never comes back cheaper.
    #[test]
    fn the_two_ways_of_naming_a_trade_never_favour_the_liquidator() {
        let market = market(1_000_000);
        let factor = incentive(EIGHTY).unwrap();
        let by_shares = seizure(
            Given::Shares(Amount::new(500_000_000_000)),
            &market,
            parity(),
            factor,
        )
        .unwrap();
        let by_assets =
            seizure(Given::Assets(by_shares.seized), &market, parity(), factor).unwrap();
        assert!(by_assets.repaid_shares >= by_shares.repaid_shares);
    }

    /// A market and a price awkward enough that every step of both directions leaves a
    /// remainder, so that each of the six roundings decides a digit of the answer.
    ///
    /// The expected values are computed by exact integer arithmetic outside this workspace, the
    /// way the numeric contract's own vectors are (karman/docs/ARITHMETIC.md). Reversing any single one
    /// of the six directions changes one of the two, which is what makes them a check on the
    /// directions rather than on the code that happens to implement them.
    mod vectors {
        use super::*;

        const TOTAL_BORROW_ASSETS: u128 = 1_000_003;
        const TOTAL_BORROW_SHARES: u128 = 999_999_999_999;
        const PRICE: u128 = 1_234_567_890_123_456_789_012_345_678_901_234_567;

        fn awkward() -> Market {
            Market {
                total_supply_assets: Amount::new(TOTAL_BORROW_ASSETS * 4),
                total_supply_shares: Amount::new(TOTAL_BORROW_SHARES * 4),
                total_borrow_assets: Amount::new(TOTAL_BORROW_ASSETS),
                total_borrow_shares: Amount::new(TOTAL_BORROW_SHARES),
                last_update: Tick(0),
                fee: Amount::ZERO,
            }
        }

        #[test]
        fn naming_the_collateral_charges_the_shares_the_reference_charges() {
            let taken = seizure(
                Given::Assets(Amount::new(987_654_321)),
                &awkward(),
                Price::new(PRICE),
                incentive(EIGHTY).unwrap(),
            )
            .unwrap();
            assert_eq!(taken.seized, Amount::new(987_654_321));
            assert_eq!(taken.repaid_shares, Amount::new(1_146_163_295_512_406));
        }

        #[test]
        fn naming_the_debt_pays_the_collateral_the_reference_pays() {
            let taken = seizure(
                Given::Shares(Amount::new(123_456_789_012)),
                &awkward(),
                Price::new(PRICE),
                incentive(EIGHTY).unwrap(),
            )
            .unwrap();
            assert_eq!(taken.repaid_shares, Amount::new(123_456_789_012));
            assert_eq!(taken.seized, Amount::new(106_382));
        }
    }

    proptest! {
        /// More collateral never costs fewer shares, and more shares never earn less collateral.
        /// Every step of both directions is monotone, so a trade that grew and got cheaper would
        /// mean one of them is not.
        #[test]
        fn a_larger_trade_is_never_a_cheaper_one(
            borrowed in 1_000u128..1_000_000_000_000u128,
            smaller in 1u128..(1 << 40),
            extra in 1u128..(1 << 40),
        ) {
            let market = market(borrowed);
            let factor = incentive(EIGHTY).unwrap();
            let larger = smaller + extra;

            let charged = |collateral| {
                seizure(Given::Assets(Amount::new(collateral)), &market, parity(), factor)
                    .map(|taken| taken.repaid_shares)
            };
            if let (Ok(less), Ok(more)) = (charged(smaller), charged(larger)) {
                prop_assert!(more >= less, "seizing more collateral cost fewer shares");
            }

            let earned = |shares| {
                seizure(Given::Shares(Amount::new(shares)), &market, parity(), factor)
                    .map(|taken| taken.seized)
            };
            if let (Ok(less), Ok(more)) = (earned(smaller), earned(larger)) {
                prop_assert!(more >= less, "clearing more debt earned less collateral");
            }
        }
    }

    /// The write-off is what the borrower still owes, and it cannot exceed what the market says
    /// is owed in total — the valuation rounds up and the total does not.
    #[test]
    fn a_write_off_never_exceeds_what_the_market_records() {
        let market = market(1_000);
        let all = market.total_borrow_shares;
        assert_eq!(bad_debt(&market, all).unwrap(), market.total_borrow_assets);
        assert_eq!(bad_debt(&market, Amount::ZERO).unwrap(), Amount::ZERO);
    }
}
