use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use sting_karman_types::{Amount, ArithError, Rounding, Unit, Wide};

use crate::ORACLE_PRICE_SCALE;

/// The price of one unit of `From`, quoted in `To`.
///
/// Scaled by [`ORACLE_PRICE_SCALE`], with both assets' decimals already folded in by the oracle.
/// The raw value read against that scale is therefore a ratio of base units directly: `raw` of
/// `To` for every `ORACLE_PRICE_SCALE` of `From`, which is what makes valuing an amount an
/// ordinary conversion rather than an operation of its own.
///
/// Both units are in the type because a price replaces a unit rather than cancelling one. A
/// scaling by a ratio leaves a quantity in the unit it started in; turning collateral into loan
/// does not, and a price written the other way round — or taken from another market's pair —
/// then fails to compile instead of silently valuing the wrong thing.
///
/// It is held wider than money because the reference holds it wider: `IOracle.price()` returns
/// `uint256`, and a market of cbBTC against USDC quotes `6.48e38` — nearly twice what the width
/// a quantity is carried in can hold. Narrowing here would not model that market badly, it would
/// refuse to load it at all.
pub struct Price<From: Unit, To: Unit> {
    raw: Wide,
    from: PhantomData<From>,
    to: PhantomData<To>,
}

impl<From: Unit, To: Unit> Price<From, To> {
    /// A price as an oracle reports it: `To` base units per `From` base unit, times
    /// [`ORACLE_PRICE_SCALE`].
    #[must_use]
    pub const fn new(scaled: u128) -> Self {
        Price {
            raw: Wide::from_u128(scaled),
            from: PhantomData,
            to: PhantomData,
        }
    }

    /// The same, for the prices that do not fit the width a quantity is carried in.
    #[must_use]
    pub const fn wider(scaled: Wide) -> Self {
        Price {
            raw: scaled,
            from: PhantomData,
            to: PhantomData,
        }
    }

    /// The scaled value, as an oracle reported it.
    #[must_use]
    pub const fn scaled(self) -> Wide {
        self.raw
    }

    /// What `amount` of `From` is worth in `To`.
    ///
    /// The direction is the caller's to choose and never has a default: valuing collateral to
    /// decide whether a position stands rounds down, and the same conversion done to charge
    /// someone would round up.
    ///
    /// # Errors
    ///
    /// Fails when the valuation has no answer inside the width money is carried in.
    pub fn quote(self, amount: Amount<From>, rounding: Rounding) -> Result<Amount<To>, ArithError> {
        self.raw
            .multiplying(amount.base_units(), ORACLE_PRICE_SCALE, rounding)
            .map(Amount::new)
    }

    /// How much `From` a quantity of `To` buys at this price.
    ///
    /// The inverse of [`Price::quote`], and the direction a trade settled in `To` needs: a
    /// repayment buys collateral rather than valuing it.
    ///
    /// # Errors
    ///
    /// Fails when the price is zero, which buys nothing at any quantity, or when the answer
    /// leaves the width money is carried in.
    pub fn buys(self, amount: Amount<To>, rounding: Rounding) -> Result<Amount<From>, ArithError> {
        self.raw
            .dividing(amount.base_units(), ORACLE_PRICE_SCALE, rounding)
            .map(Amount::new)
    }
}

// Written out for the reason `Amount`'s are: the derives would demand these traits of the unit
// tags, which are zero-sized markers carrying none of them. A price compares as its number does.
impl<From: Unit, To: Unit> Clone for Price<From, To> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<From: Unit, To: Unit> Copy for Price<From, To> {}

impl<From: Unit, To: Unit> PartialEq for Price<From, To> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<From: Unit, To: Unit> Eq for Price<From, To> {}

impl<From: Unit, To: Unit> Hash for Price<From, To> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<From: Unit, To: Unit> fmt::Debug for Price<From, To> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} per {}", self.raw, To::SYMBOL, From::SYMBOL)
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
    use super::*;
    use crate::{Collateral, Loan};

    /// One collateral unit is worth two loan units.
    const DOUBLE: Price<Collateral, Loan> = Price::new(2 * ORACLE_PRICE_SCALE);

    #[test]
    fn a_price_values_an_amount_in_the_unit_it_quotes() {
        let worth = DOUBLE
            .quote(Amount::<Collateral>::new(50), Rounding::Down)
            .unwrap();
        assert_eq!(worth, Amount::<Loan>::new(100));
    }

    /// The two directions are the floor and the ceiling of one exact ratio, and which of them a
    /// caller wants is the whole difference between a position that stands and one that does not.
    #[test]
    fn the_direction_decides_the_remainder() {
        let third = Price::<Collateral, Loan>::new(ORACLE_PRICE_SCALE / 3);
        let amount = Amount::<Collateral>::new(10);
        assert_eq!(third.quote(amount, Rounding::Down).unwrap(), Amount::new(3));
        assert_eq!(third.quote(amount, Rounding::Up).unwrap(), Amount::new(4));
    }

    /// A valuation whose answer leaves the width a quantity is carried in is refused rather than
    /// wrapped: a price at full width against an amount at full width has no answer money holds.
    #[test]
    fn a_valuation_with_no_representable_answer_is_refused() {
        let extreme = Price::<Collateral, Loan>::new(u128::MAX);
        let result = extreme.quote(Amount::<Collateral>::new(u128::MAX), Rounding::Down);
        assert_eq!(result, Err(ArithError::Overflow));
    }

    /// The market this workspace pulls quotes a price no quantity could hold, and it values a
    /// position exactly: one whole cbBTC is `1e8` base units, and `64 800` USDC is `6.48e10`.
    #[test]
    fn a_price_wider_than_money_values_a_position() {
        let quoted = Price::<Collateral, Loan>::wider(
            "648000000000000000000000000000000000000".parse().unwrap(),
        );
        let worth = quoted
            .quote(Amount::<Collateral>::new(100_000_000), Rounding::Down)
            .unwrap();
        assert_eq!(worth, Amount::<Loan>::new(64_800_000_000));
        assert_eq!(
            quoted.buys(worth, Rounding::Down).unwrap(),
            Amount::new(100_000_000)
        );
    }

    /// Valuing and buying are the same price read in opposite directions, and neither is the
    /// other's exact undo: each resolves its own remainder, and the caller says which way.
    #[test]
    fn buying_is_valuing_read_backwards() {
        assert_eq!(
            DOUBLE
                .buys(Amount::<Loan>::new(100), Rounding::Down)
                .unwrap(),
            Amount::new(50)
        );
        let third = Price::<Collateral, Loan>::new(ORACLE_PRICE_SCALE / 3);
        assert_eq!(
            third.buys(Amount::<Loan>::new(1), Rounding::Down).unwrap(),
            Amount::new(3)
        );
        assert_eq!(
            third.buys(Amount::<Loan>::new(1), Rounding::Up).unwrap(),
            Amount::new(4)
        );
    }

    /// A price of nothing buys nothing, and says so rather than dividing by it.
    #[test]
    fn a_price_of_zero_buys_nothing() {
        let worthless = Price::<Collateral, Loan>::new(0);
        assert_eq!(
            worthless.quote(Amount::new(10), Rounding::Up).unwrap(),
            Amount::ZERO
        );
        assert_eq!(
            worthless.buys(Amount::<Loan>::new(10), Rounding::Down),
            Err(ArithError::DivideByZero)
        );
    }

    #[test]
    fn a_price_reads_as_the_pair_it_quotes() {
        assert_eq!(
            format!("{:?}", Price::<Collateral, Loan>::new(7)),
            "7 loan per collateral"
        );
    }
}
