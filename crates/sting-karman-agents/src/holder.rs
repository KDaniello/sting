//! Somebody who owns things, and decides what to say they are worth.
//!
//! One behaviour and not two. A collector is not a different rule from an ordinary holder — it is
//! the same rule with a stronger attachment to what it has, and writing it twice would be two
//! copies of one decision drifting apart the first time either was touched. What separates them is
//! [`Holder::devotion`] and [`Holder::patience`], and [`Holder::collector`] is the pair of them
//! that makes one.

use std::sync::Arc;

use sting_karman_core::{Agent, RunRng};
use sting_karman_mech_showcase::{
    Bps, Cash, ItemId, Showcase, ShowcaseAction, ShowcaseState, WHOLE, Worth,
};
use sting_karman_types::{AgentId, Amount, Rounding, Tick};

/// A holder, and what it declares its things are worth.
///
/// **What it does.** It puts what it holds on the showcase, if it is the sort that would ever sell,
/// and it takes anything it finds priced below what that thing is worth to it. It does not settle a
/// charge of its own accord: the charge is lazy and capped, it comes out of the price at a buyout,
/// and a holder who never touches an item is a holder the mechanism reaches through the sale rather
/// than through the calendar. That is not laziness in the behaviour — it is what the mechanism does
/// to somebody who ignores it, and a population that settled eagerly would hide it.
///
/// **What it declares.** Its own valuation, marked up. The markup is where sixty years of theory
/// enter a population:
///
/// > Weyl and Zhang, *Depreciating Licenses*, Theorem 1 — where the charge equals the probability
/// > of selling, a holder declares its true valuation; below that it declares above, and above it
/// > declares below.
///
/// So neither number a seller holds is a taste of the modeller's. [`Holder::floor`] is what it
/// will not go below — its own valuation until the charge passes that rate, and under it after —
/// and [`Holder::retain`] is how fast it gets there, which is its own patience scaled by how much
/// of that patience the charge leaves alive. At the rate where truth-telling is optimal none of it
/// survives: every seller lands on its own valuation the first time it looks, they all stand at the
/// same multiple of what they think a thing is worth, and what is left between their prices is only
/// what they disagree about. Below that rate the markups differ and the order is noisy; above it
/// the floor goes under a valuation, which is why the curve a sweep draws has an interior best rate
/// on it rather than only rising — the whole question the experiment asks.
///
/// **Calibration.** `patience` is fitted, and against one thing:
/// the gap between what this market asks and what it pays, measured at 3.7 times in the median
/// collection. `truthful_at` is a measurement rather than a fit — it is the observed turnover, by
/// the theorem above. `attention` is neither: it says what one tick means, and the scenario's tick
/// count carries the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// What this world is worth, and what this holder in particular thinks of it.
    ///
    /// One table between all of them. It is the same table for everybody — the levels a census
    /// measured, and the two fitted numbers a world was built with — and what makes one holder's
    /// opinion its own is the seat it reads that table from, not a copy of it. A population of a
    /// quarter of a million each holding a copy is a copy of the market per participant, which
    /// measured at thirty megabytes a run and made a sweep memory-bound long before it was
    /// arithmetic-bound.
    pub worth: Arc<Worth>,
    /// The items it believes it holds. Kept because finding them in a market of a hundred thousand
    /// would cost a scan of the whole market every tick, and a participant knows what it owns.
    pub mine: Vec<ItemId>,
    /// How much more than everybody this one wants what it has, in basis points.
    ///
    /// Zero for an ordinary holder. A collector is somebody for whom the same thing is simply
    /// worth more, and that is a fact about the person rather than about the thing.
    pub devotion: Amount<Bps>,
    /// What it declares above its own valuation when it first puts something up, in basis points.
    ///
    /// The same for everybody, because what a market opens at is a convention of the market and
    /// what separates one seller from another is how long they hold to it. See [`Holder::retain`].
    pub opening: u32,
    /// What it will not go below, in basis points, signed. See [`Holder::floor`].
    ///
    /// Zero where nothing is charged: a seller with no carrying cost will come down to what the
    /// thing is worth to it and no further, because below that it would rather keep the thing.
    /// Negative under a charge, because a thing that costs something to hold is worth shedding
    /// below one's own valuation, and how far below is what the charge decides.
    pub floor: i32,
    /// What share of its standing markup it keeps each time it looks at an unsold listing.
    ///
    /// **This is what patience is.** A market where nobody ever changed a declared price is not a
    /// model of this mechanism — the mechanism has a reprice, and lowering binds at once while
    /// raising waits out the lag, which is the whole of its asymmetry. A population that never
    /// repriced could not produce what was measured: a standing inventory asking three and seven
    /// tenths of what the same market pays, while better than a third of that inventory turns over
    /// in a quarter. Asking high and selling often are the same sellers at different points of the
    /// same walk down, and a fixed markup can only be one or the other.
    pub retain: u32,
    /// Whether it would sell at any price at all.
    ///
    /// The share of a population for which this is true is the third of the three fitted numbers,
    /// and it is what decides how much of a market is reachable. Measured against the share of
    /// items carrying a price: six and seven tenths per cent of them.
    pub sells: bool,
    /// How many items it looks at in one tick.
    ///
    /// Not a fit. It says what a tick is, and how many ticks make a quarter is the scenario's to
    /// say; a population that looked at twice as much half as often would be the same population.
    pub attention: u32,
    /// Where in its own holdings it last looked.
    pub at: u32,
}

impl Holder {
    /// An ordinary holder: no attachment beyond the common view, and it would sell.
    #[must_use]
    pub fn new(worth: Arc<Worth>, mine: Vec<ItemId>, opening: u32, sells: bool) -> Self {
        Holder {
            worth,
            mine,
            devotion: Amount::ZERO,
            opening,
            floor: 0,
            retain: 0,
            sells,
            attention: 1,
            at: 0,
        }
    }

    /// A collector: the same rule, wanting what it holds more than anybody and letting go harder.
    ///
    /// It is on the showcase like everybody, because the question §12.7 of the manifesto asks about
    /// collectors is whether the charge drives out the people the mechanism is for — and one that
    /// stayed off the showcase would answer a different question, about whether they can hide.
    #[must_use]
    pub fn collector(
        worth: Arc<Worth>,
        mine: Vec<ItemId>,
        devotion: Amount<Bps>,
        opening: u32,
    ) -> Self {
        Holder {
            devotion,
            ..Holder::new(worth, mine, opening, true)
        }
    }

    /// One holder's own patience, drawn about the median a scenario states.
    ///
    /// **Not one number shared by a population**, and the difference is the whole behaviour of a
    /// market rather than a detail of it. Where every holder marks up alike, two things follow and
    /// both are wrong: nothing ever trades, because a buyer differs from a seller only by taste and
    /// no taste covers a markup everybody has applied; and a declared price is a fixed multiple of
    /// a worth, so it puts things in exactly the order their worth does and a market with no charge
    /// on it appears to have solved the problem the charge exists for.
    ///
    /// What a market actually looks like is a few holders who would let a thing go and many who
    /// would not, and the measured gap between what this market asks and what it pays — 3.7 times
    /// in the median collection — is the gap between those two crowds rather than a markup anybody
    /// applies. So the median is stated and the spread is uniform about it: half of a population is
    /// more patient than the number a scenario gives and half is less, and the impatient half is
    /// what makes a market.
    ///
    /// Drawn from the run's own seeded stream, keyed by the seat, so a population is reproducible
    /// from a seed and one holder's patience does not depend on how many were built before it.
    #[must_use]
    pub fn patience_of(seed: u64, seat: u32, median_bps: u32) -> u32 {
        let mut rng = RunRng::new(seed ^ PATIENCE, u64::from(seat));
        let across = u64::from(median_bps).saturating_mul(2).saturating_add(1);
        let drawn = u32::try_from(rng.below(across)).unwrap_or(median_bps);
        // A step off the standing markup and not a share of it kept, because a share kept makes a
        // population of two kinds and nothing between: at a share of nine tenths a seller reaches
        // what the thing is worth in five looks, and at a whole it never moves at all. A market
        // has every speed in it. Stated as a step, the middle of the draw sets how long the middle
        // seller takes to come down, and the ones drawn near zero are the listings that stand for
        // a whole quarter — the same distribution, read the way a seller would state it.
        // A share of a markup, so never a whole one: a seller keeping all of it never comes down
        // at all, and that is the top of this distribution rather than something outside it. Where
        // the median is high enough for the draw to reach the whole, the sellers that would have
        // gone past it pile up there, and they are the ones whose listings stand unsold for the
        // length of a quarter. A market has both kinds and the share of each is not a second
        // number: it follows from where the middle of this one sits.
        drawn.min(BPS)
    }

    /// What a holder declares above its own valuation, at a posted charge.
    ///
    /// `patience` is the markup where nothing is charged at all. It falls to nothing at
    /// `truthful_at` and turns negative beyond it, which is Weyl and Zhang's Theorem 1 and not a
    /// shape chosen to be convenient: a holder is at once selling a share of the thing to whoever
    /// takes it and buying a share back from the venue, and at the rate where those two are equal
    /// it has no reason to misstate the price at all.
    #[must_use]
    pub fn floor(opening: u32, charge: Amount<Bps>, truthful_at: Amount<Bps>) -> i32 {
        if truthful_at.is_zero() {
            return 0;
        }
        let posted = i128::try_from(charge.base_units()).unwrap_or(i128::MAX);
        let truthful = i128::try_from(truthful_at.base_units()).unwrap_or(1);
        // Nothing below its own valuation until the charge passes the rate where telling the truth
        // is optimal. Below that rate a seller would rather keep the thing than let it go under
        // what the thing is worth to it; at that rate it declares exactly that, which is the
        // theorem; only past it does holding cost enough to be worth shedding at a discount.
        let below = i128::from(opening) * (posted - truthful).max(0) / truthful;
        // Never through the whole: a declared price is a price, and a price is positive.
        i32::try_from((-below).clamp(-(i128::from(BPS) - 1), 0)).unwrap_or(0)
    }

    /// What share of its standing markup a seller keeps at each look, from the step it takes off
    /// and what the charge costs it to wait.
    ///
    /// A charge is a price on holding out, so it is a price on patience, and the more it costs the
    /// less of any seller's patience survives it. At the charge where truth-telling is optimal
    /// nothing survives: every seller goes to its floor on the first look, they all stand at the
    /// same multiple of their own valuation, and what is left between their declared prices is
    /// only what they disagree about. That is what the mechanism claims to do, stated as a rule
    /// somebody could follow.
    #[must_use]
    pub fn retain(step: u32, charge: Amount<Bps>, truthful_at: Amount<Bps>) -> u32 {
        let kept = BPS.saturating_sub(step.min(BPS));
        if truthful_at.is_zero() {
            return kept;
        }
        let posted = i128::try_from(charge.base_units()).unwrap_or(i128::MAX);
        let truthful = i128::try_from(truthful_at.base_units()).unwrap_or(1);
        let left = (truthful - posted).max(0);
        u32::try_from(i128::from(kept) * left / truthful).unwrap_or(0)
    }

    /// What one item is worth to this holder, its own attachment included.
    fn worth_to(&self, me: AgentId, state: &ShowcaseState, item: ItemId) -> Option<Amount<Cash>> {
        let collection = state.items.get(item.0 as usize)?.collection;
        let plain = self.worth.value(me, collection, item).ok()??;
        plain
            .mul_div(
                WHOLE.checked_add(self.devotion).ok()?,
                WHOLE,
                Rounding::Down,
            )
            .ok()
    }

    /// What it would declare for one item.
    fn declares(&self, me: AgentId, state: &ShowcaseState, item: ItemId) -> Option<Amount<Cash>> {
        self.at_markup(
            me,
            state,
            item,
            i32::try_from(self.opening).unwrap_or(i32::MAX),
        )
    }

    /// What this holder would ask for one item at a given markup over its own valuation.
    fn at_markup(
        &self,
        me: AgentId,
        state: &ShowcaseState,
        item: ItemId,
        markup: i32,
    ) -> Option<Amount<Cash>> {
        let worth = self.worth_to(me, state, item)?;
        let against = u128::try_from(i128::from(BPS) + i128::from(markup)).ok()?;
        let asked = worth
            .mul_div(Amount::new(against), WHOLE, Rounding::Down)
            .ok()?;
        Some(if asked.is_zero() {
            Amount::new(1)
        } else {
            asked
        })
    }

    /// The next price down for a listing of its own that nobody has taken.
    ///
    /// The standing markup is read back off the listing rather than remembered, so a holder needs
    /// no memory per item and a market of a million items costs a population nothing to carry. The
    /// walk stops at the floor, and a step that would not move the price is not taken at all —
    /// a reprice that changes nothing is an action the mechanism would apply and a tick nobody
    /// spent on anything.
    fn comes_down(
        &self,
        me: AgentId,
        state: &ShowcaseState,
        item: ItemId,
        declared: Amount<Cash>,
    ) -> Option<Amount<Cash>> {
        let worth = self.worth_to(me, state, item)?;
        if worth.is_zero() {
            return None;
        }
        let standing = i128::try_from(declared.base_units()).ok()? * i128::from(BPS)
            / i128::try_from(worth.base_units()).ok()?
            - i128::from(BPS);
        // Towards the floor and not towards nothing: what a seller walks down to is what it will
        // not go below, and under a charge that is under its own valuation rather than at it.
        let floor = i128::from(self.floor);
        let next = floor + (standing - floor) * i128::from(self.retain) / i128::from(BPS);
        if next >= standing {
            return None;
        }
        let asked = self.at_markup(me, state, item, i32::try_from(next).ok()?)?;
        (asked < declared).then_some(asked)
    }

    /// The next of its own holdings, dropping any it turns out no longer to have.
    fn next_of_mine(&mut self, me: AgentId, state: &ShowcaseState) -> Option<ItemId> {
        for _ in 0..self.mine.len() {
            let at = self.at as usize % self.mine.len();
            self.at = self.at.wrapping_add(1);
            let item = self.mine[at];
            match state.items.get(item.0 as usize) {
                Some(held) if held.holder == me => return Some(item),
                // Somebody took it, or it never was ours. A participant notices by looking.
                _ => {
                    self.mine.swap_remove(at);
                }
            }
        }
        None
    }
}

/// One whole, in the basis points this crate quotes every share in.
const BPS: u32 = 10_000;

/// The stream a population's patience is drawn from, kept apart from anything a run draws later.
const PATIENCE: u64 = 0x0070_6174_6965_6E74;

impl Agent<Showcase> for Holder {
    fn decide(
        &mut self,
        me: AgentId,
        state: &ShowcaseState,
        _now: Tick,
        rng: &mut RunRng,
    ) -> Option<ShowcaseAction> {
        // What it does with the one of its own it looks at this tick. A holder that would sell
        // puts it up; one that would not takes it back down — and taking it down matters as much
        // as putting it up, because a bought item stays on the showcase at the price its new
        // holder just paid (§6.6), so somebody who never meant to sell arrives holding a listing.
        // Without this a showcase only ever accumulates, the share carrying a price climbs to the
        // share ever listed, and turnover is capped by it — which is not the market that was
        // measured, where over a third of a float changed hands in a quarter while an eighth of it
        // carried a price at any moment.
        if let Some(item) = self.next_of_mine(me, state) {
            match (self.sells, state.items[item.0 as usize].listing) {
                (true, None) => {
                    let price = self.declares(me, state, item)?;
                    return Some(ShowcaseAction::List { item, price });
                }
                // Still there, so it was asking too much. It comes down.
                (true, Some(listing)) => {
                    if let Some(price) = self.comes_down(me, state, item, listing.declared) {
                        return Some(ShowcaseAction::Reprice { item, price });
                    }
                }
                (false, Some(_)) => return Some(ShowcaseAction::Withdraw { item }),
                (false, None) => {}
            }
        }

        // Then a look at what somebody else has put up. A few items and not all of them: a market
        // this size is not read end to end by anybody, and a participant who could would be a
        // participant with the venue's own view of it.
        let stock = u32::try_from(state.items.len()).ok()?;
        if stock == 0 {
            return None;
        }
        for _ in 0..self.attention.max(1) {
            let item = ItemId(u32::try_from(rng.below(u64::from(stock))).ok()?);
            let held = &state.items[item.0 as usize];
            if held.holder == me {
                continue;
            }
            let Some(listing) = held.listing else {
                continue;
            };
            let worth = self.worth_to(me, state, item)?;
            if listing.declared >= worth || state.purses[me.0 as usize].balance < listing.declared {
                continue;
            }
            self.mine.push(item);
            return Some(ShowcaseAction::Buy { item });
        }
        None
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
    use std::num::NonZeroU64;

    use sting_karman_core::{Mechanism, NullObserver, World};
    use sting_karman_mech_showcase::{Arrears, CollectionId, Distribution, Item, Listing, Purse};
    use sting_karman_types::SecondsPerTick;

    use super::*;

    const GIFTS: CollectionId = CollectionId(0);
    const VENUE: AgentId = AgentId(0);

    /// A world where everything is worth the same, so a test can reason about one number.
    fn flat(spread: u128) -> Arc<Worth> {
        Arc::new(Worth::new(vec![Amount::new(1_000_000)], 0, Amount::new(spread), 7).unwrap())
    }

    fn state(items: usize, holders: &[(AgentId, usize)], capital: u128) -> ShowcaseState {
        let mut all = Vec::new();
        for (who, count) in holders {
            for _ in 0..*count {
                all.push(Item {
                    collection: GIFTS,
                    holder: *who,
                    listing: None,
                });
            }
        }
        assert_eq!(all.len(), items);
        ShowcaseState::open(
            all,
            vec![
                Purse {
                    balance: Amount::new(capital),
                    ..Purse::default()
                };
                4
            ],
        )
        .unwrap()
    }

    fn showcase(charge: u128) -> Showcase {
        Showcase {
            venue: VENUE,
            seconds_per_tick: SecondsPerTick(NonZeroU64::new(86_400).unwrap()),
            charge: vec![Amount::new(charge)],
            lag: 2,
            commission: Amount::ZERO,
            execution: Amount::ZERO,
            arrears: Arrears::Forfeit,
            distribution: Distribution::Even,
            share: WHOLE,
            period: 0,
        }
    }

    /// The theorem, in three points: below the rate at which telling the truth is optimal a holder
    /// comes down only as far as what the thing is worth to it, at that rate it declares exactly
    /// that, and above it it goes below — a thing that costs something to hold is worth shedding
    /// under one's own valuation. A floor that could only be at or above a valuation would give a
    /// curve that could only rise, and a curve that can only rise has no best rate on it to find.
    #[test]
    fn the_floor_follows_the_theorem() {
        let truthful = Amount::new(3_850);
        assert_eq!(Holder::floor(5_000, Amount::ZERO, truthful), 0);
        assert_eq!(Holder::floor(5_000, truthful, truthful), 0);
        assert!(Holder::floor(5_000, Amount::new(7_700), truthful) < 0);
        // And never through the whole, because a declared price is a price.
        let whole = i32::try_from(BPS).expect("a share in ten-thousandths fits a word");
        assert!(Holder::floor(60_000, Amount::new(100_000), truthful) > -whole);
    }

    /// And the other half of the theorem: a charge is a price on holding out, so the more it costs
    /// the less of anybody's patience survives. At the rate where truth-telling is optimal none of
    /// it does — every seller goes to its floor the first time it looks, and what is left between
    /// their declared prices is only what they disagree about.
    #[test]
    fn a_charge_is_a_price_on_holding_out() {
        let truthful = Amount::new(3_850);
        // A seller taking a tenth off at each look keeps nine tenths, where nothing is charged.
        assert_eq!(Holder::retain(1_000, Amount::ZERO, truthful), 9_000);
        // One that never steps down keeps all of it — and the charge still takes that away.
        assert_eq!(Holder::retain(0, Amount::ZERO, truthful), BPS);
        assert_eq!(Holder::retain(1_000, truthful, truthful), 0);
        assert_eq!(Holder::retain(0, Amount::new(7_700), truthful), 0);
        assert!(Holder::retain(1_000, Amount::new(1_925), truthful) < 9_000);
    }

    /// A seller whose listing nobody took asks less next time, and never less than its floor.
    #[test]
    fn a_seller_that_did_not_sell_comes_down() {
        let mut holder = Holder {
            retain: 5_000,
            ..Holder::new(flat(0), vec![ItemId(0)], 8_000, true)
        };
        let mut state = state(1, &[(AgentId(1), 1)], 0);
        let mut rng = RunRng::new(1, 0);
        let value = holder
            .worth
            .value(AgentId(1), GIFTS, ItemId(0))
            .unwrap()
            .unwrap();

        // First it puts it up at the opening.
        let listed = holder.decide(AgentId(1), &state, Tick(0), &mut rng);
        let Some(ShowcaseAction::List { price, .. }) = listed else {
            panic!("did not list, got {listed:?}");
        };
        assert_eq!(
            price,
            value
                .mul_div(Amount::new(18_000), WHOLE, Rounding::Down)
                .unwrap()
        );

        // Then it comes down, halving the markup each look, and stops at what it is worth.
        let mut asked = price;
        for _ in 0..40 {
            state.items[0].listing = Some(Listing {
                declared: asked,
                assessed_to: Tick(0),
                pending: None,
            });
            match holder.decide(AgentId(1), &state, Tick(0), &mut rng) {
                Some(ShowcaseAction::Reprice { price, .. }) => {
                    assert!(price < asked, "a reprice that did not come down");
                    asked = price;
                }
                _ => break,
            }
        }
        assert_eq!(
            asked, value,
            "it did not walk down to what the thing is worth to it"
        );
    }

    /// Under a charge it goes below its own valuation, because holding costs something.
    #[test]
    fn under_a_charge_it_goes_below_what_it_is_worth() {
        let truthful = Amount::new(3_850);
        let over = Amount::new(7_700);
        let floor = Holder::floor(8_000, over, truthful);
        assert!(floor < 0);
        let mut holder = Holder {
            retain: Holder::retain(1_000, over, truthful),
            floor,
            ..Holder::new(flat(0), vec![ItemId(0)], 8_000, true)
        };
        let mut state = state(1, &[(AgentId(1), 1)], 0);
        let mut rng = RunRng::new(1, 0);
        let value = holder
            .worth
            .value(AgentId(1), GIFTS, ItemId(0))
            .unwrap()
            .unwrap();
        state.items[0].listing = Some(Listing {
            declared: value
                .mul_div(Amount::new(18_000), WHOLE, Rounding::Down)
                .unwrap(),
            assessed_to: Tick(0),
            pending: None,
        });
        let Some(ShowcaseAction::Reprice { price, .. }) =
            holder.decide(AgentId(1), &state, Tick(0), &mut rng)
        else {
            panic!("it did not come down at all");
        };
        // Nothing of its patience survives this charge, so it lands on the floor in one step.
        assert!(
            price < value,
            "it stopped at its own valuation under a charge that costs to wait"
        );
    }

    /// Every holder its own patience, about the median a scenario states. One markup shared by a
    /// population is a population that never trades — a buyer differs from a seller only by taste,
    /// and no taste covers a markup everybody applied — and a declared price that is a fixed
    /// multiple of a worth orders things exactly as their worth does, so a market with no charge on
    /// it would appear to have solved the problem the charge exists for.
    #[test]
    fn patience_is_one_holder_at_a_time() {
        let drawn: Vec<u32> = (0..500)
            .map(|seat| Holder::patience_of(9, seat, 4_000))
            .collect();
        let mut apart = drawn.clone();
        apart.sort_unstable();
        apart.dedup();
        assert!(
            apart.len() > 200,
            "five hundred holders shared {} patiences",
            apart.len()
        );
        assert!(apart[0] < 400, "nobody was impatient enough to ever sell");
        assert!(apart[apart.len() - 1] > 7_000, "nobody was patient");
        // The median a scenario states is the median it gets, to a fortieth.
        let middle = u64::from(apart[apart.len() / 2]);
        assert!(
            (3_600..=4_400).contains(&middle),
            "the median came out at {middle}"
        );
        // And one seed is one population.
        assert_eq!(
            drawn,
            (0..500)
                .map(|seat| Holder::patience_of(9, seat, 4_000))
                .collect::<Vec<_>>()
        );
        assert_ne!(
            drawn,
            (0..500)
                .map(|seat| Holder::patience_of(10, seat, 4_000))
                .collect::<Vec<_>>()
        );
    }

    /// A holder puts what it has on the showcase, at its own valuation marked up.
    #[test]
    fn a_holder_lists_what_it_holds() {
        let worth = flat(0);
        let mut holder = Holder::new(worth.clone(), vec![ItemId(0)], 5_000, true);
        let state = state(1, &[(AgentId(1), 1)], 0);
        let mut rng = RunRng::new(1, 0);
        let action = holder.decide(AgentId(1), &state, Tick(0), &mut rng);
        let wanted = worth.value(AgentId(1), GIFTS, ItemId(0)).unwrap().unwrap();
        assert_eq!(
            action,
            Some(ShowcaseAction::List {
                item: ItemId(0),
                price: wanted
                    .mul_div(Amount::new(15_000), WHOLE, Rounding::Down)
                    .unwrap()
            })
        );
    }

    /// One that would never sell puts up nothing, which is what makes the share of a population
    /// that would the thing the count of priced items is fitted against.
    #[test]
    fn a_holder_that_would_not_sell_lists_nothing() {
        let mut holder = Holder::new(flat(0), vec![ItemId(0)], 5_000, false);
        let state = state(1, &[(AgentId(1), 1)], 0);
        let mut rng = RunRng::new(1, 0);
        assert_eq!(holder.decide(AgentId(1), &state, Tick(0), &mut rng), None);
    }

    /// A bought item stays on the showcase at the price its new holder just paid, so somebody who
    /// never meant to sell arrives holding a listing. They take it down. Without this a showcase
    /// only accumulates: the share of a market carrying a price climbs to the share ever listed and
    /// turnover is capped by it, which is not the market the census measured.
    #[test]
    fn a_holder_that_would_not_sell_takes_back_what_it_holds() {
        let mut holder = Holder::new(flat(0), vec![ItemId(0)], 5_000, false);
        let mut state = state(1, &[(AgentId(1), 1)], 0);
        state.items[0].listing = Some(Listing {
            declared: Amount::new(1),
            assessed_to: Tick(0),
            pending: None,
        });
        let mut rng = RunRng::new(1, 0);
        assert_eq!(
            holder.decide(AgentId(1), &state, Tick(0), &mut rng),
            Some(ShowcaseAction::Withdraw { item: ItemId(0) })
        );
    }

    /// One that would sell leaves its own listing standing. A market where every seller took its
    /// own offer down again the tick after making it would have no standing inventory at all.
    #[test]
    fn a_holder_that_would_sell_leaves_its_own_listing_alone() {
        let mut holder = Holder::new(flat(0), vec![ItemId(0)], 5_000, true);
        let mut state = state(1, &[(AgentId(1), 1)], 0);
        state.items[0].listing = Some(Listing {
            declared: Amount::new(1_000_000),
            assessed_to: Tick(0),
            pending: None,
        });
        let mut rng = RunRng::new(1, 0);
        assert_eq!(holder.decide(AgentId(1), &state, Tick(0), &mut rng), None);
    }

    /// A collector wants the same thing more than anybody, so it declares more for it and pays
    /// more for it. That is a fact about the person and not about the thing.
    #[test]
    fn a_collector_wants_the_same_thing_more() {
        let worth = flat(0);
        let plain = Holder::new(worth.clone(), vec![ItemId(0)], 1_000, true);
        let collector = Holder::collector(worth, vec![ItemId(0)], Amount::new(5_000), 1_000);
        let state = state(1, &[(AgentId(1), 1)], 0);
        let asked = |who: &Holder| {
            who.declares(AgentId(1), &state, ItemId(0))
                .unwrap()
                .base_units()
        };
        assert!(asked(&collector) > asked(&plain));
        // Half again as much wanted is half again as much asked.
        assert_eq!(asked(&collector) * 2, asked(&plain) * 3);
    }

    /// It takes what it finds priced below what the thing is worth to it, and leaves what is
    /// priced above. Nothing else is arbitrage.
    #[test]
    fn a_holder_takes_what_is_cheap_and_leaves_what_is_not() {
        let worth = flat(0);
        let mechanism = showcase(0);
        let mut seller = Holder::new(worth.clone(), vec![ItemId(0)], 0, true);
        let mut buyer = Holder::new(worth, Vec::new(), 0, false);
        let mut state = state(1, &[(AgentId(1), 1)], 10_000_000);
        let mut rng = RunRng::new(3, 0);

        let listing = seller
            .decide(AgentId(1), &state, Tick(0), &mut rng)
            .unwrap();
        mechanism
            .apply(&mut state, AgentId(1), listing, Tick(0), &mut Vec::new())
            .unwrap();
        // Priced at exactly what it is worth to the buyer, so there is nothing in it.
        assert_eq!(buyer.decide(AgentId(2), &state, Tick(1), &mut rng), None);

        let cut = ShowcaseAction::Reprice {
            item: ItemId(0),
            price: Amount::new(500_000),
        };
        mechanism
            .apply(&mut state, AgentId(1), cut, Tick(1), &mut Vec::new())
            .unwrap();
        assert_eq!(
            buyer.decide(AgentId(2), &state, Tick(2), &mut rng),
            Some(ShowcaseAction::Buy { item: ItemId(0) })
        );
    }

    /// It does not buy what it cannot pay for, however cheap the thing looks.
    #[test]
    fn an_empty_purse_buys_nothing() {
        let worth = flat(0);
        let mut buyer = Holder::new(worth.clone(), Vec::new(), 0, false);
        let mut state = state(1, &[(AgentId(1), 1)], 0);
        let value = worth.value(AgentId(2), GIFTS, ItemId(0)).unwrap().unwrap();
        // Priced at a tenth of what it is worth to the buyer, and still out of reach.
        state.items[0].listing = Some(Listing {
            declared: value
                .mul_div(Amount::new(1_000), WHOLE, Rounding::Down)
                .unwrap(),
            assessed_to: Tick(0),
            pending: None,
        });
        let mut rng = RunRng::new(5, 0);
        assert_eq!(buyer.decide(AgentId(2), &state, Tick(1), &mut rng), None);
    }

    /// A participant notices that something was taken from it by looking, which is the only way
    /// anybody notices anything here: an agent is handed the state and never a message.
    #[test]
    fn a_holder_notices_what_it_no_longer_has() {
        let mut holder = Holder::new(flat(0), vec![ItemId(0), ItemId(1)], 0, true);
        let mut state = state(2, &[(AgentId(1), 2)], 0);
        state.items[0].holder = AgentId(2);
        let mut rng = RunRng::new(9, 0);
        holder.decide(AgentId(1), &state, Tick(0), &mut rng);
        assert_eq!(
            holder.mine,
            vec![ItemId(1)],
            "it kept claiming something somebody else has"
        );
    }

    /// A population of these makes a market: things get listed, things change hands, and the
    /// engine's invariants hold through every one of it.
    #[test]
    fn a_population_of_them_makes_a_market() {
        let worth = flat(2_000);
        let mechanism = showcase(1_000);
        let items: Vec<Item> = (0..60)
            .map(|at| Item {
                collection: GIFTS,
                holder: AgentId(1 + (at % 3)),
                listing: None,
            })
            .collect();
        let mine = |who: u32| {
            (0..60u32)
                .filter(|at| 1 + (at % 3) == who)
                .map(ItemId)
                .collect::<Vec<_>>()
        };
        let state = ShowcaseState::open(
            items,
            vec![
                Purse {
                    balance: Amount::new(500_000_000),
                    ..Purse::default()
                };
                4
            ],
        )
        .unwrap();
        let mut world = World::new(
            mechanism,
            state,
            vec![
                Box::new(Holder::new(worth.clone(), Vec::new(), 0, false))
                    as Box<dyn Agent<Showcase>>,
                Box::new(Holder {
                    attention: 4,
                    ..Holder::new(worth.clone(), mine(1), 2_000, true)
                }),
                Box::new(Holder {
                    attention: 4,
                    ..Holder::new(worth.clone(), mine(2), 2_000, true)
                }),
                Box::new(Holder {
                    attention: 4,
                    ..Holder::collector(worth, mine(3), Amount::new(4_000), 6_000)
                }),
            ],
        );
        let mut rng = RunRng::new(11, 0);
        world
            .run(400, &mut rng, &mut NullObserver)
            .expect("the run aborted");

        let listed = world
            .state()
            .items
            .iter()
            .filter(|item| item.listing.is_some())
            .count();
        assert!(
            listed > 30,
            "only {listed} of sixty ever reached the showcase"
        );
        let moved = world
            .state()
            .items
            .iter()
            .filter(|item| item.holder == AgentId(2) || item.holder == AgentId(3))
            .count();
        assert!(
            moved > 20,
            "only {moved} items are held by anybody but their first holder"
        );
        assert!(world.stats().actions_applied > 100, "nothing much happened");
    }

    /// The whole point, in one assertion: a market of these under a charge that pushes towards the
    /// truth prices things closer to what they are worth than one under no charge at all. This is
    /// the shape the charge is graded on, on four agents rather than four thousand.
    #[test]
    fn a_charge_moves_declared_prices_towards_worth() {
        let worth = flat(1_000);
        let truthful = Amount::new(3_850);
        let spread_at = |charge: u128| {
            let mechanism = showcase(charge);
            let retain = Holder::retain(2_000, Amount::new(charge), truthful);
            let floor = Holder::floor(8_000, Amount::new(charge), truthful);
            let seller = |mine: Vec<ItemId>| Holder {
                floor,
                retain,
                ..Holder::new(worth.clone(), mine, 8_000, true)
            };
            let items: Vec<Item> = (0..30)
                .map(|at| Item {
                    collection: GIFTS,
                    holder: AgentId(1 + (at % 3)),
                    listing: None,
                })
                .collect();
            let mine = |who: u32| {
                (0..30u32)
                    .filter(|at| 1 + (at % 3) == who)
                    .map(ItemId)
                    .collect::<Vec<_>>()
            };
            let state = ShowcaseState::open(
                items,
                vec![
                    Purse {
                        balance: Amount::new(1_000_000_000),
                        ..Purse::default()
                    };
                    4
                ],
            )
            .unwrap();
            let mut world = World::new(
                mechanism,
                state,
                vec![
                    Box::new(Holder::new(worth.clone(), Vec::new(), 0, false))
                        as Box<dyn Agent<Showcase>>,
                    Box::new(seller(mine(1))),
                    Box::new(seller(mine(2))),
                    Box::new(seller(mine(3))),
                ],
            );
            let mut rng = RunRng::new(13, 0);
            world.run(60, &mut rng, &mut NullObserver).unwrap();
            let declared: Vec<u128> = world
                .state()
                .items
                .iter()
                .filter_map(|item| item.listing.map(|of| of.declared.base_units()))
                .collect();
            let sum: u128 = declared.iter().sum();
            sum / u128::try_from(declared.len().max(1)).unwrap()
        };
        let (idle, charged) = (spread_at(0), spread_at(3_850));
        assert!(
            charged < idle,
            "a charge left prices at {charged} against {idle} with none"
        );
    }

    /// A run of one seed is a run of one seed, agents and all.
    #[test]
    fn a_population_is_reproducible() {
        sting_karman_core::conformance::assert_deterministic(80, 17, || {
            let worth = flat(2_000);
            let items: Vec<Item> = (0..24)
                .map(|at| Item {
                    collection: GIFTS,
                    holder: AgentId(1 + (at % 3)),
                    listing: None,
                })
                .collect();
            let mine = |who: u32| {
                (0..24u32)
                    .filter(|at| 1 + (at % 3) == who)
                    .map(ItemId)
                    .collect::<Vec<_>>()
            };
            let state = ShowcaseState::open(
                items,
                vec![
                    Purse {
                        balance: Amount::new(200_000_000),
                        ..Purse::default()
                    };
                    4
                ],
            )
            .unwrap();
            World::new(
                showcase(1_500),
                state,
                vec![
                    Box::new(Holder::new(worth.clone(), Vec::new(), 0, false))
                        as Box<dyn Agent<Showcase>>,
                    Box::new(Holder::new(worth.clone(), mine(1), 3_000, true)),
                    Box::new(Holder::new(worth.clone(), mine(2), 3_000, true)),
                    Box::new(Holder::collector(
                        worth.clone(),
                        mine(3),
                        Amount::new(3_000),
                        8_000,
                    )),
                ],
            )
        });
    }
}
