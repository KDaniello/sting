//! The boundary this mechanism has, against the boundary algebra says it has.
//!
//! Two prices decide the fate of one position, and both are closed forms. A position stops being
//! covered at `p = D·λ⁻¹/C`, and the whole of its collateral stops being able to repay its debt at
//! `p = D·LIF/C`. Their ratio is `LIF·λ` and nothing else — neither size cancels, both do — so
//! from the moment a position becomes liquidatable the price may fall a further `1 − LIF·λ` before
//! the market can be left with a loss.
//!
//! That band is the whole reason a liquidation ladder holds. Inside it, no seizure of any size can
//! produce bad debt, because taking enough collateral to repay the debt always leaves some behind.
//! Below it, taking everything leaves a debt nobody is left to owe.
//!
//! This is a stronger check than any statistical one and it needs no data: the answer is known in
//! advance, so the error is measurable to the last representable price. Written against the public
//! surface and against the formula, never against the code that implements it — the two arrive at
//! the same number by different expressions or the mechanism is wrong.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use sting_karman_core::{Mechanism, PathSource, SourceError};
use sting_karman_mech_lending::{
    Collateral, FixedRate, Given, Lending, LendingAction, LendingEvent, LendingState, Loan, Market,
    MarketParams, ORACLE_PRICE_SCALE, Position, Price, WAD,
};
use sting_karman_types::{AgentId, Amount, MarketId, Rounding, SecondsPerTick, Tick, Unit};

/// The thresholds Morpho has enabled. Every market in existence is listed at one of these.
const ENABLED: [u128; 8] = [
    385_000_000_000_000_000,
    625_000_000_000_000_000,
    770_000_000_000_000_000,
    860_000_000_000_000_000,
    915_000_000_000_000_000,
    945_000_000_000_000_000,
    965_000_000_000_000_000,
    980_000_000_000_000_000,
];

const MARKET: MarketId = MarketId(0);
const LENDER: AgentId = AgentId(0);
const BORROWER: AgentId = AgentId(1);
const LIQUIDATOR: AgentId = AgentId(2);

/// A plain number, for the arithmetic this test does on its own account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Scalar;

impl Unit for Scalar {
    const SYMBOL: &'static str = "";
}

/// `a · b / c`, at the width the workspace carries a product in.
fn ratio(a: u128, b: u128, c: u128) -> u128 {
    Amount::<Scalar>::new(a)
        .mul_div(
            Amount::<Scalar>::new(b),
            Amount::<Scalar>::new(c),
            Rounding::Down,
        )
        .expect("the test's own arithmetic left the width money is carried in")
        .base_units()
}

/// `min(1.15, 1 / (1 − 0.3·(1 − λ)))`, written out here rather than asked of the crate.
fn incentive(lltv: u128) -> u128 {
    let discount = ratio(
        300_000_000_000_000_000,
        WAD.base_units() - lltv,
        WAD.base_units(),
    );
    ratio(
        WAD.base_units(),
        WAD.base_units(),
        WAD.base_units() - discount,
    )
    .min(1_150_000_000_000_000_000)
}

/// The price at which a position stops being covered: `C·p·λ = D`.
fn liquidation_price(collateral: u128, debt: u128, lltv: u128) -> u128 {
    ratio(
        ratio(debt, ORACLE_PRICE_SCALE, collateral),
        WAD.base_units(),
        lltv,
    )
}

/// The price below which the whole collateral can no longer repay the debt: `C·p/LIF = D`.
fn insolvency_price(collateral: u128, debt: u128, lltv: u128) -> u128 {
    ratio(
        ratio(debt, ORACLE_PRICE_SCALE, collateral),
        incentive(lltv),
        WAD.base_units(),
    )
}

#[derive(Debug, Clone, Copy)]
struct Quoted(u128);

impl PathSource<Price<Collateral, Loan>> for Quoted {
    fn value_at(&self, _now: Tick) -> Result<Price<Collateral, Loan>, SourceError> {
        Ok(Price::new(self.0))
    }
}

/// One market, one borrower at `(collateral, debt)`, one lender holding the other side.
///
/// The rate is zero and the clock never moves, so nothing accrues: what this measures is the
/// liquidation arithmetic and not the interest around it.
fn market(
    lltv: u128,
    collateral: u128,
    debt: u128,
    price: u128,
) -> (Lending<Quoted, FixedRate>, LendingState) {
    let supplied = debt * 4;
    let mechanism = Lending::new(SecondsPerTick(std::num::NonZeroU64::new(1).unwrap()))
        .with_market(
            MARKET,
            MarketParams {
                lltv: Amount::new(lltv),
                fee_recipient: LENDER,
            },
            Quoted(price),
            FixedRate(Amount::ZERO),
        )
        .unwrap();
    let opened = Market {
        total_supply_assets: Amount::new(supplied),
        total_supply_shares: Amount::new(supplied * 1_000_000),
        total_borrow_assets: Amount::new(debt),
        total_borrow_shares: Amount::new(debt * 1_000_000),
        last_update: Tick(0),
        fee: Amount::ZERO,
    };
    let state = LendingState::new()
        .with_market(MARKET, opened)
        .unwrap()
        .with_position(
            MARKET,
            LENDER,
            Position {
                supply_shares: Amount::new(supplied * 1_000_000),
                borrow_shares: Amount::ZERO,
                collateral: Amount::ZERO,
            },
        )
        .unwrap()
        .with_position(
            MARKET,
            BORROWER,
            Position {
                supply_shares: Amount::ZERO,
                borrow_shares: Amount::new(debt * 1_000_000),
                collateral: Amount::new(collateral),
            },
        )
        .unwrap();
    (mechanism, state)
}

/// What a seizure of `seized` collateral did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Took {
    /// The mechanism refused it.
    Refused,
    /// It went through and the market kept its books whole.
    Whole,
    /// It went through and left this much debt with nobody to owe it.
    Loss(u128),
}

fn seize(lltv: u128, collateral: u128, debt: u128, price: u128, seized: u128) -> Took {
    let (mechanism, mut state) = market(lltv, collateral, debt, price);
    let mut events = Vec::new();
    let action = LendingAction::Liquidate {
        market: MARKET,
        borrower: BORROWER,
        given: Given::Assets(Amount::new(seized)),
    };
    if mechanism
        .apply(&mut state, LIQUIDATOR, action, Tick(0), &mut events)
        .is_err()
    {
        return Took::Refused;
    }
    for event in &events {
        if let LendingEvent::Liquidated { bad_debt, .. } = event
            && !bad_debt.is_zero()
        {
            return Took::Loss(bad_debt.base_units());
        }
    }
    Took::Whole
}

/// One WETH against a thousand of an asset with six decimals: the shape of a real market, and
/// small enough that every price these formulas produce stays inside the width.
const COLLATERAL: u128 = 1_000_000_000_000_000_000;
const DEBT: u128 = 1_000_000_000;

/// Above the liquidation price nothing may be taken, whatever is offered for it.
#[test]
fn a_covered_position_cannot_be_liquidated_at_any_size() {
    for lltv in ENABLED {
        let covered = liquidation_price(COLLATERAL, DEBT, lltv) + ORACLE_PRICE_SCALE / 1_000;
        for part in [1, 2, 3, 10, 1_000] {
            assert_eq!(
                seize(lltv, COLLATERAL, DEBT, covered, COLLATERAL / part),
                Took::Refused,
                "a covered position was liquidated at threshold {lltv}"
            );
        }
    }
}

/// The band the ladder lives in. Between the two prices a position is liquidatable and no seizure
/// of any size can leave a loss, because taking enough to repay the debt always leaves collateral
/// behind. This is the claim the whole finding rests on, and it is checked at every listed
/// threshold and across the whole band rather than at a point.
#[test]
fn no_seizure_inside_the_band_can_leave_a_loss() {
    for lltv in ENABLED {
        let liquidatable = liquidation_price(COLLATERAL, DEBT, lltv);
        let insolvent = insolvency_price(COLLATERAL, DEBT, lltv);
        assert!(
            insolvent < liquidatable,
            "the band is empty at threshold {lltv}"
        );

        let span = liquidatable - insolvent;
        for step in 1..=16u128 {
            let price = insolvent + span * step / 17;
            for part in [1, 2, 3, 5, 10, 100, 10_000] {
                let took = seize(lltv, COLLATERAL, DEBT, price, COLLATERAL / part);
                assert!(
                    !matches!(took, Took::Loss(_)),
                    "a seizure inside the band left a loss at threshold {lltv}, price {price}: \
                     {took:?}"
                );
            }
        }
    }
}

/// Below the band, taking everything leaves a debt with nobody to owe it — and the size of it is
/// what the algebra says: the debt, less what the collateral could repay at the incentive.
#[test]
fn below_the_band_taking_everything_leaves_the_loss_the_formula_names() {
    for lltv in ENABLED {
        let insolvent = insolvency_price(COLLATERAL, DEBT, lltv);
        for tenth in 1..=9u128 {
            let price = insolvent * tenth / 10;
            let Took::Loss(measured) = seize(lltv, COLLATERAL, DEBT, price, COLLATERAL) else {
                panic!("no loss below the band at threshold {lltv}, price {price}");
            };
            // The collateral, valued and discounted by the incentive, is what it repays.
            let repaid = ratio(
                ratio(COLLATERAL, price, ORACLE_PRICE_SCALE),
                WAD.base_units(),
                incentive(lltv),
            );
            let expected = DEBT - repaid;
            let apart = measured.abs_diff(expected);
            assert!(
                apart <= 2,
                "at threshold {lltv} the loss is {measured} where the formula says {expected}"
            );
        }
    }
}

/// The boundary itself, found by asking the mechanism and compared with the closed form.
///
/// Bisection over the price for the largest one at which taking everything still leaves a loss.
/// The two answers are reached by different arithmetic — the mechanism through a valuation, a
/// division by the incentive and two share conversions, this through one ratio — so agreeing to a
/// handful of the smallest representable price is the strongest statement available about a
/// boundary: it is not approximately where the algebra puts it, it is there.
#[test]
fn the_boundary_the_mechanism_has_is_the_boundary_the_formula_names() {
    for lltv in ENABLED {
        let expected = insolvency_price(COLLATERAL, DEBT, lltv);
        let (mut lost, mut whole) = (expected / 2, expected * 2);
        assert!(matches!(
            seize(lltv, COLLATERAL, DEBT, lost, COLLATERAL),
            Took::Loss(_)
        ));
        assert!(!matches!(
            seize(lltv, COLLATERAL, DEBT, whole, COLLATERAL),
            Took::Loss(_)
        ));

        while whole - lost > 1 {
            let middle = lost + (whole - lost) / 2;
            if matches!(
                seize(lltv, COLLATERAL, DEBT, middle, COLLATERAL),
                Took::Loss(_)
            ) {
                lost = middle;
            } else {
                whole = middle;
            }
        }
        // The tolerance is derived rather than chosen: the mechanism values the collateral,
        // divides by the incentive and converts to shares, and each of those rounds against
        // the liquidator by up to a unit of debt. Three units of debt, expressed as the price
        // they sit at, is the width the answer can honestly be off by.
        let a_unit_of_debt = insolvency_price(COLLATERAL, 1, lltv);
        let apart = lost.abs_diff(expected);
        assert!(
            apart <= a_unit_of_debt * 3,
            "at threshold {lltv} the mechanism turns at {lost} and the formula says {expected}, \
             {apart} apart"
        );
    }
}

/// The buffer a market carries, as one number per threshold: how far the price may fall below the
/// point a position became liquidatable before a loss becomes possible.
///
/// It depends on nothing but the threshold — not the size of the position, not the market, not the
/// population — which is why a finding about it survives every assumption this workspace makes
/// about behaviour.
#[test]
fn the_buffer_is_one_less_the_incentive_times_the_threshold() {
    let mut measured = Vec::new();
    for lltv in ENABLED {
        let liquidatable = liquidation_price(COLLATERAL, DEBT, lltv);
        let insolvent = insolvency_price(COLLATERAL, DEBT, lltv);
        // The ratio of the two prices, in hundredths of a percent.
        let fallen = 1_000_000 - ratio(insolvent, 1_000_000, liquidatable);
        let algebra = 1_000_000
            - ratio(
                ratio(incentive(lltv), lltv, WAD.base_units()),
                1_000_000,
                WAD.base_units(),
            );
        assert!(
            fallen.abs_diff(algebra) <= 1,
            "the buffer at {lltv} measures {fallen} and the algebra says {algebra}"
        );
        measured.push((lltv, fallen));
    }

    // Monotone, and it has to be: a threshold closer to the collateral's full value leaves less
    // room above the debt to pay an incentive out of.
    for pair in measured.windows(2) {
        assert!(
            pair[1].1 < pair[0].1,
            "the buffer did not shrink from {:?} to {:?}",
            pair[0],
            pair[1]
        );
    }
    // The whole table, spelled out, so a reader can check it without running anything. Read as
    // hundredths of a percent: a market listed at 86% carries 10.23% of room below the price a
    // position became liquidatable at, and one listed at 98% carries 1.41%.
    assert_eq!(
        measured,
        vec![
            (385_000_000_000_000_000, 557_250),
            (625_000_000_000_000_000, 295_775),
            (770_000_000_000_000_000, 172_933),
            (860_000_000_000_000_000, 102_297),
            (915_000_000_000_000_000, 61_057),
            (945_000_000_000_000_000, 39_146),
            (965_000_000_000_000_000, 24_760),
            (980_000_000_000_000_000, 14_085),
        ]
    );
}
