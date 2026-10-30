use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    arith::{ArithError, Rounding, Wide, mul_div, wide_mul},
    wire::{i128_str, u128_str},
};

/// What a quantity counts.
///
/// A unit is a compile-time tag that carries no value: it exists so that two quantities which
/// must never be added cannot be. The foundation declares none — a unit belongs to the family
/// that speaks it, which is what lets a new mechanism family arrive without this crate changing.
pub trait Unit: 'static {
    /// How the unit reads beside a quantity.
    const SYMBOL: &'static str;
}

/// A quantity of `U`, in integer base units.
///
/// Unsigned, because a quantity that exists cannot be negative — a change is [`Delta`]. 128 bits,
/// because that is the width the references store (karman/docs/ARITHMETIC.md §2). Every operation is
/// checked, and on the wire the value is a decimal string, since JSON's number type cannot carry
/// it.
///
/// Two units of the same shape are still two units, and the compiler is what says so:
///
/// ```compile_fail
/// use sting_karman_types::{Amount, Unit};
/// # struct Token;
/// # impl Unit for Token {
/// #     const SYMBOL: &'static str = "token";
/// # }
/// # struct Share;
/// # impl Unit for Share {
/// #     const SYMBOL: &'static str = "share";
/// # }
/// let mixed = Amount::<Token>::new(1).checked_add(Amount::<Share>::new(1));
/// ```
pub struct Amount<U: Unit> {
    raw: u128,
    unit: PhantomData<U>,
}

/// A signed quantity of `U`: a change, or a net position that has a direction.
///
/// Separate from [`Amount`] for the reason `Duration` is separate from `Instant` — a span and a
/// point are not the same thing, and the type that can be negative is the span.
pub struct Delta<U: Unit> {
    raw: i128,
    unit: PhantomData<U>,
}

impl<U: Unit> Amount<U> {
    /// None of this unit at all.
    pub const ZERO: Self = Amount {
        raw: 0,
        unit: PhantomData,
    };

    /// A quantity in integer base units — wei, USDC micros, whatever the asset's smallest unit is.
    #[must_use]
    pub const fn new(base_units: u128) -> Self {
        Amount {
            raw: base_units,
            unit: PhantomData,
        }
    }

    /// The quantity as the integer the mechanism arithmetic works in, unit dropped.
    #[must_use]
    pub const fn base_units(self) -> u128 {
        self.raw
    }

    /// Whether there is nothing here — the check a mechanism owes an input before acting on it.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.raw == 0
    }

    /// Adds, or reports that the sum has no representation.
    ///
    /// # Errors
    /// [`ArithError::Overflow`] when the sum does not fit.
    pub fn checked_add(self, rhs: Self) -> Result<Self, ArithError> {
        self.raw
            .checked_add(rhs.raw)
            .map(Amount::new)
            .ok_or(ArithError::Overflow)
    }

    /// Subtracts, or reports that the result would be a negative quantity.
    ///
    /// # Errors
    /// [`ArithError::Underflow`] when the result would be negative.
    pub fn checked_sub(self, rhs: Self) -> Result<Self, ArithError> {
        self.raw
            .checked_sub(rhs.raw)
            .map(Amount::new)
            .ok_or(ArithError::Underflow)
    }

    /// Scales by the ratio `num / den`, resolving the remainder in `rounding`.
    ///
    /// The ratio's own unit cancels, so the result stays in `U`: this is a fee, a threshold, a
    /// share of something, applied to a quantity that keeps its meaning.
    ///
    /// # Errors
    /// [`ArithError::DivideByZero`] when `den` is zero, and [`ArithError::Overflow`] when the
    /// scaled quantity does not fit.
    pub fn mul_div<V: Unit>(
        self,
        num: Amount<V>,
        den: Amount<V>,
        rounding: Rounding,
    ) -> Result<Self, ArithError> {
        mul_div(self.raw, num.raw, den.raw, rounding).map(Amount::new)
    }

    /// Converts into `V` at a rate of `num` per `den`.
    ///
    /// The unit of `self` cancels against the denominator, which is what a conversion is and
    /// what makes a mixed-up one fail to compile: converting assets into shares demands the
    /// asset total in the denominator, and no other quantity will typecheck there.
    ///
    /// # Errors
    /// [`ArithError::DivideByZero`] when `den` is zero, and [`ArithError::Overflow`] when the
    /// converted quantity does not fit.
    pub fn convert<V: Unit>(
        self,
        num: Amount<V>,
        den: Amount<U>,
        rounding: Rounding,
    ) -> Result<Amount<V>, ArithError> {
        mul_div(self.raw, num.raw, den.raw, rounding).map(Amount::new)
    }

    /// The full-width product with a quantity of another unit.
    ///
    /// For invariants stated over a product — a constant-product pool, a reserve ratio — where a
    /// 128-bit product would report an overflow the mechanism never had.
    #[must_use]
    pub fn wide_mul<V: Unit>(self, rhs: Amount<V>) -> Wide {
        wide_mul(self.raw, rhs.raw)
    }
}

impl<U: Unit> Delta<U> {
    /// No movement in either direction.
    pub const ZERO: Self = Delta {
        raw: 0,
        unit: PhantomData,
    };

    /// The change of gaining `amount`.
    ///
    /// # Errors
    /// [`ArithError::Overflow`] when the quantity is too large to state as a signed change.
    pub fn positive(amount: Amount<U>) -> Result<Self, ArithError> {
        i128::try_from(amount.raw)
            .map(Delta::from_raw)
            .map_err(|_| ArithError::Overflow)
    }

    /// The change of giving up `amount`.
    ///
    /// # Errors
    /// [`ArithError::Overflow`] when the quantity is too large to state as a signed change.
    pub fn negative(amount: Amount<U>) -> Result<Self, ArithError> {
        i128::try_from(amount.raw)
            .map(|raw| Delta::from_raw(-raw))
            .map_err(|_| ArithError::Overflow)
    }

    /// The change as the signed integer it is carried as, unit dropped.
    #[must_use]
    pub const fn base_units(self) -> i128 {
        self.raw
    }

    const fn from_raw(raw: i128) -> Self {
        Delta {
            raw,
            unit: PhantomData,
        }
    }
}

// The derives would demand the same traits of the unit tag, which is a zero-sized marker that
// carries none of them. Written out, the impls depend on the quantity alone — which is the
// truth: two amounts of one unit compare as their numbers do.
macro_rules! value_semantics {
    ($name:ident) => {
        impl<U: Unit> Clone for $name<U> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<U: Unit> Copy for $name<U> {}

        impl<U: Unit> Default for $name<U> {
            fn default() -> Self {
                Self::ZERO
            }
        }

        impl<U: Unit> PartialEq for $name<U> {
            fn eq(&self, other: &Self) -> bool {
                self.raw == other.raw
            }
        }

        impl<U: Unit> Eq for $name<U> {}

        impl<U: Unit> PartialOrd for $name<U> {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl<U: Unit> Ord for $name<U> {
            fn cmp(&self, other: &Self) -> Ordering {
                self.raw.cmp(&other.raw)
            }
        }

        impl<U: Unit> Hash for $name<U> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.raw.hash(state);
            }
        }

        impl<U: Unit> fmt::Debug for $name<U> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{} {}", self.raw, U::SYMBOL)
            }
        }
    };
}

value_semantics!(Amount);
value_semantics!(Delta);

impl<U: Unit> Serialize for Amount<U> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        u128_str::serialize(&self.raw, serializer)
    }
}

impl<'de, U: Unit> Deserialize<'de> for Amount<U> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u128_str::deserialize(deserializer).map(Amount::new)
    }
}

impl<U: Unit> Serialize for Delta<U> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        i128_str::serialize(&self.raw, serializer)
    }
}

impl<'de, U: Unit> Deserialize<'de> for Delta<U> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        i128_str::deserialize(deserializer).map(Delta::from_raw)
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

    #[derive(Debug)]
    struct Token;
    impl Unit for Token {
        const SYMBOL: &'static str = "token";
    }

    #[derive(Debug)]
    struct Share;
    impl Unit for Share {
        const SYMBOL: &'static str = "share";
    }

    type Tokens = Amount<Token>;
    type Shares = Amount<Share>;

    #[test]
    fn addition_and_subtraction_are_checked() {
        assert_eq!(
            Tokens::new(2).checked_add(Tokens::new(3)),
            Ok(Tokens::new(5))
        );
        assert_eq!(
            Tokens::new(u128::MAX).checked_add(Tokens::new(1)),
            Err(ArithError::Overflow)
        );
        assert_eq!(
            Tokens::new(3).checked_sub(Tokens::new(2)),
            Ok(Tokens::new(1))
        );
        assert_eq!(
            Tokens::new(2).checked_sub(Tokens::new(3)),
            Err(ArithError::Underflow)
        );
    }

    /// The share conversion of the reference, in this crate's vocabulary: the asset total sits
    /// in the denominator and cancels, and the result is in shares because nothing else can be.
    #[test]
    fn a_conversion_cancels_the_unit_it_divides_by() {
        let assets = Tokens::new(1_234_567_000_000_000_000_000_007);
        let total_shares = Shares::new(5_000_000_000_000_000_000_000_001_000_000);
        let total_assets = Tokens::new(5_000_000_000_000_000_000_000_001);
        assert_eq!(
            assets.convert(total_shares, total_assets, Rounding::Down),
            Ok(Shares::new(1_234_567_000_000_000_000_000_007_000_000))
        );
    }

    #[test]
    fn scaling_by_a_ratio_keeps_the_unit() {
        // A quarter of the position, rounded each way: the direction decides the dust.
        let position = Tokens::new(7);
        let (num, den) = (Shares::new(1), Shares::new(4));
        assert_eq!(
            position.mul_div(num, den, Rounding::Down),
            Ok(Tokens::new(1))
        );
        assert_eq!(position.mul_div(num, den, Rounding::Up), Ok(Tokens::new(2)));
    }

    #[test]
    fn a_product_of_two_units_stays_at_full_width() {
        let wide = Tokens::new(u128::MAX).wide_mul(Shares::new(u128::MAX));
        assert!(wide > Tokens::new(u128::MAX).wide_mul(Shares::new(u128::MAX - 1)));
    }

    #[test]
    fn a_change_carries_its_direction() {
        let given_up = Delta::negative(Tokens::new(500)).unwrap();
        let received = Delta::positive(Tokens::new(500)).unwrap();
        assert_eq!(given_up.base_units(), -500);
        assert_eq!(received.base_units(), 500);
        assert_eq!(Delta::<Token>::ZERO.base_units(), 0);
    }

    #[test]
    fn a_change_beyond_the_signed_range_is_an_overflow() {
        let beyond = Tokens::new(u128::MAX);
        assert_eq!(Delta::positive(beyond), Err(ArithError::Overflow));
        assert_eq!(Delta::negative(beyond), Err(ArithError::Overflow));
    }

    #[test]
    fn money_crosses_the_wire_as_a_decimal_string() {
        for value in [0, 1, u128::MAX] {
            let amount = Tokens::new(value);
            let json = serde_json::to_string(&amount).unwrap();
            assert_eq!(json, format!("\"{value}\""));
            assert_eq!(serde_json::from_str::<Tokens>(&json).unwrap(), amount);
        }
        let delta = Delta::negative(Tokens::new(i128::MAX as u128)).unwrap();
        let json = serde_json::to_string(&delta).unwrap();
        assert_eq!(serde_json::from_str::<Delta<Token>>(&json).unwrap(), delta);
    }

    #[test]
    fn a_quantity_debugs_with_its_unit() {
        assert_eq!(format!("{:?}", Tokens::new(5)), "5 token");
        assert_eq!(format!("{:?}", Shares::new(5)), "5 share");
    }
}
