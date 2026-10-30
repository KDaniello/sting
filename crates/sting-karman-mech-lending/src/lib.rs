//! Lending-market mechanics: the family where price is exogenous (an oracle) and the mechanism
//! is credit — collateralised positions, interest that compounds on them, and a loan-to-value
//! threshold below which a position may be taken over by whoever pays its debt.
//!
//! The reference semantics are Morpho Blue's, and the arithmetic is bound by level 0 of the
//! verification ladder: bit-exact agreement with the reference Solidity executed on `revm`
//! (karman/docs/VERIFICATION.md). That constraint fixes the numeric representation — the reference
//! computes in unsigned 256-bit integers with explicit rounding directions, so this crate
//! does too.
//!
//! Two scales belong to this family and to nowhere else: [`WAD`] for every rate, threshold and
//! fee, and [`ORACLE_PRICE_SCALE`] for a price. The foundation declares neither, because a scale
//! is one family's convention (karman/docs/ARITHMETIC.md §3).
//!
//! What this family reads but does not generate arrives through two seams of different kinds.
//! The oracle is a [`sting_karman_core::PathSource`], because a price is a function of the clock and of
//! nothing this mechanism does. The rate is [`BorrowRate`], a port of this family, because it
//! reads the market it prices — and that feedback loop is what a credit market exists to
//! exhibit.

mod accrual;
mod action;
mod liquidation;
mod market;
mod mechanism;
mod price;
mod rate;
mod shares;

use sting_karman_types::{Amount, Unit};

pub use crate::{
    action::{Given, LendingAction, LendingEvent},
    market::{LendingState, Market, MarketParams, Position},
    mechanism::Lending,
    price::Price,
    rate::{BorrowRate, Charged, FixedRate, RecordedRate},
};

/// The asset a market lends: supplied by one side, borrowed by the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loan;

impl Unit for Loan {
    const SYMBOL: &'static str = "loan";
}

/// The asset pledged against a debt.
///
/// Never lent out and never part of the supply side: collateral backs one position and is
/// valued by the oracle, which is why it is a unit of its own rather than a second balance of
/// the loan asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collateral;

impl Unit for Collateral {
    const SYMBOL: &'static str = "collateral";
}

/// A claim on the supply side of a market.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupplyShare;

impl Unit for SupplyShare {
    const SYMBOL: &'static str = "supply share";
}

/// A share of a market's debt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorrowShare;

impl Unit for BorrowShare {
    const SYMBOL: &'static str = "borrow share";
}

/// A dimensionless quantity at this family's rate scale.
///
/// A rate, a liquidation threshold and a fee are all ratios, and two of them may be added or
/// compared; what none of them may be is confused with a quantity of an asset, which is what
/// the unit prevents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wad;

impl Unit for Wad {
    const SYMBOL: &'static str = "wad";
}

/// Elapsed wall-clock time, as a borrow rate is quoted against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Seconds;

impl Unit for Seconds {
    const SYMBOL: &'static str = "s";
}

/// The denominator of every rate, threshold and fee this family carries.
pub const WAD: Amount<Wad> = Amount::new(1_000_000_000_000_000_000);

/// The denominator of an oracle price. See [`Price`].
pub const ORACLE_PRICE_SCALE: u128 = 1_000_000_000_000_000_000_000_000_000_000_000_000;

/// The most of its accrued interest a market may divert to its fee recipient.
pub const MAX_FEE: Amount<Wad> = Amount::new(250_000_000_000_000_000);

/// The denominator of a per-second rate, so that the elapsed time cancels.
const ONE_SECOND: Amount<Seconds> = Amount::new(1);

/// Twice [`WAD`], the divisor of the compounding series' second term.
const WAD_TIMES_TWO: Amount<Wad> = Amount::new(2_000_000_000_000_000_000);

/// Three times [`WAD`], the divisor of its third.
const WAD_TIMES_THREE: Amount<Wad> = Amount::new(3_000_000_000_000_000_000);
