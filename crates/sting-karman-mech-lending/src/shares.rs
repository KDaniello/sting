//! Assets and shares, and the direction that separates them.
//!
//! A market holds its supply and its debt twice over: as an asset total that interest moves, and
//! as shares that divide it. Converting between the two is where a rounding direction decides
//! who absorbs the remainder, and the reference resolves every one of those cases against the
//! party asking (karman/docs/ARCHITECTURE.md §5, invariant 6).
//!
//! The offsets are the reference's virtual shares. They price the very first supply against a
//! fictional position rather than against an empty market, which removes both the division by
//! zero and the donation that would otherwise let the first depositor set the share price.

use sting_karman_types::{Amount, ArithError, Rounding, Unit};

/// Shares the ledger pretends already exist.
const VIRTUAL_SHARES: u128 = 1_000_000;

/// The asset those shares pretend to divide.
const VIRTUAL_ASSETS: u128 = 1;

/// Which way value is crossing a market's edge, which is what fixes the rounding.
///
/// One rule reads both ways: assets arriving are credited as few shares as the ratio allows and
/// shares arriving are charged as many assets, while assets leaving are charged as many shares
/// and shares leaving are credited as few assets. Every truncated remainder stays with the
/// market.
///
/// The direction is derived here rather than passed at each call site because the reference
/// fixes it per action, and a table stated once cannot be flipped one call at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Value entering the market: a supply, a repayment, the fee a market takes for itself.
    In,
    /// Value leaving it: a withdrawal or a borrow.
    Out,
}

impl Flow {
    /// How a conversion into shares resolves its remainder.
    const fn onto_shares(self) -> Rounding {
        match self {
            Flow::In => Rounding::Down,
            Flow::Out => Rounding::Up,
        }
    }

    /// How a conversion into assets resolves its remainder.
    const fn onto_assets(self) -> Rounding {
        match self {
            Flow::In => Rounding::Up,
            Flow::Out => Rounding::Down,
        }
    }
}

/// The shares `assets` is worth against a market holding `total_assets` behind `total_shares`.
///
/// The reference's `toSharesDown` and `toSharesUp`, which differ only in the direction `flow`
/// decides.
pub(crate) fn to_shares<A: Unit, S: Unit>(
    assets: Amount<A>,
    total_assets: Amount<A>,
    total_shares: Amount<S>,
    flow: Flow,
) -> Result<Amount<S>, ArithError> {
    assets.convert(
        offset(total_shares, VIRTUAL_SHARES)?,
        offset(total_assets, VIRTUAL_ASSETS)?,
        flow.onto_shares(),
    )
}

/// The assets `shares` is worth against the same market.
///
/// The reference's `toAssetsDown` and `toAssetsUp`.
pub(crate) fn to_assets<A: Unit, S: Unit>(
    shares: Amount<S>,
    total_assets: Amount<A>,
    total_shares: Amount<S>,
    flow: Flow,
) -> Result<Amount<A>, ArithError> {
    shares.convert(
        offset(total_assets, VIRTUAL_ASSETS)?,
        offset(total_shares, VIRTUAL_SHARES)?,
        flow.onto_assets(),
    )
}

/// A total as the conversion sees it, with the virtual quantity added.
fn offset<U: Unit>(total: Amount<U>, virtual_units: u128) -> Result<Amount<U>, ArithError> {
    total.checked_add(Amount::new(virtual_units))
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
    use crate::{Loan, SupplyShare};

    type Assets = Amount<Loan>;
    type Shares = Amount<SupplyShare>;

    /// The share price a market opens at is the virtual pair's, so a first supply is priced
    /// against something rather than against nothing.
    #[test]
    fn the_first_supply_is_priced_against_the_virtual_pair() {
        let shares = to_shares(Assets::new(1_000), Assets::ZERO, Shares::ZERO, Flow::In).unwrap();
        assert_eq!(shares, Shares::new(1_000_000_000));
    }

    /// A market whose assets have grown past its shares prices later supplies against the growth:
    /// interest belongs to the holders who were there when it accrued.
    #[test]
    fn interest_already_earned_is_not_shared_with_a_later_supplier() {
        // Two thousand assets behind the shares a thousand assets originally bought.
        let total_assets = Assets::new(2_000);
        let total_shares = Shares::new(1_000_000_000);
        let shares = to_shares(Assets::new(1_000), total_assets, total_shares, Flow::In).unwrap();
        assert!(
            shares < Shares::new(1_000_000_000),
            "a later supplier bought in at the opening price"
        );
        assert_eq!(shares, Shares::new(500_249_875));
    }

    /// The eight cases of the reference's table are these two rows read twice, and this is the
    /// row that separates them: one direction rounds up where the other rounds down.
    #[test]
    fn the_two_flows_are_the_ceiling_and_the_floor_of_one_ratio() {
        let (total_assets, total_shares) = (Assets::new(2), Shares::ZERO);
        let entering = to_shares(Assets::new(1), total_assets, total_shares, Flow::In).unwrap();
        let leaving = to_shares(Assets::new(1), total_assets, total_shares, Flow::Out).unwrap();
        assert_eq!(entering, Shares::new(333_333));
        assert_eq!(leaving, Shares::new(333_334));
    }

    /// Both totals at full width leave no room for the virtual offset, and the conversion says
    /// so rather than wrapping into a share count nobody holds.
    #[test]
    fn a_total_with_no_room_for_the_offset_is_refused() {
        let result = to_shares(
            Assets::new(1),
            Assets::new(u128::MAX),
            Shares::ZERO,
            Flow::In,
        );
        assert_eq!(result, Err(ArithError::Overflow));
    }

    /// Totals small enough that the exact products stay inside 128 bits, so the properties can
    /// be checked against arithmetic written out rather than against the code under test.
    fn market() -> impl Strategy<Value = (u128, u128, u128)> {
        (0u128..(1 << 50), 0u128..(1 << 50), 1u128..(1 << 40))
    }

    proptest! {
        /// The remainder stays with the market, in all four of the cases the reference's table
        /// spells out: assets entering buy as few shares as the ratio allows and shares entering
        /// settle as many assets, while assets leaving cost as many shares and shares leaving
        /// fetch as few assets.
        #[test]
        fn a_conversion_never_favours_the_party_asking(
            (total_assets, total_shares, amount) in market(),
        ) {
            let (a, s) = (Assets::new(total_assets), Shares::new(total_shares));
            let (with_shares, with_assets) =
                (total_shares + VIRTUAL_SHARES, total_assets + VIRTUAL_ASSETS);

            let onto_shares =
                |flow| to_shares(Assets::new(amount), a, s, flow).unwrap().base_units();
            prop_assert!(onto_shares(Flow::In) * with_assets <= amount * with_shares);
            prop_assert!(onto_shares(Flow::Out) * with_assets >= amount * with_shares);

            let onto_assets =
                |flow| to_assets(Shares::new(amount), a, s, flow).unwrap().base_units();
            prop_assert!(onto_assets(Flow::In) * with_shares >= amount * with_assets);
            prop_assert!(onto_assets(Flow::Out) * with_shares <= amount * with_assets);
        }

        /// The two directions are one ratio resolved two ways, so they never differ by more than
        /// the remainder that separates them.
        #[test]
        fn the_two_directions_differ_by_at_most_one(
            (total_assets, total_shares, shares) in market(),
        ) {
            let (a, s) = (Assets::new(total_assets), Shares::new(total_shares));
            let entering = to_assets(Shares::new(shares), a, s, Flow::In).unwrap();
            let leaving = to_assets(Shares::new(shares), a, s, Flow::Out).unwrap();
            prop_assert!(entering >= leaving);
            prop_assert!(entering.base_units() - leaving.base_units() <= 1);
        }

        /// Converting one way and back cannot manufacture value: the shares a supply buys, sold
        /// back at the same ratio, never return more assets than went in. This is the property a
        /// flipped row of the rounding table breaks, and it breaks it in one direction only.
        #[test]
        fn a_round_trip_through_shares_never_returns_more(
            (total_assets, total_shares, assets) in market(),
        ) {
            let (a, s) = (Assets::new(total_assets), Shares::new(total_shares));
            let bought = to_shares(Assets::new(assets), a, s, Flow::In).unwrap();
            let returned = to_assets(bought, a, s, Flow::Out).unwrap();
            prop_assert!(returned.base_units() <= assets, "a round trip through shares paid out a profit");
        }
    }
}
