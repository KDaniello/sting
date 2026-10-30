//! How closely two columns agree about the order of things.
//!
//! Spearman's coefficient, and it is here rather than in a mechanism because it is a statistic of
//! two columns and knows nothing about what they hold. What it exists for is the question a
//! showcase is built to answer — do the prices a market declares put things in the order their
//! worth does — and it answers that question for any two columns that come in pairs.
//!
//! **Rank rather than value**, because the question is about order and not about scale: a market
//! that priced everything at a tenth of its worth would have ranked it perfectly, and a coefficient
//! that punished it for the tenth would be measuring the wrong thing.
//!
//! **Integer throughout, and exact.** Ranks are carried doubled, so the average of two tied places
//! is a whole number rather than a half; the sums are 128-bit, which the widths below show cannot
//! overflow; and the square root is an integer square root. Two machines therefore cannot disagree
//! about a coefficient, which is what lets one appear in a report at all.

/// One whole, in the basis points a coefficient is reported in.
const WHOLE: i128 = 10_000;

/// Spearman's rank correlation between two columns, in basis points from `-10000` to `10000`.
///
/// Absent where there is nothing to correlate: fewer than two pairs, columns of different lengths,
/// or a column whose values are all the same. That last one is a real answer and not a failure —
/// a column with no order in it cannot agree or disagree with another about order, and a zero
/// there would read as "no relation" when the truth is "no question".
///
/// # Widths
///
/// The ranks are centred before anything is squared, and that is what makes the bound usable
/// rather than decorative. Doubled ranks over `n` rows always sum to `n(n+1)`, so their mean is
/// exactly `n+1` and subtracting it leaves whole numbers again — no rounding is introduced and
/// nothing is approximated. Centred, each rank is at most `n`, so each column's spread `Σc²`
/// reaches about `n³/3` and the product of two of them about `n⁶/9`, which stays inside 128 bits
/// for any `n` below about **three and a half million** rows.
///
/// Uncentred it would be otherwise: `n·Σr² − (Σr)²` reaches about `4n⁴/3` and the product of two
/// of those about `16n⁸/9`, which leaves 128 bits at around **sixty thousand** rows. A showcase
/// generated from the measured market carries more listings than that, so the uncentred form
/// returned no coefficient at all exactly where one was wanted, and returned it silently.
#[must_use]
pub fn rank_correlation(xs: &[i128], ys: &[i128]) -> Option<i128> {
    if xs.len() != ys.len() || xs.len() < 2 {
        return None;
    }
    let n = i128::try_from(xs.len()).ok()?;
    let (rx, ry) = (ranked(xs), ranked(ys));

    // Doubled ranks over n rows sum to n(n+1) whatever the ties, so their mean is exactly n+1 and
    // centring them leaves whole numbers. It is the same coefficient either way; what it buys is
    // two orders of magnitude of headroom, because nothing is multiplied by n before it is squared.
    let middle = n + 1;
    let centre = |of: Vec<i128>| of.into_iter().map(|at| at - middle).collect::<Vec<_>>();
    let (cx, cy) = (centre(rx), centre(ry));

    let square = |of: &[i128]| of.iter().map(|at| at * at).sum::<i128>();
    let (spread_x, spread_y) = (square(&cx), square(&cy));
    if spread_x == 0 || spread_y == 0 {
        return None;
    }
    let over: i128 = cx.iter().zip(&cy).map(|(x, y)| x * y).sum();

    let below = root(
        u128::try_from(spread_x)
            .ok()?
            .checked_mul(u128::try_from(spread_y).ok()?)?,
    );
    let below = i128::try_from(below).ok()?;
    if below == 0 {
        return None;
    }
    Some((over * WHOLE / below).clamp(-WHOLE, WHOLE))
}

/// The places a column's values occupy, doubled.
///
/// Doubled so that a tie — where the places are shared and the rank is their average — stays a
/// whole number: two values sharing places three and four rank at seven halves, which is seven of
/// these. Nothing downstream cares about the factor, because a correlation is scale-free in each
/// column separately.
fn ranked(of: &[i128]) -> Vec<i128> {
    let mut order: Vec<usize> = (0..of.len()).collect();
    order.sort_by_key(|at| of[*at]);
    let mut ranks = vec![0i128; of.len()];
    let mut from = 0usize;
    while from < order.len() {
        let mut to = from + 1;
        while to < order.len() && of[order[to]] == of[order[from]] {
            to += 1;
        }
        // Places `from..to` are shared, so each of them takes the sum of the two ends: the doubled
        // average of a run of consecutive integers, with no remainder to lose.
        let shared = i128::try_from(from + to).unwrap_or(i128::MAX) + 1;
        for at in &order[from..to] {
            ranks[*at] = shared;
        }
        from = to;
    }
    ranks
}

/// The integer square root: the largest `r` with `r² ≤ of`.
///
/// By halving rather than by a floating-point guess, because a guess would be the one place a
/// coefficient could come out differently on two machines.
fn root(of: u128) -> u128 {
    if of < 2 {
        return of;
    }
    let mut low = 0u128;
    let mut high = (1u128 << 64).min(of);
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if mid.checked_mul(mid).is_some_and(|square| square <= of) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    low
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

    fn bps(xs: &[i128], ys: &[i128]) -> Option<i128> {
        rank_correlation(xs, ys)
    }

    /// Two columns that order things the same way agree completely, whatever the scale between
    /// them: the question is about order, and a market that priced everything at a tenth of its
    /// worth got the order exactly right.
    #[test]
    fn one_order_told_twice_agrees_completely() {
        assert_eq!(bps(&[1, 2, 3, 4, 5], &[10, 20, 30, 40, 50]), Some(10_000));
        assert_eq!(
            bps(&[1, 2, 3, 4, 5], &[3, 7, 9, 400, 100_000]),
            Some(10_000)
        );
    }

    /// And one told backwards disagrees completely.
    #[test]
    fn one_order_told_backwards_disagrees_completely() {
        assert_eq!(bps(&[1, 2, 3, 4, 5], &[50, 40, 30, 20, 10]), Some(-10_000));
    }

    /// A column that says nothing about order — every value the same — has no coefficient, rather
    /// than a zero that would read as "no relation" when the truth is that there was no question.
    #[test]
    fn a_column_with_no_order_has_no_coefficient() {
        assert_eq!(bps(&[7, 7, 7, 7], &[1, 2, 3, 4]), None);
        assert_eq!(bps(&[1, 2, 3, 4], &[7, 7, 7, 7]), None);
    }

    /// Ties take the place they share, so a coefficient does not depend on which of two equal
    /// values the sort happened to put first.
    #[test]
    fn ties_share_their_place() {
        assert_eq!(ranked(&[5, 5, 9]), vec![3, 3, 6]);
        assert_eq!(ranked(&[9, 5, 5]), vec![6, 3, 3]);
        // Reordering equal values leaves the coefficient where it was.
        assert_eq!(
            bps(&[5, 5, 9, 12], &[1, 2, 3, 4]),
            bps(&[5, 5, 9, 12], &[2, 1, 3, 4])
        );
    }

    /// Two middling cases against the textbook form, `1 − 6·Σd² / n(n²−1)`, worked by hand.
    ///
    /// One swap at the bottom of four gives `Σd² = 2`, so `1 − 12/60 = 0.8`. Swapping the two
    /// ends instead gives `Σd² = 18`, so `1 − 108/60 = −0.8`. A coefficient that drifted from
    /// either would have changed what every report saying `convergence` means.
    #[test]
    fn a_middling_agreement_is_what_the_textbook_says() {
        assert_eq!(bps(&[1, 2, 3, 4], &[2, 1, 3, 4]), Some(8_000));
        assert_eq!(bps(&[1, 2, 3, 4], &[4, 2, 3, 1]), Some(-8_000));
    }

    /// Nothing to correlate is no coefficient, and mismatched columns are a question nobody asked.
    #[test]
    fn too_little_to_say_says_nothing() {
        assert_eq!(bps(&[], &[]), None);
        assert_eq!(bps(&[1], &[1]), None);
        assert_eq!(bps(&[1, 2, 3], &[1, 2]), None);
    }

    /// The square root is the integer one, exactly, at the edges where a guess would slip.
    #[test]
    fn the_root_is_the_integer_root() {
        assert_eq!(root(0), 0);
        assert_eq!(root(1), 1);
        assert_eq!(root(15), 3);
        assert_eq!(root(16), 4);
        assert_eq!(root(17), 4);
        assert_eq!(
            root(u128::from(u64::MAX) * u128::from(u64::MAX)),
            u128::from(u64::MAX)
        );
    }

    /// A column of twenty thousand, which is the size a world of this workspace is generated at,
    /// stays inside the widths and comes out where it should.
    #[test]
    fn a_world_sized_column_stays_inside_its_widths() {
        let xs: Vec<i128> = (0..20_000).collect();
        let ys: Vec<i128> = (0..20_000).map(|at| at * 3 + 1).collect();
        assert_eq!(bps(&xs, &ys), Some(10_000));
        let backwards: Vec<i128> = (0..20_000).rev().collect();
        assert_eq!(bps(&xs, &backwards), Some(-10_000));
    }

    /// A market the size of the one this workspace generates, and one ten times larger again.
    ///
    /// The uncentred form returned nothing at all here: two column spreads of about four thirds of
    /// `n` to the fourth multiply past 128 bits at around sixty thousand rows, and the coefficient
    /// came back absent rather than wrong — which is worse, because a run reports absent as zero
    /// and a gate stated in this number would have read a perfect disagreement.
    #[test]
    fn a_market_sized_column_still_has_a_coefficient() {
        for rows in [100_000i128, 1_000_000] {
            let xs: Vec<i128> = (0..rows).collect();
            let ys: Vec<i128> = (0..rows).map(|at| at * 7 % rows).collect();
            assert!(
                rank_correlation(&xs, &xs).is_some(),
                "{rows} rows agreeing perfectly produced no coefficient"
            );
            assert_eq!(rank_correlation(&xs, &xs), Some(10_000));
            let reversed: Vec<i128> = xs.iter().rev().copied().collect();
            assert_eq!(rank_correlation(&xs, &reversed), Some(-10_000));
            assert!(rank_correlation(&xs, &ys).is_some());
        }
    }
}
