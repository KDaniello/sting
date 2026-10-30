use std::{collections::BTreeMap, fmt};

use sting_karman_core::{Abort, InvariantViolation, MechError, Mechanism, PathSource, SourceError};
use sting_karman_types::{
    AgentId, Amount, ArithError, MarketId, Rounding, SecondsPerTick, Tick, Unit,
};

use crate::{
    Collateral, Loan, Seconds, WAD, Wad,
    accrual::{Accrual, AccrualError, accrue},
    action::{Given, LendingAction, LendingEvent},
    liquidation::{bad_debt, incentive, seizure},
    market::{LendingState, Market, MarketParams, Position},
    price::Price,
    rate::BorrowRate,
    shares::{Flow, to_assets, to_shares},
};

/// What one market is priced by: its terms, the oracle valuing its collateral, and the model
/// charging its borrowers.
#[derive(Clone)]
struct Feeds<O, R> {
    params: MarketParams,
    oracle: O,
    rate: R,
}

/// A lending market, with Morpho Blue's semantics.
///
/// Stateless itself: the terms and the two feeds are configuration, and everything a run moves
/// lives in [`LendingState`]. A market therefore has to be declared twice at a composition root —
/// its terms here and its opening balances there — because the two have different lifetimes and
/// only one of them is part of the run's result.
#[derive(Clone)]
pub struct Lending<O, R> {
    feeds: BTreeMap<MarketId, Feeds<O, R>>,
    seconds_per_tick: SecondsPerTick,
}

impl<O, R> fmt::Debug for Lending<O, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lending")
            .field("markets", &self.feeds.len())
            .finish_non_exhaustive()
    }
}

impl<O, R> Lending<O, R> {
    /// A mechanism with no markets, where one tick stands for `seconds_per_tick`.
    #[must_use]
    pub fn new(seconds_per_tick: SecondsPerTick) -> Self {
        Lending {
            feeds: BTreeMap::new(),
            seconds_per_tick,
        }
    }

    /// Declares the terms and feeds of the market under `id`.
    ///
    /// # Errors
    ///
    /// Refuses an identifier already declared: replacing the terms of a market silently would
    /// reprice every position already held under it.
    pub fn with_market(
        mut self,
        id: MarketId,
        params: MarketParams,
        oracle: O,
        rate: R,
    ) -> Result<Self, MechError> {
        if params.lltv >= WAD {
            // At the full value of its collateral a position may owe everything it holds, which
            // leaves nothing to pay a liquidator out of — and a threshold nobody is paid to
            // enforce is one a position crosses and never comes back from.
            return Err(MechError::new(
                "a threshold must sit below the collateral's full value",
            ));
        }
        if self
            .feeds
            .insert(
                id,
                Feeds {
                    params,
                    oracle,
                    rate,
                },
            )
            .is_some()
        {
            return Err(MechError::new(
                "a market is already declared under that identifier",
            ));
        }
        Ok(self)
    }

    /// The wall-clock seconds between two ticks.
    ///
    /// The engine's clock only moves forward and `last_update` is only ever set to a tick that
    /// has arrived, so the saturation never triggers. What it rules out is accruing backwards if
    /// either of those ever stopped being true.
    fn elapsed(&self, last: Tick, now: Tick) -> Amount<Seconds> {
        Amount::new(self.seconds_per_tick.seconds(now.0.saturating_sub(last.0)))
    }
}

/// Every write one transition makes, resolved before any of it lands.
///
/// The reference reverts a transaction that fails a check after moving storage; there is no such
/// undo here, so the whole transition — accrual included — is computed against copies and
/// written in one step. Rejection safety is then a property of the shape rather than of
/// remembering to unwind.
struct Settled {
    market: Market,
    /// The holder this action rewrites and their position: the actor's, or a liquidated
    /// borrower's. One slot rather than one per role, because the roles are not always different
    /// agents — an agent may liquidate itself — and two slots for one holder would let the later
    /// write discard the earlier.
    subject: (AgentId, Position),
    /// The fee recipient's position, when accrual minted shares for someone the action does not
    /// already rewrite.
    fee: Option<(AgentId, Position)>,
    /// What accrual did, when it ran.
    accrued: Option<LendingEvent>,
    /// What the action did.
    done: LendingEvent,
}

impl<O, R> Lending<O, R>
where
    O: PathSource<Price<Collateral, Loan>>,
    R: BorrowRate,
{
    /// Brings a market up to `now`, or `None` when no time has passed since it was last read.
    ///
    /// Several transitions on one tick therefore compound once between them rather than once
    /// each, which is the reference's behaviour: the first transaction of a block accrues and the
    /// rest see the result.
    fn bring_forward(
        &self,
        market: &Market,
        feeds: &Feeds<O, R>,
        now: Tick,
    ) -> Result<Option<Accrual>, AccrualError> {
        let elapsed = self.elapsed(market.last_update, now);
        if elapsed.is_zero() {
            return Ok(None);
        }
        let rate = feeds.rate.per_second(market, now)?;
        Ok(Some(accrue(market, rate, elapsed, now)?))
    }

    /// Whether a position can stand as it is.
    ///
    /// The debt is valued as it would be repaid and the collateral as it would be sold, so both
    /// roundings run against the holder and a position on the exact boundary counts as unhealthy.
    ///
    /// A position owing nothing needs no price, and none is asked for: a market whose oracle has
    /// nothing to say at this tick can still be supplied to and withdrawn from.
    fn stands(
        market: &Market,
        feeds: &Feeds<O, R>,
        position: &Position,
        now: Tick,
    ) -> Result<bool, MechError> {
        if position.borrow_shares.is_zero() {
            return Ok(true);
        }
        Ok(covered(
            market,
            feeds.params.lltv,
            position,
            Self::price(feeds, now)?,
        )?)
    }

    /// The price of this market's collateral at `now`.
    ///
    /// A source that cannot answer refuses the action rather than stopping the run, because that
    /// is what the reference does with a transaction it cannot price: it reverts one call and
    /// the chain goes on.
    fn price(feeds: &Feeds<O, R>, now: Tick) -> Result<Price<Collateral, Loan>, MechError> {
        feeds
            .oracle
            .value_at(now)
            .map_err(|source| MechError::new(source.reason))
    }

    /// Resolves one action in full, touching nothing.
    #[allow(
        clippy::too_many_lines,
        reason = "one branch per action, each resolving in full before anything moves: splitting it would hide that a refusal leaves the state untouched"
    )]
    fn resolve(
        &self,
        state: &LendingState,
        actor: AgentId,
        action: LendingAction,
        now: Tick,
    ) -> Result<Settled, MechError> {
        let id = action.market();
        let stored = state
            .market(id)
            .ok_or_else(|| MechError::new("no market under that identifier"))?;
        let feeds = self
            .feeds
            .get(&id)
            .ok_or_else(|| MechError::new("the market has no rate model or oracle"))?;

        let mut market = stored;
        let holder = action.subject(actor);
        let mut subject = state.position(id, holder);
        let mut fee = None;
        let mut accrued = None;

        if action.accrues_first()
            && let Some(step) = self.bring_forward(&stored, feeds, now)?
        {
            market = step.market;
            accrued = Some(LendingEvent::InterestAccrued {
                market: id,
                interest: step.interest,
                fee_shares: step.fee_shares,
            });
            if !step.fee_shares.is_zero() {
                let recipient = feeds.params.fee_recipient;
                let mut paid = if recipient == holder {
                    subject
                } else {
                    state.position(id, recipient)
                };
                paid.supply_shares = paid.supply_shares.checked_add(step.fee_shares)?;
                // The fee lands before the action, as it would if the two were separate
                // transactions in that order — including when the holder being rewritten is
                // the recipient and the action then reads the shares it was just minted.
                if recipient == holder {
                    subject = paid;
                } else {
                    fee = Some((recipient, paid));
                }
            }
        }

        let done = match action {
            LendingAction::Supply { given, .. } => {
                let (assets, shares) = split(
                    &given,
                    market.total_supply_assets,
                    market.total_supply_shares,
                    Flow::In,
                )?;
                subject.supply_shares = subject.supply_shares.checked_add(shares)?;
                market.total_supply_shares = market.total_supply_shares.checked_add(shares)?;
                market.total_supply_assets = market.total_supply_assets.checked_add(assets)?;
                LendingEvent::Supplied {
                    market: id,
                    holder: actor,
                    assets,
                    shares,
                }
            }
            LendingAction::Withdraw { given, .. } => {
                let (assets, shares) = split(
                    &given,
                    market.total_supply_assets,
                    market.total_supply_shares,
                    Flow::Out,
                )?;
                subject.supply_shares = subject.supply_shares.checked_sub(shares)?;
                market.total_supply_shares = market.total_supply_shares.checked_sub(shares)?;
                market.total_supply_assets = market.total_supply_assets.checked_sub(assets)?;
                if market.total_borrow_assets > market.total_supply_assets {
                    return Err(MechError::new(
                        "withdrawal exceeds the market's idle liquidity",
                    ));
                }
                LendingEvent::Withdrawn {
                    market: id,
                    holder: actor,
                    assets,
                    shares,
                }
            }
            LendingAction::Borrow { given, .. } => {
                let (assets, shares) = split(
                    &given,
                    market.total_borrow_assets,
                    market.total_borrow_shares,
                    Flow::Out,
                )?;
                subject.borrow_shares = subject.borrow_shares.checked_add(shares)?;
                market.total_borrow_shares = market.total_borrow_shares.checked_add(shares)?;
                market.total_borrow_assets = market.total_borrow_assets.checked_add(assets)?;
                if !Self::stands(&market, feeds, &subject, now)? {
                    return Err(MechError::new("the position could not carry the debt"));
                }
                if market.total_borrow_assets > market.total_supply_assets {
                    return Err(MechError::new(
                        "borrowing exceeds the market's idle liquidity",
                    ));
                }
                LendingEvent::Borrowed {
                    market: id,
                    holder: actor,
                    assets,
                    shares,
                }
            }
            LendingAction::Repay { given, .. } => {
                let (assets, shares) = split(
                    &given,
                    market.total_borrow_assets,
                    market.total_borrow_shares,
                    Flow::In,
                )?;
                subject.borrow_shares = subject.borrow_shares.checked_sub(shares)?;
                market.total_borrow_shares = market.total_borrow_shares.checked_sub(shares)?;
                // The assets a repayment settles are rounded up against the payer, so clearing
                // the last of a market's debt can name more than it records as owed. The floor is
                // the reference's, and the alternative to it is a debt nobody can clear.
                market.total_borrow_assets = market
                    .total_borrow_assets
                    .checked_sub(assets)
                    .unwrap_or(Amount::ZERO);
                LendingEvent::Repaid {
                    market: id,
                    holder: actor,
                    assets,
                    shares,
                }
            }
            LendingAction::SupplyCollateral { assets, .. } => {
                if assets.is_zero() {
                    return Err(MechError::new("an action must state a non-zero amount"));
                }
                subject.collateral = subject.collateral.checked_add(assets)?;
                LendingEvent::CollateralAdded {
                    market: id,
                    holder: actor,
                    assets,
                }
            }
            LendingAction::WithdrawCollateral { assets, .. } => {
                if assets.is_zero() {
                    return Err(MechError::new("an action must state a non-zero amount"));
                }
                subject.collateral = subject.collateral.checked_sub(assets)?;
                if !Self::stands(&market, feeds, &subject, now)? {
                    return Err(MechError::new("the debt would be left uncovered"));
                }
                LendingEvent::CollateralRemoved {
                    market: id,
                    holder: actor,
                    assets,
                }
            }
            LendingAction::Liquidate {
                borrower, given, ..
            } => {
                if given.is_zero() {
                    return Err(MechError::new("an action must state a non-zero amount"));
                }
                // One reading of the price serves the test and the trade, as it does in the
                // reference: a position judged at one price and settled at another would be a
                // liquidation nobody could reproduce.
                let price = Self::price(feeds, now)?;
                if covered(&market, feeds.params.lltv, &subject, price)? {
                    return Err(MechError::new("the position still covers its debt"));
                }
                let taken = seizure(given, &market, price, incentive(feeds.params.lltv)?)?;
                let repaid = to_assets(
                    taken.repaid_shares,
                    market.total_borrow_assets,
                    market.total_borrow_shares,
                    Flow::In,
                )?;

                subject.borrow_shares = subject.borrow_shares.checked_sub(taken.repaid_shares)?;
                market.total_borrow_shares = market
                    .total_borrow_shares
                    .checked_sub(taken.repaid_shares)?;
                market.total_borrow_assets = market
                    .total_borrow_assets
                    .checked_sub(repaid)
                    .unwrap_or(Amount::ZERO);
                subject.collateral = subject.collateral.checked_sub(taken.seized)?;

                // Written off only once the repayment has landed, so the debt that is about to
                // be forgiven is not first charged the interest of a position nobody can service.
                let mut written_off = Amount::ZERO;
                let mut written_off_shares = Amount::ZERO;
                if subject.collateral.is_zero() {
                    written_off_shares = subject.borrow_shares;
                    written_off = bad_debt(&market, written_off_shares)?;
                    market.total_borrow_assets =
                        market.total_borrow_assets.checked_sub(written_off)?;
                    // The loss lands on the supply side. Both totals fall by the same amount, so
                    // what nobody borrowed is where it was, and what a supply share is worth is
                    // the only thing that moved.
                    market.total_supply_assets =
                        market.total_supply_assets.checked_sub(written_off)?;
                    market.total_borrow_shares =
                        market.total_borrow_shares.checked_sub(written_off_shares)?;
                    subject.borrow_shares = Amount::ZERO;
                }

                LendingEvent::Liquidated {
                    market: id,
                    liquidator: actor,
                    borrower,
                    repaid,
                    repaid_shares: taken.repaid_shares,
                    seized: taken.seized,
                    bad_debt: written_off,
                    bad_debt_shares: written_off_shares,
                }
            }
        };

        Ok(Settled {
            market,
            subject: (holder, subject),
            fee,
            accrued,
            done,
        })
    }
}

/// Whether a position's collateral still covers what it owes, at `price`.
///
/// The debt is valued as it would be repaid and the collateral as it would be sold, so both
/// roundings run against the holder and a position on the exact boundary counts as uncovered.
///
/// Takes the price rather than fetching one, because a liquidation has to judge and settle at
/// the same reading.
fn covered(
    market: &Market,
    lltv: Amount<Wad>,
    position: &Position,
    price: Price<Collateral, Loan>,
) -> Result<bool, ArithError> {
    let owed = to_assets(
        position.borrow_shares,
        market.total_borrow_assets,
        market.total_borrow_shares,
        Flow::In,
    )?;
    let backing =
        price
            .quote(position.collateral, Rounding::Down)?
            .mul_div(lltv, WAD, Rounding::Down)?;
    Ok(backing >= owed)
}

/// Both sides of one conversion, whichever side the action stated.
///
/// A market's holders think in assets and its accounting thinks in shares; the side an action
/// names is the side that is exact, and the other is derived at the direction `flow` fixes.
fn split<A: Unit, S: Unit>(
    given: &Given<A, S>,
    total_assets: Amount<A>,
    total_shares: Amount<S>,
    flow: Flow,
) -> Result<(Amount<A>, Amount<S>), MechError> {
    if given.is_zero() {
        return Err(MechError::new("an action must state a non-zero amount"));
    }
    Ok(match *given {
        Given::Assets(assets) => (assets, to_shares(assets, total_assets, total_shares, flow)?),
        Given::Shares(shares) => (to_assets(shares, total_assets, total_shares, flow)?, shares),
    })
}

impl<O, R> Mechanism for Lending<O, R>
where
    O: PathSource<Price<Collateral, Loan>>,
    R: BorrowRate,
{
    type State = LendingState;
    type Action = LendingAction;
    type Event = LendingEvent;

    fn apply(
        &self,
        state: &mut LendingState,
        actor: AgentId,
        action: LendingAction,
        now: Tick,
        events: &mut Vec<LendingEvent>,
    ) -> Result<(), MechError> {
        let settled = self.resolve(state, actor, action, now)?;
        let id = action.market();
        state.set_market(id, settled.market);
        let (holder, position) = settled.subject;
        state.set_position(id, holder, position);
        if let Some((recipient, position)) = settled.fee {
            state.set_position(id, recipient, position);
        }
        events.extend(settled.accrued);
        events.push(settled.done);
        Ok(())
    }

    /// Brings every open market up to date, in identifier order.
    ///
    /// A market nobody touched still owes its suppliers the interest the tick earned them, and a
    /// rate the model cannot supply leaves the run nowhere to go: the alternative to stopping is
    /// a market whose totals are dated and whose report does not say so.
    fn tick(
        &self,
        state: &mut LendingState,
        now: Tick,
        events: &mut Vec<LendingEvent>,
    ) -> Result<(), Abort> {
        let mut reached = 0;
        for (id, feeds) in &self.feeds {
            let Some(market) = state.market(*id) else {
                continue;
            };
            reached += 1;
            let Some(step) = self.bring_forward(&market, feeds, now)? else {
                continue;
            };
            state.set_market(*id, step.market);
            if !step.fee_shares.is_zero() {
                let mut paid = state.position(*id, feeds.params.fee_recipient);
                paid.supply_shares = paid.supply_shares.checked_add(step.fee_shares)?;
                state.set_position(*id, feeds.params.fee_recipient, paid);
            }
            events.push(LendingEvent::InterestAccrued {
                market: *id,
                interest: step.interest,
                fee_shares: step.fee_shares,
            });
        }
        // A market with no rate model would otherwise stand still and say nothing, and a sweep
        // runs with the invariant that would have caught it turned off. Counting what was
        // reached costs nothing and turns a silent frozen market into a stopped run.
        if reached != state.markets().len() {
            return Err(Abort::Source(SourceError::new(
                "a market in the run has no rate model or oracle",
            )));
        }
        Ok(())
    }

    /// What every reachable state satisfies.
    ///
    /// Health is deliberately not among these. A position that no longer covers its debt is
    /// reachable — a price falls and it is unhealthy — and declaring that impossible would halt
    /// exactly the runs this family exists to study.
    ///
    /// The share sums cost one pass over the holders of each market. They are the only claim here
    /// that the totals cannot make on their own, and they are what catches a transition that
    /// writes a position without the total behind it.
    fn check_invariants(&self, state: &LendingState) -> Result<(), InvariantViolation> {
        let mut counted = 0;
        for (id, market) in state.markets() {
            if !self.feeds.contains_key(&id) {
                return Err(InvariantViolation::new(
                    "an open market has no rate model or oracle",
                ));
            }
            if market.total_borrow_assets > market.total_supply_assets {
                return Err(InvariantViolation::new(
                    "more is owed to a market than was ever supplied to it",
                ));
            }
            if market.total_borrow_shares.is_zero() && !market.total_borrow_assets.is_zero() {
                return Err(InvariantViolation::new(
                    "a debt remains with no shares to own it",
                ));
            }

            let mut supply = Amount::ZERO;
            let mut borrow = Amount::ZERO;
            for (_, position) in state.holders(id) {
                supply = supply.checked_add(position.supply_shares)?;
                borrow = borrow.checked_add(position.borrow_shares)?;
                counted += 1;
            }
            if supply != market.total_supply_shares {
                return Err(InvariantViolation::new(
                    "supply shares held differ from those issued",
                ));
            }
            if borrow != market.total_borrow_shares {
                return Err(InvariantViolation::new(
                    "debt shares held differ from those issued",
                ));
            }
        }

        // Every position an open market claims has now been counted once, so a ledger longer
        // than that count holds one belonging to no market. Reading the length is not a second
        // walk over it.
        if counted != state.holdings().len() {
            return Err(InvariantViolation::new(
                "a position is held in a market that is not open",
            ));
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

    use proptest::prelude::*;
    use sting_karman_core::{
        Agent, NullObserver, Observer, RunRng, SourceError, World, conformance,
    };

    use super::*;
    use crate::{
        BorrowShare, FixedRate, MAX_FEE, ORACLE_PRICE_SCALE, SupplyShare, Wad, market::Market,
    };

    const BASELINE: &str = include_str!("../../../benches/baseline.toml");

    const MARKET: MarketId = MarketId(0);
    /// The fee recipient, and therefore the one holder accrual writes to on its own. Agent zero,
    /// so the case where the actor is also the recipient is reached by the population as well.
    const TREASURY: AgentId = AgentId(0);
    const ALICE: AgentId = AgentId(1);
    const BOB: AgentId = AgentId(2);

    /// Eighty percent: the threshold a mainstream collateral pair is listed at.
    const LLTV: Amount<Wad> = Amount::new(800_000_000_000_000_000);
    /// A tenth of the interest to the fee recipient.
    const FEE: Amount<Wad> = Amount::new(100_000_000_000_000_000);
    /// Ten percent a year, per second, at WAD.
    const RATE: Amount<Wad> = Amount::new(3_170_979_198);
    /// One block.
    const BLOCK: SecondsPerTick = SecondsPerTick(match NonZeroU64::new(12) {
        Some(seconds) => seconds,
        None => unreachable!(),
    });

    /// Collateral worth `before` until `at`, and `after` from then on.
    ///
    /// A step rather than a slide, and that is the point of it. A price that walks down is one
    /// liquidators keep up with: a position taken over at a discount comes back inside the
    /// threshold, and the ladder repeats until the borrower is out rather than until the market
    /// is short. What leaves a market with a loss is the price it never had a chance to be
    /// liquidated at.
    ///
    /// A constant price is the case where the two levels agree. The oracle is the one input this
    /// family reads that is a function of the clock alone, which is what lets a whole episode be
    /// written as a path.
    #[derive(Debug, Clone, Copy)]
    struct Falling {
        before: u128,
        at: u64,
        after: u128,
    }

    impl PathSource<Price<Collateral, Loan>> for Falling {
        fn value_at(&self, now: Tick) -> Result<Price<Collateral, Loan>, SourceError> {
            Ok(Price::new(if now.0 < self.at {
                self.before
            } else {
                self.after
            }))
        }
    }

    /// An oracle with nothing to say, ever.
    #[derive(Debug, Clone, Copy)]
    struct SilentPrice;

    impl PathSource<Price<Collateral, Loan>> for SilentPrice {
        fn value_at(&self, _now: Tick) -> Result<Price<Collateral, Loan>, SourceError> {
            Err(SourceError::new("the price path has nothing at this tick"))
        }
    }

    /// A rate model that cannot price anything.
    #[derive(Debug, Clone, Copy)]
    struct SilentRate;

    impl BorrowRate for SilentRate {
        fn per_second(&self, _market: &Market, _now: Tick) -> Result<Amount<Wad>, SourceError> {
            Err(SourceError::new("the rate path has nothing at this tick"))
        }
    }

    type Priced = Lending<Falling, FixedRate>;

    /// A price that never moves.
    fn parity() -> Falling {
        Falling {
            before: ORACLE_PRICE_SCALE,
            at: 0,
            after: ORACLE_PRICE_SCALE,
        }
    }

    /// Thirty percent off at the fiftieth tick: enough that a position opened at the threshold
    /// owes more than its collateral is worth the moment the gap lands.
    fn sliding() -> Falling {
        Falling {
            before: ORACLE_PRICE_SCALE,
            at: 50,
            after: ORACLE_PRICE_SCALE * 7 / 10,
        }
    }

    /// The same gap early enough to land inside the span a generated sequence covers, so that a
    /// property about liquidation is asserted on runs where liquidations are accepted.
    fn plunging() -> Falling {
        Falling {
            before: ORACLE_PRICE_SCALE,
            at: 8,
            after: ORACLE_PRICE_SCALE / 2,
        }
    }

    fn params() -> MarketParams {
        MarketParams {
            lltv: LLTV,
            fee_recipient: TREASURY,
        }
    }

    fn under(oracle: Falling, rate: Amount<Wad>) -> Priced {
        Lending::new(BLOCK)
            .with_market(MARKET, params(), oracle, FixedRate(rate))
            .unwrap()
    }

    fn mechanism(rate: Amount<Wad>) -> Priced {
        under(parity(), rate)
    }

    fn opening(fee: Amount<Wad>) -> LendingState {
        LendingState::new()
            .with_market(MARKET, Market::opening(Tick(0), fee).unwrap())
            .unwrap()
    }

    /// The engine hands the mechanism a buffer it reuses; a direct call brings its own.
    fn act<O, R>(
        lending: &Lending<O, R>,
        state: &mut LendingState,
        actor: AgentId,
        action: LendingAction,
        now: Tick,
    ) -> Result<Vec<LendingEvent>, MechError>
    where
        O: PathSource<Price<Collateral, Loan>>,
        R: BorrowRate,
    {
        let mut events = Vec::new();
        lending.apply(state, actor, action, now, &mut events)?;
        Ok(events)
    }

    fn supply(assets: u128) -> LendingAction {
        LendingAction::Supply {
            market: MARKET,
            given: Given::Assets(Amount::new(assets)),
        }
    }

    fn pledge(assets: u128) -> LendingAction {
        LendingAction::SupplyCollateral {
            market: MARKET,
            assets: Amount::new(assets),
        }
    }

    fn borrow(assets: u128) -> LendingAction {
        LendingAction::Borrow {
            market: MARKET,
            given: Given::Assets(Amount::new(assets)),
        }
    }

    /// A market with a lender and a borrower already in it, at the tick it opened.
    fn funded() -> (Priced, LendingState) {
        let lending = mechanism(RATE);
        let mut state = opening(Amount::ZERO);
        act(&lending, &mut state, ALICE, supply(1_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, pledge(1_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, borrow(500_000), Tick(0)).unwrap();
        (lending, state)
    }

    #[test]
    fn a_supply_is_credited_shares_and_the_market_records_both_sides() {
        let lending = mechanism(RATE);
        let mut state = opening(Amount::ZERO);
        let events = act(&lending, &mut state, ALICE, supply(1_000), Tick(0)).unwrap();

        let market = state.market(MARKET).unwrap();
        assert_eq!(market.total_supply_assets, Amount::new(1_000));
        assert_eq!(market.total_supply_shares, Amount::new(1_000_000_000));
        assert_eq!(
            state.position(MARKET, ALICE).supply_shares,
            Amount::new(1_000_000_000)
        );
        assert_eq!(
            events,
            vec![LendingEvent::Supplied {
                market: MARKET,
                holder: ALICE,
                assets: Amount::new(1_000),
                shares: Amount::new(1_000_000_000),
            }]
        );
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// The threshold is a ceiling on the debt: the position on the exact boundary stands, and the
    /// one a single unit past it does not.
    #[test]
    fn a_borrow_stops_at_the_liquidation_threshold() {
        let lending = mechanism(RATE);
        let mut state = opening(Amount::ZERO);
        act(&lending, &mut state, ALICE, supply(1_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, pledge(1_000), Tick(0)).unwrap();

        let before = state.clone();
        assert!(act(&lending, &mut state, BOB, borrow(801), Tick(0)).is_err());
        assert_eq!(state, before, "a refused borrow moved the market");
        assert!(act(&lending, &mut state, BOB, borrow(800), Tick(0)).is_ok());
    }

    /// Collateral is not liquidity: it backs a debt and is never lent out, so pledging it lets
    /// nobody borrow against it.
    #[test]
    fn collateral_is_not_available_to_borrow() {
        let lending = mechanism(RATE);
        let mut state = opening(Amount::ZERO);
        act(&lending, &mut state, BOB, pledge(1_000_000), Tick(0)).unwrap();
        assert!(act(&lending, &mut state, BOB, borrow(1), Tick(0)).is_err());
    }

    #[test]
    fn a_withdrawal_stops_at_the_liquidity_left_in_the_market() {
        let (lending, mut state) = funded();
        let all = state.position(MARKET, ALICE).supply_shares;
        let everything = LendingAction::Withdraw {
            market: MARKET,
            given: Given::Shares(all),
        };
        let before = state.clone();
        assert!(act(&lending, &mut state, ALICE, everything, Tick(0)).is_err());
        assert_eq!(state, before);

        let idle = LendingAction::Withdraw {
            market: MARKET,
            given: Given::Assets(Amount::new(500_000)),
        };
        assert!(act(&lending, &mut state, ALICE, idle, Tick(0)).is_ok());
    }

    /// Interest accrues once per tick however many transitions land on it: the first brings the
    /// market forward and the rest read the result.
    #[test]
    fn several_actions_on_one_tick_compound_once_between_them() {
        let (lending, mut state) = funded();
        let opened = state.market(MARKET).unwrap();

        let first = act(&lending, &mut state, ALICE, supply(1), Tick(100)).unwrap();
        let grown = state.market(MARKET).unwrap();
        assert!(grown.total_borrow_assets > opened.total_borrow_assets);
        assert_eq!(grown.last_update, Tick(100));
        assert!(matches!(first[0], LendingEvent::InterestAccrued { .. }));

        let second = act(&lending, &mut state, ALICE, supply(1), Tick(100)).unwrap();
        assert_eq!(
            state.market(MARKET).unwrap().total_borrow_assets,
            grown.total_borrow_assets,
            "the second action of one tick compounded again"
        );
        assert!(!matches!(second[0], LendingEvent::InterestAccrued { .. }));
    }

    /// The fee is minted as shares to its recipient, out of interest the debt grew by in full.
    #[test]
    fn the_fee_recipient_is_paid_in_shares_of_the_supply_side() {
        let lending = mechanism(RATE);
        let mut state = opening(FEE);
        act(&lending, &mut state, ALICE, supply(1_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, pledge(1_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, borrow(500_000_000), Tick(0)).unwrap();

        let mut events = Vec::new();
        lending
            .tick(&mut state, Tick(2_628_000), &mut events)
            .unwrap();

        let LendingEvent::InterestAccrued {
            interest,
            fee_shares,
            ..
        } = events[0]
        else {
            panic!("a market that was borrowed from reported no accrual");
        };
        assert!(!interest.is_zero());
        assert_eq!(state.position(MARKET, TREASURY).supply_shares, fee_shares);
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// The recipient acting on its own market sees the shares accrual just minted it, which is
    /// what it would see if the two were separate transactions in that order.
    #[test]
    fn the_fee_recipient_acting_sees_the_shares_it_was_just_minted() {
        let lending = mechanism(RATE);
        let mut state = opening(FEE);
        act(
            &lending,
            &mut state,
            TREASURY,
            supply(1_000_000_000),
            Tick(0),
        )
        .unwrap();
        act(&lending, &mut state, BOB, pledge(1_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, borrow(500_000_000), Tick(0)).unwrap();

        let before = state.position(MARKET, TREASURY).supply_shares;
        act(&lending, &mut state, TREASURY, supply(1), Tick(2_628_000)).unwrap();
        assert!(state.position(MARKET, TREASURY).supply_shares > before);
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// A position owing nothing needs no valuation and none is asked for, so a market whose
    /// oracle is silent can still be supplied to; a position owing something cannot be managed.
    #[test]
    fn a_silent_oracle_stops_only_what_needs_a_price() {
        let lending = Lending::new(BLOCK)
            .with_market(MARKET, params(), SilentPrice, FixedRate(RATE))
            .unwrap();
        let mut state = opening(Amount::ZERO);
        assert!(act(&lending, &mut state, ALICE, supply(1_000_000), Tick(0)).is_ok());
        assert!(act(&lending, &mut state, BOB, pledge(1_000), Tick(0)).is_ok());
        assert!(act(&lending, &mut state, BOB, borrow(1), Tick(0)).is_err());
    }

    /// A rate the model cannot supply refuses the action that needed it, and stops the run when
    /// nobody asked: a market whose totals are dated is not one a report may quote.
    #[test]
    fn a_silent_rate_refuses_an_action_and_stops_a_tick() {
        let lending = Lending::new(BLOCK)
            .with_market(MARKET, params(), parity(), SilentRate)
            .unwrap();
        let mut state = opening(Amount::ZERO);
        act(&lending, &mut state, ALICE, supply(1_000), Tick(0)).unwrap();

        let before = state.clone();
        assert!(act(&lending, &mut state, ALICE, supply(1_000), Tick(1)).is_err());
        assert_eq!(state, before);
        assert!(matches!(
            lending.tick(&mut state, Tick(1), &mut Vec::new()),
            Err(Abort::Source(_))
        ));
    }

    /// Pledging collateral is the transition a rate has nothing to say about, and it goes through
    /// where every other one would not.
    #[test]
    fn pledging_collateral_never_asks_for_a_rate() {
        let lending = Lending::new(BLOCK)
            .with_market(MARKET, params(), parity(), SilentRate)
            .unwrap();
        let mut state = opening(Amount::ZERO);
        assert!(act(&lending, &mut state, BOB, pledge(1_000), Tick(500)).is_ok());
    }

    /// The last repayment of a market names more assets than the market records as owed, because
    /// the conversion rounds up against the payer. The debt still clears.
    #[test]
    fn the_last_repayment_clears_a_debt_it_names_more_than() {
        let (lending, mut state) = funded();
        let mut events = Vec::new();
        lending
            .tick(&mut state, Tick(100_000), &mut events)
            .unwrap();

        let owed = state.position(MARKET, BOB).borrow_shares;
        let all = LendingAction::Repay {
            market: MARKET,
            given: Given::Shares(owed),
        };
        act(&lending, &mut state, BOB, all, Tick(100_000)).unwrap();

        let market = state.market(MARKET).unwrap();
        assert!(market.total_borrow_shares.is_zero());
        assert!(
            market.total_borrow_assets.is_zero(),
            "a debt outlived the shares that owned it"
        );
        assert!(lending.check_invariants(&state).is_ok());
    }

    #[test]
    fn an_action_on_a_market_that_is_not_open_is_refused() {
        let lending = mechanism(RATE);
        let mut state = LendingState::new();
        assert!(act(&lending, &mut state, ALICE, supply(1_000), Tick(0)).is_err());
    }

    /// An action must state one side or the other, and state it as something.
    #[test]
    fn an_action_stating_nothing_is_refused() {
        let lending = mechanism(RATE);
        let mut state = opening(Amount::ZERO);
        let nothing = [
            LendingAction::Supply {
                market: MARKET,
                given: Given::Assets(Amount::ZERO),
            },
            LendingAction::Supply {
                market: MARKET,
                given: Given::Shares(Amount::ZERO),
            },
            LendingAction::SupplyCollateral {
                market: MARKET,
                assets: Amount::ZERO,
            },
            LendingAction::WithdrawCollateral {
                market: MARKET,
                assets: Amount::ZERO,
            },
        ];
        for action in nothing {
            assert!(act(&lending, &mut state, ALICE, action, Tick(0)).is_err());
        }
    }

    /// The invariant suite has to fail on a state only a broken transition could produce, or it
    /// is checking nothing.
    #[test]
    fn a_total_that_no_position_backs_is_caught() {
        let (lending, mut state) = funded();
        let mut market = state.market(MARKET).unwrap();
        market.total_supply_shares = market
            .total_supply_shares
            .checked_add(Amount::new(1))
            .unwrap();
        state.set_market(MARKET, market);
        assert!(lending.check_invariants(&state).is_err());
    }

    #[test]
    fn a_market_with_no_feeds_is_caught() {
        let lending = mechanism(RATE);
        let stray = LendingState::new()
            .with_market(MarketId(9), Market::opening(Tick(0), Amount::ZERO).unwrap())
            .unwrap();
        assert!(lending.check_invariants(&stray).is_err());
    }

    /// The invariant above is the only thing that would notice a market nobody can price, and a
    /// sweep turns invariants off. The tick has to notice it too, or such a market stands still
    /// for a whole run and the report says nothing about it.
    #[test]
    fn a_market_no_rate_model_can_price_stops_the_run() {
        let lending = mechanism(RATE);
        let mut state = opening(Amount::ZERO)
            .with_market(MarketId(9), Market::opening(Tick(0), Amount::ZERO).unwrap())
            .unwrap();
        let outcome = lending.tick(&mut state, Tick(1), &mut Vec::new());
        assert!(matches!(outcome, Err(Abort::Source(_))));
    }

    /// A fee at the ceiling is still a fee, and the ledgers balance under it.
    #[test]
    fn a_market_at_the_maximum_fee_still_balances() {
        let lending = mechanism(RATE);
        let mut state = opening(MAX_FEE);
        let mut events = Vec::new();
        lending
            .apply(
                &mut state,
                ALICE,
                supply(1_000_000_000),
                Tick(0),
                &mut events,
            )
            .unwrap();
        lending
            .apply(&mut state, BOB, pledge(1_000_000_000), Tick(0), &mut events)
            .unwrap();
        lending
            .apply(&mut state, BOB, borrow(700_000_000), Tick(0), &mut events)
            .unwrap();
        lending
            .tick(&mut state, Tick(2_628_000), &mut events)
            .unwrap();
        assert!(lending.check_invariants(&state).is_ok());
        assert!(!state.position(MARKET, TREASURY).supply_shares.is_zero());
    }

    /// A market with a lender and one borrower at the threshold, priced by `oracle`.
    fn at_the_threshold(oracle: Falling) -> (Priced, LendingState) {
        let lending = under(oracle, RATE);
        let mut state = opening(Amount::ZERO);
        act(&lending, &mut state, ALICE, supply(4_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, pledge(1_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, borrow(790_000_000), Tick(0)).unwrap();
        (lending, state)
    }

    fn seize(borrower: AgentId, collateral: u128) -> LendingAction {
        LendingAction::Liquidate {
            market: MARKET,
            borrower,
            given: Given::Assets(Amount::new(collateral)),
        }
    }

    /// The threshold is what a liquidation needs, and nothing else is: a position that still
    /// covers its debt is refused however much a liquidator would like it.
    #[test]
    fn a_position_that_still_covers_its_debt_cannot_be_liquidated() {
        let (lending, mut state) = at_the_threshold(parity());
        let before = state.clone();
        assert!(act(&lending, &mut state, ALICE, seize(BOB, 1_000), Tick(0)).is_err());
        assert_eq!(state, before, "a refused liquidation moved the market");
    }

    /// A market whose threshold sits at the full value of its collateral has nothing to pay a
    /// liquidator out of, and is refused where it is declared rather than where it fails.
    #[test]
    fn a_threshold_at_the_collateral_full_value_is_refused() {
        let full = MarketParams {
            lltv: WAD,
            fee_recipient: TREASURY,
        };
        assert!(
            Lending::new(BLOCK)
                .with_market(MARKET, full, parity(), FixedRate(RATE))
                .is_err()
        );
    }

    /// A liquidation rewrites the borrower and leaves the liquidator alone: no agent holds a
    /// balance in this mechanism, so there is nowhere here for one to be paid.
    #[test]
    fn a_liquidation_rewrites_the_borrower_and_not_the_liquidator() {
        let (lending, mut state) = at_the_threshold(sliding());
        let liquidator = state.position(MARKET, ALICE);
        act(
            &lending,
            &mut state,
            ALICE,
            seize(BOB, 100_000_000),
            Tick(60),
        )
        .unwrap();
        assert_eq!(state.position(MARKET, ALICE), liquidator);
        assert!(state.position(MARKET, BOB).collateral < Amount::new(1_000_000_000));
    }

    /// Part of a position taken over leaves the rest of it standing, and nothing is written off
    /// however far under the threshold the remainder still is.
    #[test]
    fn a_partial_liquidation_writes_nothing_off() {
        let (lending, mut state) = at_the_threshold(sliding());
        let events = act(
            &lending,
            &mut state,
            ALICE,
            seize(BOB, 400_000_000),
            Tick(150),
        )
        .unwrap();

        let taken = events.last().unwrap();
        let LendingEvent::Liquidated {
            seized,
            repaid,
            bad_debt,
            bad_debt_shares,
            ..
        } = *taken
        else {
            panic!("a liquidation reported something else");
        };
        assert_eq!(seized, Amount::new(400_000_000));
        assert!(!repaid.is_zero());
        assert!(bad_debt.is_zero() && bad_debt_shares.is_zero());
        let left = state.position(MARKET, BOB);
        assert!(!left.collateral.is_zero() && !left.borrow_shares.is_zero());
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// The last of the collateral gone is what turns a debt into a loss, and the loss lands on
    /// the suppliers: both totals fall by it, so the liquidity nobody borrowed is where it was
    /// and what a supply share is worth is the only thing that moved.
    #[test]
    fn a_borrower_stripped_of_collateral_leaves_the_rest_with_the_suppliers() {
        let (lending, mut state) = at_the_threshold(sliding());
        let before = state.market(MARKET).unwrap();
        let idle =
            |m: Market| m.total_supply_assets.base_units() - m.total_borrow_assets.base_units();

        let events = act(
            &lending,
            &mut state,
            ALICE,
            seize(BOB, 1_000_000_000),
            Tick(180),
        )
        .unwrap();
        let LendingEvent::Liquidated {
            bad_debt,
            bad_debt_shares,
            ..
        } = *events.last().unwrap()
        else {
            panic!("a liquidation reported something else");
        };

        assert!(
            !bad_debt.is_zero(),
            "a stripped position left no loss behind"
        );
        assert!(!bad_debt_shares.is_zero());
        let after = state.market(MARKET).unwrap();
        assert!(
            state.position(MARKET, BOB).is_empty(),
            "a written-off borrower kept a record"
        );
        assert!(after.total_supply_assets < before.total_supply_assets);
        assert_eq!(after.total_supply_shares, before.total_supply_shares);
        assert!(
            idle(after) >= idle(before),
            "socialising a loss moved the liquidity nobody had borrowed"
        );
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// A price of nothing hands a position away: the collateral it names is worth nothing, so
    /// nothing is charged for it, and what is left of the debt is written off in full.
    ///
    /// This is the reference's behaviour and not an oversight in reproducing it. Level 0 asks
    /// for agreement rather than for improvement, and a market's oracle is trusted by the market
    /// that names it — this is the price of that trust, stated rather than guarded against.
    #[test]
    fn an_oracle_reporting_nothing_gives_a_position_away() {
        let worthless = Falling {
            before: ORACLE_PRICE_SCALE,
            at: 1,
            after: 0,
        };
        let lending = under(worthless, RATE);
        let mut state = opening(Amount::ZERO);
        act(&lending, &mut state, ALICE, supply(4_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, pledge(1_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, borrow(790_000_000), Tick(0)).unwrap();

        let events = act(
            &lending,
            &mut state,
            ALICE,
            seize(BOB, 1_000_000_000),
            Tick(2),
        )
        .unwrap();
        let LendingEvent::Liquidated {
            repaid,
            seized,
            bad_debt,
            ..
        } = *events.last().unwrap()
        else {
            panic!("a liquidation reported something else");
        };
        assert_eq!(seized, Amount::new(1_000_000_000));
        assert!(
            repaid.is_zero(),
            "collateral taken at a price of nothing cost something"
        );
        assert!(bad_debt > Amount::new(790_000_000));
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// An agent liquidating its own position names one holder twice over. Resolving it into two
    /// writes would let one of them discard the other, so the shape has to make that
    /// unrepresentable rather than merely unlikely.
    #[test]
    fn an_agent_may_liquidate_its_own_position() {
        let lending = under(sliding(), RATE);
        let mut state = opening(Amount::ZERO);
        act(&lending, &mut state, ALICE, supply(4_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, pledge(1_000_000_000), Tick(0)).unwrap();
        act(&lending, &mut state, BOB, borrow(790_000_000), Tick(0)).unwrap();

        act(
            &lending,
            &mut state,
            BOB,
            seize(BOB, 1_000_000_000),
            Tick(180),
        )
        .unwrap();
        assert!(state.position(MARKET, BOB).is_empty());
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// The same coincidence from the other side: accrual mints the fee into the position a
    /// liquidation is about to rewrite, and both changes have to survive.
    #[test]
    fn the_fee_recipient_may_be_the_one_being_liquidated() {
        let lending = under(sliding(), RATE);
        let mut state = opening(FEE);
        act(&lending, &mut state, ALICE, supply(4_000_000_000), Tick(0)).unwrap();
        act(
            &lending,
            &mut state,
            TREASURY,
            pledge(1_000_000_000),
            Tick(0),
        )
        .unwrap();
        act(&lending, &mut state, TREASURY, borrow(790_000_000), Tick(0)).unwrap();

        act(
            &lending,
            &mut state,
            ALICE,
            seize(TREASURY, 400_000_000),
            Tick(180),
        )
        .unwrap();
        let paid = state.position(MARKET, TREASURY);
        assert!(
            !paid.supply_shares.is_zero(),
            "the fee the accrual minted was discarded"
        );
        assert_eq!(paid.collateral, Amount::new(600_000_000));
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// Naming the debt to clear rather than the collateral to take is the same trade from the
    /// other end, and it settles.
    #[test]
    fn a_liquidator_may_name_the_debt_instead_of_the_collateral() {
        let (lending, mut state) = at_the_threshold(sliding());
        let owed = state.position(MARKET, BOB).borrow_shares;
        let clear = LendingAction::Liquidate {
            market: MARKET,
            borrower: BOB,
            given: Given::Shares(Amount::new(owed.base_units() / 4)),
        };
        let events = act(&lending, &mut state, ALICE, clear, Tick(120)).unwrap();
        let LendingEvent::Liquidated {
            seized,
            repaid_shares,
            ..
        } = *events.last().unwrap()
        else {
            panic!("a liquidation reported something else");
        };
        assert_eq!(repaid_shares, Amount::new(owed.base_units() / 4));
        assert!(!seized.is_zero());
        assert!(lending.check_invariants(&state).is_ok());
    }

    /// Draws `amount` divided by one to four, so a holder unwinds in steps rather than in one
    /// move and the market spends time in every state between.
    fn part<U: Unit>(amount: Amount<U>, rng: &mut RunRng) -> Amount<U> {
        Amount::new(amount.base_units() / u128::from(rng.below(4) + 1))
    }

    /// Test-local population: lends, pledges, borrows and unwinds at random. Lives here so the
    /// mechanism crate's conformance run does not depend on the agent library above it.
    struct Participant;

    impl Agent<Priced> for Participant {
        fn decide(
            &mut self,
            me: AgentId,
            state: &LendingState,
            _now: Tick,
            rng: &mut RunRng,
        ) -> Option<LendingAction> {
            let held = state.position(MARKET, me);
            let size = u128::from(rng.below(1_000_000) + 1);
            Some(match rng.below(6) {
                0 => LendingAction::Supply {
                    market: MARKET,
                    given: Given::Assets(Amount::new(size)),
                },
                1 => LendingAction::SupplyCollateral {
                    market: MARKET,
                    assets: Amount::new(size),
                },
                2 => LendingAction::Borrow {
                    market: MARKET,
                    given: Given::Assets(Amount::new(size)),
                },
                3 => LendingAction::Repay {
                    market: MARKET,
                    given: Given::Shares(part(held.borrow_shares, rng)),
                },
                4 => LendingAction::Withdraw {
                    market: MARKET,
                    given: Given::Shares(part(held.supply_shares, rng)),
                },
                _ => LendingAction::WithdrawCollateral {
                    market: MARKET,
                    assets: part(held.collateral, rng),
                },
            })
        }
    }

    /// The cascade population: a lender, four borrowers who open at the threshold, and a
    /// liquidator that keeps trying whether or not a position is ready to be taken.
    ///
    /// The liquidator judges nothing. A position that still covers its debt refuses it, which is
    /// what a real bot discovers by being refused, and it means the run reaches a write-off
    /// because the arithmetic let it rather than because the population was told when to act.
    struct Cascade;

    const LIQUIDATOR: AgentId = AgentId(5);

    impl Agent<Priced> for Cascade {
        fn decide(
            &mut self,
            me: AgentId,
            state: &LendingState,
            now: Tick,
            rng: &mut RunRng,
        ) -> Option<LendingAction> {
            match me {
                TREASURY => (now == Tick(0)).then(|| supply(4_000_000_000)),
                LIQUIDATOR => {
                    let borrower = AgentId(1 + u32::try_from(rng.below(4)).unwrap());
                    let held = state.position(MARKET, borrower);
                    let part = held.collateral.base_units() / u128::from(rng.below(3) + 1);
                    (part > 0).then(|| LendingAction::Liquidate {
                        market: MARKET,
                        borrower,
                        given: Given::Assets(Amount::new(part)),
                    })
                }
                _ => match now {
                    Tick(0) => Some(pledge(1_000_000_000)),
                    Tick(1) => Some(borrow(790_000_000)),
                    _ => None,
                },
            }
        }
    }

    /// What a run's liquidations came to, and the interest they happened against.
    #[derive(Debug, Default)]
    struct Losses {
        liquidations: u32,
        with_a_write_off: u32,
        written_off: u128,
        earned: u128,
    }

    impl Observer<Priced> for Losses {
        fn on_events(&mut self, _now: Tick, events: &[LendingEvent]) {
            for event in events {
                match *event {
                    LendingEvent::Liquidated {
                        bad_debt,
                        bad_debt_shares,
                        ..
                    } => {
                        self.liquidations += 1;
                        if !bad_debt_shares.is_zero() {
                            self.with_a_write_off += 1;
                            self.written_off += bad_debt.base_units();
                        }
                    }
                    LendingEvent::InterestAccrued { interest, .. } => {
                        self.earned += interest.base_units();
                    }
                    _ => {}
                }
            }
        }

        fn on_rejected(&mut self, _now: Tick, _actor: AgentId, _reason: &MechError) {}

        fn on_tick_end(&mut self, _now: Tick, _state: &LendingState) {}
    }

    fn cascade() -> World<Priced> {
        let agents: Vec<Box<dyn Agent<Priced>>> = (0..6)
            .map(|_| Box::new(Cascade) as Box<dyn Agent<Priced>>)
            .collect();
        World::new(under(sliding(), RATE), opening(Amount::ZERO), agents)
    }

    /// The gate: a seeded cascade writes debt off exactly when a seizure takes the last of a
    /// borrower's collateral, and not once before.
    ///
    /// Both outcomes have to occur in the one run, or the claim is untested in one direction:
    /// liquidations that leave collateral behind write nothing off, and the ones that do not
    /// write off everything still owed.
    #[test]
    fn a_seeded_cascade_writes_off_debt_only_when_collateral_runs_out() {
        let mut losses = Losses::default();
        let mut world = cascade();
        world
            .run(200, &mut RunRng::new(42, 0), &mut losses)
            .unwrap();

        assert!(
            losses.liquidations > 0,
            "the price fell and nobody was liquidated"
        );
        assert!(
            losses.with_a_write_off > 0,
            "no position was ever stripped of its collateral"
        );
        assert!(
            losses.with_a_write_off < losses.liquidations,
            "every liquidation ended in a write-off; the run tests only one direction"
        );

        // Every write-off left an empty position behind, and every borrower still holding
        // collateral still owes what it backs.
        for (holder, position) in world.state().holders(MARKET) {
            if holder != TREASURY {
                assert!(
                    !position.collateral.is_zero() || position.borrow_shares.is_zero(),
                    "a borrower with no collateral kept a debt"
                );
            }
        }
        assert!(
            under(sliding(), RATE)
                .check_invariants(world.state())
                .is_ok()
        );

        // The whole episode as one identity: what was supplied, plus what it earned, less what
        // was written off, is what the supply side is left holding. Nothing else in this run
        // could move it, so an accounting error anywhere in the cascade shows up here.
        let market = world.state().market(MARKET).unwrap();
        assert_eq!(
            4_000_000_000 + losses.earned - losses.written_off,
            market.total_supply_assets.base_units(),
            "the supply side lost something no event accounted for"
        );

        let pin = |key: &str, value: u128| {
            conformance::assert_golden(BASELINE, key, i128::try_from(value).unwrap());
        };
        pin("lending.cascade_written_off", losses.written_off);
        pin(
            "lending.cascade_liquidations",
            u128::from(losses.liquidations),
        );
        pin(
            "lending.cascade_write_offs",
            u128::from(losses.with_a_write_off),
        );
        pin(
            "lending.cascade_supply_assets",
            market.total_supply_assets.base_units(),
        );
        pin("lending.cascade_interest", losses.earned);
    }

    #[test]
    fn conformance_deterministic_under_a_cascade() {
        conformance::assert_deterministic(200, 42, cascade);
    }

    fn world() -> World<Priced> {
        let agents: Vec<Box<dyn Agent<Priced>>> = (0..8)
            .map(|_| Box::new(Participant) as Box<dyn Agent<Priced>>)
            .collect();
        World::new(mechanism(RATE), opening(FEE), agents)
    }

    #[test]
    fn conformance_deterministic() {
        conformance::assert_deterministic(500, 42, world);
    }

    #[test]
    fn conformance_golden() {
        let mut rng = RunRng::new(42, 0);
        let mut w = world();
        w.run(500, &mut rng, &mut NullObserver).unwrap();
        let market = w.state().market(MARKET).unwrap();
        let pin = |key: &str, value: u128| {
            conformance::assert_golden(BASELINE, key, i128::try_from(value).unwrap());
        };
        pin(
            "lending.golden_supply_assets",
            market.total_supply_assets.base_units(),
        );
        pin(
            "lending.golden_supply_shares",
            market.total_supply_shares.base_units(),
        );
        pin(
            "lending.golden_borrow_assets",
            market.total_borrow_assets.base_units(),
        );
        pin(
            "lending.golden_borrow_shares",
            market.total_borrow_shares.base_units(),
        );
    }

    /// A supply share's price, as the pair the conversions themselves divide: assets over shares,
    /// both carrying the virtual offset, so the ratio is defined on an empty market too.
    fn supply_price(market: &Market) -> (u128, u128) {
        (
            market.total_supply_assets.base_units() + 1,
            market.total_supply_shares.base_units() + 1_000_000,
        )
    }

    /// Whether `now` is worth at least what `then` was, compared by cross-multiplication so that
    /// the comparison itself rounds nothing away.
    fn at_least_as_dear(now: (u128, u128), then: (u128, u128)) -> bool {
        now.0 * then.1 >= then.0 * now.1
    }

    /// Actions from the whole vocabulary, enough of them that a market is built up and unwound
    /// several times over inside one sequence.
    fn a_sequence() -> impl Strategy<Value = Vec<(u8, u8, u128)>> {
        prop::collection::vec((0u8..3u8, 0u8..9u8, 1u128..2_000_000u128), 1..48)
    }

    /// One drawn triple as an action against what the actor holds, so that unwinding actions name
    /// amounts a holder could plausibly name.
    fn drawn(state: &LendingState, actor: AgentId, kind: u8, size: u128) -> LendingAction {
        let held = state.position(MARKET, actor);
        match kind {
            0 => LendingAction::Supply {
                market: MARKET,
                given: Given::Assets(Amount::new(size)),
            },
            1 => LendingAction::SupplyCollateral {
                market: MARKET,
                assets: Amount::new(size),
            },
            2 => LendingAction::Borrow {
                market: MARKET,
                given: Given::Assets(Amount::new(size)),
            },
            3 => LendingAction::Repay {
                market: MARKET,
                given: Given::Shares(Amount::<BorrowShare>::new(
                    held.borrow_shares.base_units().min(size),
                )),
            },
            4 => LendingAction::Withdraw {
                market: MARKET,
                given: Given::Shares(Amount::<SupplyShare>::new(
                    held.supply_shares.base_units().min(size),
                )),
            },
            5 => LendingAction::WithdrawCollateral {
                market: MARKET,
                assets: Amount::new(held.collateral.base_units().min(size)),
            },
            6 => LendingAction::Supply {
                market: MARKET,
                given: Given::Shares(Amount::<SupplyShare>::new(size)),
            },
            7 => LendingAction::Withdraw {
                market: MARKET,
                given: Given::Assets(Amount::new(size)),
            },
            _ => {
                // Against one of the others, for a third, a half or the whole of what it holds:
                // taking all of it is what reaches a write-off.
                let borrower = AgentId((actor.0 + 1) % 3);
                let pledged = state.position(MARKET, borrower).collateral.base_units();
                LendingAction::Liquidate {
                    market: MARKET,
                    borrower,
                    given: Given::Assets(Amount::new(pledged / (1 + size % 3))),
                }
            }
        }
    }

    proptest! {
        /// Conservation. Every share a market issued is held by someone and every share held was
        /// issued: the two ledgers never drift apart, whatever sequence runs over them and
        /// whatever interest accrues between.
        #[test]
        fn the_two_share_ledgers_never_drift_apart(steps in a_sequence()) {
            let lending = under(plunging(), RATE);
            let mut state = opening(FEE);
            let mut events = Vec::new();
            for (index, (who, kind, size)) in steps.into_iter().enumerate() {
                let now = Tick(index as u64);
                let actor = AgentId(u32::from(who));
                let action = drawn(&state, actor, kind, size);
                let _ = lending.apply(&mut state, actor, action, now, &mut events);
                events.clear();
                prop_assert!(lending.check_invariants(&state).is_ok());
                prop_assert!(lending.tick(&mut state, now, &mut events).is_ok());
                events.clear();
                prop_assert!(lending.check_invariants(&state).is_ok());
            }
        }

        /// Rejection safety. A refused action leaves the state as it found it, and leaves the
        /// event buffer alone too — the engine reuses one for the whole run.
        #[test]
        fn a_refused_action_moves_nothing(steps in a_sequence()) {
            let lending = under(plunging(), RATE);
            let mut state = opening(FEE);
            let mut events = Vec::new();
            for (index, (who, kind, size)) in steps.into_iter().enumerate() {
                let now = Tick(index as u64);
                let actor = AgentId(u32::from(who));
                // Half the draws are sized against a holder who is not acting, so plainly
                // impossible amounts are tried alongside the plausible ones.
                let against = if index % 2 == 0 { actor } else { AgentId(u32::from(who) + 9) };
                let action = drawn(&state, against, kind, size);
                let before = state.clone();
                let reported = events.len();
                if lending.apply(&mut state, actor, action, now, &mut events).is_err() {
                    prop_assert_eq!(&state, &before, "a refusal moved the market");
                    prop_assert_eq!(events.len(), reported, "a refusal reported an event");
                }
                events.clear();
            }
        }

        /// Monotone accrual. Time only ever adds to what is owed and to what is due, never takes
        /// away, and a tick that took no time moves nothing at all.
        #[test]
        fn time_only_ever_adds(
            rate in 0u128..100_000_000_000u128,
            gaps in prop::collection::vec(0u64..5_000u64, 1..24),
        ) {
            let lending = mechanism(Amount::new(rate));
            let mut state = opening(FEE);
            let mut events = Vec::new();
            lending.apply(&mut state, ALICE, supply(1_000_000_000), Tick(0), &mut events).unwrap();
            lending.apply(&mut state, BOB, pledge(1_000_000_000), Tick(0), &mut events).unwrap();
            lending.apply(&mut state, BOB, borrow(700_000_000), Tick(0), &mut events).unwrap();
            events.clear();

            let mut now = Tick(0);
            for gap in gaps {
                let before = state.market(MARKET).unwrap();
                now = Tick(now.0 + gap);
                prop_assert!(lending.tick(&mut state, now, &mut events).is_ok());
                events.clear();
                let after = state.market(MARKET).unwrap();

                prop_assert!(after.total_borrow_assets >= before.total_borrow_assets);
                prop_assert!(after.total_supply_assets >= before.total_supply_assets);
                prop_assert!(after.total_supply_shares >= before.total_supply_shares);
                prop_assert!(after.last_update >= before.last_update);
                prop_assert_eq!(after.last_update, now);
                // Interest lands on both sides equally, so the liquidity nobody borrowed stays
                // exactly where it was.
                prop_assert_eq!(
                    after.total_supply_assets.base_units()
                        - after.total_borrow_assets.base_units(),
                    before.total_supply_assets.base_units()
                        - before.total_borrow_assets.base_units()
                );
                if gap == 0 {
                    prop_assert_eq!(after, before, "a tick that took no time moved a market");
                }
                prop_assert!(lending.check_invariants(&state).is_ok());
            }
        }

        /// Conservatism, in the units the rounding lives in. One thing in this mechanism can
        /// make a supply share cheaper, and it is a realised loss.
        ///
        /// Stated as a price rather than as a round trip through assets, and that is what makes
        /// it sharp. A share opens worth a millionth of an asset, so a conversion that credits
        /// one share too many moves nothing an asset-denominated round trip can see, while the
        /// same error moves this immediately. All four cells of the reference's table for the
        /// supply side are covered, because the generated sequence states both a supply and a
        /// withdrawal each way round.
        ///
        /// The exception is not a weakening. A debt whose collateral is gone is written off
        /// against the supply side, and a share of that side being worth less afterwards is what
        /// bearing the loss means. What the property still forbids is a share getting cheaper
        /// for any other reason.
        ///
        /// The debt side has no such claim, and not for want of one: clearing debt costs the
        /// ceiling of what the shares are worth, so a repayment retires more assets than shares
        /// and the shares left behind owe less each. That is the direction that protects the
        /// market, and a monotone price would be the symptom of the opposite.
        #[test]
        fn only_a_realised_loss_makes_a_supply_share_cheaper(steps in a_sequence()) {
            let lending = under(plunging(), RATE);
            let mut state = opening(FEE);
            let mut events = Vec::new();
            for (index, (who, kind, size)) in steps.into_iter().enumerate() {
                let now = Tick(index as u64);
                let actor = AgentId(u32::from(who));
                let action = drawn(&state, actor, kind, size);

                let before = supply_price(&state.market(MARKET).unwrap());
                let accepted = lending.apply(&mut state, actor, action, now, &mut events).is_ok();
                let wrote_off = events.iter().any(|event| {
                    matches!(event, LendingEvent::Liquidated { bad_debt_shares, .. }
                        if !bad_debt_shares.is_zero())
                });
                events.clear();
                if accepted && !wrote_off {
                    let after = supply_price(&state.market(MARKET).unwrap());
                    prop_assert!(at_least_as_dear(after, before), "an action cheapened a share");
                }

                let before = supply_price(&state.market(MARKET).unwrap());
                prop_assert!(lending.tick(&mut state, now, &mut events).is_ok());
                events.clear();
                let after = supply_price(&state.market(MARKET).unwrap());
                prop_assert!(at_least_as_dear(after, before), "accrual cheapened a share");
            }
        }

        /// A loss is realised exactly when a seizure takes the last of the collateral: never
        /// while any is left, and always once none is. The second half is what stops a debt
        /// nobody can service from staying on the books and accruing interest forever.
        #[test]
        fn a_loss_is_realised_exactly_when_the_collateral_runs_out(steps in a_sequence()) {
            let lending = under(plunging(), RATE);
            let mut state = opening(FEE);
            let mut events = Vec::new();
            for (index, (who, kind, size)) in steps.into_iter().enumerate() {
                let now = Tick(index as u64);
                let actor = AgentId(u32::from(who));
                let action = drawn(&state, actor, kind, size);
                let accepted = lending.apply(&mut state, actor, action, now, &mut events).is_ok();

                for event in &events {
                    let LendingEvent::Liquidated { borrower, bad_debt_shares, .. } = *event else {
                        continue;
                    };
                    prop_assert!(accepted);
                    let left = state.position(MARKET, borrower);
                    if !bad_debt_shares.is_zero() {
                        prop_assert!(left.collateral.is_zero(), "a loss was taken too early");
                    }
                    prop_assert!(
                        !left.collateral.is_zero() || left.borrow_shares.is_zero(),
                        "a borrower with nothing left kept a debt"
                    );
                }
                events.clear();
                prop_assert!(lending.tick(&mut state, now, &mut events).is_ok());
                events.clear();
                prop_assert!(lending.check_invariants(&state).is_ok());
            }
        }
    }
}
