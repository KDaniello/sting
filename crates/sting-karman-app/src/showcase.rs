//! A measured market as the world a run starts from.
//!
//! The two halves that meet here have to. The boundary crate knows how to count a chain and
//! nothing about a showcase; the mechanism knows about a showcase and reads nothing. A composition
//! root is the only place both are in scope, which is where the translation belongs.
//!
//! What this builds is specified in
//! [`DEAD-CAPITAL-SPEC.md`](https://github.com/karman-dev/karman/blob/main/karman/docs/DEAD-CAPITAL-SPEC.md)
//! §12, and the division that section draws is the thing to keep hold of while reading:
//!
//! - **Taken from the census and never fitted** — how many items a collection has, what share sits
//!   at its issuer, how many holders the rest is spread over and how much the largest of them have,
//!   and the median price its sales actually paid. These are the shape of the market.
//! - **Fitted, and only three** — the shape of the common worth, the spread of taste, and the share
//!   of holders who would sell. The third is a property of the population rather than of the world,
//!   so it is not here.
//!
//! Three parameters cannot fit four numbers, and the fourth — what a hundred and seventeen
//! collections do differently from one another — is the one nothing here was fitted to.
//!
//! What a world does **not** carry is a table of worths. A million items over ten thousand
//! participants is ten billion valuations and a million common worths, and neither is a thing that
//! fits anywhere; both are computed where they are wanted, from the world's seed and one price per
//! collection. That is [`sting_karman_mech_showcase::Worth`], and it is small enough for every
//! participant to hold one of its own.

use std::{num::NonZeroU64, path::Path, sync::Arc};

use sting_karman_agents::Holder;
use sting_karman_core::{Agent, MechError, Observer, World};
use sting_karman_data::{AccountId, Census, CollectionCensus, read_json_fixture};
use sting_karman_mech_showcase::{
    Arrears, Bps, Cash, CollectionId, Distribution, Item, ItemId, Purse, Showcase, ShowcaseEvent,
    ShowcaseState, WHOLE, Worth,
};
use sting_karman_report::rank_correlation;
use sting_karman_scenario::{
    ArrearsConfig, BehaviourConfig, DistributionConfig, MechanismConfig, ScenarioConfig,
};
use sting_karman_types::{AgentId, Amount, SecondsPerTick, Tick};

use crate::AppError;

/// The venue: it holds a purse, takes what rules send it, and never acts.
pub const VENUE: AgentId = AgentId(0);

/// The issuer: what it holds is stock rather than ownership, and never part of a float.
pub const ISSUER: AgentId = AgentId(1);

/// The first seat a holder can occupy, the venue and the issuer having the two before it.
const FIRST_HOLDER: u64 = 2;

/// How many of a collection's largest holders the census names, and this reproduces one for one.
const NAMED: usize = 10;

/// What a generated world is, beyond what the census decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSpec {
    /// How many items the world holds across every collection it covers.
    ///
    /// The census's own stock is more than a million and a sweep is millions of runs; the two do
    /// not fit together and are not meant to. What a world preserves is every proportion and no
    /// count, which is why every number the gate compares is a ratio.
    pub items: u32,
    /// How many participants there are, the venue and the issuer included.
    pub participants: u32,
    /// What each participant opens with.
    pub capital: Amount<Cash>,
    /// The shape of the common worth, at `1 << 16`. **Fitted.**
    pub shape: u32,
    /// How far taste reaches around the common worth. **Fitted.**
    pub spread: Amount<Bps>,
    /// The world's own seed, which fixes every worth and every opinion in it.
    pub seed: u64,
    /// Which class of the census to generate from, by the path its descriptions are served under.
    ///
    /// A census covers several classes at once — gifts, names, numbers, domains — and they behave
    /// nothing alike: one keeps half its stock at an issuer and the others keep none. A world of
    /// all of them at once would be a world of no market in particular, so a run says which. The
    /// rule is the census's own, and mechanical: the path, and nothing about the names.
    pub covering: Option<String>,
    /// Whether to leave out collections that an issuer did not deploy, or has no stock of.
    ///
    /// Serving a platform's description is not being that platform's collection, and on the market
    /// this was measured on fifty-three collections share a description with another. Neither mark
    /// is proof, which is why leaving them in is a choice a run makes rather than one made for it.
    pub only_issued: bool,
}

/// One collection as it was generated, beside what it was generated from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// What the census calls it.
    pub name: String,
    /// The first item it was given, and how many. Items of one collection are contiguous.
    pub first: ItemId,
    /// How many items it received.
    pub items: u32,
    /// How many of them the issuer holds.
    pub issuer_held: u32,
    /// How many participants hold the rest.
    pub holders: u32,
}

impl Generated {
    /// The items somebody other than the issuer holds.
    #[must_use]
    pub const fn float(&self) -> u32 {
        self.items.saturating_sub(self.issuer_held)
    }
}

/// A generated world, and everything a run needs about it that its state does not carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Market {
    /// What the mechanism sees.
    pub state: ShowcaseState,
    /// What agents and measurement see, and the mechanism never does.
    pub worth: Worth,
    /// The collections, in the order the census lists them.
    pub collections: Vec<Generated>,
}

/// The collections a spec covers, in the order the census lists them.
fn covering<'a>(census: &'a Census, spec: &WorldSpec) -> Vec<&'a CollectionCensus> {
    census
        .collections
        .iter()
        .filter(|of| of.items > 0 && of.float() > 0)
        .filter(|of| {
            spec.covering
                .as_ref()
                .is_none_or(|path| of.content.contains(path.as_str()))
        })
        .filter(|of| !spec.only_issued || (of.renounced() && of.issuer_held > 0))
        .collect()
}

/// The smallest population a world of this many items can be spread over.
///
/// Not a free choice beside the stock: the census fixes how thinly a collection's float is held,
/// so a world of a given size needs at least as many holders as its widest collection had. Seats
/// are shared between collections, as they are on a chain where one person holds several things,
/// so what binds is the widest collection and not the sum of them.
///
/// A caller that sets both sizes without consulting this is setting a concentration the census did
/// not measure, which is why [`from_census`] refuses rather than quietly crowding a float.
#[must_use]
pub fn participants_for(census: &Census, spec: &WorldSpec) -> u64 {
    let covered = covering(census, spec);
    let stock: u64 = covered.iter().map(|of| u64::from(of.items)).sum();
    let widest = covered
        .iter()
        .map(|of| {
            let size = scaled(u64::from(of.items), u64::from(spec.items), stock).max(1);
            let held = scaled(u64::from(of.issuer_held), size, u64::from(of.items)).min(size - 1);
            scaled(u64::from(of.holders), size, u64::from(of.items)).clamp(1, size - held)
        })
        .max()
        .unwrap_or(1);
    widest + FIRST_HOLDER
}

/// Builds a world from a census on disk.
///
/// # Errors
///
/// Fails when the census cannot be read, when it covers nothing to generate from, or when the
/// population is too small to hold what the census says a collection is spread over.
pub fn generate(census: &Path, spec: &WorldSpec) -> Result<Market, AppError> {
    let census: Census = read_json_fixture(census).map_err(AppError::Census)?;
    from_census(&census, spec)
}

/// Builds a world from a census already in hand.
///
/// # Errors
///
/// As [`generate`], less the reading.
pub fn from_census(census: &Census, spec: &WorldSpec) -> Result<Market, AppError> {
    if u64::from(spec.participants) <= FIRST_HOLDER {
        return Err(AppError::Ungenerable(
            "a world needs a venue, an issuer and somebody to hold something".into(),
        ));
    }
    let covered = covering(census, spec);
    if covered.is_empty() {
        return Err(AppError::Ungenerable(
            "this census has nothing to generate from".into(),
        ));
    }
    let needed = participants_for(census, spec);
    if u64::from(spec.participants) < needed {
        return Err(AppError::Ungenerable(format!(
            "{} items are held this thinly by {needed} participants, and this world has {}",
            spec.items, spec.participants
        )));
    }
    let stock: u64 = covered.iter().map(|of| u64::from(of.items)).sum();

    let mut purses = vec![
        Purse {
            balance: spec.capital,
            ..Purse::default()
        };
        spec.participants as usize
    ];
    purses[VENUE.0 as usize].balance = spec.capital;

    let mut items: Vec<Item> = Vec::new();
    let mut medians: Vec<Amount<Cash>> = Vec::new();
    let mut generated: Vec<Generated> = Vec::new();

    for (at, of) in covered.iter().enumerate() {
        let collection = CollectionId(u32::try_from(at).map_err(|_| too_many())?);
        let size = scaled(u64::from(of.items), u64::from(spec.items), stock).max(1);
        let issuer_held =
            scaled(u64::from(of.issuer_held), size, u64::from(of.items)).min(size - 1);
        let float = size - issuer_held;
        let holders = scaled(u64::from(of.holders), size, u64::from(of.items)).clamp(1, float);

        let first = ItemId(u32::try_from(items.len()).map_err(|_| too_many())?);
        medians.push(median(of));
        for step in 0..size {
            // Named to be refused where a world outgrows what an identifier can hold, rather than
            // to be used: what an item is worth is computed from its number, not stored beside it.
            let _ = ItemId(
                first
                    .0
                    .checked_add(u32::try_from(step).map_err(|_| too_many())?)
                    .ok_or_else(too_many)?,
            );
            items.push(Item {
                collection,
                holder: ISSUER,
                listing: None,
            });
        }
        hand_out(
            &mut items,
            first,
            size,
            issuer_held,
            holders,
            of,
            &census.issuers,
            spec,
        )?;
        generated.push(Generated {
            name: of.name.clone(),
            first,
            items: count(size),
            issuer_held: count(issuer_held),
            holders: count(holders),
        });
    }

    let state =
        ShowcaseState::open(items, purses).map_err(|e| AppError::Ungenerable(e.to_string()))?;
    let worth = Worth::new(medians, spec.shape, spec.spread, spec.seed)
        .map_err(|e| AppError::Ungenerable(e.to_string()))?;
    Ok(Market {
        state,
        worth,
        collections: generated,
    })
}

/// Hands a collection's float to holders, reproducing what the census says about who has it.
///
/// The largest holders the census names are reproduced one for one and scaled; everybody else is
/// given an equal part of what is left. The count of holders and the share the largest of them have
/// therefore come out of the census rather than out of a fit, which is why neither is a number the
/// gate checks.
///
/// Where the collections start in the population is a function of the world's seed, so that one
/// participant holds across several collections as it does on a chain, and so that every world of
/// one seed does it the same way.
#[allow(
    clippy::too_many_arguments,
    clippy::unnecessary_wraps,
    reason = "the shape of one collection, its issuer's stock and the world it belongs to: a struct here would be built at the one call site and read at the one body"
)]
fn hand_out(
    items: &mut [Item],
    first: ItemId,
    size: u64,
    issuer_held: u64,
    holders: u64,
    of: &CollectionCensus,
    issuers: &[AccountId],
    spec: &WorldSpec,
) -> Result<(), AppError> {
    let seats = u64::from(spec.participants) - FIRST_HOLDER;
    let float = size - issuer_held;
    let base = spec.seed.wrapping_add(u64::from(of.items)) % seats;
    let stride = stride(spec.seed ^ u64::from(of.items), seats);

    // The census's own issuers are not holders: what they have is the stock this world already
    // gave the issuer, and handing it out again would put it in a float twice.
    let named: Vec<u64> = of
        .largest_holders
        .iter()
        .filter(|holder| !issuers.contains(&holder.holder))
        .take(NAMED.min(usize::try_from(holders).unwrap_or(NAMED)))
        .map(|holder| scaled(u64::from(holder.items), float, u64::from(of.float())).max(1))
        .collect();

    let mut at = issuer_held;
    let mut rank = 0u64;
    for count in &named {
        let who = seat(base, stride, rank, seats);
        for _ in 0..*count {
            if at >= size {
                break;
            }
            let index = usize::try_from(u64::from(first.0) + at).unwrap_or(usize::MAX);
            items[index].holder = who;
            at += 1;
        }
        rank += 1;
    }
    let rest = holders.saturating_sub(rank).max(1);
    let left = size.saturating_sub(at);
    let each = left / rest;
    let over = left % rest;
    for step in 0..rest {
        let who = seat(base, stride, rank + step, seats);
        let take = each + u64::from(step < over);
        for _ in 0..take {
            if at >= size {
                break;
            }
            let index = usize::try_from(u64::from(first.0) + at).unwrap_or(usize::MAX);
            items[index].holder = who;
            at += 1;
        }
    }
    Ok(())
}

/// Which participant a collection's `rank`-th holder is.
///
/// A stride rather than the next seat along, and the stride is coprime with the number of seats so
/// that the first `holders` ranks still land on `holders` distinct participants. Walking
/// consecutively would seat a collection's largest holder, its second and its third side by side,
/// and any property a world hands out by seat — which of them would sell, above all — would then
/// arrive correlated with how much they hold. Nothing measured says a market's whales are more or
/// less willing to sell than anybody else, so nothing here should make them so.
fn seat(base: u64, stride: u64, rank: u64, seats: u64) -> AgentId {
    let step = (base + rank.wrapping_mul(stride)) % seats;
    AgentId(u32::try_from(FIRST_HOLDER + step).unwrap_or(2))
}

/// A step around the seats that visits every one of them before returning.
fn stride(seed: u64, seats: u64) -> u64 {
    if seats <= 2 {
        return 1;
    }
    let mut step = 1 + seed % (seats - 1);
    while gcd(step, seats) != 1 {
        step = 1 + step % (seats - 1);
    }
    step
}

/// The greatest common divisor, by Euclid.
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The world holds more items than an identifier can name.
fn too_many() -> AppError {
    AppError::Ungenerable("this world holds more items than can be named".into())
}

/// A count at the width a world carries counts in.
fn count(of: u64) -> u32 {
    u32::try_from(of).unwrap_or(u32::MAX)
}

/// `part · of / whole`, rounded down.
fn scaled(part: u64, of: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    u64::try_from(u128::from(part) * u128::from(of) / u128::from(whole)).unwrap_or(u64::MAX)
}

/// The price a collection's own sales settled at, or what its listings ask where none sold.
///
/// A level and not a shape. What is drawn around it is scale-free, so a median that is a little low
/// — and the median of what sold is a little low, because what sold is what somebody would part
/// with — moves every worth in a collection together and moves no ratio at all.
fn median(of: &CollectionCensus) -> Amount<Cash> {
    of.prices
        .as_ref()
        .or(of.asking.as_ref())
        .map_or(Amount::new(1_000_000_000), |seen| Amount::new(seen.median))
}

/// The showcase a scenario describes, and the world it opens.
///
/// Everything a run needs and nothing it does not: the mechanism with its posted terms, the state,
/// the population, and the worths that measurement scores against and no agent may read whole.
#[derive(Debug, Clone)]
pub struct Opened {
    /// The mechanism, with the terms the scenario posted.
    pub mechanism: Showcase,
    /// The market, generated from the census.
    pub market: Market,
    /// The population, in the order the engine will give them their turns.
    pub population: Vec<Holder>,
}

/// Reads a scenario's showcase and opens the world it describes.
///
/// # Errors
///
/// Fails when the scenario is not a showcase, when its census cannot be read, or when its
/// population is too small to hold a market spread as thinly as the census says this one is.
pub fn opened(scenario: &ScenarioConfig) -> Result<Opened, AppError> {
    let MechanismConfig::Showcase {
        ref census,
        ref covering,
        only_issued,
        items,
        capital,
        seconds_per_tick,
        shape,
        spread_bps,
        charge_bps,
        lag,
        commission_bps,
        execution,
        arrears,
        distribution,
        share_bps,
        period,
    } = scenario.mechanism
    else {
        return Err(AppError::WrongFamily {
            wanted: "a showcase",
        });
    };

    // Every seat the scenario's groups ask for, plus the venue's and the issuer's. One source for
    // the size of a population, so a world and the agents in it cannot disagree about how many
    // there are.
    let seats: u64 = scenario
        .agents
        .iter()
        .map(|group| u64::from(group.count))
        .sum();
    let participants = u32::try_from(seats + FIRST_HOLDER)
        .map_err(|_| AppError::Ungenerable("more participants than can be named".into()))?;

    let spec = WorldSpec {
        items,
        participants,
        capital: Amount::new(capital),
        shape,
        spread: Amount::new(u128::from(spread_bps)),
        seed: scenario.seed,
        covering: covering.clone(),
        only_issued,
    };
    let market = generate(Path::new(census), &spec)?;

    let mechanism = Showcase {
        venue: VENUE,
        seconds_per_tick: SecondsPerTick(
            NonZeroU64::new(seconds_per_tick)
                .ok_or_else(|| AppError::Ungenerable("a tick worth no time".into()))?,
        ),
        // One rate for every collection here. The rate is a property of a collection and a sweep
        // that varies it per collection will give them their own, but a scenario that states one states one.
        charge: vec![Amount::new(u128::from(charge_bps)); market.collections.len()],
        lag,
        commission: Amount::new(u128::from(commission_bps)),
        execution: Amount::new(execution),
        arrears: match arrears {
            ArrearsConfig::Forfeit => Arrears::Forfeit,
            ArrearsConfig::ForcedSale => Arrears::ForcedSale,
        },
        distribution: match distribution {
            DistributionConfig::Even => Distribution::Even,
            DistributionConfig::ByTrades => Distribution::ByTrades,
            DistributionConfig::ByCharge => Distribution::ByCharge,
        },
        share: Amount::new(u128::from(share_bps)),
        period,
    };

    let population = populate(scenario, &market, &mechanism)?;
    Ok(Opened {
        mechanism,
        market,
        population,
    })
}

/// Gives every seat a behaviour, and every behaviour what it holds.
///
/// The venue and the issuer take the first two seats and act as nothing: the venue because the
/// mechanism refuses it, the issuer because stock that has not been handed out is not somebody
/// trading. Both are still holders in the population, because the engine gives every seat a turn
/// and one that sat out would be a hole in the acting order.
fn populate(
    scenario: &ScenarioConfig,
    market: &Market,
    mechanism: &Showcase,
) -> Result<Vec<Holder>, AppError> {
    let seats = market.state.purses.len();
    let mut mine: Vec<Vec<ItemId>> = vec![Vec::new(); seats];
    for (at, item) in market.state.items.iter().enumerate() {
        let at = u32::try_from(at).map_err(|_| AppError::Ungenerable("too many items".into()))?;
        mine[item.holder.0 as usize].push(ItemId(at));
    }

    // One table of worths behind the whole population. Everybody reads the same one from their
    // own seat, so a quarter of a million participants cost a quarter of a million handles rather
    // than a quarter of a million copies of the market.
    let worth = Arc::new(market.worth.clone());

    // The venue and the issuer: they hold what they hold and never put any of it up.
    let idle = |mine: Vec<ItemId>| Holder::new(Arc::clone(&worth), mine, 0, false);
    let mut population = vec![
        idle(std::mem::take(&mut mine[0])),
        idle(std::mem::take(&mut mine[1])),
    ];

    let charge = mechanism.charge.first().copied().unwrap_or(Amount::ZERO);
    let mut seat = usize::try_from(FIRST_HOLDER).unwrap_or(0);
    for group in &scenario.agents {
        let BehaviourConfig::Holder {
            opening_bps,
            patience_bps,
            truthful_at_bps,
            devotion_bps,
            sells_bps,
            attention,
        } = group.behaviour
        else {
            return Err(AppError::WrongFamily {
                wanted: "a population of holders",
            });
        };
        // Which of a group would sell is decided by where in the group somebody sits, so a share
        // of six per cent is six per cent whatever the group's size and whatever the seed.
        let whole = u64::try_from(WHOLE.base_units()).unwrap_or(10_000);
        let sellers = u64::from(group.count) * u64::from(sells_bps) / whole;
        for step in 0..u64::from(group.count) {
            if seat >= seats {
                return Err(AppError::Ungenerable(
                    "a scenario asks for more agents than the world seats".into(),
                ));
            }
            // Every holder its own patience, about the median the scenario states. One patience
            // shared by a population is a population that never trades and a market that appears
            // to have solved the problem the charge exists for. Everybody opens at the same
            // markup; what separates them is how fast they come down from it, and the charge is
            // what decides how much of anybody's patience survives at all.
            let seat_number = u32::try_from(seat).unwrap_or(u32::MAX);
            let patience = Holder::patience_of(scenario.seed, seat_number, patience_bps);
            let truthful = Amount::new(u128::from(truthful_at_bps));
            population.push(Holder {
                devotion: Amount::new(u128::from(devotion_bps)),
                attention,
                floor: Holder::floor(opening_bps, charge, truthful),
                retain: Holder::retain(patience, charge, truthful),
                ..Holder::new(
                    Arc::clone(&worth),
                    std::mem::take(&mut mine[seat]),
                    opening_bps,
                    step < sellers,
                )
            });
            seat += 1;
        }
    }
    Ok(population)
}

/// The builder a sweep calls once per run.
///
/// # Errors
///
/// As [`opened`].
pub fn world_builder(
    scenario: &ScenarioConfig,
) -> Result<impl Fn() -> World<Showcase> + Sync + use<>, AppError> {
    let Opened {
        mechanism,
        market,
        population,
    } = opened(scenario)?;
    Ok(move || {
        World::new(
            mechanism.clone(),
            market.state.clone(),
            population
                .iter()
                .cloned()
                .map(|who| Box::new(who) as Box<dyn Agent<Showcase>>)
                .collect(),
        )
    })
}

/// What one run of a showcase is watched for.
///
/// Two things a state cannot answer once the run is over: which items changed hands, and what was
/// paid for them. A final state says who holds what now and not who held it first, and a price
/// paid is gone from the state the moment the transfer settles — so it is watched, or it is lost.
#[derive(Debug, Clone, Default)]
pub struct Traded {
    /// One flag per item, raised the first time it changes hands.
    moved: Vec<bool>,
    /// Every price a buyout settled at, and the item it was paid for, in the order they settled.
    ///
    /// The item is kept because the spread of what a market pays is a spread *within* a
    /// collection. Pooled across a hundred and seventeen of them it would measure the distance
    /// between a two-ton collection and a five-thousand-ton one, which is a fact about which
    /// collections exist and not about how widely one thing of a kind is priced against
    /// another of the same kind.
    paid: Vec<(ItemId, u128)>,
}

impl Traded {
    /// A watcher for a market of this many items.
    #[must_use]
    pub fn watching(items: usize) -> Self {
        Traded {
            moved: vec![false; items],
            paid: Vec::new(),
        }
    }

    /// How many distinct items changed hands.
    #[must_use]
    pub fn moved(&self) -> u64 {
        self.moved.iter().filter(|at| **at).count() as u64
    }

    /// Whether one item changed hands at any point in the run.
    #[must_use]
    pub fn has_moved(&self, item: usize) -> bool {
        self.moved.get(item).copied().unwrap_or(false)
    }

    /// What was paid and for what, in the order it was paid.
    #[must_use]
    pub fn paid(&self) -> &[(ItemId, u128)] {
        &self.paid
    }
}

impl Observer<Showcase> for Traded {
    fn on_events(&mut self, _now: Tick, events: &[ShowcaseEvent]) {
        for event in events {
            match *event {
                ShowcaseEvent::Bought { item, price, .. } => {
                    if let Some(flag) = self.moved.get_mut(item.0 as usize) {
                        *flag = true;
                    }
                    self.paid.push((item, price.base_units()));
                }
                // A forced sale moves an item too, and at a price. It is a different thing from a
                // buyout and is counted apart, because a market where the venue takes half the
                // stock for unpaid charges is not a market that traded.
                ShowcaseEvent::ForceSold { item, .. } => {
                    if let Some(flag) = self.moved.get_mut(item.0 as usize) {
                        *flag = true;
                    }
                }
                _ => {}
            }
        }
    }

    fn on_rejected(&mut self, _now: Tick, _actor: AgentId, _reason: &MechError) {}

    fn on_tick_end(&mut self, _now: Tick, _state: &ShowcaseState) {}
}

/// What one run of a showcase leaves behind, in the numbers the gates are stated in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landed {
    /// Items carrying a declared price at the end.
    pub listed: u64,
    /// Distinct items that changed hands.
    pub moved: u64,
    /// What share of the float changed hands, in basis points. The first of the four
    /// figures the synthetic market is graded on.
    pub turnover_bps: i128,
    /// What share of the stock carries a price, in basis points. The second.
    pub priced_bps: i128,
    /// The upper quartile of prices paid over the lower, in thousandths. The third.
    ///
    /// One collection at a time — the middle of the collections that settled enough sales to have
    /// quartiles — because a spread pooled across collections measures which collections exist
    /// rather than how widely one thing of a kind is priced against another of its own kind.
    ///
    /// Zero where no collection settled enough, which is a fact about the run and is counted as
    /// one rather than filled in.
    pub paid_dispersion: i128,
    /// The fifth percentile of what one collection turned over, in basis points. The fourth.
    ///
    /// Over the collections holding enough of a float to have a rate at all, so it is the census's
    /// own band and not a pair of outliers.
    pub narrowest_bps: i128,
    /// And the ninety-fifth. The pair of them is the number nothing was fitted to.
    pub widest_bps: i128,
    /// How closely declared prices order things the way their worth does, in basis points.
    ///
    /// **What the charge is graded on.** One collection at a time — whether a market tells one thing from another thing
    /// of its own kind — and the middle of the collections that carry enough listings to be
    /// scored. Pooled across collections it would be scoring the price levels the census
    /// handed the world, which nobody in the run discovered and which span three orders of
    /// magnitude.
    ///
    /// Zero where no collection carried enough, or where every listing in each carried the same
    /// price, which is a run with no order in it rather than a run that disagreed.
    pub convergence_bps: i128,
    /// The median price asked over the median price paid, in thousandths, one collection at a
    /// time, and the middle of the collections that settled enough sales.
    ///
    /// The measured gap between what this market says its things are worth and what they fetch:
    /// 3.7 times in the median collection, and above one in every one of the hundred and
    /// seventeen. Per collection because that is how it was measured — pooled it would be
    /// comparing the asks of dear collections against the sales of cheap ones.
    /// It is what a population's patience is fitted against, so it is measured rather than
    /// assumed. Zero where nothing sold, which is a fact about the run and is counted as one.
    pub asked_over_paid: i128,
    /// Whether any one collection settled enough sales to have quartiles at all.
    pub priced_enough: bool,
    /// Whether enough was listed to say anything about the order of prices.
    pub ordered_enough: bool,
}

/// The names the numbers above are reported under, in their order.
pub const LANDED: [&str; 7] = [
    "turnover_bps",
    "priced_bps",
    "paid_dispersion_milli",
    "asked_over_paid_milli",
    "narrowest_collection_bps",
    "widest_collection_bps",
    "convergence_bps",
];

/// How few listings leave a collection with nothing to say about the order of its prices.
///
/// A coefficient over a handful of items is a fact about the handful. The floor is deliberately
/// low, because what it guards against is a collection with almost nothing on its showcase rather
/// than a collection whose coefficient is merely noisy: the middle is taken across collections
/// afterwards, and a middle is what turns many noisy readings into one that is not.
const ORDERED: usize = 16;

/// How small a float leaves a collection with no rate worth reading.
///
/// The census's own floor. A collection holding nine items can only report a turnover in ninths,
/// and a band assembled out of ninths would be reporting how the world was cut up rather than what
/// the collections did. A world too small to clear this floor in most of its collections is a world
/// too small to be graded on the number nothing was fitted to, which is why the size is stated with
/// the gate rather than chosen for speed.
const RESOLVES: u64 = 100;

/// How few sales in one collection leave it with nothing to say about the spread of what it paid.
///
/// The same floor the census applies, so the two numbers are cut from the same cloth.
const ENOUGH: usize = 4;

/// Reads one finished run into the numbers the gates are stated in.
///
/// The worths are consulted from here and nowhere inside the run, which is the only place a score
/// against them means anything: an agent that could read them would be an agent that knows the
/// answer, and a convergence measured through one would be the simulation marking its own work.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one paragraph per measurement the report carries, each derived from the same state in one pass"
)]
pub fn landed(state: &ShowcaseState, worth: &Worth, watched: &Traded, market: &Market) -> Landed {
    let items = state.items.len() as u64;
    let listed = state
        .items
        .iter()
        .filter(|item| item.listing.is_some())
        .count() as u64;
    let float: u64 = market
        .collections
        .iter()
        .map(|of| u64::from(of.float()))
        .sum();
    let moved = watched.moved();

    // Every number below is read one collection at a time, and the middle of the collections is
    // what a gate is stated in. Pooled, each of them would be answering a different question from
    // the one it was measured to answer: what separates two collections is their price level, that
    // level came out of the census rather than out of anybody's decision in the run, and it spans
    // three orders of magnitude. A spread pooled across collections measures which collections
    // exist; a coefficient pooled across them rewards a market for knowing that one collection is
    // dearer than another, which it was told; a gap between asking and paying pooled across them
    // compares the asks of dear collections with the sales of cheap ones. The census reports all
    // three per collection, so the model reports all three per collection.
    let mut paid_by: Vec<Vec<u128>> = vec![Vec::new(); market.collections.len()];
    for &(item, price) in watched.paid() {
        let of = state.items[item.0 as usize].collection.0 as usize;
        if let Some(here) = paid_by.get_mut(of) {
            here.push(price);
        }
    }

    let quarter =
        |sorted: &[u128], share: usize| sorted[(sorted.len() * share / 100).min(sorted.len() - 1)];
    let middle = |sorted: &[i128]| sorted.get(sorted.len() / 2).copied();

    let mut dispersions: Vec<i128> = Vec::new();
    let mut gaps: Vec<i128> = Vec::new();
    let mut scores: Vec<i128> = Vec::new();
    let mut rates: Vec<i128> = Vec::new();
    let mut declared: Vec<i128> = Vec::new();
    let mut common: Vec<i128> = Vec::new();

    for (index, of) in market.collections.iter().enumerate() {
        let from = of.first.0 as usize;
        let upto = from + of.items as usize;

        // What this collection turned over. The census's own floor on the float, because a
        // collection of nine items can only report a rate in ninths and a band assembled out of
        // ninths would be reporting how the world was cut up rather than what the collections did.
        let float = u64::from(of.float());
        if float >= RESOLVES {
            let moved = (from..upto).filter(|at| watched.has_moved(*at)).count() as u64;
            rates.push(i128::from(moved) * 10_000 / i128::from(float));
        }

        // What it asks, and how its asks are ordered against what its things are really worth.
        declared.clear();
        common.clear();
        for at in from..upto {
            let Some(item) = state.items.get(at) else {
                continue;
            };
            let Some(listing) = item.listing else {
                continue;
            };
            let Ok(index) = u32::try_from(at) else {
                continue;
            };
            let Some(worth) = worth.common(item.collection, ItemId(index)) else {
                continue;
            };
            declared.push(i128::try_from(listing.declared.base_units()).unwrap_or(i128::MAX));
            common.push(i128::try_from(worth.base_units()).unwrap_or(i128::MAX));
        }
        if declared.len() >= ORDERED
            && let Some(score) = rank_correlation(&declared, &common)
        {
            scores.push(score);
        }

        // What it paid: the spread inside it, and how far its asks stand above its sales.
        let Some(here) = paid_by.get_mut(index) else {
            continue;
        };
        if here.len() < ENOUGH {
            continue;
        }
        here.sort_unstable();
        let low = quarter(here, 25).max(1);
        dispersions.push(i128::try_from(quarter(here, 75) * 1_000 / low).unwrap_or(i128::MAX));

        if !declared.is_empty() {
            declared.sort_unstable();
            let paid_here = i128::try_from(quarter(here, 50))
                .unwrap_or(i128::MAX)
                .max(1);
            gaps.push(declared[declared.len() / 2] * 1_000 / paid_here);
        }
    }

    dispersions.sort_unstable();
    gaps.sort_unstable();
    scores.sort_unstable();
    rates.sort_unstable();

    let priced_enough = !dispersions.is_empty();
    let paid_dispersion = middle(&dispersions).unwrap_or(0);
    let asked_over_paid = middle(&gaps).unwrap_or(0);
    let convergence = middle(&scores);

    // The band is the fifth and the ninety-fifth of the collections rather than the least and the
    // greatest: two single collections, and the two most extreme ones at that, would move with the
    // sampling and not with the market. The census reads its own band the same way.
    let band = |share: usize| {
        if rates.is_empty() {
            0
        } else {
            rates[(rates.len() * share / 100).min(rates.len() - 1)]
        }
    };
    let (narrowest, widest) = (band(5), band(95));

    Landed {
        listed,
        moved,
        turnover_bps: if float == 0 {
            0
        } else {
            i128::from(moved) * 10_000 / i128::from(float)
        },
        priced_bps: if items == 0 {
            0
        } else {
            i128::from(listed) * 10_000 / i128::from(items)
        },
        paid_dispersion,
        narrowest_bps: narrowest,
        widest_bps: widest,
        convergence_bps: convergence.unwrap_or(0),
        asked_over_paid,
        priced_enough,
        ordered_enough: convergence.is_some(),
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
    use std::path::PathBuf;

    use sting_karman_data::{CollectionCensus, HolderCount, Prices, Sweep};
    use sting_karman_scenario::AgentGroup;

    use super::*;

    /// The census this workspace measured, which every world here is generated from.
    fn measured() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/karman/fixtures/ton-census.json")
    }

    fn spec_over(census: &Census, items: u32) -> WorldSpec {
        let wanted = WorldSpec { items, ..spec() };
        WorldSpec {
            participants: u32::try_from(participants_for(census, &wanted)).unwrap(),
            ..wanted
        }
    }

    fn spec() -> WorldSpec {
        WorldSpec {
            items: 20_000,
            participants: 20_000,
            capital: Amount::new(100_000_000_000),
            shape: 16_345,
            spread: Amount::new(2_000),
            seed: 42,
            covering: None,
            only_issued: false,
        }
    }

    fn account(of: u8) -> AccountId {
        format!("0:{of:064x}").parse().unwrap()
    }

    fn prices(median: u128) -> Prices {
        Prices {
            sales: 10,
            total: median * 10,
            min: median / 2,
            p25: median * 3 / 4,
            median,
            p75: median * 5 / 4,
            max: median * 2,
        }
    }

    /// A census of two collections, one concentrated and one spread thin.
    fn small() -> Census {
        Census {
            sweep: Sweep {
                network: "mainnet".into(),
                source: "test".into(),
                seqno_before: 1,
                seqno_after: 1,
                window_start: 0,
                window_end: 7_776_000,
            },
            issuers: vec![account(0xff)],
            outside_window: 0,
            collections: vec![
                CollectionCensus {
                    collection: account(1),
                    name: "Concentrated".into(),
                    content: "https://example.test/collection/one.json".into(),
                    owner: None,
                    sequence: None,
                    items: 1_000,
                    restated: 0,
                    on_sale: 50,
                    holders: 101,
                    issuer_held: 400,
                    largest_holders: vec![
                        HolderCount {
                            holder: account(0xff),
                            items: 400,
                        },
                        HolderCount {
                            holder: account(2),
                            items: 300,
                        },
                    ],
                    transfers: 100,
                    moved: 40,
                    sold: 10,
                    prices: Some(prices(2_000_000_000)),
                    asking: None,
                },
                CollectionCensus {
                    collection: account(3),
                    name: "Spread".into(),
                    content: "https://example.test/collection/two.json".into(),
                    owner: None,
                    sequence: None,
                    items: 3_000,
                    restated: 0,
                    on_sale: 100,
                    holders: 1_001,
                    issuer_held: 0,
                    largest_holders: vec![HolderCount {
                        holder: account(4),
                        items: 6,
                    }],
                    transfers: 900,
                    moved: 300,
                    sold: 90,
                    prices: Some(prices(500_000_000)),
                    asking: None,
                },
            ],
        }
    }

    /// Every proportion of the census survives the scaling, and no count does. That split is the
    /// whole of why the gate compares ratios.
    #[test]
    fn a_world_keeps_the_shape_of_what_it_was_generated_from() {
        let census = small();
        let built = from_census(&census, &spec_over(&census, 20_000)).unwrap();
        let stock: u64 = census
            .collections
            .iter()
            .map(|of| u64::from(of.items))
            .sum();
        for (generated, of) in built.collections.iter().zip(&census.collections) {
            let wanted = scaled(u64::from(of.items), 20_000, stock);
            assert_eq!(
                u64::from(generated.items),
                wanted,
                "{} was sized wrong",
                of.name
            );
            let held = scaled(
                u64::from(of.issuer_held),
                u64::from(generated.items),
                u64::from(of.items),
            );
            assert_eq!(
                u64::from(generated.issuer_held),
                held,
                "{} kept the wrong stock",
                of.name
            );
        }
    }

    /// The issuer holds exactly the stock the census says it does, and not one item of any float.
    #[test]
    fn the_issuers_stock_is_nobodys_float() {
        let built = from_census(&small(), &spec_over(&small(), 20_000)).unwrap();
        for generated in &built.collections {
            let at = generated.first.0 as usize;
            let held = built.state.items[at..at + generated.items as usize]
                .iter()
                .filter(|item| item.holder == ISSUER)
                .count();
            assert_eq!(
                u32::try_from(held).unwrap(),
                generated.issuer_held,
                "{}",
                generated.name
            );
        }
    }

    /// A float is spread over as many holders as the census counted, and no more.
    #[test]
    fn a_float_reaches_as_many_holders_as_it_did() {
        let built = from_census(&small(), &spec_over(&small(), 20_000)).unwrap();
        for generated in &built.collections {
            let at = generated.first.0 as usize;
            let mut holders: Vec<AgentId> = built.state.items[at..at + generated.items as usize]
                .iter()
                .map(|item| item.holder)
                .filter(|who| *who != ISSUER)
                .collect();
            holders.sort_unstable();
            holders.dedup();
            assert_eq!(
                u32::try_from(holders.len()).unwrap(),
                generated.holders,
                "{} reached {} holders and the census counted {}",
                generated.name,
                holders.len(),
                generated.holders
            );
        }
    }

    /// One spec and one seed make one world, every time and on every machine.
    #[test]
    fn a_world_is_its_spec_and_its_seed() {
        let census = small();
        assert_eq!(
            from_census(&census, &spec_over(&census, 20_000)).unwrap(),
            from_census(&census, &spec_over(&census, 20_000)).unwrap()
        );
        let elsewhere = WorldSpec {
            seed: 43,
            ..spec_over(&census, 20_000)
        };
        assert_ne!(
            from_census(&census, &spec_over(&census, 20_000)).unwrap(),
            from_census(&census, &elsewhere).unwrap()
        );
    }

    /// A different seed changes what things are worth and not how many there are of them: the
    /// shape comes from the census, and only the draw comes from the seed.
    #[test]
    fn a_seed_moves_the_worths_and_not_the_shape() {
        let census = small();
        let one = from_census(&census, &spec_over(&census, 20_000)).unwrap();
        let other = from_census(
            &census,
            &WorldSpec {
                seed: 9_001,
                ..spec_over(&census, 20_000)
            },
        )
        .unwrap();
        assert_eq!(one.collections, other.collections);
        let worths = |built: &Market| {
            (0..8)
                .map(|at| built.worth.common(CollectionId(0), ItemId(at)).unwrap())
                .collect::<Vec<_>>()
        };
        assert_ne!(worths(&one), worths(&other));
    }

    /// Everything opens with what it was given, and nothing has appeared from anywhere.
    #[test]
    fn a_world_opens_with_what_it_was_given() {
        let spec = spec_over(&small(), 20_000);
        let built = from_census(&small(), &spec).unwrap();
        let wanted = spec.capital.base_units() * u128::from(spec.participants);
        assert_eq!(built.state.opened_with().base_units(), wanted);
        assert!(
            built
                .state
                .items
                .iter()
                .all(|item| (item.holder.0 as usize) < built.state.purses.len())
        );
    }

    /// A population with fewer seats than a collection has holders cannot hold it, and says so
    /// where the world is asked for rather than where it is used.
    #[test]
    fn a_population_too_small_is_refused() {
        let tight = WorldSpec {
            participants: 20,
            ..spec_over(&small(), 20_000)
        };
        let refused = from_census(&small(), &tight);
        assert!(
            matches!(refused, Err(AppError::Ungenerable(_))),
            "{refused:?}"
        );
    }

    /// Every collection is priced at its own median, which is what the census measured, and the
    /// draw around it is scale-free. A world that priced two collections alike would have thrown
    /// away the one thing about them that the fit did not put there.
    #[test]
    fn every_collection_keeps_its_own_price() {
        let built = from_census(&small(), &spec_over(&small(), 40_000)).unwrap();
        let level = |at: usize| {
            let of = &built.collections[at];
            let taken = of.items.min(400);
            let sum: u128 = (of.first.0..of.first.0 + taken)
                .map(|item| {
                    built
                        .worth
                        .common(CollectionId(u32::try_from(at).unwrap()), ItemId(item))
                        .unwrap()
                        .base_units()
                })
                .sum();
            sum / u128::from(taken)
        };
        // The census prices the first at four times the second, and so does the world.
        let ratio = level(0) * 100 / level(1);
        assert!(
            (360..=440).contains(&ratio),
            "the levels came out at {ratio} per hundred"
        );
    }

    /// A scenario naming a showcase, sized so it can be run in a test.
    fn scenario() -> ScenarioConfig {
        ScenarioConfig {
            name: "showcase".into(),
            seed: 5,
            n_ticks: 40,
            mechanism: MechanismConfig::Showcase {
                census: measured().display().to_string(),
                covering: Some("nft.fragment.com/collection/".into()),
                only_issued: true,
                items: 12_000,
                capital: 40_000_000_000,
                seconds_per_tick: 21_600,
                shape: 16_345,
                spread_bps: 2_500,
                charge_bps: 0,
                lag: 4,
                commission_bps: 500,
                execution: 0,
                arrears: ArrearsConfig::Forfeit,
                distribution: DistributionConfig::ByTrades,
                share_bps: 9_000,
                period: 20,
            },
            agents: vec![AgentGroup {
                count: 2_500,
                behaviour: BehaviourConfig::Holder {
                    opening_bps: 8_000,
                    patience_bps: 4_000,
                    truthful_at_bps: 3_850,
                    devotion_bps: 0,
                    sells_bps: 4_000,
                    attention: 3,
                },
            }],
            regimes: Vec::new(),
        }
    }

    /// The population is the scenario's groups, and two seats before them: the venue, which the
    /// mechanism refuses, and the issuer, whose holdings are stock rather than anybody's float.
    #[test]
    fn a_scenario_seats_its_groups_behind_the_venue_and_the_issuer() {
        let opened = opened(&scenario()).unwrap();
        assert_eq!(opened.population.len(), 2_502);
        assert_eq!(opened.market.state.purses.len(), 2_502);
        assert!(!opened.population[0].sells, "the venue offered something");
        assert!(!opened.population[1].sells, "the issuer offered its stock");
        let held: usize = opened
            .market
            .state
            .items
            .iter()
            .filter(|it| it.holder == ISSUER)
            .count();
        assert_eq!(opened.population[1].mine.len(), held);
    }

    /// The share of a group that would sell is the share the scenario stated, whatever the size of
    /// the group and whatever the seed — it is a share and not a draw.
    #[test]
    fn the_share_that_would_sell_is_the_share_that_was_asked_for() {
        let opened = opened(&scenario()).unwrap();
        let sellers = opened.population[2..]
            .iter()
            .filter(|who| who.sells)
            .count();
        assert_eq!(sellers, 1_000, "four tenths of two and a half thousand");
    }

    /// Every holder its own patience. One markup shared by a population is a population that never
    /// trades and a market that looks as though it had solved the problem the charge exists for.
    #[test]
    fn a_population_does_not_share_one_markup() {
        let opened = opened(&scenario()).unwrap();
        let mut walks: Vec<u32> = opened.population[2..]
            .iter()
            .map(|who| who.retain)
            .collect();
        walks.sort_unstable();
        walks.dedup();
        assert!(
            walks.len() > 50,
            "two and a half thousand holders shared {} walks down",
            walks.len()
        );
        assert!(walks[0] < walks[walks.len() - 1]);
    }

    /// Every item is held by somebody who knows they hold it, so a market of a hundred thousand
    /// never costs anybody a scan of the whole of it to find their own.
    #[test]
    fn everybody_knows_what_they_hold() {
        let opened = opened(&scenario()).unwrap();
        let claimed: usize = opened.population.iter().map(|who| who.mine.len()).sum();
        assert_eq!(claimed, opened.market.state.items.len());
        for (seat, who) in opened.population.iter().enumerate() {
            for item in &who.mine {
                assert_eq!(
                    opened.market.state.items[item.0 as usize].holder.0 as usize, seat,
                    "seat {seat} claims an item somebody else holds"
                );
            }
        }
    }

    /// One scenario is one world, so a profiled run and a measured run cannot diverge.
    #[test]
    fn two_builds_of_one_scenario_open_identically() {
        let config = scenario();
        let build = world_builder(&config).unwrap();
        assert_eq!(build().state(), build().state());
    }

    /// A run of it measures the numbers the gates are stated in, and a market that never traded
    /// says so rather than filling the gap in.
    #[test]
    fn a_run_lands_on_the_numbers_the_gates_are_stated_in() {
        let config = scenario();
        let opened = opened(&config).unwrap();
        let items = opened.market.state.items.len();
        let mut world = world_builder(&config).unwrap()();
        let mut watched = Traded::watching(items);
        let mut rng = sting_karman_core::RunRng::new(config.seed, 0);
        world
            .run(config.n_ticks, &mut rng, &mut watched)
            .expect("the run aborted");

        let one = landed(
            world.state(),
            &opened.market.worth,
            &watched,
            &opened.market,
        );
        assert!(one.listed > 0, "nothing reached the showcase");
        assert!(one.priced_bps > 0 && one.priced_bps <= 10_000);
        assert!(one.moved > 0, "nothing changed hands");
        assert!(one.turnover_bps > 0);
        assert!(one.ordered_enough, "nothing to score the order of");
        assert!(one.convergence_bps.abs() <= 10_000);
        assert!(one.widest_bps >= one.narrowest_bps);
    }

    /// A world of two collections a hundredfold apart in price, each priced tightly within itself.
    fn two_levels() -> (ShowcaseState, Market, Traded) {
        const EACH: usize = 200;
        let mut items = Vec::new();
        for of in 0..2u32 {
            for _ in 0..EACH {
                items.push(Item {
                    collection: CollectionId(of),
                    holder: AgentId(2),
                    listing: None,
                });
            }
        }
        let state = ShowcaseState::open(items, vec![Purse::default(); 3]).unwrap();
        let collections = vec![
            Generated {
                name: "cheap".into(),
                first: ItemId(0),
                items: u32::try_from(EACH).expect("a test's own item count"),
                issuer_held: 0,
                holders: 1,
            },
            Generated {
                name: "dear".into(),
                first: ItemId(u32::try_from(EACH).expect("a test's own item count")),
                items: u32::try_from(EACH).expect("a test's own item count"),
                issuer_held: 0,
                holders: 1,
            },
        ];
        let worth = Worth::new(
            vec![Amount::new(1_000), Amount::new(100_000)],
            10_000,
            Amount::ZERO,
            7,
        )
        .unwrap();
        let market = Market {
            state: state.clone(),
            worth,
            collections,
        };
        let mut watched = Traded::watching(state.items.len());
        // Twelve sales in each, spanning a fifth within a collection and a hundredfold across them.
        let mut sales = Vec::new();
        for step in 0..12u128 {
            sales.push((
                ItemId(u32::try_from(step).expect("a test's own step")),
                1_000 + step * 20,
            ));
            let at = u32::try_from(EACH).expect("a test's own item count")
                + u32::try_from(step).expect("a test's own step");
            sales.push((ItemId(at), 100_000 + step * 2_000));
        }
        let events: Vec<ShowcaseEvent> = sales
            .iter()
            .map(|&(item, price)| ShowcaseEvent::Bought {
                item,
                from: AgentId(2),
                by: AgentId(2),
                price: Amount::new(price),
                charge: Amount::ZERO,
                commission: Amount::ZERO,
            })
            .collect();
        watched.on_events(Tick(1), &events);
        (state, market, watched)
    }

    /// The spread of what a market pays is a spread within a collection. Pooled across collections
    /// it would be reading the distance between a cheap collection and an expensive one — a fact
    /// about which collections exist, not about how widely one thing of a kind is priced against
    /// another of its own kind. This market spans that distance a hundredfold, and the number the
    /// gate is stated in must not notice.
    #[test]
    fn the_spread_of_what_was_paid_is_read_inside_a_collection() {
        let (state, market, watched) = two_levels();
        let one = landed(&state, &market.worth, &watched, &market);
        assert!(one.priced_enough);
        // Within either collection the upper quartile is about a fifth above the lower.
        assert!(
            (1_100..=1_300).contains(&one.paid_dispersion),
            "read {} rather than a within-collection spread",
            one.paid_dispersion
        );
        // Pooled, the same sales would have read a hundredfold apart.
        let mut all: Vec<u128> = watched.paid().iter().map(|&(_, price)| price).collect();
        all.sort_unstable();
        assert!(all[all.len() * 3 / 4] / all[all.len() / 4] > 50);
    }

    /// The band nothing is fitted to is read only where a collection holds enough of a float for a
    /// rate to mean anything — the census's own floor — and it is the fifth and the ninety-fifth of
    /// the collections rather than the least and the greatest, which would be two single
    /// collections and the two most extreme ones at that.
    #[test]
    fn the_band_skips_collections_too_small_to_have_a_rate() {
        let (state, market, watched) = two_levels();
        let mut thin = market.collections.clone();
        thin.push(Generated {
            name: "a handful".into(),
            first: ItemId(0),
            items: 9,
            issuer_held: 0,
            holders: 1,
        });
        let with_thin = Market {
            collections: thin,
            ..market.clone()
        };
        assert_eq!(
            landed(&state, &market.worth, &watched, &market).narrowest_bps,
            landed(&state, &with_thin.worth, &watched, &with_thin).narrowest_bps,
            "a collection of nine items moved the band"
        );
    }

    /// A collection's largest holder, its second and its third do not sit side by side. They would
    /// under a walk that took the next seat along, and then any property a world hands out by seat
    /// — which of them would sell, above all — would arrive correlated with how much they hold.
    #[test]
    fn the_biggest_holders_of_a_collection_are_not_neighbours() {
        let seats = 1_000u64;
        let step = stride(12_345, seats);
        assert_eq!(gcd(step, seats), 1, "a stride that misses seats");
        let mut visited: Vec<u32> = (0..seats)
            .map(|rank| seat(7, step, rank, seats).0)
            .collect();
        let ranks = visited.clone();
        visited.sort_unstable();
        visited.dedup();
        assert_eq!(
            visited.len(),
            usize::try_from(seats).expect("a test's own seat count"),
            "two ranks landed on one seat"
        );
        let adjacent = ranks
            .windows(2)
            .filter(|pair| pair[1].abs_diff(pair[0]) == 1)
            .count();
        assert!(
            adjacent < 4,
            "{adjacent} of a thousand ranks landed next to the one before it"
        );
    }

    /// The watcher counts an item once however often it changes hands, because turnover is a count
    /// of things that moved and not of the times they moved — which is the same reason the census
    /// counts distinct items rather than transfer messages.
    #[test]
    fn a_thing_that_moves_twice_moved_once() {
        let mut watched = Traded::watching(4);
        let bought = |item: u32, price: u128| ShowcaseEvent::Bought {
            item: ItemId(item),
            from: AgentId(2),
            by: AgentId(3),
            price: Amount::new(price),
            charge: Amount::ZERO,
            commission: Amount::ZERO,
        };
        watched.on_events(Tick(1), &[bought(0, 100), bought(0, 200), bought(1, 300)]);
        assert_eq!(watched.moved(), 2);
        let prices: Vec<u128> = watched.paid().iter().map(|&(_, price)| price).collect();
        assert_eq!(prices, vec![100, 200, 300]);
        assert_eq!(
            watched.paid()[2].0,
            ItemId(1),
            "a sale forgot which collection it was in"
        );
    }

    /// The world this workspace actually measured, generated at a size a sweep can carry, keeping
    /// every proportion the census has and no count.
    #[test]
    fn the_measured_market_generates() {
        let census: Census = read_json_fixture(&measured()).unwrap();
        let gifts = WorldSpec {
            covering: Some("nft.fragment.com/collection/".into()),
            only_issued: true,
            ..spec()
        };
        let sized = WorldSpec {
            participants: u32::try_from(participants_for(&census, &gifts)).unwrap(),
            ..gifts
        };
        let built = from_census(&census, &sized).expect("the census this workspace measured");
        assert!(
            built.collections.len() > 100,
            "only {} collections",
            built.collections.len()
        );
        let items: u64 = built.collections.iter().map(|of| u64::from(of.items)).sum();
        assert_eq!(u64::try_from(built.state.items.len()).unwrap(), items);

        // The share the issuer holds is what the census measured, to a point either way.
        let held: u64 = built
            .collections
            .iter()
            .map(|of| u64::from(of.issuer_held))
            .sum();
        let share = held * 1_000 / items;
        assert!(
            (440..=470).contains(&share),
            "the issuer holds {share} per thousand"
        );
    }
}
