//! Level 1: one real episode, recorded, and put back through the mechanism.
//!
//! Where level 0 asks whether the arithmetic is the reference's, this asks whether the mechanism
//! keeps up with a market that actually happened — the actions in the order a chain accepted
//! them, against the rates it charged and the prices it was quoted, ending where the chain
//! ended.
//!
//! The rates are recorded rather than modelled, and that is the point of the borrow rate being a
//! port. The curve that priced this episode carries state of its own that this workspace does
//! not model; handing the mechanism the rates the episode was actually charged is what lets the
//! rest of it be checked without waiting for that.
//!
//! What a node is asked, and what is done with the answers:
//!
//! ```text
//! eth_getLogs          { address: <contract>, topics: [null, <market id>], from, to }
//! eth_getBlockByNumber for every block those logs fall in, for its timestamp
//! eth_call             market(bytes32) and position(bytes32,address) at each end of the window
//! eth_call             price() on the market's oracle, at each of those blocks
//!
//! { "chain_id": …, "logs": […], "block_times": {…}, "opening": {…}, "closing": {…},
//!   "market_params": {…}, "prices": {…} }
//!   | cargo run -p karman-reference --bin karman-episode > testdata/episodes/<name>.json
//! ```

use sting_karman_data::PulledMarket;
use sting_karman_mech_lending::{
    Charged, Collateral, Given, LendingAction, LendingEvent, LendingState, Loan, Wad,
};
use sting_karman_types::{u128_str, AgentId, Amount, MarketId, Tick};
use sting_karman_verify::replay::RecordedAction;
use serde::{Deserialize, Serialize};

use crate::Quoted;

/// The market a recorded episode opens under.
pub const EPISODE: MarketId = MarketId(0);

/// Where a recorded episode came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeOrigin {
    /// The chain that answered.
    pub chain_id: u64,
    /// The contract the market lives in.
    pub contract: String,
    /// The market's identifier within it.
    pub market: String,
    /// The first block the episode covers.
    pub from_block: u64,
    /// The last.
    pub to_block: u64,
    /// The timestamp tick zero stands for.
    pub opening_timestamp: u64,
}

/// What a market held when the episode ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closing {
    /// Everything supplied.
    #[serde(with = "u128_str")]
    pub total_supply_assets: u128,
    /// The shares dividing it.
    #[serde(with = "u128_str")]
    pub total_supply_shares: u128,
    /// Everything owed.
    #[serde(with = "u128_str")]
    pub total_borrow_assets: u128,
    /// The shares dividing that.
    #[serde(with = "u128_str")]
    pub total_borrow_shares: u128,
}

/// One episode of a real market: where it started, what happened, and where it ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    /// Where this came from.
    pub origin: EpisodeOrigin,
    /// The threshold the market is listed at.
    pub lltv: Amount<Wad>,
    /// The market and its holders as the episode opened.
    pub opening: PulledMarket,
    /// What the collateral was quoted at, and from when.
    pub prices: Vec<Quoted>,
    /// What the market charged, and from when.
    pub rates: Vec<Charged>,
    /// The actions the chain accepted, in the order it accepted them.
    pub steps: Vec<RecordedAction<LendingAction, LendingEvent>>,
    /// What the market held when it was over.
    pub closing: Closing,
    /// Everyone the window names, in the order that gives them their identifiers.
    pub actors: Vec<String>,
}

/// The one action a recorded event stands for, and who the venue settled it against.
///
/// The venue records both sides of every conversion and not which side the caller named, so a
/// replay names the assets. Where a transaction named the shares instead, the side derived here
/// can differ from the side recorded by the unit the two roundings disagree over, and the replay
/// reports that rather than hiding it — an episode is evidence about a mechanism, and a
/// recording bent until it agreed would be evidence about nothing.
pub(crate) fn acted(event: &LendingEvent) -> Option<(AgentId, LendingAction)> {
    let market = EPISODE;
    Some(match *event {
        LendingEvent::Supplied { holder, assets, .. } => {
            (holder, LendingAction::Supply { market, given: Given::Assets(assets) })
        }
        LendingEvent::Withdrawn { holder, assets, .. } => {
            (holder, LendingAction::Withdraw { market, given: Given::Assets(assets) })
        }
        LendingEvent::Borrowed { holder, assets, .. } => {
            (holder, LendingAction::Borrow { market, given: Given::Assets(assets) })
        }
        LendingEvent::Repaid { holder, assets, .. } => {
            (holder, LendingAction::Repay { market, given: Given::Assets(assets) })
        }
        LendingEvent::CollateralAdded { holder, assets, .. } => {
            (holder, LendingAction::SupplyCollateral { market, assets })
        }
        LendingEvent::CollateralRemoved { holder, assets, .. } => {
            (holder, LendingAction::WithdrawCollateral { market, assets })
        }
        LendingEvent::Liquidated { liquidator, borrower, seized, .. } => (
            liquidator,
            LendingAction::Liquidate { market, borrower, given: Given::Assets(seized) },
        ),
        // Not an action. Interest is what a transition does before it does anything else, so an
        // accrual belongs to the step that caused it rather than being a step of its own.
        LendingEvent::InterestAccrued { .. } => return None,
    })
}

/// One raw log as the events this workspace reports.
///
/// # Errors
///
/// Fails when the log is not one of this market's, or names an actor the window did not.
pub fn decode(
    address: &str,
    topics: &[String],
    data: &str,
    name: &dyn Fn(String) -> Result<AgentId, String>,
) -> Result<Vec<LendingEvent>, String> {
    let log = raw(address, topics, data)?;
    crate::evm::reported(&[log], EPISODE, &|who| name(who.to_string().to_lowercase()))
}

/// What the market charged, if this log is the accrual that says so.
///
/// The rate is the one number in an accrual this workspace does not carry in its own event, and
/// it is the one a replay cannot do without: the curve that produced it is not modelled here.
///
/// # Errors
///
/// Fails when the log cannot be read at all.
pub fn charged_at(topics: &[String], data: &str) -> Result<Option<u128>, String> {
    use alloy_sol_types::SolEvent;

    let log = raw("0x0000000000000000000000000000000000000000", topics, data)?;
    let Ok(accrued) = crate::evm::AccrueInterest::decode_log(&log) else {
        return Ok(None);
    };
    u128::try_from(accrued.prevBorrowRate)
        .map(Some)
        .map_err(|_| String::from("a recorded rate is past what this workspace can carry"))
}

/// What a market held, from what its getter answered.
///
/// # Errors
///
/// Fails when the answer is not the six totals the getter returns.
pub fn closing(answer: &str) -> Result<Closing, String> {
    let body = answer.strip_prefix("0x").unwrap_or(answer);
    if body.len() != 6 * 64 {
        return Err(String::from("the closing market is not six words"));
    }
    let word = |at: usize| -> Result<u128, String> {
        u128::from_str_radix(&body[at * 64 + 32..at * 64 + 64], 16)
            .map_err(|_| format!("closing word {at} is not a number"))
    };
    Ok(Closing {
        total_supply_assets: word(0)?,
        total_supply_shares: word(1)?,
        total_borrow_assets: word(2)?,
        total_borrow_shares: word(3)?,
    })
}

/// The action a recorded event stands for, for whoever is assembling a recording.
#[must_use]
pub fn acted_on(event: &LendingEvent) -> Option<(AgentId, LendingAction)> {
    acted(event)
}

fn raw(address: &str, topics: &[String], data: &str) -> Result<revm::primitives::Log, String> {
    use revm::primitives::{Address, Bytes, LogData, B256};

    let address: Address = address.parse().map_err(|_| format!("`{address}` is not an address"))?;
    let topics: Vec<B256> = topics
        .iter()
        .map(|topic| topic.parse::<B256>().map_err(|_| format!("`{topic}` is not a topic")))
        .collect::<Result<_, _>>()?;
    let bytes: Bytes = data.parse().map_err(|_| String::from("a log carries unreadable data"))?;
    Ok(revm::primitives::Log {
        address,
        data: LogData::new(topics, bytes)
            .ok_or_else(|| String::from("a log has too many topics"))?,
    })
}

/// The prices an episode was quoted, as the path the mechanism reads them through.
///
/// Holds forward between quotes, which is what an oracle does: a price is what it was until the
/// next time it was published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPrice(Vec<Quoted>);

impl RecordedPrice {
    /// The quotes, in the order they were published.
    ///
    /// # Errors
    ///
    /// Refuses a schedule out of order rather than sorting one, for the reason a recorded rate
    /// does: a quote against a tick that never carried it is worse than none.
    pub fn new(quoted: Vec<Quoted>) -> Result<Self, sting_karman_core::SourceError> {
        if quoted.windows(2).any(|pair| pair[1].from <= pair[0].from) {
            return Err(sting_karman_core::SourceError::new("a recorded price is out of order"));
        }
        Ok(RecordedPrice(quoted))
    }
}

impl sting_karman_core::PathSource<sting_karman_mech_lending::Price<Collateral, Loan>> for RecordedPrice {
    fn value_at(
        &self,
        now: Tick,
    ) -> Result<sting_karman_mech_lending::Price<Collateral, Loan>, sting_karman_core::SourceError> {
        self.0
            .iter()
            .rev()
            .find(|quoted| quoted.from <= now)
            .map(|quoted| sting_karman_mech_lending::Price::wider(quoted.price))
            .ok_or_else(|| sting_karman_core::SourceError::new("the recording quoted nothing here"))
    }
}

/// The state a recorded episode opens from.
///
/// Built the way a pulled market is, because it is one: the holders the episode names, and one
/// standing for everybody it does not, so that every share the market issued is held by
/// somebody.
///
/// # Errors
///
/// Fails when the opening does not reconcile, or holds a quantity the mechanism cannot open.
pub fn opening(episode: &Episode) -> Result<LendingState, String> {
    let unnamed = episode.opening.unaccounted("unnamed").map_err(|e| e.to_string())?;
    let market = sting_karman_mech_lending::Market {
        total_supply_assets: Amount::<Loan>::new(episode.opening.total_supply_assets),
        total_supply_shares: Amount::new(episode.opening.total_supply_shares),
        total_borrow_assets: Amount::new(episode.opening.total_borrow_assets),
        total_borrow_shares: Amount::new(episode.opening.total_borrow_shares),
        last_update: Tick(0),
        fee: Amount::new(episode.opening.fee),
    };
    let mut state = LendingState::new().with_market(EPISODE, market).map_err(|e| e.to_string())?;
    for held in &episode.opening.holders {
        let at =
            episode.actors.iter().position(|known| known == &held.holder).ok_or_else(|| {
                format!("the opening names a holder the window did not: {}", held.holder)
            })?;
        state = state
            .with_position(
                EPISODE,
                AgentId(u32::try_from(at).map_err(|_| "too many holders".to_owned())?),
                sting_karman_mech_lending::Position {
                    supply_shares: Amount::new(held.supply_shares),
                    borrow_shares: Amount::new(held.borrow_shares),
                    collateral: Amount::new(held.collateral),
                },
            )
            .map_err(|e| e.to_string())?;
    }
    let rest = AgentId(u32::try_from(episode.actors.len()).unwrap_or(u32::MAX));
    state
        .with_position(
            EPISODE,
            rest,
            sting_karman_mech_lending::Position {
                supply_shares: Amount::new(unnamed.supply_shares),
                borrow_shares: Amount::new(unnamed.borrow_shares),
                collateral: Amount::new(unnamed.collateral),
            },
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use sting_karman_mech_lending::{Lending, MarketParams, RecordedRate};
    use sting_karman_types::{SecondsPerTick, Wide};
    use sting_karman_verify::{replay::replay, Verdict};

    use super::*;

    /// One episode of a real market, as a chain answered for it.
    const RECORDED: &str = include_str!("../../../testdata/episodes/morpho-liquidation.json");

    fn recorded() -> Episode {
        serde_json::from_str(RECORDED).expect("the committed episode is not readable")
    }

    fn mechanism(episode: &Episode) -> Lending<RecordedPrice, RecordedRate> {
        Lending::new(SecondsPerTick(NonZeroU64::new(1).unwrap()))
            .with_market(
                EPISODE,
                MarketParams { lltv: episode.lltv, fee_recipient: AgentId(0) },
                RecordedPrice::new(episode.prices.clone()).unwrap(),
                RecordedRate::new(episode.rates.clone()).unwrap(),
            )
            .unwrap()
    }

    /// A quote holds until the next one, and a schedule out of order is refused rather than
    /// sorted — the same rule the recorded rate keeps, for the same reason.
    #[test]
    fn a_recorded_price_holds_forward_and_refuses_disorder() {
        use sting_karman_core::PathSource;

        let quoted = RecordedPrice::new(vec![
            Quoted { from: Tick(5), price: Wide::from_u128(7) },
            Quoted { from: Tick(9), price: Wide::from_u128(11) },
        ])
        .unwrap();
        assert!(quoted.value_at(Tick(4)).is_err());
        assert_eq!(quoted.value_at(Tick(8)).unwrap(), sting_karman_mech_lending::Price::new(7));
        assert_eq!(quoted.value_at(Tick(900)).unwrap(), sting_karman_mech_lending::Price::new(11));

        assert!(RecordedPrice::new(vec![
            Quoted { from: Tick(9), price: Wide::from_u128(11) },
            Quoted { from: Tick(5), price: Wide::from_u128(7) },
        ])
        .is_err());
    }

    /// The gate: the actions a chain accepted, in the order it accepted them, produce the events
    /// it reported and leave the market where it left it.
    #[test]
    fn the_mechanism_keeps_up_with_a_real_liquidation() {
        let episode = recorded();
        assert!(
            episode.steps.iter().any(|step| matches!(step.action, LendingAction::Liquidate { .. })),
            "an episode recorded for a liquidation has none in it"
        );

        let mut state = opening(&episode).expect("the episode does not open");
        let verdict = replay(&mechanism(&episode), &mut state, episode.steps.clone());
        assert_eq!(verdict, Verdict::Confirmed);

        let market = state.market(EPISODE).unwrap();
        assert_eq!(market.total_supply_assets.base_units(), episode.closing.total_supply_assets);
        assert_eq!(market.total_supply_shares.base_units(), episode.closing.total_supply_shares);
        assert_eq!(market.total_borrow_assets.base_units(), episode.closing.total_borrow_assets);
        assert_eq!(market.total_borrow_shares.base_units(), episode.closing.total_borrow_shares);
    }

    /// The rates are the episode's own, and this is what says they are doing the work. A market
    /// charged a percent more for the same seconds owes a percent more, and an episode that still
    /// replayed would be one whose rates were decoration.
    ///
    /// A percent rather than a unit: a rate is quoted per second at eighteen decimals, and over
    /// the minutes this episode covers a single unit of it moves nothing an integer can hold.
    #[test]
    fn an_episode_charged_a_different_rate_does_not_replay() {
        let episode = recorded();
        let nudged: Vec<Charged> = episode
            .rates
            .iter()
            .map(|charged| Charged {
                from: charged.from,
                per_second: charged
                    .per_second
                    .checked_add(Amount::new(charged.per_second.base_units() / 100))
                    .unwrap(),
            })
            .collect();
        let mechanism = Lending::new(SecondsPerTick(NonZeroU64::new(1).unwrap()))
            .with_market(
                EPISODE,
                MarketParams { lltv: episode.lltv, fee_recipient: AgentId(0) },
                RecordedPrice::new(episode.prices.clone()).unwrap(),
                RecordedRate::new(nudged).unwrap(),
            )
            .unwrap();
        let mut state = opening(&episode).unwrap();
        assert!(matches!(
            replay(&mechanism, &mut state, episode.steps.clone()),
            Verdict::Diverged { .. }
        ));
    }

    /// The episode is one this workspace could get wrong in the direction that matters: a market
    /// listed a percentage point looser would have let the liquidation through differently.
    #[test]
    fn an_episode_replayed_against_a_looser_market_does_not_replay() {
        let episode = recorded();
        let loose = Episode {
            lltv: episode.lltv.checked_add(Amount::new(50_000_000_000_000_000)).unwrap(),
            ..episode.clone()
        };
        let mut state = opening(&episode).unwrap();
        assert!(matches!(
            replay(&mechanism(&loose), &mut state, episode.steps.clone()),
            Verdict::Diverged { .. }
        ));
    }
}
