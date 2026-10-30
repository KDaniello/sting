//! Widening integer arithmetic: the operation every mechanism family shares.
//!
//! The foundation owns the operation and no scale. Morpho alone speaks two — `1e18` for rates
//! and `1e36` for prices — and Hyperliquid speaks neither, so a fixed-point type here would be
//! one family's convention imposed on the rest (karman/docs/ARITHMETIC.md §3).
//!
//! What is shared is `x * y / d` with the product held at 256 bits, because the references
//! compute in `uint256` while storing in `uint128`, and a 128-bit product would reject inputs a
//! real venue accepts.

use std::{fmt, num::NonZeroU128, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

/// Which way an operation resolves a remainder it cannot represent.
///
/// There is no default and no `Nearest`: neither reference rounds to nearest, and a direction
/// that may be omitted is a direction that will be omitted. Which one is correct is a mechanism
/// decision — the one favouring the venue (karman/docs/ARCHITECTURE.md §5, invariant 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rounding {
    /// Toward zero: the remainder stays where it already was.
    Down,
    /// Away from zero by one, whenever a remainder exists at all.
    Up,
}

/// An operation had no representable answer. Never a wrap and never a saturation: a saturated
/// number still looks like a number, and would travel into a report as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ArithError {
    /// The answer exists and does not fit the width money is carried in.
    #[error("value overflows 128 bits")]
    Overflow,
    /// A quantity was asked to go below zero, which no quantity can do.
    #[error("value would fall below zero")]
    Underflow,
    /// A ratio was asked for with nothing in the denominator.
    #[error("division by zero")]
    DivideByZero,
}

/// A value 256 bits wide, unsigned.
///
/// Two things need this width and nothing else does. An invariant stated over a product — a
/// constant product, a reserve ratio — holds only at the width the product actually has. And a
/// price is `uint256` in the interface the references read it through, where a live market
/// already exceeds 128 bits: cbBTC quoted in USDC is `6.48e38` against a scale of `1e36`.
///
/// It is not money and cannot become money by accident. No `Amount` is of this width, no state
/// holds one, and every operation that ends in a quantity hands back the width money is carried
/// in or an error (karman/docs/ARITHMETIC.md §2). The halves stay private for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Wide {
    // Ordering is derived, and the field order is the reason it is correct: comparing the high
    // halves first and the low halves only on a tie is exactly 256-bit magnitude comparison.
    high: u128,
    low: u128,
}

const HALF: u32 = 64;
const LOW: u128 = u64::MAX as u128;

/// The full 256-bit product of two 128-bit values.
///
/// Schoolbook multiplication over 64-bit halves. Every partial product fits 128 bits because
/// its factors are 64-bit, and the recombined high half is at most `2¹²⁸ − 1`, so no step of
/// this needs a width the language does not have.
#[must_use]
pub(crate) fn wide_mul(x: u128, y: u128) -> Wide {
    let (x_high, x_low) = (x >> HALF, x & LOW);
    let (y_high, y_low) = (y >> HALF, y & LOW);

    let low_low = x_low * y_low;
    let high_low = x_high * y_low;
    let low_high = x_low * y_high;
    let high_high = x_high * y_high;

    // The two crossing products straddle the 64-bit boundary: their low halves join the low
    // word, their high halves the high word, and the carry out of the low word is counted once.
    let middle = (low_low >> HALF) + (high_low & LOW) + (low_high & LOW);
    Wide {
        high: high_high + (high_low >> HALF) + (low_high >> HALF) + (middle >> HALF),
        low: (low_low & LOW) | ((middle & LOW) << HALF),
    }
}

/// `x * y / d`, with the product held at 256 bits and the remainder resolved in `rounding`.
///
/// This is the reference's `mulDivDown` and `mulDivUp` (karman/docs/ARITHMETIC.md §4). Rounding up is
/// expressed as quotient-plus-nonzero-remainder rather than as the reference's `+ (d - 1)` in
/// the numerator; the two agree for every `d > 0`, and this form needs no room above the
/// product.
pub(crate) fn mul_div(x: u128, y: u128, d: u128, rounding: Rounding) -> Result<u128, ArithError> {
    let divisor = NonZeroU128::new(d).ok_or(ArithError::DivideByZero)?;
    let (quotient, remainder) = match x.checked_mul(y) {
        // The product fits 128 bits in every swap and every fee; the wide path exists for share
        // arithmetic, where a virtual-share offset pushes it past 180 bits routinely.
        Some(product) => (product / d, product % d),
        None => divide(wide_mul(x, y), divisor)?,
    };
    match rounding {
        Rounding::Down => Ok(quotient),
        Rounding::Up if remainder == 0 => Ok(quotient),
        Rounding::Up => quotient.checked_add(1).ok_or(ArithError::Overflow),
    }
}

/// Divides a 256-bit value by a 128-bit divisor, with both parts back at money's width.
///
/// One division algorithm serves the whole crate — [`Wide::div_rem`] — and this is the narrow
/// divisor's way in. A quotient that does not fit is the overflow `mul_div` reports, reached by
/// asking whether it fits rather than by a case of its own.
fn divide(value: Wide, d: NonZeroU128) -> Result<(u128, u128), ArithError> {
    let (quotient, remainder) = value.div_rem(Wide::from_u128(d.get()))?;
    Ok((quotient.narrowed()?, remainder.low))
}

impl Wide {
    /// Nothing.
    pub const ZERO: Wide = Wide { high: 0, low: 0 };

    /// A value that already fits the width money is carried in.
    #[must_use]
    pub const fn from_u128(value: u128) -> Wide {
        Wide {
            high: 0,
            low: value,
        }
    }

    /// Back down to the width money is carried in.
    ///
    /// # Errors
    ///
    /// Fails when the value does not fit. That is the only honest answer: a quantity that does
    /// not fit has not been rounded, it has been lost.
    pub const fn narrowed(self) -> Result<u128, ArithError> {
        if self.high == 0 {
            Ok(self.low)
        } else {
            Err(ArithError::Overflow)
        }
    }

    /// `x · self / den`, with the answer at the width money is carried in.
    ///
    /// The wide operand multiplies, so the product `x · self` would need 384 bits. It is never
    /// formed: splitting the wide operand against the divisor first — `self = whole · den + part`
    /// with `part < den` — leaves a whole part that multiplies exactly and a remainder narrow
    /// enough for the ordinary widening divide. The first term is exact, so resolving the
    /// remainder of the second resolves the remainder of the whole expression.
    ///
    /// # Errors
    ///
    /// Fails on a zero divisor, and when the answer leaves the width money is carried in.
    pub fn multiplying(self, x: u128, den: u128, rounding: Rounding) -> Result<u128, ArithError> {
        // A value that fits money pays nothing for the width it is not using.
        if self.high == 0 {
            return mul_div(x, self.low, den, rounding);
        }
        let divisor = NonZeroU128::new(den).ok_or(ArithError::DivideByZero)?;
        if x == 0 {
            return Ok(0);
        }
        let (whole, part) = self.div_rem(Wide::from_u128(divisor.get()))?;
        let exact = x
            .checked_mul(whole.narrowed()?)
            .ok_or(ArithError::Overflow)?;
        exact
            .checked_add(mul_div(x, part.low, den, rounding)?)
            .ok_or(ArithError::Overflow)
    }

    /// `x · num / self`, with the answer at the width money is carried in.
    ///
    /// The wide operand divides. Here it is the numerator that is formed at full width and the
    /// division that is the wide one, so there is nothing to split.
    ///
    /// # Errors
    ///
    /// Fails on a zero divisor, and when the answer leaves the width money is carried in.
    pub fn dividing(self, x: u128, num: u128, rounding: Rounding) -> Result<u128, ArithError> {
        if self.high == 0 {
            return mul_div(x, num, self.low, rounding);
        }
        let (quotient, remainder) = wide_mul(x, num).div_rem(self)?;
        let quotient = quotient.narrowed()?;
        match rounding {
            Rounding::Down => Ok(quotient),
            Rounding::Up if remainder == Wide::ZERO => Ok(quotient),
            Rounding::Up => quotient.checked_add(1).ok_or(ArithError::Overflow),
        }
    }

    /// `self · num / den`, staying at full width.
    ///
    /// For moving a value that is already wider than money — a price against a shock — where
    /// narrowing on the way through would be exactly the loss the width exists to prevent.
    ///
    /// # Errors
    ///
    /// Fails on a zero divisor, and when the answer leaves 256 bits.
    pub fn mul_div(self, num: u128, den: u128, rounding: Rounding) -> Result<Wide, ArithError> {
        let divisor = NonZeroU128::new(den).ok_or(ArithError::DivideByZero)?;
        let wide_divisor = Wide::from_u128(divisor.get());
        let (whole, part) = self.div_rem(wide_divisor)?;
        let (rest, remainder) = wide_mul(part.low, num).div_rem(wide_divisor)?;
        let sum = whole.checked_mul(num)?.checked_add(rest)?;
        match rounding {
            Rounding::Down => Ok(sum),
            Rounding::Up if remainder == Wide::ZERO => Ok(sum),
            Rounding::Up => sum.checked_add(Wide::from_u128(1)),
        }
    }

    /// Quotient and remainder against another value of this width.
    ///
    /// Restoring long division, as [`divide`] is, and for the same reason: one bit of quotient
    /// per shift-compare-subtract step is short enough to read and to check against the narrow
    /// path.
    ///
    /// It steps only over the bits the quotient can actually have, which is the difference
    /// between the two highest set bits. Everything above that is a leading zero of the answer,
    /// and stepping over those is most of the work at the sizes this is used at: a price divided
    /// by its scale is ten bits of answer, not two hundred and fifty-six.
    fn div_rem(self, divisor: Wide) -> Result<(Wide, Wide), ArithError> {
        if divisor == Wide::ZERO {
            return Err(ArithError::DivideByZero);
        }
        if self < divisor {
            return Ok((Wide::ZERO, self));
        }
        let highest = self.top_bit() - divisor.top_bit();
        // Everything above the window, which is already below the divisor by that construction.
        let mut remainder = self.shifted_right(highest + 1);
        let mut quotient = Wide::ZERO;
        for bit in (0..=highest).rev() {
            // The bit leaving the top carries a value of 2^256, which is above every divisor, so
            // a set carry admits the subtraction without a comparison.
            let carry = remainder.high >> 127;
            remainder = remainder.doubled();
            remainder.low |= self.bit(bit);
            quotient = quotient.doubled();
            if carry == 1 || remainder >= divisor {
                remainder = remainder.wrapping_sub(divisor);
                quotient.low |= 1;
            }
        }
        Ok((quotient, remainder))
    }

    /// The position of the highest set bit, of a value known to have one.
    ///
    /// `ilog2` on each half rather than a subtraction from the width: the two say the same thing
    /// for a non-zero half, and the named one cannot be off by one.
    const fn top_bit(self) -> u32 {
        if self.high == 0 {
            self.low.ilog2()
        } else {
            128 + self.high.ilog2()
        }
    }

    const fn bit(self, at: u32) -> u128 {
        if at < 128 {
            (self.low >> at) & 1
        } else {
            (self.high >> (at - 128)) & 1
        }
    }

    /// The value with its bits moved down, and zeros arriving at the top.
    const fn shifted_right(self, by: u32) -> Wide {
        if by >= 256 {
            Wide::ZERO
        } else if by >= 128 {
            Wide {
                high: 0,
                low: self.high >> (by - 128),
            }
        } else if by == 0 {
            self
        } else {
            Wide {
                high: self.high >> by,
                low: (self.low >> by) | (self.high << (128 - by)),
            }
        }
    }

    const fn doubled(self) -> Wide {
        Wide {
            high: (self.high << 1) | (self.low >> 127),
            low: self.low << 1,
        }
    }

    fn wrapping_sub(self, other: Wide) -> Wide {
        let borrow = u128::from(self.low < other.low);
        Wide {
            high: self.high.wrapping_sub(other.high).wrapping_sub(borrow),
            low: self.low.wrapping_sub(other.low),
        }
    }

    fn checked_add(self, other: Wide) -> Result<Wide, ArithError> {
        let low = self.low.wrapping_add(other.low);
        let carry = u128::from(low < self.low);
        let high = self
            .high
            .checked_add(other.high)
            .and_then(|high| high.checked_add(carry))
            .ok_or(ArithError::Overflow)?;
        Ok(Wide { high, low })
    }

    fn checked_mul(self, y: u128) -> Result<Wide, ArithError> {
        let upper = wide_mul(self.high, y);
        if upper.high != 0 {
            return Err(ArithError::Overflow);
        }
        Wide {
            high: upper.low,
            low: 0,
        }
        .checked_add(wide_mul(self.low, y))
    }
}

/// A decimal string was not a value of this width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ParseWideError {
    /// Nothing to read.
    #[error("no digits")]
    Empty,
    /// Something that is not a digit.
    #[error("not a decimal digit")]
    NotADigit,
    /// More digits than 256 bits can hold.
    #[error("value overflows 256 bits")]
    Overflow,
}

impl Wide {
    /// The thirty-two bytes a chain carries a word in, most significant first.
    ///
    /// The other direction of [`Wide::from_hex`], for a caller that speaks to a machine rather
    /// than reads an answer.
    #[must_use]
    pub const fn to_be_bytes(self) -> [u8; 32] {
        let (high, low) = (self.high.to_be_bytes(), self.low.to_be_bytes());
        let mut word = [0u8; 32];
        let mut at = 0;
        while at < 16 {
            word[at] = high[at];
            word[at + 16] = low[at];
            at += 1;
        }
        word
    }

    /// A value as its hexadecimal digits, which is the form a node answers a call in.
    ///
    /// Without a `0x`, because a caller reading fixed-width words has already split them.
    ///
    /// # Errors
    ///
    /// Fails on anything that is not a hexadecimal digit, and on more digits than this width
    /// holds.
    pub fn from_hex(text: &str) -> Result<Wide, ParseWideError> {
        if text.is_empty() {
            return Err(ParseWideError::Empty);
        }
        let mut value = Wide::ZERO;
        for byte in text.bytes() {
            let digit = char::from(byte)
                .to_digit(16)
                .ok_or(ParseWideError::NotADigit)?;
            value = value
                .checked_mul(16)
                .and_then(|shifted| shifted.checked_add(Wide::from_u128(u128::from(digit))))
                .map_err(|_| ParseWideError::Overflow)?;
        }
        Ok(value)
    }
}

impl FromStr for Wide {
    type Err = ParseWideError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Err(ParseWideError::Empty);
        }
        let mut value = Wide::ZERO;
        for byte in text.bytes() {
            if !byte.is_ascii_digit() {
                return Err(ParseWideError::NotADigit);
            }
            let digit = Wide::from_u128(u128::from(byte - b'0'));
            value = value
                .checked_mul(10)
                .and_then(|shifted| shifted.checked_add(digit))
                .map_err(|_| ParseWideError::Overflow)?;
        }
        Ok(value)
    }
}

impl fmt::Display for Wide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.high == 0 {
            return write!(f, "{}", self.low);
        }
        // Produced least significant first; 256 bits is at most 78 decimal digits.
        let mut digits = Vec::with_capacity(78);
        let mut value = *self;
        while value != Wide::ZERO {
            let (quotient, remainder) =
                value.div_rem(Wide::from_u128(10)).map_err(|_| fmt::Error)?;
            digits.push(b'0' + u8::try_from(remainder.low).map_err(|_| fmt::Error)?);
            value = quotient;
        }
        digits.reverse();
        f.write_str(std::str::from_utf8(&digits).map_err(|_| fmt::Error)?)
    }
}

impl From<u128> for Wide {
    fn from(value: u128) -> Self {
        Wide::from_u128(value)
    }
}

impl Serialize for Wide {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Wide {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl de::Visitor<'_> for Visitor {
            type Value = Wide;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a non-negative integer, or a decimal string for 256-bit range")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Wide, E> {
                v.parse().map_err(E::custom)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Wide, E> {
                Ok(Wide::from_u128(u128::from(v)))
            }

            fn visit_u128<E: de::Error>(self, v: u128) -> Result<Wide, E> {
                Ok(Wide::from_u128(v))
            }
        }

        deserializer.deserialize_any(Visitor)
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
    use proptest::prelude::*;

    use super::*;

    /// Vectors of the shape Morpho's share conversions produce, with the expected results
    /// computed by exact big-integer arithmetic outside this crate. Every product here is wider
    /// than 128 bits, so each one exercises the long-division path.
    const VECTORS: &[(u128, u128, u128, u128, u128)] = &[
        // Shape of `toSharesDown`: assets into shares at the 1e6 virtual offset. Exact, because
        // a pristine market converts at exactly the offset.
        (
            1_234_567_000_000_000_000_000_007,
            5_000_000_000_000_000_000_000_001_000_000,
            5_000_000_000_000_000_000_000_001,
            1_234_567_000_000_000_000_000_007_000_000,
            1_234_567_000_000_000_000_000_007_000_000,
        ),
        // Shape of `toAssetsUp`: shares back into assets, with dust the direction has to place.
        (
            370_000_000_000_000_000_000_000_000_013,
            2_000_000_000_000_000_000_000_001,
            2_000_000_000_000_000_000_000_001_000_000,
            370_000_000_000_000_000_000_000,
            370_000_000_000_000_000_000_001,
        ),
        // The widest operands whose result still fits: a full 256-bit product.
        (u128::MAX, u128::MAX, u128::MAX, u128::MAX, u128::MAX),
        // A 255-bit product with a remainder, where the two directions differ at the top of the
        // range rather than in comfortable numbers.
        (
            170_141_183_460_469_231_731_687_303_715_884_105_729,
            170_141_183_460_469_231_731_687_303_715_884_105_729,
            170_141_183_460_469_231_731_687_303_715_884_105_728,
            170_141_183_460_469_231_731_687_303_715_884_105_730,
            170_141_183_460_469_231_731_687_303_715_884_105_731,
        ),
    ];

    /// Adds a 128-bit value to a 256-bit one, so a test can reconstruct `q * d + r` at the width
    /// the division happened in.
    fn wide_add(product: Wide, addend: u128) -> Wide {
        let low = product.low.wrapping_add(addend);
        let carry = u128::from(low < product.low);
        Wide {
            high: product.high + carry,
            low,
        }
    }

    #[test]
    fn the_reference_vectors_agree_in_both_directions() {
        for &(x, y, d, down, up) in VECTORS {
            assert_eq!(
                mul_div(x, y, d, Rounding::Down),
                Ok(down),
                "down: {x} * {y} / {d}"
            );
            assert_eq!(
                mul_div(x, y, d, Rounding::Up),
                Ok(up),
                "up: {x} * {y} / {d}"
            );
        }
    }

    #[test]
    fn a_quotient_that_does_not_fit_is_an_overflow() {
        assert_eq!(
            mul_div(u128::MAX, u128::MAX, u128::MAX - 1, Rounding::Down),
            Err(ArithError::Overflow)
        );
    }

    #[test]
    fn rounding_up_at_the_ceiling_is_an_overflow_and_not_a_wrap() {
        // The quotient is exactly u128::MAX with a remainder, so up has nowhere left to go.
        assert_eq!(
            mul_div(u128::MAX, 2, 2, Rounding::Up),
            Ok(u128::MAX),
            "an exact quotient at the ceiling is not an overflow"
        );
        assert_eq!(
            mul_div(u128::MAX, 3, 2, Rounding::Up),
            Err(ArithError::Overflow)
        );
    }

    #[test]
    fn dividing_by_zero_is_named_as_such() {
        assert_eq!(
            mul_div(1, 1, 0, Rounding::Down),
            Err(ArithError::DivideByZero)
        );
    }

    #[test]
    fn the_widening_bound_of_the_numeric_contract_holds() {
        // karman/docs/ARITHMETIC.md §2: the largest product of two 128-bit values is 2^256 - 2^129 + 1.
        let widest = wide_mul(u128::MAX, u128::MAX);
        assert_eq!(
            widest,
            Wide {
                high: u128::MAX - 1,
                low: 1
            }
        );
    }

    #[test]
    fn a_product_orders_by_magnitude_across_the_halves() {
        assert!(wide_mul(1, 1) < wide_mul(u128::MAX, 1));
        assert!(wide_mul(u128::MAX, 1) < wide_mul(u128::MAX, 2));
        assert_eq!(wide_mul(7, 11), wide_mul(11, 7));
    }

    proptest! {
        /// The division contract, checked at the width the division happens in: the quotient
        /// times the divisor plus the remainder reconstructs the product exactly, and the
        /// remainder is below the divisor.
        #[test]
        fn quotient_and_remainder_reconstruct_the_product(x: u128, y: u128, d in 1u128..) {
            let product = wide_mul(x, y);
            let Ok((quotient, remainder)) = divide(product, NonZeroU128::new(d).unwrap()) else {
                return Ok(());
            };
            prop_assert!(remainder < d);
            prop_assert_eq!(wide_add(wide_mul(quotient, d), remainder), product);
        }

        /// The long division is pinned against the language's own divide, on the operands
        /// where both are defined. `divide` is called directly: routing through `mul_div` would
        /// take the fast path for every product that fits 128 bits, which is every product two
        /// 64-bit operands can make, and the division under test would never run.
        #[test]
        fn the_long_division_agrees_with_narrow_arithmetic(x in 0u64.., y in 0u64.., d in 1u128..) {
            let (x, y) = (u128::from(x), u128::from(y));
            let divided = divide(wide_mul(x, y), NonZeroU128::new(d).unwrap());
            prop_assert_eq!(divided, Ok((x * y / d, x * y % d)));
        }

        /// Rounding up exceeds rounding down by exactly one, and only when the division left a
        /// remainder — the property that makes a direction a choice rather than a coin flip.
        #[test]
        fn the_two_directions_differ_by_at_most_one(x: u128, y: u128, d in 1u128..) {
            let (Ok(down), Ok(up)) =
                (mul_div(x, y, d, Rounding::Down), mul_div(x, y, d, Rounding::Up))
            else {
                return Ok(());
            };
            let exact = wide_mul(x, y) == wide_mul(down, d);
            prop_assert_eq!(up - down, u128::from(!exact));
        }

        /// Scaling by a larger numerator never returns less: a monotone operation cannot hide a
        /// sign error in the recombination of the halves.
        #[test]
        fn scaling_is_monotone_in_the_numerator(x: u128, y: u128, d in 1u128..) {
            let (Ok(smaller), Ok(larger)) = (
                mul_div(x, y, d, Rounding::Down),
                mul_div(x, y.saturating_add(1), d, Rounding::Down),
            ) else {
                return Ok(());
            };
            prop_assert!(larger >= smaller);
        }
    }

    /// The cbBTC/USDC market's own price: `6.48e38` against a scale of `1e36`, which is 1.9 times
    /// what 128 bits can hold. One whole unit of collateral is `1e8` base units and is worth
    /// `64 800` USDC, which is `6.48e10` base units of the loan asset.
    const QUOTED: Wide = Wide {
        high: 1,
        low: 307_717_633_079_061_536_536_625_392_568_231_788_544,
    };
    const PRICE_SCALE: u128 = 1_000_000_000_000_000_000_000_000_000_000_000_000;

    /// `x · value / den`, as `(high, low, x, den, down, up)`.
    ///
    /// Computed outside this workspace in exact integer arithmetic. A vector derived by the code
    /// it checks would only confirm that the code agrees with itself.
    const MULTIPLYING: &[(u128, u128, u128, u128, u128, u128)] = &[
        // One whole cbBTC, valued in USDC base units.
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_544,
            100_000_000,
            PRICE_SCALE,
            64_800_000_000,
            64_800_000_000,
        ),
        // The same price off by one, so the two directions have a remainder to disagree about.
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_545,
            3,
            PRICE_SCALE,
            1_944,
            1_945,
        ),
        // One base unit of collateral is worth 648 base units of the loan asset.
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_544,
            1,
            PRICE_SCALE,
            648,
            648,
        ),
        (
            4_722_366_482_869_645_213_696,
            12_345,
            1,
            1_000_000_000_000_000_000_000_000,
            1_606_938_044_258_990_275_541_962_092_341_162_602,
            1_606_938_044_258_990_275_541_962_092_341_162_603,
        ),
        // The narrowest value this width holds that the narrow one does not.
        (
            1,
            1,
            1,
            10,
            34_028_236_692_093_846_346_337_460_743_176_821_145,
            34_028_236_692_093_846_346_337_460_743_176_821_146,
        ),
    ];

    /// `x · num / value`, as `(high, low, x, num, down, up)`.
    const DIVIDING: &[(u128, u128, u128, u128, u128, u128)] = &[
        // The trade above read backwards: what 64 800 USDC buys.
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_544,
            64_800_000_000,
            PRICE_SCALE,
            100_000_000,
            100_000_000,
        ),
        // One base unit of the loan asset buys less than one of collateral, and the direction
        // decides whether the buyer gets nothing or the venue gives away a unit.
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_544,
            1,
            PRICE_SCALE,
            0,
            1,
        ),
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_545,
            64_800_000_000,
            PRICE_SCALE,
            99_999_999,
            100_000_000,
        ),
        (
            4_722_366_482_869_645_213_696,
            12_345,
            1_000_000_000_000_000_000_000_000_000_000,
            1_000_000_000_000_000_000,
            0,
            1,
        ),
    ];

    /// `value · num / den` staying wide, as `(high, low, num, den, out_high, out_low)`.
    const MOVED: &[(u128, u128, u128, u128, u128, u128)] = &[
        // A price gapped to eight tenths of where it opened.
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_544,
            8_000,
            10_000,
            1,
            178_117_633_079_061_536_536_625_392_568_231_788_544,
        ),
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_544,
            10_001,
            10_000,
            1,
            307_782_433_079_061_536_536_625_392_568_231_788_544,
        ),
        (
            1,
            307_717_633_079_061_536_536_625_392_568_231_788_545,
            1,
            3,
            0,
            216_000_000_000_000_000_000_000_000_000_000_000_000,
        ),
    ];

    #[test]
    fn a_wide_value_multiplies_into_the_width_money_is_carried_in() {
        for &(high, low, x, den, down, up) in MULTIPLYING {
            let value = Wide { high, low };
            assert_eq!(
                value.multiplying(x, den, Rounding::Down),
                Ok(down),
                "{value} down"
            );
            assert_eq!(
                value.multiplying(x, den, Rounding::Up),
                Ok(up),
                "{value} up"
            );
        }
    }

    #[test]
    fn a_wide_value_divides_into_the_width_money_is_carried_in() {
        for &(high, low, x, num, down, up) in DIVIDING {
            let value = Wide { high, low };
            assert_eq!(
                value.dividing(x, num, Rounding::Down),
                Ok(down),
                "{value} down"
            );
            assert_eq!(value.dividing(x, num, Rounding::Up), Ok(up), "{value} up");
        }
    }

    #[test]
    fn a_wide_value_moved_by_a_ratio_stays_wide() {
        for &(high, low, num, den, out_high, out_low) in MOVED {
            let moved = Wide { high, low }.mul_div(num, den, Rounding::Down);
            assert_eq!(
                moved,
                Ok(Wide {
                    high: out_high,
                    low: out_low
                })
            );
        }
    }

    /// The two directions of a move differ by one exactly when a remainder exists, which is the
    /// same promise the narrow operation makes.
    #[test]
    fn moving_a_wide_value_resolves_its_remainder_in_the_stated_direction() {
        let exact = QUOTED.mul_div(1, 2, Rounding::Down).unwrap();
        assert_eq!(QUOTED.mul_div(1, 2, Rounding::Up), Ok(exact));
        let third = Wide { high: 1, low: 1 };
        assert_eq!(
            third.mul_div(1, 3, Rounding::Up),
            third
                .mul_div(1, 3, Rounding::Down)
                .unwrap()
                .checked_add(Wide::from_u128(1)),
        );
    }

    /// An answer that does not fit money is refused rather than carried at a width no state has.
    #[test]
    fn a_wide_operation_whose_answer_is_not_money_is_refused() {
        assert_eq!(
            QUOTED.multiplying(u128::MAX, 1, Rounding::Down),
            Err(ArithError::Overflow)
        );
        // The answer exists at 256 bits and is still not money, in both terms of the split: the
        // whole part alone leaves the width, and so does the sum of a whole part that fits.
        let past = Wide {
            high: 4_722_366_482_869_645_213_696,
            low: 12_345,
        };
        assert_eq!(
            past.multiplying(7, 1_000_000_000_000_000_000, Rounding::Down),
            Err(ArithError::Overflow)
        );
        assert_eq!(
            Wide { high: 1, low: 1 }.multiplying(1, 1, Rounding::Down),
            Err(ArithError::Overflow)
        );
        assert_eq!(QUOTED.narrowed(), Err(ArithError::Overflow));
        assert_eq!(QUOTED.dividing(1, 1, Rounding::Down), Ok(0));
        assert_eq!(
            QUOTED.multiplying(1, 0, Rounding::Down),
            Err(ArithError::DivideByZero)
        );
        assert_eq!(
            QUOTED.mul_div(1, 0, Rounding::Down),
            Err(ArithError::DivideByZero)
        );
        assert_eq!(
            Wide::ZERO.dividing(1, 1, Rounding::Down),
            Err(ArithError::DivideByZero)
        );
    }

    /// Nothing of this width reaches a fixture as a number: JSON cannot carry 128 bits, let alone
    /// 256, so it crosses as a decimal string in both directions.
    #[test]
    fn a_wide_value_crosses_a_boundary_as_its_digits() {
        assert_eq!(
            QUOTED.to_string(),
            "648000000000000000000000000000000000000"
        );
        assert_eq!(
            "648000000000000000000000000000000000000".parse::<Wide>(),
            Ok(QUOTED)
        );
        assert_eq!("0".parse::<Wide>(), Ok(Wide::ZERO));
        assert_eq!(
            Wide::from_hex(&format!("{:064x}", 648u128)),
            Ok(Wide::from_u128(648))
        );
        assert_eq!(
            Wide::from_hex("00000000000000000000000000000001e78042a954d9eeedd76aa08000000000"),
            Ok(QUOTED),
        );
        assert_eq!(
            Wide::from_hex(
                &QUOTED
                    .to_be_bytes()
                    .map(|byte| format!("{byte:02x}"))
                    .concat()
            ),
            Ok(QUOTED),
        );
        assert_eq!(Wide::from_hex(""), Err(ParseWideError::Empty));
        assert_eq!(Wide::from_hex("0x1"), Err(ParseWideError::NotADigit));
        assert_eq!(
            Wide::from_hex(&"f".repeat(65)),
            Err(ParseWideError::Overflow)
        );
        assert_eq!("".parse::<Wide>(), Err(ParseWideError::Empty));
        assert_eq!("64_800".parse::<Wide>(), Err(ParseWideError::NotADigit));
        assert_eq!("-1".parse::<Wide>(), Err(ParseWideError::NotADigit));
        // Two to the two hundred and fifty-sixth, which is one past the end of this width.
        let past = "115792089237316195423570985008687907853269984665640564039457584007913129639936";
        assert_eq!(past.parse::<Wide>(), Err(ParseWideError::Overflow));
    }

    proptest! {
        /// The quotient and remainder reconstruct the dividend, which is the whole contract of a
        /// division and the property the narrow path is held to as well.
        #[test]
        fn a_wide_quotient_and_remainder_reconstruct_the_dividend(
            high: u128, low: u128, d in 1u128..,
        ) {
            let value = Wide { high, low };
            let divisor = Wide::from_u128(d);
            let (quotient, remainder) = value.div_rem(divisor).unwrap();
            prop_assert!(remainder < divisor);
            prop_assert_eq!(
                quotient.checked_mul(d).unwrap().checked_add(remainder).unwrap(),
                value,
            );
        }

        /// A value that fits money takes the narrow path, and the two paths are one operation:
        /// whatever the narrow one answers — including the overflow it reports — is the answer.
        #[test]
        fn the_wide_path_agrees_with_the_narrow_one_wherever_both_apply(
            x: u128, y: u128, d in 1u128..,
        ) {
            let wide = Wide::from_u128(y);
            for rounding in [Rounding::Down, Rounding::Up] {
                prop_assert_eq!(wide.multiplying(x, d, rounding), mul_div(x, y, d, rounding));
                prop_assert_eq!(Wide::from_u128(d).dividing(x, y, rounding), mul_div(x, y, d, rounding));
            }
        }

        /// Digits out and digits in are the same value.
        #[test]
        fn a_wide_value_survives_its_decimal_form(high: u128, low: u128) {
            let value = Wide { high, low };
            prop_assert_eq!(value.to_string().parse::<Wide>(), Ok(value));
        }
    }
}
