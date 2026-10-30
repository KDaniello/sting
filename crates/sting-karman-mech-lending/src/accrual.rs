//! Interest, and the share of it a market takes for itself.

use sting_karman_core::{Abort, MechError, SourceError};
use sting_karman_types::{Amount, ArithError, Rounding, Tick};
use thiserror::Error;

use crate::{
    Loan, ONE_SECOND, Seconds, SupplyShare, WAD, WAD_TIMES_THREE, WAD_TIMES_TWO, Wad,
    market::Market,
    shares::{Flow, to_shares},
};

/// Why a market could not be brought up to date.
///
/// Two kinds, because the same failure means different things to the two callers: an action that
/// cannot be priced is refused and the run goes on, while a scheduled accrual that cannot be
/// priced leaves the run nowhere to go.
#[derive(Debug, Error)]
pub(crate) enum AccrualError {
    /// The rate model could not price this market.
    #[error(transparent)]
    Rate(#[from] SourceError),
    /// The interest has no representable answer.
    #[error(transparent)]
    Arithmetic(#[from] ArithError),
}

impl From<AccrualError> for MechError {
    fn from(error: AccrualError) -> Self {
        match error {
            // The reason already says what was asked for and why it could not be given, which is
            // exactly what a refusal has to carry.
            AccrualError::Rate(source) => MechError::new(source.reason),
            AccrualError::Arithmetic(arith) => arith.into(),
        }
    }
}

impl From<AccrualError> for Abort {
    fn from(error: AccrualError) -> Self {
        match error {
            AccrualError::Rate(source) => Abort::Source(source),
            AccrualError::Arithmetic(arith) => Abort::Invariant(arith.into()),
        }
    }
}

/// One market brought up to date, and what that cost.
///
/// Nothing here has been written: the market is a copy carrying the new totals, so the caller
/// commits accrual and whatever else it resolved in one step or commits neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Accrual {
    /// The market with both totals moved and `last_update` brought forward.
    pub(crate) market: Market,
    /// What the debt grew by, which is also what the supply side gained.
    pub(crate) interest: Amount<Loan>,
    /// The shares minted out of that interest for the fee recipient.
    pub(crate) fee_shares: Amount<SupplyShare>,
}

/// Accrues `elapsed` seconds of interest at `rate` onto `market`.
///
/// Interest is added to both totals, which is what leaves the difference between them — the
/// liquidity nobody has borrowed — where it was. The fee is minted as supply shares rather than
/// paid out in assets, so the market's books need no third balance to hold it.
///
/// A pure function of numbers: the rate arrives already fetched, because the two callers of this
/// diagnose a source that cannot answer differently and neither wants that decision made here.
pub(crate) fn accrue(
    market: &Market,
    rate: Amount<Wad>,
    elapsed: Amount<Seconds>,
    now: Tick,
) -> Result<Accrual, ArithError> {
    let interest =
        market
            .total_borrow_assets
            .mul_div(compounded(rate, elapsed)?, WAD, Rounding::Down)?;

    let mut market = *market;
    market.total_borrow_assets = market.total_borrow_assets.checked_add(interest)?;
    market.total_supply_assets = market.total_supply_assets.checked_add(interest)?;
    market.last_update = now;

    let mut fee_shares = Amount::ZERO;
    if !market.fee.is_zero() {
        let taken = interest.mul_div(market.fee, WAD, Rounding::Down)?;
        // The fee is priced against the supply side as it stands without the fee itself.
        // Including it would let the recipient's shares be valued against the interest it is in
        // the middle of being paid.
        fee_shares = to_shares(
            taken,
            market.total_supply_assets.checked_sub(taken)?,
            market.total_supply_shares,
            Flow::In,
        )?;
        market.total_supply_shares = market.total_supply_shares.checked_add(fee_shares)?;
    }

    Ok(Accrual {
        market,
        interest,
        fee_shares,
    })
}

/// The growth factor of a per-second rate compounded over `elapsed`, at [`WAD`].
///
/// The first three terms of the exponential series, as the reference computes it. The series is
/// truncated rather than approximated to nearest, and every term rounds down, so the factor is
/// an underestimate of continuous compounding: the borrower is undercharged by fractions of a
/// basis point rather than overcharged by them.
fn compounded(rate: Amount<Wad>, elapsed: Amount<Seconds>) -> Result<Amount<Wad>, ArithError> {
    // The rate is quoted per second, so the exponent is the rate times the elapsed time. Written
    // as a ratio over one second because that is the checked widening multiply this workspace
    // has, and because the seconds then cancel where they should.
    let first = rate.mul_div(elapsed, ONE_SECOND, Rounding::Down)?;
    let second = first.mul_div(first, WAD_TIMES_TWO, Rounding::Down)?;
    let third = second.mul_div(first, WAD_TIMES_THREE, Rounding::Down)?;
    first.checked_add(second)?.checked_add(third)
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

    /// Ten percent a year, per second, at WAD. A rate a real market could quote, so the terms of
    /// the series land where they would in a run rather than at an extreme.
    const TEN_PERCENT_A_YEAR: Amount<Wad> = Amount::new(3_170_979_198);
    const YEAR: Amount<Seconds> = Amount::new(31_536_000);

    fn market(borrowed: u128, supplied: u128, fee: u128) -> Market {
        Market {
            total_supply_assets: Amount::new(supplied),
            total_supply_shares: Amount::new(supplied * 1_000_000),
            total_borrow_assets: Amount::new(borrowed),
            total_borrow_shares: Amount::new(borrowed * 1_000_000),
            last_update: Tick(0),
            fee: Amount::new(fee),
        }
    }

    #[test]
    fn a_year_at_ten_percent_compounds_to_about_ten_and_a_half() {
        let factor = compounded(TEN_PERCENT_A_YEAR, YEAR).unwrap();
        // Continuous compounding would grow by 0.1051709…; three terms of the series reach
        // 0.1051666…, short of it and never past it.
        assert_eq!(factor, Amount::new(105_166_666_653_548_106));
    }

    /// The truncation has a direction, and it is the one that undercharges: a series of positive
    /// terms cut after the third cannot exceed the sum of all of them. Checked against the
    /// continuous growth the series approaches, over spans from a second to a decade.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        reason = "the comparison is about direction against a continuous curve, not about the last unit"
    )]
    fn the_series_never_overstates_the_growth() {
        let per_second = TEN_PERCENT_A_YEAR.base_units() as f64 / 1e18;
        for seconds in [1, 3_600, 86_400, 31_536_000, 315_360_000] {
            let factor = compounded(TEN_PERCENT_A_YEAR, Amount::new(seconds)).unwrap();
            let continuous = (per_second * seconds as f64).exp_m1() * 1e18;
            assert!(
                factor.base_units() as f64 <= continuous,
                "the series overstated the growth at {seconds} seconds"
            );
        }
    }

    #[test]
    fn interest_lands_on_both_sides_of_the_market() {
        let before = market(500, 1_000, 0);
        let after = accrue(&before, TEN_PERCENT_A_YEAR, YEAR, Tick(7)).unwrap();
        assert_eq!(after.interest, Amount::new(52));
        assert_eq!(after.market.total_borrow_assets, Amount::new(552));
        assert_eq!(after.market.total_supply_assets, Amount::new(1_052));
        assert_eq!(after.market.last_update, Tick(7));
        assert!(after.fee_shares.is_zero());
    }

    /// The liquidity nobody has borrowed is untouched by time: interest moves both totals by the
    /// same amount, so the gap between them is what it was.
    #[test]
    fn idle_liquidity_is_the_same_after_a_year_as_before() {
        let before = market(500, 1_000, 0);
        let after = accrue(&before, TEN_PERCENT_A_YEAR, YEAR, Tick(1))
            .unwrap()
            .market;
        let idle =
            |m: Market| m.total_supply_assets.base_units() - m.total_borrow_assets.base_units();
        assert_eq!(idle(after), idle(before));
    }

    /// A market with nothing borrowed has nothing to charge, whatever the rate and however long
    /// it has been.
    #[test]
    fn an_unborrowed_market_accrues_nothing() {
        let before = market(0, 1_000, 100_000_000_000_000_000);
        let after = accrue(&before, TEN_PERCENT_A_YEAR, YEAR, Tick(9)).unwrap();
        assert!(after.interest.is_zero());
        assert!(after.fee_shares.is_zero());
        assert_eq!(
            after.market,
            Market {
                last_update: Tick(9),
                ..before
            }
        );
    }

    /// The fee is a share of the interest and never of the principal, and the shares minted for
    /// it are worth what was taken.
    #[test]
    fn the_fee_is_minted_out_of_the_interest_alone() {
        let tenth = 100_000_000_000_000_000;
        let before = market(500_000_000, 1_000_000_000, tenth);
        let after = accrue(&before, TEN_PERCENT_A_YEAR, YEAR, Tick(1)).unwrap();

        let taken = after.interest.base_units() * tenth / WAD.base_units();
        let supply = after.market.total_supply_assets.base_units();
        let shares = after.market.total_supply_shares.base_units();
        // What the minted shares are worth back in assets is what the market took, up to the
        // unit the conversion rounds away from the recipient.
        let worth = after.fee_shares.base_units() * (supply + 1) / (shares + 1_000_000);
        assert!(
            taken - worth <= 1,
            "the fee recipient was minted the wrong claim"
        );
        assert!(!after.fee_shares.is_zero());
    }

    /// Growth compounds, so a rate applied over a span is more than the same rate applied over
    /// half of it twice — but never less, which is the property the series must not break.
    #[test]
    fn compounding_over_a_span_is_never_less_than_over_its_halves() {
        let whole = compounded(TEN_PERCENT_A_YEAR, YEAR).unwrap();
        let half = compounded(TEN_PERCENT_A_YEAR, Amount::new(YEAR.base_units() / 2)).unwrap();
        assert!(whole > half);
    }

    /// An accrual with no answer inside the width is reported rather than wrapped: the market's
    /// totals would otherwise come back smaller than they went in.
    #[test]
    fn interest_that_does_not_fit_is_refused() {
        let before = Market {
            total_borrow_assets: Amount::new(u128::MAX / 2),
            total_supply_assets: Amount::new(u128::MAX / 2),
            ..market(0, 0, 0)
        };
        let result = accrue(
            &before,
            Amount::new(u128::MAX),
            Amount::new(u128::MAX),
            Tick(1),
        );
        assert_eq!(result, Err(ArithError::Overflow));
    }
}
