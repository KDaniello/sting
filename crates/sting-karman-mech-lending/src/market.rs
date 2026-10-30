use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sting_karman_core::MechError;
use sting_karman_types::{AgentId, Amount, MarketId, Tick};

use crate::{BorrowShare, Collateral, Loan, MAX_FEE, SupplyShare, Wad};

/// The terms of one market: what it is, rather than what it currently holds.
///
/// Two markets over the same pair at different liquidation thresholds are two markets, which is
/// why the reference derives a market's identity from terms like these. They belong to the
/// mechanism rather than to the state because nothing a run does can change them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketParams {
    /// The share of its collateral's value a position may owe, at [`crate::WAD`].
    pub lltv: Amount<Wad>,
    /// Who receives the shares a market mints out of its own accrued interest.
    pub fee_recipient: AgentId,
}

/// What one market holds, and the tick it was last brought up to date on.
///
/// Six fields, as the reference stores them: the supply and the debt each carried both as an
/// asset total that interest moves and as the shares dividing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Market {
    /// Everything supplied, including every unit of interest accrued onto it.
    pub total_supply_assets: Amount<Loan>,
    /// The shares dividing it.
    pub total_supply_shares: Amount<SupplyShare>,
    /// Everything owed, including every unit of interest accrued onto it.
    pub total_borrow_assets: Amount<Loan>,
    /// The shares dividing that.
    pub total_borrow_shares: Amount<BorrowShare>,
    /// The tick interest has been accrued to.
    ///
    /// Every dated transition brings this forward before reading the totals, so several actions
    /// on one tick compound once between them rather than once each.
    pub last_update: Tick,
    /// The share of accrued interest this market mints for its fee recipient, at [`crate::WAD`].
    pub fee: Amount<Wad>,
}

impl Market {
    /// An empty market opening at `now`.
    ///
    /// # Errors
    ///
    /// Refuses a fee above [`MAX_FEE`]: a market that could take all of its own interest would
    /// leave the supply side earning nothing while still bearing the risk.
    pub fn opening(now: Tick, fee: Amount<Wad>) -> Result<Self, MechError> {
        if fee > MAX_FEE {
            return Err(MechError::new("fee exceeds the maximum a market may take"));
        }
        Ok(Market {
            total_supply_assets: Amount::ZERO,
            total_supply_shares: Amount::ZERO,
            total_borrow_assets: Amount::ZERO,
            total_borrow_shares: Amount::ZERO,
            last_update: now,
            fee,
        })
    }
}

/// What one agent holds in one market.
///
/// Absent is the same as empty, as it is in the reference's storage: a holder with nothing has
/// no record, and reading one produces zeroes rather than an error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// The holder's claim on the supply side.
    pub supply_shares: Amount<SupplyShare>,
    /// The holder's share of the debt.
    pub borrow_shares: Amount<BorrowShare>,
    /// What backs that debt. Never lent out, and valued only by the oracle.
    pub collateral: Amount<Collateral>,
}

impl Position {
    /// Whether anything at all is recorded here.
    pub(crate) fn is_empty(&self) -> bool {
        self.supply_shares.is_zero() && self.borrow_shares.is_zero() && self.collateral.is_zero()
    }
}

/// Every market a run holds, and every position in them.
///
/// Positions are keyed by the pair rather than nested under their market. The traversal order is
/// the one a nested map would give — the key orders by market first — and a run with many markets
/// does not carry a map per market to get it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LendingState {
    markets: BTreeMap<MarketId, Market>,
    #[serde(with = "ledger")]
    positions: BTreeMap<(MarketId, AgentId), Position>,
}

impl LendingState {
    /// A run holding no markets yet.
    #[must_use]
    pub fn new() -> Self {
        LendingState::default()
    }

    /// Opens `market` under `id`.
    ///
    /// # Errors
    ///
    /// Refuses an identifier already in use: silently replacing a market would discard the
    /// positions still keyed to it.
    pub fn with_market(mut self, id: MarketId, market: Market) -> Result<Self, MechError> {
        if self.markets.insert(id, market).is_some() {
            return Err(MechError::new(
                "a market is already open under that identifier",
            ));
        }
        Ok(self)
    }

    /// Records what `holder` holds in `id`.
    ///
    /// A state is built rather than only walked: a fixture pulled from a live market, or a
    /// corpus read out of the implementation this one models, arrives as balances rather than as
    /// the sequence of actions that produced them.
    ///
    /// # Errors
    ///
    /// Refuses a position in a market that is not open, which would be a holding nothing prices.
    pub fn with_position(
        mut self,
        id: MarketId,
        holder: AgentId,
        position: Position,
    ) -> Result<Self, MechError> {
        if !self.markets.contains_key(&id) {
            return Err(MechError::new("no market is open under that identifier"));
        }
        self.set_position(id, holder, position);
        Ok(self)
    }

    /// The market under `id`, if one is open.
    #[must_use]
    pub fn market(&self, id: MarketId) -> Option<Market> {
        self.markets.get(&id).copied()
    }

    /// What `holder` holds in `id`. A holder with no record holds nothing.
    #[must_use]
    pub fn position(&self, id: MarketId, holder: AgentId) -> Position {
        self.positions
            .get(&(id, holder))
            .copied()
            .unwrap_or_default()
    }

    /// Every open market, in identifier order.
    ///
    /// Exact-size, so a caller that needs the count pays nothing to ask for it.
    #[must_use]
    pub fn markets(&self) -> impl ExactSizeIterator<Item = (MarketId, &Market)> {
        self.markets.iter().map(|(id, market)| (*id, market))
    }

    /// Every holder in one market, in identifier order.
    pub fn holders(&self, id: MarketId) -> impl Iterator<Item = (AgentId, &Position)> {
        self.positions
            .range((id, AgentId(u32::MIN))..=(id, AgentId(u32::MAX)))
            .map(|((_, holder), position)| (*holder, position))
    }

    /// Every position on record, with the market it belongs to. Exact-size for the same reason.
    #[must_use]
    pub fn holdings(&self) -> impl ExactSizeIterator<Item = (MarketId, AgentId, &Position)> {
        self.positions
            .iter()
            .map(|((id, holder), position)| (*id, *holder, position))
    }

    /// Replaces the market under `id`. Only ever called with a market read from this state, so
    /// there is nothing here to create.
    pub(crate) fn set_market(&mut self, id: MarketId, market: Market) {
        self.markets.insert(id, market);
    }

    /// Replaces what `holder` holds in `id`.
    ///
    /// A position emptied to nothing is removed rather than recorded as zeroes: a population
    /// that cycles through a market would otherwise leave a row behind for every agent that ever
    /// touched it, and every invariant check would then walk them.
    pub(crate) fn set_position(&mut self, id: MarketId, holder: AgentId, position: Position) {
        if position.is_empty() {
            self.positions.remove(&(id, holder));
        } else {
            self.positions.insert((id, holder), position);
        }
    }
}

/// A pair-keyed map as a sequence of records.
///
/// JSON has no key but a string, and a composite key encoded into one would have to be parsed
/// back out on the way in. A record that carries its own market and holder needs no such
/// convention, and reads as a hand-written fixture should.
mod ledger {
    use super::{
        AgentId, BTreeMap, Deserialize, Deserializer, MarketId, Position, Serialize, Serializer,
    };

    #[derive(Serialize, Deserialize)]
    struct Held {
        market: MarketId,
        holder: AgentId,
        position: Position,
    }

    pub(super) fn serialize<S: Serializer>(
        positions: &BTreeMap<(MarketId, AgentId), Position>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(positions.iter().map(|((market, holder), position)| Held {
            market: *market,
            holder: *holder,
            position: *position,
        }))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<(MarketId, AgentId), Position>, D::Error> {
        Ok(Vec::<Held>::deserialize(deserializer)?
            .into_iter()
            .map(|held| ((held.market, held.holder), held.position))
            .collect())
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

    const FIRST: MarketId = MarketId(0);
    const SECOND: MarketId = MarketId(1);
    const ALICE: AgentId = AgentId(0);

    fn empty_market() -> Market {
        Market::opening(Tick(0), Amount::ZERO).unwrap()
    }

    fn state() -> LendingState {
        LendingState::new()
            .with_market(FIRST, empty_market())
            .unwrap()
    }

    #[test]
    fn a_market_may_not_take_more_than_its_share_of_interest() {
        assert!(Market::opening(Tick(0), MAX_FEE).is_ok());
        assert!(Market::opening(Tick(0), MAX_FEE.checked_add(Amount::new(1)).unwrap()).is_err());
    }

    /// Reopening under a live identifier would orphan every position keyed to it, so the state
    /// refuses rather than replacing.
    #[test]
    fn a_market_cannot_be_opened_twice_under_one_identifier() {
        assert!(state().with_market(FIRST, empty_market()).is_err());
        assert!(state().with_market(SECOND, empty_market()).is_ok());
    }

    #[test]
    fn a_holder_with_no_record_holds_nothing() {
        assert_eq!(state().position(FIRST, ALICE), Position::default());
    }

    /// An emptied position leaves no row, so a market cycled through by a large population does
    /// not accumulate one per agent that ever touched it.
    #[test]
    fn a_position_emptied_to_nothing_leaves_no_record() {
        let mut state = state();
        state.set_position(
            FIRST,
            ALICE,
            Position {
                collateral: Amount::new(1),
                ..Position::default()
            },
        );
        assert_eq!(state.holders(FIRST).count(), 1);
        state.set_position(FIRST, ALICE, Position::default());
        assert_eq!(state.holders(FIRST).count(), 0);
    }

    /// Positions are keyed by market first, so one market's holders are a contiguous range and
    /// reading them never touches another market's.
    #[test]
    fn holders_are_read_one_market_at_a_time() {
        let mut state = state().with_market(SECOND, empty_market()).unwrap();
        let held = Position {
            collateral: Amount::new(1),
            ..Position::default()
        };
        state.set_position(SECOND, AgentId(7), held);
        state.set_position(FIRST, ALICE, held);
        assert_eq!(
            state.holders(FIRST).map(|(who, _)| who).collect::<Vec<_>>(),
            vec![ALICE]
        );
        assert_eq!(
            state
                .holders(SECOND)
                .map(|(who, _)| who)
                .collect::<Vec<_>>(),
            vec![AgentId(7)]
        );
        assert_eq!(state.holdings().count(), 2);
    }

    /// A fixture has to be able to carry a whole run's markets and positions back in, and the
    /// pair key has no JSON spelling of its own.
    #[test]
    fn a_state_survives_the_serialisation_boundary() {
        let mut state = state().with_market(SECOND, empty_market()).unwrap();
        state.set_position(
            FIRST,
            ALICE,
            Position {
                supply_shares: Amount::new(u128::MAX),
                borrow_shares: Amount::new(7),
                collateral: Amount::new(11),
            },
        );
        let restored: LendingState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(restored, state);
    }
}
