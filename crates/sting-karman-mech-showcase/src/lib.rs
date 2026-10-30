//! The showcase: assets held under a declared price and a standing offer to sell at it.
//!
//! The family this crate implements is specified in
//! [`DEAD-CAPITAL-SPEC.md`](https://github.com/karman-dev/karman/blob/main/karman/docs/DEAD-CAPITAL-SPEC.md),
//! and the specification is the source: where the two disagree, this is the bug.
//!
//! One asset exists in one copy; its holder names a price; anyone may take it at that price; and
//! holding it under those two obligations is charged for, as a share of the price named. Naming a
//! low price is punished by losing the thing cheaply and naming a high one by the charge, which is
//! the whole of the mechanism and the whole of why it is expected to produce a truthful price.
//!
//! Three properties of this implementation are worth stating where they cannot be missed.
//!
//! **The charge is lazy.** It is computed when an item is touched and at no other moment; nothing
//! walks the items on a tick. That is not an optimisation but the mechanism: a venue that walked a
//! million items every period would spend more on collecting the charge than the charge is worth
//! on everything cheap, and would apply only to the top of its own market.
//!
//! **The charge rounds down**, against the venue, which inverts the usual direction of this
//! workspace and is argued in [`ARCHITECTURE.md`](https://github.com/karman-dev/karman/blob/main/karman/docs/ARCHITECTURE.md)
//! §5 invariant 6: there is no reference to agree with here, so the conservative direction is the
//! one that understates the design's own effect.
//!
//! **The venue is a participant.** It has a purse, it takes the commission, and a forced sale hands
//! it the item and takes the price out of that purse. A venue outside the accounts would be a hole
//! in them, and the conservation invariant could not be checked at all.

mod worth;

use serde::{Deserialize, Serialize};
use sting_karman_core::{Abort, InvariantViolation, MechError, Mechanism};
use sting_karman_types::{AgentId, Amount, ArithError, Rounding, SecondsPerTick, Tick, Unit};
pub use worth::Worth;

/// The asset everything here is settled in.
///
/// One asset and not two: what an item is bought with is what the charge is paid in and what the
/// pool shares out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cash;

impl Unit for Cash {
    const SYMBOL: &'static str = "cash";
}

/// A share, in ten-thousandths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bps;

impl Unit for Bps {
    const SYMBOL: &'static str = "bps";
}

/// Elapsed time, in the unit a rate is quoted against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seconds;

impl Unit for Seconds {
    const SYMBOL: &'static str = "s";
}

/// The whole of a share.
pub const WHOLE: Amount<Bps> = Amount::new(10_000);

/// A year, and always this many seconds.
///
/// Three hundred and sixty-five days, exactly, whatever the calendar does. A rate needs a
/// denominator that never moves: a leap year that lengthened this would make two runs of the same
/// scenario disagree, which is the one thing this workspace does not permit.
const SECONDS_PER_YEAR: Amount<Seconds> = Amount::new(31_536_000);

/// One item of one collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub u32);

/// One family of items, and the unit the charge is a property of.
///
/// The rate belongs to a collection rather than to the system because the turnover it is
/// calibrated against differs between collections by a factor of nine, and a rate right for one is
/// wrong for another by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CollectionId(pub u32);

/// What happens when the charge cannot be paid out of a purse.
///
/// Both are simulated because they are not obviously ranked: one lets a holder sit on an asset for
/// nothing by keeping an empty purse, the other makes an empty purse the most expensive way to
/// hold anything, and which of the two a market survives is a result rather than a preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrears {
    /// Take what the purse holds, leave the item with its holder off the showcase, and write the
    /// rest of the debt off.
    Forfeit,
    /// Sell the item to the venue at its declared price: the debt comes out of that price and what
    /// is left reaches the holder. Where the venue's purse cannot cover the price the sale cannot
    /// happen and [`Arrears::Forfeit`] applies instead — which is a measurement rather than a
    /// detail, because a venue that cannot afford its own rule has a rule only on paper.
    ForcedSale,
}

/// How the pool is shared out at a period boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Distribution {
    /// Equally between every participant but the venue.
    Even,
    /// In proportion to the buyouts each was a party to.
    ByTrades,
    /// In proportion to the charge each paid.
    ByCharge,
}

/// A change to a listing that has been asked for and has not taken effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// The price it will bind at, or nothing where the item is leaving the showcase.
    pub price: Option<Amount<Cash>>,
    /// The first tick it binds on.
    pub effective: Tick,
}

/// An item's presence on the showcase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    /// The price that binds: what anyone may take the item at.
    pub declared: Amount<Cash>,
    /// The moment the charge has been settled up to.
    pub assessed_to: Tick,
    /// A raise or a withdrawal waiting on the lag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
}

/// One item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Which family it belongs to, and therefore what it is charged at.
    pub collection: CollectionId,
    /// Who holds it. Exactly one participant, at every tick.
    pub holder: AgentId,
    /// Present exactly when the item is on the showcase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing: Option<Listing>,
}

/// What one participant can spend, and what it has done inside the current period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Purse {
    /// What it can spend.
    pub balance: Amount<Cash>,
    /// How many buyouts it has been a party to since the last distribution.
    pub traded: u32,
    /// What it has paid in charge since the last distribution.
    pub paid: Amount<Cash>,
}

/// The whole state of one showcase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowcaseState {
    /// Every item, by its own identifier.
    pub items: Vec<Item>,
    /// Every participant's purse, by agent identifier. The venue has one of these like anybody.
    pub purses: Vec<Purse>,
    /// What the charge has collected and not yet shared out.
    pub pool: Amount<Cash>,
    /// What execution has consumed. It leaves the system and does not come back.
    pub spent: Amount<Cash>,
    /// The last period boundary the pool was shared out at.
    pub distributed_through: Tick,
    /// What the world opened with, so that conservation is checkable rather than asserted.
    opened_with: Amount<Cash>,
    /// What the purses hold between them, kept as they move rather than counted when asked.
    ///
    /// A showcase of a million items is held by a quarter of a million purses, and conservation
    /// is checked after every applied action. Counting them there makes a run quadratic in its
    /// own population, which measured at ninety-two per cent of a run before this field existed.
    /// Every balance moves through [`ShowcaseState::credit`] and [`ShowcaseState::debit`], which
    /// is what makes this the sum and not a cache of it, and the tick-boundary sweep counts the
    /// purses back out to confirm that no path around those two was ever added.
    held: Amount<Cash>,
}

impl ShowcaseState {
    /// Moves cash into one purse, and keeps what the purses hold between them in step.
    ///
    /// # Errors
    ///
    /// Fails when the purse or the running total would leave the width money is carried in.
    fn credit(&mut self, who: usize, amount: Amount<Cash>) -> Result<(), ArithError> {
        self.purses[who].balance = self.purses[who].balance.checked_add(amount)?;
        self.held = self.held.checked_add(amount)?;
        Ok(())
    }

    /// Moves cash out of one purse, and keeps what the purses hold between them in step.
    ///
    /// # Errors
    ///
    /// Fails when the purse does not hold that much.
    fn debit(&mut self, who: usize, amount: Amount<Cash>) -> Result<(), ArithError> {
        self.purses[who].balance = self.purses[who].balance.checked_sub(amount)?;
        self.held = self.held.checked_sub(amount)?;
        Ok(())
    }

    /// Opens a showcase over a population and a stock of items.
    ///
    /// # Errors
    ///
    /// Fails when an item is held by nobody in the population, or when the opening balances
    /// between them are wider than the width money is carried in.
    pub fn open(items: Vec<Item>, purses: Vec<Purse>) -> Result<Self, MechError> {
        let mut opened_with = Amount::ZERO;
        for purse in &purses {
            opened_with = opened_with.checked_add(purse.balance)?;
        }
        for item in &items {
            if item.holder.0 as usize >= purses.len() {
                return Err(MechError::new(
                    "an item is held by nobody in this population",
                ));
            }
        }
        Ok(ShowcaseState {
            items,
            purses,
            pool: Amount::ZERO,
            spent: Amount::ZERO,
            distributed_through: Tick(0),
            opened_with,
            held: opened_with,
        })
    }

    /// What the world opened with: every purse, before anything happened.
    #[must_use]
    pub const fn opened_with(&self) -> Amount<Cash> {
        self.opened_with
    }
}

/// What an agent can do to a showcase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShowcaseAction {
    /// Put an item on the showcase at a price.
    List {
        /// Which item.
        item: ItemId,
        /// What anyone may take it at.
        price: Amount<Cash>,
    },
    /// Name a different price. Lower binds at once, higher after the lag.
    Reprice {
        /// Which item.
        item: ItemId,
        /// The new price.
        price: Amount<Cash>,
    },
    /// Take an item off the showcase, after the lag.
    Withdraw {
        /// Which item.
        item: ItemId,
    },
    /// Pay what an item owes.
    Settle {
        /// Which item.
        item: ItemId,
    },
    /// Take a listed item at its declared price.
    Buy {
        /// Which item.
        item: ItemId,
    },
}

/// What the showcase reports happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShowcaseEvent {
    /// An item joined the showcase.
    Listed {
        /// Which item.
        item: ItemId,
        /// Who put it there.
        by: AgentId,
        /// At what price.
        price: Amount<Cash>,
    },
    /// A price was named. `effective` is absent where it bound at once.
    Repriced {
        /// Which item.
        item: ItemId,
        /// Who named it.
        by: AgentId,
        /// The new price.
        price: Amount<Cash>,
        /// When it binds, where it does not bind now.
        effective: Option<Tick>,
    },
    /// A withdrawal was asked for, and binds at `effective`.
    Withdrawing {
        /// Which item.
        item: ItemId,
        /// Who asked.
        by: AgentId,
        /// When it binds.
        effective: Tick,
    },
    /// An item left the showcase.
    Withdrawn {
        /// Which item.
        item: ItemId,
        /// Who held it.
        by: AgentId,
    },
    /// An item changed hands at its declared price.
    Bought {
        /// Which item.
        item: ItemId,
        /// Who sold it, whether they meant to or not.
        from: AgentId,
        /// Who took it.
        by: AgentId,
        /// What the buyer paid.
        price: Amount<Cash>,
        /// What the listing owed, taken out of that price.
        charge: Amount<Cash>,
        /// What the venue took from what was left.
        commission: Amount<Cash>,
    },
    /// A charge was paid out of a purse.
    Settled {
        /// Which item.
        item: ItemId,
        /// Who paid.
        by: AgentId,
        /// How much.
        charge: Amount<Cash>,
    },
    /// A holder could not pay, and kept the item off the showcase.
    Forfeited {
        /// Which item.
        item: ItemId,
        /// Who could not pay.
        by: AgentId,
        /// What was taken.
        paid: Amount<Cash>,
        /// What was never collected.
        written_off: Amount<Cash>,
    },
    /// A holder could not pay, and the venue bought the item at its declared price.
    ForceSold {
        /// Which item.
        item: ItemId,
        /// Who held it.
        from: AgentId,
        /// What the venue paid.
        price: Amount<Cash>,
        /// What the listing owed, taken out of that price.
        charge: Amount<Cash>,
    },
    /// The pool was shared out.
    Distributed {
        /// What reached the participants.
        shared: Amount<Cash>,
        /// What the venue took.
        to_venue: Amount<Cash>,
    },
}

/// The showcase mechanism: every parameter of one run, and no mutable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Showcase {
    /// The participant that takes the commission and buys at a forced sale. It never acts.
    pub venue: AgentId,
    /// What one tick is worth, which is what turns an annual rate into a charge.
    pub seconds_per_tick: SecondsPerTick,
    /// The annual charge, per collection, indexed by [`CollectionId`].
    pub charge: Vec<Amount<Bps>>,
    /// How many ticks a raise or a withdrawal waits before it binds.
    pub lag: u64,
    /// What the venue takes from a buyout.
    pub commission: Amount<Bps>,
    /// What one action costs the participant taking it. It leaves the system.
    pub execution: Amount<Cash>,
    /// What happens when the charge cannot be paid.
    pub arrears: Arrears,
    /// How the pool is shared out.
    pub distribution: Distribution,
    /// How much of the pool is shared out; the rest is the venue's.
    pub share: Amount<Bps>,
    /// How many ticks pass between distributions.
    pub period: u64,
}

impl Showcase {
    /// What one collection is charged at.
    fn rate(&self, of: CollectionId) -> Result<Amount<Bps>, MechError> {
        self.charge
            .get(of.0 as usize)
            .copied()
            .ok_or_else(|| MechError::new("this collection has no rate"))
    }

    /// The charge one price accrues between two ticks, capped at that price.
    ///
    /// Two steps rather than one, and the reason is arithmetic: a 128-bit price by a 32-bit share
    /// by a 64-bit span reaches 2²²⁴, which no width here holds. Each step is one `mul_div` of the
    /// kind karman/docs/ARITHMETIC.md §2 describes — 128-bit operands, a 256-bit intermediate that
    /// provably cannot overflow, a 128-bit result or an error.
    ///
    /// Both steps round **down**. Rounding up would let the mechanism collect dust it may not
    /// collect in practice, and would flatter it exactly where it most needs doubting: the cheap
    /// end, where the charge is small enough for the rounding direction to be most of it.
    ///
    /// The cap is what makes voluntary entry credible. Being on the showcase at a price can never
    /// cost more than that price between one settlement and the next, so the worst a holder can be
    /// asked for is what they said the thing was worth.
    fn accrued(
        &self,
        price: Amount<Cash>,
        rate: Amount<Bps>,
        from: Tick,
        to: Tick,
    ) -> Result<Amount<Cash>, MechError> {
        let ticks = u128::from(to.0.saturating_sub(from.0));
        let seconds =
            Amount::<Seconds>::new(ticks.saturating_mul(u128::from(self.seconds_per_tick.0.get())));
        let a_year = price.mul_div(rate, WHOLE, Rounding::Down)?;
        let owed = a_year.mul_div(seconds, SECONDS_PER_YEAR, Rounding::Down)?;
        Ok(owed.min(price))
    }

    /// What a listing owes since it was last settled, and what price binds now.
    ///
    /// A change that has waited out its lag is in force whether or not anybody has touched the
    /// item, so the charge is piecewise: the old price up to the moment it matured, the new one
    /// after. Each part is capped at the price it accrued on, because that is the price whose
    /// declaration it is the cost of.
    fn standing(&self, item: &Item, now: Tick) -> Result<Standing, MechError> {
        let Some(listing) = item.listing else {
            return Ok(Standing {
                owed: Amount::ZERO,
                binds: None,
            });
        };
        let rate = self.rate(item.collection)?;
        let matured = listing.pending.filter(|change| change.effective <= now);
        match matured {
            Some(Pending {
                price: Some(raised),
                effective,
            }) => {
                let before =
                    self.accrued(listing.declared, rate, listing.assessed_to, effective)?;
                let after = self.accrued(raised, rate, effective, now)?;
                Ok(Standing {
                    owed: before.checked_add(after)?,
                    binds: Some(raised),
                })
            }
            // A withdrawal that has matured stops the charge where it matured and takes the item
            // off the showcase; nothing accrues after that and nothing may be bought.
            Some(Pending {
                price: None,
                effective,
            }) => Ok(Standing {
                owed: self.accrued(listing.declared, rate, listing.assessed_to, effective)?,
                binds: None,
            }),
            None => Ok(Standing {
                owed: self.accrued(listing.declared, rate, listing.assessed_to, now)?,
                binds: Some(listing.declared),
            }),
        }
    }

    /// Settles what an item owes out of its holder's purse, and says what became of the item.
    ///
    /// Where the purse cannot cover the charge the parameter decides, and the decision replaces
    /// what the holder asked for rather than accompanying it: an action that would have repriced
    /// an item forfeits it instead. One transition, one outcome, and the event says which.
    fn settle(
        &self,
        state: &mut ShowcaseState,
        item: ItemId,
        now: Tick,
        events: &mut Vec<ShowcaseEvent>,
    ) -> Result<Outcome, MechError> {
        let at = item.0 as usize;
        let held = state.items[at];
        let standing = self.standing(&held, now)?;
        let holder = held.holder.0 as usize;

        if standing.owed <= state.purses[holder].balance {
            if !standing.owed.is_zero() {
                state.debit(holder, standing.owed)?;
                state.purses[holder].paid = state.purses[holder].paid.checked_add(standing.owed)?;
                state.pool = state.pool.checked_add(standing.owed)?;
                events.push(ShowcaseEvent::Settled {
                    item,
                    by: held.holder,
                    charge: standing.owed,
                });
            }
            return Ok(if let Some(price) = standing.binds {
                state.items[at].listing = Some(Listing {
                    declared: price,
                    assessed_to: now,
                    pending: None,
                });
                Outcome::Settled
            } else {
                state.items[at].listing = None;
                events.push(ShowcaseEvent::Withdrawn {
                    item,
                    by: held.holder,
                });
                Outcome::LeftTheShowcase
            });
        }

        let binding = standing.binds.unwrap_or(Amount::ZERO);
        let venue = self.venue.0 as usize;
        let affordable = matches!(self.arrears, Arrears::ForcedSale)
            && !binding.is_zero()
            && binding <= state.purses[venue].balance;
        if affordable {
            let to_holder = binding.checked_sub(standing.owed.min(binding))?;
            state.debit(venue, binding)?;
            state.pool = state.pool.checked_add(standing.owed.min(binding))?;
            state.credit(holder, to_holder)?;
            state.purses[holder].paid = state.purses[holder]
                .paid
                .checked_add(standing.owed.min(binding))?;
            state.items[at].holder = self.venue;
            state.items[at].listing = None;
            events.push(ShowcaseEvent::ForceSold {
                item,
                from: held.holder,
                price: binding,
                charge: standing.owed.min(binding),
            });
            return Ok(Outcome::LeftTheHolder);
        }

        let paid = state.purses[holder].balance;
        state.debit(holder, paid)?;
        state.purses[holder].paid = state.purses[holder].paid.checked_add(paid)?;
        state.pool = state.pool.checked_add(paid)?;
        state.items[at].listing = None;
        events.push(ShowcaseEvent::Forfeited {
            item,
            by: held.holder,
            paid,
            written_off: standing.owed.checked_sub(paid)?,
        });
        Ok(Outcome::LeftTheShowcase)
    }

    /// Takes what one action costs the participant that took it, out of the system for good.
    fn charge_execution(&self, state: &mut ShowcaseState, actor: AgentId) -> Result<(), MechError> {
        let at = actor.0 as usize;
        state
            .debit(at, self.execution)
            .map_err(|_| MechError::new("cannot pay what this action costs to execute"))?;
        state.spent = state.spent.checked_add(self.execution)?;
        Ok(())
    }

    /// The item an action names, refusing one that is not there.
    fn item_at(state: &ShowcaseState, item: ItemId) -> Result<Item, MechError> {
        state
            .items
            .get(item.0 as usize)
            .copied()
            .ok_or_else(|| MechError::new("no such item"))
    }

    /// Shares the pool out, and gives the venue what is not shared.
    fn distribute(
        &self,
        state: &mut ShowcaseState,
        events: &mut Vec<ShowcaseEvent>,
    ) -> Result<(), Abort> {
        let shared = state.pool.mul_div(self.share, WHOLE, Rounding::Down)?;
        let to_venue = state.pool.checked_sub(shared)?;
        let venue = self.venue.0 as usize;
        state.credit(venue, to_venue)?;
        state.pool = shared;

        let over: Amount<Bps> = match self.distribution {
            Distribution::Even => Amount::new(
                u128::try_from(state.purses.len().saturating_sub(1)).unwrap_or(u128::MAX),
            ),
            Distribution::ByTrades => Self::weights(state, venue, |purse| u128::from(purse.traded)),
            Distribution::ByCharge => Self::weights(state, venue, |purse| purse.paid.base_units()),
        };

        let mut handed = Amount::ZERO;
        if !over.is_zero() {
            for who in 0..state.purses.len() {
                if who == venue {
                    continue;
                }
                let weight = Amount::<Bps>::new(match self.distribution {
                    Distribution::Even => 1,
                    Distribution::ByTrades => u128::from(state.purses[who].traded),
                    Distribution::ByCharge => state.purses[who].paid.base_units(),
                });
                let portion = shared.mul_div(weight, over, Rounding::Down)?;
                state.credit(who, portion)?;
                handed = handed.checked_add(portion)?;
            }
        }
        // What the division could not place stays where it was collected, because the alternative
        // is inventing the last unit.
        state.pool = shared.checked_sub(handed)?;
        for purse in &mut state.purses {
            purse.traded = 0;
            purse.paid = Amount::ZERO;
        }
        events.push(ShowcaseEvent::Distributed {
            shared: handed,
            to_venue,
        });
        Ok(())
    }

    /// The total weight a distribution divides by, over everybody but the venue.
    fn weights(state: &ShowcaseState, venue: usize, of: impl Fn(&Purse) -> u128) -> Amount<Bps> {
        let mut total = 0u128;
        for (who, purse) in state.purses.iter().enumerate() {
            if who != venue {
                total = total.saturating_add(of(purse));
            }
        }
        Amount::new(total)
    }
}

/// What a listing owes, and what price binds after any change that has waited out its lag.
struct Standing {
    owed: Amount<Cash>,
    /// Absent where the item has left the showcase, or a withdrawal has matured onto it.
    binds: Option<Amount<Cash>>,
}

/// What became of an item when its charge was settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// It paid and is still listed.
    Settled,
    /// It is off the showcase and its holder still has it.
    LeftTheShowcase,
    /// It is off the showcase and the venue has it.
    LeftTheHolder,
}

impl Mechanism for Showcase {
    type State = ShowcaseState;
    type Action = ShowcaseAction;
    type Event = ShowcaseEvent;

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per action, each establishing what it needs before anything moves: a refusal has to leave the state untouched, and splitting the arms hides where that is established"
    )]
    fn apply(
        &self,
        state: &mut ShowcaseState,
        actor: AgentId,
        action: ShowcaseAction,
        now: Tick,
        events: &mut Vec<ShowcaseEvent>,
    ) -> Result<(), MechError> {
        if actor.0 as usize >= state.purses.len() {
            return Err(MechError::new("no such participant"));
        }
        if actor == self.venue {
            return Err(MechError::new("the venue does not act"));
        }
        match action {
            ShowcaseAction::List { item, price } => {
                let held = Self::item_at(state, item)?;
                if held.holder != actor {
                    return Err(MechError::new("only a holder may list what it holds"));
                }
                if held.listing.is_some() {
                    return Err(MechError::new("this item is already on the showcase"));
                }
                // Zero is not a price. It says the item may be taken for nothing, and in the state
                // it is indistinguishable from an item nobody has priced.
                if price.is_zero() {
                    return Err(MechError::new("a declared price must be positive"));
                }
                self.rate(held.collection)?;
                self.charge_execution(state, actor)?;
                state.items[item.0 as usize].listing = Some(Listing {
                    declared: price,
                    assessed_to: now,
                    pending: None,
                });
                events.push(ShowcaseEvent::Listed {
                    item,
                    by: actor,
                    price,
                });
                Ok(())
            }
            ShowcaseAction::Reprice { item, price } => {
                let held = Self::item_at(state, item)?;
                if held.holder != actor {
                    return Err(MechError::new("only a holder may reprice what it holds"));
                }
                if held.listing.is_none() {
                    return Err(MechError::new("this item is not on the showcase"));
                }
                if price.is_zero() {
                    return Err(MechError::new("a declared price must be positive"));
                }
                self.charge_execution(state, actor)?;
                if self.settle(state, item, now, events)? != Outcome::Settled {
                    return Ok(());
                }
                let bound = state.items[item.0 as usize]
                    .listing
                    .ok_or_else(|| MechError::new("a settled listing vanished"))?;
                if price < bound.declared {
                    // A lower price cannot defeat a buyout — it can only make one easier — so
                    // delaying it buys nothing and costs discovery in the one direction a market
                    // of dead assets most needs to move.
                    state.items[item.0 as usize].listing = Some(Listing {
                        declared: price,
                        assessed_to: now,
                        pending: None,
                    });
                    events.push(ShowcaseEvent::Repriced {
                        item,
                        by: actor,
                        price,
                        effective: None,
                    });
                } else {
                    let effective = Tick(now.0.saturating_add(self.lag));
                    state.items[item.0 as usize].listing = Some(Listing {
                        pending: Some(Pending {
                            price: Some(price),
                            effective,
                        }),
                        ..bound
                    });
                    events.push(ShowcaseEvent::Repriced {
                        item,
                        by: actor,
                        price,
                        effective: Some(effective),
                    });
                }
                Ok(())
            }
            ShowcaseAction::Withdraw { item } => {
                let held = Self::item_at(state, item)?;
                if held.holder != actor {
                    return Err(MechError::new("only a holder may withdraw what it holds"));
                }
                if held.listing.is_none() {
                    return Err(MechError::new("this item is not on the showcase"));
                }
                self.charge_execution(state, actor)?;
                if self.settle(state, item, now, events)? != Outcome::Settled {
                    return Ok(());
                }
                let bound = state.items[item.0 as usize]
                    .listing
                    .ok_or_else(|| MechError::new("a settled listing vanished"))?;
                let effective = Tick(now.0.saturating_add(self.lag));
                state.items[item.0 as usize].listing = Some(Listing {
                    pending: Some(Pending {
                        price: None,
                        effective,
                    }),
                    ..bound
                });
                events.push(ShowcaseEvent::Withdrawing {
                    item,
                    by: actor,
                    effective,
                });
                Ok(())
            }
            ShowcaseAction::Settle { item } => {
                let held = Self::item_at(state, item)?;
                if held.holder != actor {
                    return Err(MechError::new("only a holder may settle what it holds"));
                }
                if held.listing.is_none() {
                    return Err(MechError::new("this item is not on the showcase"));
                }
                self.charge_execution(state, actor)?;
                self.settle(state, item, now, events)?;
                Ok(())
            }
            ShowcaseAction::Buy { item } => {
                let held = Self::item_at(state, item)?;
                if held.holder == actor {
                    return Err(MechError::new("a holder cannot buy what it already holds"));
                }
                if held.listing.is_none() {
                    return Err(MechError::new("this item is not on the showcase"));
                }
                let standing = self.standing(&held, now)?;
                // A withdrawal that has waited out its lag has taken the item off the showcase,
                // whether or not its holder has touched it since. Refusing here leaves the state
                // exactly as it was: the withdrawal lands when the holder next acts.
                let Some(price) = standing.binds else {
                    return Err(MechError::new("this item has left the showcase"));
                };
                let total = price
                    .checked_add(self.execution)
                    .map_err(|_| MechError::new("this purchase is wider than money"))?;
                if state.purses[actor.0 as usize].balance < total {
                    return Err(MechError::new(
                        "not enough to pay the price and the execution",
                    ));
                }

                // The charge is a debt and the commission a fee, and a debt is paid before a fee:
                // a listing owing the whole declared price leaves the venue nothing rather than
                // leaving the pool short of a charge that was properly owed.
                let charge = standing.owed.min(price);
                let left = price.checked_sub(charge)?;
                let commission = price
                    .mul_div(self.commission, WHOLE, Rounding::Up)?
                    .min(left);
                let to_seller = left.checked_sub(commission)?;

                let (buyer, seller, venue) = (
                    actor.0 as usize,
                    held.holder.0 as usize,
                    self.venue.0 as usize,
                );
                state.debit(buyer, price)?;
                state.pool = state.pool.checked_add(charge)?;
                state.purses[seller].paid = state.purses[seller].paid.checked_add(charge)?;
                state.credit(venue, commission)?;
                state.credit(seller, to_seller)?;
                state.purses[buyer].traded = state.purses[buyer].traded.saturating_add(1);
                state.purses[seller].traded = state.purses[seller].traded.saturating_add(1);
                self.charge_execution(state, actor)?;

                // It stays listed, at the price its new holder just proved it was worth to them.
                // On a market whose disease is that assets stop moving, handing every buyer a free
                // pause off the showcase would be handing the disease a way back in.
                state.items[item.0 as usize].holder = actor;
                state.items[item.0 as usize].listing = Some(Listing {
                    declared: price,
                    assessed_to: now,
                    pending: None,
                });
                events.push(ShowcaseEvent::Bought {
                    item,
                    from: held.holder,
                    by: actor,
                    price,
                    charge,
                    commission,
                });
                Ok(())
            }
        }
    }

    /// The only rule with no actor is the distribution, and it touches no listing.
    ///
    /// That is the second reason the charge is lazy: a tick that had to assess every item would be
    /// a tick that could abort, and an abort in a sweep is a run that produced no evidence at all.
    fn tick(
        &self,
        state: &mut ShowcaseState,
        now: Tick,
        events: &mut Vec<ShowcaseEvent>,
    ) -> Result<(), Abort> {
        if self.period == 0 || now.0.saturating_sub(state.distributed_through.0) < self.period {
            return Ok(());
        }
        self.distribute(state, events)?;
        state.distributed_through = now;
        Ok(())
    }

    /// Conservation, after every applied action, in constant time.
    ///
    /// Nothing here walks the state. Money is the one thing a mechanism can destroy in a single
    /// transition, so it is the one thing worth asking about that often, and asking costs two
    /// additions because [`ShowcaseState`] carries what its purses hold rather than counting them
    /// when asked. What a walk is needed for is in [`Showcase::audit`], a tick away at most.
    fn check_invariants(&self, state: &ShowcaseState) -> Result<(), InvariantViolation> {
        let accounted = state
            .held
            .checked_add(state.pool)?
            .checked_add(state.spent)?;
        if accounted != state.opened_with {
            return Err(InvariantViolation::new(
                "the purses, the pool and what execution spent do not add up to what the world \
                 opened with",
            ));
        }
        Ok(())
    }

    /// Everything above, and everything that costs a walk of the showcase to establish.
    ///
    /// The running total is counted back out of the purses here, which is what makes it a total
    /// rather than a number somebody hoped was one: a path that moved a balance without going
    /// through the two doors would agree with itself after every action and disagree here.
    fn audit(&self, state: &ShowcaseState) -> Result<(), InvariantViolation> {
        self.check_invariants(state)?;

        let mut counted = Amount::ZERO;
        for purse in &state.purses {
            counted = counted.checked_add(purse.balance)?;
        }
        if counted != state.held {
            return Err(InvariantViolation::new(
                "the purses hold something other than what the state says they hold",
            ));
        }

        for item in &state.items {
            if item.holder.0 as usize >= state.purses.len() {
                return Err(InvariantViolation::new("an item is held by nobody"));
            }
            let Some(listing) = item.listing else {
                continue;
            };
            if listing.declared.is_zero() {
                return Err(InvariantViolation::new("a listing declares no price"));
            }
            if let Some(change) = listing.pending {
                if change.effective < listing.assessed_to {
                    return Err(InvariantViolation::new(
                        "a change to a listing takes effect before the listing was last assessed",
                    ));
                }
                if change.price.is_some_and(Amount::is_zero) {
                    return Err(InvariantViolation::new("a pending price is zero"));
                }
            }
        }
        Ok(())
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

    use sting_karman_core::{Agent, NullObserver, RunRng, World, conformance};

    use super::*;

    /// Everybody in these tests, by the seat the engine gives them.
    const VENUE: AgentId = AgentId(0);
    const ALICE: AgentId = AgentId(1);
    const BOB: AgentId = AgentId(2);

    /// One item, and one collection for it to belong to.
    const ITEM: ItemId = ItemId(0);
    const GIFTS: CollectionId = CollectionId(0);

    /// A tick worth a day, so a year of charge is a year of ticks and the arithmetic is legible.
    fn showcase(charge: u128) -> Showcase {
        Showcase {
            venue: VENUE,
            seconds_per_tick: SecondsPerTick(NonZeroU64::new(86_400).unwrap()),
            charge: vec![Amount::new(charge)],
            lag: 4,
            commission: Amount::new(500),
            execution: Amount::ZERO,
            arrears: Arrears::Forfeit,
            distribution: Distribution::Even,
            share: WHOLE,
            period: 0,
        }
    }

    fn purse(balance: u128) -> Purse {
        Purse {
            balance: Amount::new(balance),
            ..Purse::default()
        }
    }

    fn cash(of: u128) -> Amount<Cash> {
        Amount::new(of)
    }

    /// One item held by Alice, and three purses: the venue's, Alice's and Bob's.
    fn world(alice: u128, bob: u128) -> ShowcaseState {
        ShowcaseState::open(
            vec![Item {
                collection: GIFTS,
                holder: ALICE,
                listing: None,
            }],
            vec![purse(0), purse(alice), purse(bob)],
        )
        .unwrap()
    }

    /// Applies one action and returns what it reported, failing loudly on a rejection.
    fn act(
        mechanism: &Showcase,
        state: &mut ShowcaseState,
        actor: AgentId,
        action: ShowcaseAction,
        now: u64,
    ) -> Vec<ShowcaseEvent> {
        let mut events = Vec::new();
        mechanism
            .apply(state, actor, action, Tick(now), &mut events)
            .unwrap_or_else(|e| panic!("{action:?} at {now} was refused: {e}"));
        mechanism
            .check_invariants(state)
            .expect("the state broke its own rules");
        events
    }

    fn listed(state: &ShowcaseState) -> Listing {
        state.items[0].listing.expect("the item is on the showcase")
    }

    /// A year of holding at ten per cent costs a tenth of the declared price, and the two-step
    /// arithmetic that avoids overflowing does not change the answer.
    #[test]
    fn a_year_of_holding_costs_the_rate() {
        let mechanism = showcase(1_000);
        let mut state = world(1_000_000, 0);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Settle { item: ITEM },
            365,
        );
        assert_eq!(state.pool, cash(100));
        assert_eq!(state.purses[1].balance, cash(999_900));
        assert_eq!(listed(&state).assessed_to, Tick(365));
    }

    /// The charge rounds down, so below a price the mechanism collects nothing at all. That floor
    /// is the applicability limit of the whole design, and it is meant to be visible in a result
    /// rather than hidden inside one.
    #[test]
    fn a_price_too_small_to_charge_is_charged_nothing() {
        let mechanism = showcase(1_000);
        let mut state = world(1_000, 0);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(9),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Settle { item: ITEM },
            365,
        );
        assert_eq!(
            state.pool,
            Amount::ZERO,
            "a tenth of nine rounds to nothing"
        );
        assert_eq!(state.purses[1].balance, cash(1_000));
    }

    /// The second rounding is pinned separately from the first, because a price small enough to
    /// floor the annual charge to nothing leaves the second step nothing to round. Half a year of
    /// a hundred is forty-nine and change, and the mechanism takes forty-nine.
    #[test]
    fn a_part_year_is_charged_down_to_the_unit() {
        let mechanism = showcase(1_000);
        let mut state = world(1_000_000, 0);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Settle { item: ITEM },
            182,
        );
        assert_eq!(
            state.pool,
            cash(49),
            "a hundred a year, over a hundred and eighty-two days"
        );
    }

    /// However long a listing goes unsettled, it never owes more than what was declared on it.
    /// Without that, entering the showcase is an unbounded liability and nobody rational enters.
    #[test]
    fn a_listing_never_owes_more_than_it_declared() {
        let mechanism = showcase(1_000);
        let mut state = world(1_000_000, 0);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Settle { item: ITEM },
            365 * 40,
        );
        assert_eq!(
            state.pool,
            cash(1_000),
            "forty years of charge, capped at the declaration"
        );
    }

    /// A lower price cannot defeat a buyout, so delaying it buys nothing and costs discovery in
    /// the one direction a market of dead assets most needs to move.
    #[test]
    fn a_lower_price_binds_at_once_and_a_higher_one_waits() {
        let mechanism = showcase(0);
        let mut state = world(1_000, 0);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(100),
            },
            0,
        );

        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Reprice {
                item: ITEM,
                price: cash(60),
            },
            1,
        );
        assert_eq!(listed(&state).declared, cash(60));
        assert_eq!(listed(&state).pending, None);

        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Reprice {
                item: ITEM,
                price: cash(90),
            },
            2,
        );
        assert_eq!(
            listed(&state).declared,
            cash(60),
            "the old price still binds"
        );
        assert_eq!(
            listed(&state).pending,
            Some(Pending {
                price: Some(cash(90)),
                effective: Tick(6)
            })
        );
    }

    /// The raise binds when its lag runs out, whether or not anybody has touched the item since.
    /// A buyer arriving after that pays the new price; one arriving before pays the old.
    #[test]
    fn a_raise_binds_on_its_own_clock() {
        let mechanism = showcase(0);
        let mut state = world(1_000, 1_000);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(100),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Reprice {
                item: ITEM,
                price: cash(300),
            },
            0,
        );

        let mut early = state.clone();
        let events = act(
            &mechanism,
            &mut early,
            BOB,
            ShowcaseAction::Buy { item: ITEM },
            3,
        );
        assert!(matches!(events[0], ShowcaseEvent::Bought { price, .. } if price == cash(100)));

        let events = act(
            &mechanism,
            &mut state,
            BOB,
            ShowcaseAction::Buy { item: ITEM },
            4,
        );
        assert!(matches!(events[0], ShowcaseEvent::Bought { price, .. } if price == cash(300)));
    }

    /// A withdrawal that has waited out its lag has taken the item off the showcase, and refusing
    /// the buy is what leaves the state exactly as it was — the withdrawal lands when its holder
    /// next acts, and no third party's action settles somebody else's charge.
    #[test]
    fn a_matured_withdrawal_refuses_a_buyer_without_touching_anything() {
        let mechanism = showcase(1_000);
        let mut state = world(1_000_000, 1_000_000);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Withdraw { item: ITEM },
            10,
        );

        let before = state.clone();
        let mut events = Vec::new();
        let refused = mechanism.apply(
            &mut state,
            BOB,
            ShowcaseAction::Buy { item: ITEM },
            Tick(20),
            &mut events,
        );
        assert!(refused.is_err(), "the item had left the showcase");
        assert_eq!(state, before, "a refusal changed the state");
        assert!(events.is_empty(), "a refusal reported an event");
    }

    /// The charge is a debt and the commission a fee, and a debt is paid before a fee: a listing
    /// owing its whole declared price leaves the venue nothing rather than leaving the pool short
    /// of a charge that was properly owed.
    #[test]
    fn a_buyout_pays_the_charge_before_the_commission() {
        let mechanism = showcase(1_000);
        let mut state = world(0, 10_000);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        let events = act(
            &mechanism,
            &mut state,
            BOB,
            ShowcaseAction::Buy { item: ITEM },
            365,
        );

        // A tenth of the price to the pool, five per cent of the price to the venue, the rest to
        // the seller — whose own purse was never touched, which is why a buyout cannot fall into
        // arrears.
        assert_eq!(state.pool, cash(100));
        assert_eq!(state.purses[0].balance, cash(50));
        assert_eq!(state.purses[1].balance, cash(850));
        assert_eq!(state.purses[2].balance, cash(9_000));
        assert!(matches!(
            events[0],
            ShowcaseEvent::Bought { charge, commission, .. }
                if charge == cash(100) && commission == cash(50)
        ));
    }

    /// A seller with nothing in its purse still sells: the charge comes out of the price the buyer
    /// paid. The one transition the mechanism exists to produce is the one that cannot fail for
    /// want of money.
    #[test]
    fn a_buyout_cannot_fall_into_arrears() {
        let mechanism = showcase(10_000);
        let mut state = world(0, 10_000);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        let events = act(
            &mechanism,
            &mut state,
            BOB,
            ShowcaseAction::Buy { item: ITEM },
            365 * 5,
        );
        assert!(matches!(
            events[0],
            ShowcaseEvent::Bought { charge, commission, .. }
                if charge == cash(1_000) && commission == Amount::ZERO
        ));
        assert_eq!(
            state.purses[1].balance,
            Amount::ZERO,
            "the debt ate the whole price"
        );
    }

    /// It stays listed at the price its new holder just proved it was worth to them. On a market
    /// whose disease is that assets stop moving, a free pause off the showcase after every
    /// purchase would be handing the disease a way back in.
    #[test]
    fn a_bought_item_stays_on_the_showcase_with_its_buyer() {
        let mechanism = showcase(0);
        let mut state = world(0, 1_000);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(100),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            BOB,
            ShowcaseAction::Buy { item: ITEM },
            5,
        );
        assert_eq!(state.items[0].holder, BOB);
        assert_eq!(
            listed(&state),
            Listing {
                declared: cash(100),
                assessed_to: Tick(5),
                pending: None
            }
        );
    }

    /// A holder who cannot pay gives up what it has, keeps the item, and leaves the showcase. The
    /// rest of the debt is written off, which is exactly what makes an empty purse a way to sit on
    /// an asset for nothing — the cost of the gentler of the two rules, and why both are swept.
    #[test]
    fn a_holder_who_cannot_pay_forfeits() {
        let mechanism = showcase(1_000);
        let mut state = world(30, 0);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        let events = act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Settle { item: ITEM },
            365,
        );
        assert!(matches!(
            events[0],
            ShowcaseEvent::Forfeited { paid, written_off, .. }
                if paid == cash(30) && written_off == cash(70)
        ));
        assert_eq!(
            state.items[0].holder, ALICE,
            "the item stayed with its holder"
        );
        assert_eq!(state.items[0].listing, None);
        assert_eq!(state.pool, cash(30));
    }

    /// Under the harsher rule the venue buys at the declared price, the debt comes out of it, and
    /// what is left reaches the holder. An empty purse becomes the most expensive way to hold.
    #[test]
    fn a_holder_who_cannot_pay_is_bought_out_by_the_venue() {
        let mechanism = Showcase {
            arrears: Arrears::ForcedSale,
            ..showcase(1_000)
        };
        let mut state = ShowcaseState::open(
            vec![Item {
                collection: GIFTS,
                holder: ALICE,
                listing: None,
            }],
            vec![purse(5_000), purse(30), purse(0)],
        )
        .unwrap();
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        let events = act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Settle { item: ITEM },
            365,
        );
        assert!(matches!(
            events[0],
            ShowcaseEvent::ForceSold { price, charge, .. }
                if price == cash(1_000) && charge == cash(100)
        ));
        assert_eq!(state.items[0].holder, VENUE);
        assert_eq!(state.purses[0].balance, cash(4_000));
        assert_eq!(
            state.purses[1].balance,
            cash(930),
            "thirty it had, and nine hundred over"
        );
        assert_eq!(state.pool, cash(100));
    }

    /// A venue that has spent what it took cannot afford its own rule, and the mechanism says what
    /// happens then rather than assuming it never does.
    #[test]
    fn a_venue_that_cannot_pay_falls_back_to_forfeit() {
        let mechanism = Showcase {
            arrears: Arrears::ForcedSale,
            ..showcase(1_000)
        };
        let mut state = ShowcaseState::open(
            vec![Item {
                collection: GIFTS,
                holder: ALICE,
                listing: None,
            }],
            vec![purse(10), purse(30), purse(0)],
        )
        .unwrap();
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        let events = act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Settle { item: ITEM },
            365,
        );
        assert!(matches!(events[0], ShowcaseEvent::Forfeited { .. }));
        assert_eq!(state.items[0].holder, ALICE);
    }

    /// The venue takes what is not shared out, everybody else takes an equal part of what is, and
    /// what the division cannot place stays in the pool rather than being invented into existence.
    #[test]
    fn a_distribution_hands_out_what_divides_and_keeps_what_does_not() {
        let mechanism = Showcase {
            share: Amount::new(9_000),
            period: 10,
            ..showcase(1_000)
        };
        let mut state = world(1_000_000, 0);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(1_000),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Settle { item: ITEM },
            365,
        );
        assert_eq!(state.pool, cash(100));

        let mut events = Vec::new();
        mechanism.tick(&mut state, Tick(365), &mut events).unwrap();
        // Ninety of the hundred are shared between two participants, so forty-five each; the venue
        // takes the ten it was not asked to share.
        assert_eq!(state.purses[0].balance, cash(10));
        assert_eq!(state.purses[1].balance, cash(999_945));
        assert_eq!(state.purses[2].balance, cash(45));
        assert_eq!(state.pool, Amount::ZERO);
        mechanism.check_invariants(&state).unwrap();
    }

    /// The venue holds a purse and takes what rules send it, and it is the actor of nothing.
    ///
    /// The buy below is one the venue could otherwise make — it holds the money and does not hold
    /// the item — so what refuses it is the rule this test is named after and nothing else.
    #[test]
    fn the_venue_does_not_act() {
        let mechanism = showcase(0);
        let mut state = ShowcaseState::open(
            vec![Item {
                collection: GIFTS,
                holder: ALICE,
                listing: None,
            }],
            vec![purse(10_000), purse(0), purse(0)],
        )
        .unwrap();
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(100),
            },
            0,
        );
        let before = state.clone();

        let mut events = Vec::new();
        let refused = mechanism.apply(
            &mut state,
            VENUE,
            ShowcaseAction::Buy { item: ITEM },
            Tick(1),
            &mut events,
        );
        assert!(refused.is_err(), "the venue bought something");
        assert_eq!(state, before);
    }

    /// Nothing is created or destroyed: every purse, the pool and what execution spent add up to
    /// what the world opened with, through a whole life of one item.
    #[test]
    fn nothing_is_created_or_destroyed() {
        let mechanism = Showcase {
            execution: cash(1),
            period: 50,
            ..showcase(2_000)
        };
        let mut state = ShowcaseState::open(
            vec![Item {
                collection: GIFTS,
                holder: ALICE,
                listing: None,
            }],
            vec![purse(100), purse(5_000), purse(5_000)],
        )
        .unwrap();
        let opened = state.opened_with();
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(400),
            },
            0,
        );
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::Reprice {
                item: ITEM,
                price: cash(300),
            },
            40,
        );
        act(
            &mechanism,
            &mut state,
            BOB,
            ShowcaseAction::Buy { item: ITEM },
            100,
        );
        act(
            &mechanism,
            &mut state,
            BOB,
            ShowcaseAction::Reprice {
                item: ITEM,
                price: cash(900),
            },
            150,
        );
        let mut events = Vec::new();
        mechanism.tick(&mut state, Tick(200), &mut events).unwrap();
        act(
            &mechanism,
            &mut state,
            BOB,
            ShowcaseAction::Withdraw { item: ITEM },
            300,
        );
        act(
            &mechanism,
            &mut state,
            BOB,
            ShowcaseAction::Settle { item: ITEM },
            400,
        );

        let held = state.purses.iter().fold(Amount::ZERO, |sum, purse| {
            sum.checked_add(purse.balance).unwrap()
        });
        assert_eq!(
            held.checked_add(state.pool)
                .unwrap()
                .checked_add(state.spent)
                .unwrap(),
            opened
        );
        assert_eq!(
            state.items[0].listing, None,
            "the withdrawal landed when its holder settled"
        );
    }

    /// A rejection leaves the state exactly as it found it, and reports nothing.
    #[test]
    fn a_rejection_changes_nothing() {
        let mechanism = Showcase {
            execution: cash(5),
            ..showcase(1_000)
        };
        let mut state = world(1_000, 1);
        act(
            &mechanism,
            &mut state,
            ALICE,
            ShowcaseAction::List {
                item: ITEM,
                price: cash(100),
            },
            0,
        );
        let before = state.clone();

        for (who, action) in [
            (BOB, ShowcaseAction::Buy { item: ITEM }),
            (
                BOB,
                ShowcaseAction::Reprice {
                    item: ITEM,
                    price: cash(50),
                },
            ),
            (
                ALICE,
                ShowcaseAction::List {
                    item: ITEM,
                    price: cash(50),
                },
            ),
            (
                ALICE,
                ShowcaseAction::Reprice {
                    item: ITEM,
                    price: Amount::ZERO,
                },
            ),
            (ALICE, ShowcaseAction::Buy { item: ItemId(7) }),
        ] {
            let mut events = Vec::new();
            let refused = mechanism.apply(&mut state, who, action, Tick(1), &mut events);
            assert!(refused.is_err(), "{action:?} should have been refused");
            assert_eq!(
                state, before,
                "{action:?} changed the state on its way to being refused"
            );
            assert!(
                events.is_empty(),
                "{action:?} reported an event it did not cause"
            );
        }
    }

    /// A population that lists, reprices and buys, twice from one seed, reaching the same state
    /// and having done the same work.
    #[test]
    fn conformance_determinism() {
        conformance::assert_deterministic(64, 11, || {
            let mechanism = Showcase {
                execution: cash(1),
                period: 8,
                ..showcase(1_500)
            };
            let items = (0..8)
                .map(|_| Item {
                    collection: GIFTS,
                    holder: ALICE,
                    listing: None,
                })
                .collect();
            let state =
                ShowcaseState::open(items, vec![purse(0), purse(100_000), purse(100_000)]).unwrap();
            World::new(
                mechanism,
                state,
                vec![
                    Box::new(Idle) as Box<dyn Agent<Showcase>>,
                    Box::new(Restless { next: 0 }),
                    Box::new(Buyer),
                ],
            )
        });
    }

    /// The venue, which never acts.
    struct Idle;

    impl Agent<Showcase> for Idle {
        fn decide(
            &mut self,
            _me: AgentId,
            _state: &ShowcaseState,
            _now: Tick,
            _rng: &mut RunRng,
        ) -> Option<ShowcaseAction> {
            None
        }
    }

    /// Lists what it holds and keeps naming new prices, drawing every one from the run's stream.
    struct Restless {
        next: usize,
    }

    impl Agent<Showcase> for Restless {
        fn decide(
            &mut self,
            me: AgentId,
            state: &ShowcaseState,
            _now: Tick,
            rng: &mut RunRng,
        ) -> Option<ShowcaseAction> {
            let item = ItemId(u32::try_from(self.next % state.items.len()).ok()?);
            self.next += 1;
            let held = state.items[item.0 as usize];
            if held.holder != me {
                return None;
            }
            let price = Amount::new(u128::from(rng.below(900)) + 100);
            Some(match held.listing {
                None => ShowcaseAction::List { item, price },
                Some(_) => ShowcaseAction::Reprice { item, price },
            })
        }
    }

    /// Takes whatever is listed and not already its own.
    struct Buyer;

    impl Agent<Showcase> for Buyer {
        fn decide(
            &mut self,
            me: AgentId,
            state: &ShowcaseState,
            _now: Tick,
            _rng: &mut RunRng,
        ) -> Option<ShowcaseAction> {
            let at = state
                .items
                .iter()
                .position(|item| item.listing.is_some() && item.holder != me)?;
            Some(ShowcaseAction::Buy {
                item: ItemId(u32::try_from(at).ok()?),
            })
        }
    }

    /// The engine drives it end to end without an abort, and the invariants hold at every step.
    #[test]
    fn a_run_of_the_engine_holds_its_invariants() {
        let mechanism = Showcase {
            execution: cash(1),
            period: 5,
            ..showcase(2_500)
        };
        let items = (0..6)
            .map(|_| Item {
                collection: GIFTS,
                holder: ALICE,
                listing: None,
            })
            .collect();
        let state =
            ShowcaseState::open(items, vec![purse(0), purse(50_000), purse(50_000)]).unwrap();
        let mut world = World::new(
            mechanism,
            state,
            vec![
                Box::new(Idle) as Box<dyn Agent<Showcase>>,
                Box::new(Restless { next: 0 }),
                Box::new(Buyer),
            ],
        );
        let mut rng = RunRng::new(7, 0);
        world
            .run(200, &mut rng, &mut NullObserver)
            .expect("the run aborted");
        assert!(world.stats().actions_applied > 0, "nothing happened");
    }
}
