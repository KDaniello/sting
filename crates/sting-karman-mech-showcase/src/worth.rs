//! What a thing is worth, and to whom.
//!
//! Two quantities, after Weyl and Zhang: a **common worth** every participant shares, and a
//! **taste** that is one participant's own opinion of one item. What somebody would pay is their
//! sum, and nothing else in this family is a valuation.
//!
//! Neither reaches the mechanism. [`crate::ShowcaseState`] carries no worth of any kind, because
//! an item that knew what it was worth would let an agent read the answer the whole simulation
//! exists to measure, and every result would be circular. An agent is handed its own valuation and
//! never the common half alone; the common half is read from outside the run, where a score
//! against it means something.
//!
//! **Nothing here is stored.** A market of a million items and ten thousand participants has ten
//! billion valuations, and a table of them is not a thing that fits anywhere — nor does a table of
//! the million common worths, once every one of those participants needs to consult it. Both are
//! pure functions of the world's seed and of what they are asked about: the same value on every
//! machine and at every re-run, computed where it is wanted and thrown away. What a [`Worth`]
//! carries is one price per collection and three numbers, so every participant can hold one of its
//! own without a thought.
//!
//! Those functions are not randomness and must not be confused with it. They draw nothing from the
//! run's stream and consume no state; they index a world that was fixed before the first tick, and
//! an agent reading one is consulting its own opinion rather than tossing a coin. The
//! architecture's rule that agent randomness comes from the seeded stream is about the other thing.
//!
//! **No float touches any of this**, which is why the lognormal below is two tables of integers and
//! a shift. A world is a pure function of its scenario and its seed on any platform, and the
//! exponential of a fitted shape is exactly where that promise would be lost: two libraries' `exp`
//! agree to a hair and not to a bit.

use serde::{Deserialize, Serialize};
use sting_karman_types::{AgentId, Amount, ArithError, Rounding};

use crate::{Bps, Cash, CollectionId, ItemId, WHOLE};

/// Where the fixed-point tables put their point.
const ONE: i64 = 1 << 16;

/// The natural logarithm of two, at [`ONE`].
///
/// Turning `exp(x)` into `2^(x / ln 2)` is what lets an exponential be sixty-five integers and a
/// shift instead of a library nobody can pin.
const LN2: i64 = 45_426;

/// How far the exponent may reach either way, so a shape nobody meant cannot ask for a worth wider
/// than money is carried in.
const REACH: i64 = 20 * ONE;

/// `Φ⁻¹(1 − 2⁻ᵏ)` for `k` from one, at [`ONE`].
///
/// Indexed by the octave of a draw rather than over a uniform grid, and that is what gives this a
/// tail at all: a uniform grid over the probability stops a little past two standard deviations,
/// and the things this models are worth ten and a hundred times their median.
const TAIL: [i32; 34] = [
    0, 44_203, 75_389, 100_540, 122_076, 141_156, 158_437, 174_330, 189_113, 202_983, 216_085,
    228_531, 240_408, 251_785, 262_719, 273_257, 283_438, 293_295, 302_857, 312_148, 321_190,
    330_000, 338_597, 346_994, 355_204, 363_239, 371_111, 378_827, 386_397, 393_829, 401_130,
    408_306, 415_364, 422_310,
];

/// `2^(k/64)` for `k` from zero to sixty-four, at `1 << 32`.
const EXP2: [u64; 65] = [
    4_294_967_296,
    4_341_736_423,
    4_389_014_833,
    4_436_808_071,
    4_485_121_744,
    4_533_961_517,
    4_583_333_121,
    4_633_242_347,
    4_683_695_048,
    4_734_697_143,
    4_786_254_615,
    4_838_373_510,
    4_891_059_943,
    4_944_320_094,
    4_998_160_210,
    5_052_586_606,
    5_107_605_667,
    5_163_223_846,
    5_219_447_668,
    5_276_283_726,
    5_333_738_689,
    5_391_819_295,
    5_450_532_358,
    5_509_884_764,
    5_569_883_475,
    5_630_535_530,
    5_691_848_042,
    5_753_828_203,
    5_816_483_285,
    5_879_820_635,
    5_943_847_684,
    6_008_571_941,
    6_074_001_000,
    6_140_142_534,
    6_207_004_303,
    6_274_594_148,
    6_342_919_999,
    6_411_989_869,
    6_481_811_861,
    6_552_394_164,
    6_623_745_059,
    6_695_872_913,
    6_768_786_189,
    6_842_493_438,
    6_917_003_306,
    6_992_324_534,
    7_068_465_956,
    7_145_436_504,
    7_223_245_206,
    7_301_901_189,
    7_381_413_680,
    7_461_792_005,
    7_543_045_592,
    7_625_183_973,
    7_708_216_783,
    7_792_153_760,
    7_877_004_752,
    7_962_779_710,
    8_049_488_696,
    8_137_141_881,
    8_225_749_546,
    8_315_322_086,
    8_405_870_007,
    8_497_403_930,
    8_589_934_592,
];

/// Which of the two things a world is being asked about, so that an item's worth and one opinion
/// of it cannot come out the same number by accident.
const COMMON: u64 = 0x0063_6F6D_6D6F_6E00;
/// The other one.
const TASTE: u64 = 0x0074_6173_7465_0000;

/// What every item in a world is worth, and how far opinion of it reaches.
///
/// Small enough for every participant to hold one: a price per collection and three numbers. The
/// worths themselves are computed on the way past.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worth {
    /// What the median item of each collection is worth, by [`CollectionId`]. **From the census.**
    medians: Vec<Amount<Cash>>,
    /// How far apart the things inside one collection really are, at `1 << 16`. **Fitted.**
    shape: u32,
    /// How far one participant's opinion reaches around the common worth. **Fitted.**
    spread: Amount<Bps>,
    /// The world this belongs to. Two worlds of one seed hold one set of worths and one of
    /// opinions.
    seed: u64,
}

impl Worth {
    /// Builds a world's worths from what its collections sell for.
    ///
    /// # Errors
    ///
    /// Fails when the spread is wider than the whole, which would let a valuation fall through zero
    /// and out of the width money is carried in.
    pub fn new(
        medians: Vec<Amount<Cash>>,
        shape: u32,
        spread: Amount<Bps>,
        seed: u64,
    ) -> Result<Self, ArithError> {
        if spread > WHOLE {
            return Err(ArithError::Overflow);
        }
        Ok(Worth {
            medians,
            shape,
            spread,
            seed,
        })
    }

    /// How many collections this world knows a price for.
    #[must_use]
    pub fn collections(&self) -> usize {
        self.medians.len()
    }

    /// What everybody agrees an item is worth, which no agent may read.
    ///
    /// A lognormal draw about its collection's median, because worth here is the product of many
    /// small independent things — how rare it is, how it looks, when it was issued — and a product
    /// of such things is lognormal whatever they are individually. Nothing measured argues for
    /// another family, and a family chosen to fit would be one that cannot fail.
    ///
    /// This is the quantity convergence is scored against, and it is reachable so that a
    /// composition root can score it from outside a run. An agent handed this instead of
    /// [`value`](Self::value) would be an agent that knows the answer.
    #[must_use]
    pub fn common(&self, collection: CollectionId, item: ItemId) -> Option<Amount<Cash>> {
        let median = self.medians.get(collection.0 as usize).copied()?;
        let z = i64::from(normal(one(self.seed ^ COMMON, AgentId(0), item)));
        // `z` and the shape are both at ONE, so their product is at ONE squared; dividing by a
        // logarithm at ONE brings it back to where the exponential wants it.
        let exponent = (z * i64::from(self.shape) / LN2).clamp(-REACH, REACH);
        Some(Amount::new(
            median.base_units().saturating_mul(exp2(exponent)) >> 32,
        ))
    }

    /// What one participant thinks one item is worth.
    ///
    /// # Errors
    ///
    /// Fails only where the common worth and its spread cannot be held in the width money is
    /// carried in, which no world generated for this reaches.
    pub fn value(
        &self,
        who: AgentId,
        collection: CollectionId,
        item: ItemId,
    ) -> Result<Option<Amount<Cash>>, ArithError> {
        let Some(common) = self.common(collection, item) else {
            return Ok(None);
        };
        let band = common.mul_div(self.spread, WHOLE, Rounding::Down)?;
        if band.is_zero() {
            return Ok(Some(common));
        }
        // The band is symmetric about the common worth, so the draw is over `2·band + 1` points and
        // lands on the common worth when it lands in the middle.
        let width = band
            .base_units()
            .checked_mul(2)
            .and_then(|twice| twice.checked_add(1))
            .ok_or(ArithError::Overflow)?;
        let offset = unbiased(pair(self.seed ^ TASTE, who, item), width);
        let value = common
            .base_units()
            .checked_add(offset)
            .and_then(|raised| raised.checked_sub(band.base_units()))
            .ok_or(ArithError::Overflow)?;
        Ok(Some(Amount::new(value)))
    }
}

/// One point of a fixed world, addressed by the world it belongs to and by what it is about.
///
/// The mixing is `splitmix64`, applied twice. It is here rather than a stream of
/// [`sting_karman_core::RunRng`] because a stream costs a cipher's worth of setting up, this is asked for
/// once per item an agent looks at — several hundred million times in a sweep — and what it
/// produces is not random. The constants are the published ones and the tests pin what they
/// produce, so no cleanup can drift them without a failure.
const fn pair(seed: u64, who: AgentId, item: ItemId) -> u128 {
    let both = ((who.0 as u64) << 32) | (item.0 as u64);
    let low = mix(seed ^ mix(both));
    let high = mix(low ^ 0x5DEE_CE66_D9BB_7B01);
    ((high as u128) << 64) | (low as u128)
}

/// The lower half of the same point, where only one word is wanted.
#[allow(
    clippy::cast_possible_truncation,
    reason = "taking a word out of a wider point is what these two are for"
)]
const fn one(seed: u64, who: AgentId, item: ItemId) -> u64 {
    pair(seed, who, item) as u64
}

/// The `splitmix64` finaliser.
const fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A value below `width`, without the bias a bare remainder would leave.
///
/// The same rejection the run's own stream uses, and for the same reason: where a range does not
/// divide the space evenly, its low values come up more often than its high ones, and a world
/// tilted that way is tilted everywhere at once. What the loop advances is the mixing rather than a
/// stream, so this stays a pure function of what it was asked about.
fn unbiased(from: u128, width: u128) -> u128 {
    let limit = u128::MAX - (u128::MAX % width);
    let mut at = from;
    while at >= limit {
        at = advance(at);
    }
    at % width
}

/// The next point of the same fixed sequence, for a draw that has to be thrown away.
#[allow(
    clippy::cast_possible_truncation,
    reason = "taking a word out of a wider point is what these two are for"
)]
const fn advance(at: u128) -> u128 {
    let low = mix(at as u64);
    let high = mix(((at >> 64) as u64) ^ low);
    ((high as u128) << 64) | (low as u128)
}

/// A standard normal draw, at [`ONE`], from one uniform word.
///
/// The word's lowest bit is the sign and the rest is the magnitude, read by its octave: a word with
/// `j` leading zeros stands for a probability between `2⁻ʲ⁻²` and `2⁻ʲ⁻¹`, and the quantile is
/// interpolated between the two entries of [`TAIL`] that bound it. Reading a draw logarithmically
/// is what reaches six standard deviations out of thirty-four integers.
fn normal(from: u64) -> i32 {
    let sign = from & 1;
    let magnitude = (from >> 1).max(1);
    let octave = usize::try_from(magnitude.leading_zeros() - 1)
        .unwrap_or(0)
        .min(TAIL.len() - 2);
    // The bits below the leading one, as a fraction of the octave.
    let within = i64::try_from(((magnitude << (octave + 1)) >> 47) & 0xFFFF).unwrap_or(0);
    // A larger draw is a likelier probability and therefore a smaller quantile, so the octave is
    // walked from its far edge inwards.
    let far = i64::from(TAIL[octave + 1]);
    let near = i64::from(TAIL[octave]);
    let z = far - (((far - near) * within) >> 16);
    i32::try_from(if sign == 1 { -z } else { z }).unwrap_or(0)
}

/// `2^x` at `1 << 32`, for `x` at [`ONE`].
///
/// The whole part is a shift and the fraction is one step of the table with a linear interpolation
/// between its ends. Every operation is integer, so two machines cannot disagree about a world by a
/// bit.
fn exp2(x: i64) -> u128 {
    let whole = x >> 16;
    let frac = u64::try_from(x - (whole << 16)).unwrap_or(0);
    let step = usize::try_from((frac * 64) >> 16)
        .unwrap_or(0)
        .min(EXP2.len() - 2);
    let within = u128::from((frac * 64) & 0xFFFF);
    let (low, high) = (u128::from(EXP2[step]), u128::from(EXP2[step + 1]));
    let base = low + (((high - low) * within) >> 16);
    if whole >= 0 {
        base << u32::try_from(whole).unwrap_or(0)
    } else {
        base >> u32::try_from(-whole).unwrap_or(0)
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

    const WORLD: u64 = 4_242;
    const GIFTS: CollectionId = CollectionId(0);

    /// A shape solved from the measured dispersion of paid prices: for a lognormal,
    /// `p75/p25 = exp(2σ·Φ⁻¹(0.75))`, so a target of 1.40 gives `σ ≈ 0.2494`.
    const SOLVED: u32 = 16_345;

    fn worth(median: u128, spread: u128) -> Worth {
        Worth::new(
            vec![Amount::new(median)],
            SOLVED,
            Amount::new(spread),
            WORLD,
        )
        .unwrap()
    }

    fn value(worth: &Worth, who: u32, item: u32) -> u128 {
        worth
            .value(AgentId(who), GIFTS, ItemId(item))
            .unwrap()
            .unwrap()
            .base_units()
    }

    fn common(worth: &Worth, item: u32) -> u128 {
        worth.common(GIFTS, ItemId(item)).unwrap().base_units()
    }

    /// The mixing is pinned, because everything a world is made of hangs off it: a cleanup that
    /// changed a constant would change every worth in every result ever compared against another,
    /// silently and everywhere.
    #[test]
    fn the_mixing_is_the_mixing_it_has_always_been() {
        assert_eq!(mix(0), 16_294_208_416_658_607_535);
        assert_eq!(mix(1), 10_451_216_379_200_822_465);
        assert_eq!(mix(u64::MAX), 16_490_336_266_968_443_936);
        assert_eq!(
            pair(WORLD, AgentId(3), ItemId(7)),
            77_682_379_876_093_095_897_258_308_968_092_633_679
        );
    }

    /// The exponential is exact where an exponential can be, which is what makes a table of
    /// sixty-five integers a fair replacement for a library nobody can pin.
    #[test]
    fn the_exponential_is_exact_where_it_can_be() {
        assert_eq!(exp2(0), 1 << 32);
        assert_eq!(exp2(ONE), 1 << 33);
        assert_eq!(exp2(2 * ONE), 1 << 34);
        assert_eq!(exp2(-ONE), 1 << 31);
        // Half a doubling is the square root of two, to a part in ten thousand.
        assert_eq!((exp2(ONE / 2) * 10_000) >> 32, 14_142);
    }

    /// The normal draw reaches both ways and reaches far, which is the whole reason it is indexed
    /// by octave: a uniform grid would stop a little past two standard deviations and the things
    /// this models are worth a hundred times their median.
    #[test]
    fn the_normal_draw_is_symmetric_and_reaches() {
        let (mut above, mut below, mut furthest) = (0i64, 0i64, 0i32);
        for step in 0..200_000u64 {
            let z = normal(step.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            if z > 0 {
                above += 1;
            } else if z < 0 {
                below += 1;
            }
            furthest = furthest.max(z.abs());
        }
        assert!(above.abs_diff(below) * 40 < u64::try_from(above + below).unwrap());
        assert!(
            furthest > 4 * i32::try_from(ONE).unwrap(),
            "the tail stopped at {furthest}"
        );
    }

    /// The shape is what the measured dispersion asks for, in closed form rather than by search.
    /// This is the one number the world is fitted on, landing where the algebra says it should.
    #[test]
    fn the_shape_gives_the_dispersion_it_was_solved_for() {
        let worth = worth(1_000_000_000, 0);
        let mut worths: Vec<u128> = (0..20_000).map(|item| common(&worth, item)).collect();
        worths.sort_unstable();
        let dispersion = worths[15_000] * 1_000 / worths[5_000];
        assert!(
            (1_360..=1_440).contains(&dispersion),
            "p75/p25 came out at {dispersion} per thousand, and 1.40 was solved for"
        );
    }

    /// A wider shape spreads the worths further apart, monotonically, which is what makes the one
    /// fitted number a fit rather than a coincidence.
    #[test]
    fn a_wider_shape_spreads_the_worths() {
        let spread_of = |shape: u32| {
            let worth =
                Worth::new(vec![Amount::new(1_000_000_000)], shape, Amount::ZERO, WORLD).unwrap();
            let mut worths: Vec<u128> = (0..20_000).map(|item| common(&worth, item)).collect();
            worths.sort_unstable();
            worths[15_000] * 1_000 / worths[5_000]
        };
        assert!(spread_of(8_000) < spread_of(SOLVED));
        assert!(spread_of(SOLVED) < spread_of(32_000));
    }

    /// The same question has the same answer, asked twice or asked a thousand times apart. That is
    /// what lets a world of ten billion opinions exist without storing one of them.
    #[test]
    fn an_opinion_does_not_change_between_askings() {
        let worth = worth(1_000_000, 3_000);
        for item in 0..64 {
            let first = value(&worth, 5, item);
            assert_eq!(first, value(&worth, 5, item));
            assert_eq!(first, value(&worth, 5, item));
        }
    }

    /// Two worlds of one seed hold one set of opinions, and two seeds hold different ones.
    #[test]
    fn a_world_is_its_seed() {
        let one = worth(1_000_000, 3_000);
        let same = Worth::new(
            vec![Amount::new(1_000_000)],
            SOLVED,
            Amount::new(3_000),
            WORLD,
        )
        .unwrap();
        let other = Worth::new(
            vec![Amount::new(1_000_000)],
            SOLVED,
            Amount::new(3_000),
            WORLD + 1,
        )
        .unwrap();
        let read = |w: &Worth| (0..16).map(|item| value(w, 2, item)).collect::<Vec<_>>();
        assert_eq!(read(&one), read(&same));
        assert_ne!(read(&one), read(&other));
    }

    /// A valuation stays inside the band the spread allows, so it can never fall through zero and
    /// no truncation rule is needed to stop it.
    #[test]
    fn an_opinion_stays_inside_its_band() {
        let (median, spread) = (1_000_000u128, 2_500u128);
        let worth = worth(median, spread);
        for who in 0..40 {
            for item in 0..200 {
                let (seen, about) = (value(&worth, who, item), common(&worth, item));
                let band = about * spread / 10_000;
                assert!(
                    (about - band..=about + band).contains(&seen),
                    "agent {who} valued item {item} at {seen}, outside {band} of {about}"
                );
            }
        }
    }

    /// Two people disagree about the same thing, which is the entire reason there are gains from
    /// trade at all. A world where everybody agreed would have nothing to measure.
    #[test]
    fn two_people_disagree_about_one_thing() {
        let worth = worth(1_000_000, 2_000);
        let opinions: Vec<u128> = (0..50).map(|who| value(&worth, who, 0)).collect();
        assert!(
            opinions.iter().any(|seen| *seen != opinions[0]),
            "fifty people agreed exactly"
        );
    }

    /// With no spread there is no opinion, and everybody values everything at what it is worth.
    #[test]
    fn no_spread_is_no_disagreement() {
        let worth = worth(777_000, 0);
        for who in 0..10 {
            for item in 0..8 {
                assert_eq!(value(&worth, who, item), common(&worth, item));
            }
        }
    }

    /// Taste reaches as far up as down. A band used from one side only would tilt every valuation
    /// in the world the same way, and a market built on it would be measuring the tilt.
    #[test]
    fn opinion_reaches_both_ways() {
        let worth = worth(1_000_000, 5_000);
        let (mut above, mut below) = (0i64, 0i64);
        for who in 0..25 {
            for item in 0..400 {
                match value(&worth, who, item).cmp(&common(&worth, item)) {
                    std::cmp::Ordering::Greater => above += 1,
                    std::cmp::Ordering::Less => below += 1,
                    std::cmp::Ordering::Equal => {}
                }
            }
        }
        let total = above + below;
        assert!(total > 9_000, "too few opinions to say anything: {total}");
        assert!(
            above.abs_diff(below) * 25 < u64::try_from(total).unwrap(),
            "opinions leaned: {above} above, {below} below"
        );
    }

    /// A spread wider than the whole would let a valuation fall through zero, so it is refused
    /// where it is stated rather than where it shows.
    #[test]
    fn a_spread_wider_than_the_whole_is_refused() {
        assert!(Worth::new(vec![Amount::new(1)], SOLVED, Amount::new(10_001), WORLD).is_err());
    }

    /// A collection this world knows no price for has no worths in it, rather than zeroes that
    /// would read as worthless.
    #[test]
    fn a_collection_outside_the_world_has_no_worth() {
        let worth = worth(100_000, 1_000);
        assert_eq!(worth.common(CollectionId(1), ItemId(0)), None);
        assert_eq!(
            worth.value(AgentId(0), CollectionId(1), ItemId(0)).unwrap(),
            None
        );
        assert_eq!(worth.collections(), 1);
    }

    /// Two collections priced apart stay apart: the median is the level and the draw is the shape,
    /// and one collection's level never leaks into another's.
    #[test]
    fn a_collection_is_priced_at_its_own_median() {
        let worth = Worth::new(
            vec![Amount::new(1_000), Amount::new(1_000_000)],
            SOLVED,
            Amount::ZERO,
            WORLD,
        )
        .unwrap();
        let cheap: u128 = (0..500)
            .map(|at| {
                worth
                    .common(CollectionId(0), ItemId(at))
                    .unwrap()
                    .base_units()
            })
            .sum();
        let dear: u128 = (0..500)
            .map(|at| {
                worth
                    .common(CollectionId(1), ItemId(at))
                    .unwrap()
                    .base_units()
            })
            .sum();
        let ratio = dear / cheap;
        assert!(
            (900..=1_100).contains(&ratio),
            "a thousandfold median gave {ratio}"
        );
    }
}
