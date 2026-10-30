//! The scenario contract: everything that defines a run, as data.
//!
//! A scenario file plus a seed is the whole input of a simulation — the SDK, the CLI and a
//! stored finding all speak this one schema. The crate depends only on `sting-karman-types`: a
//! scenario *describes* mechanisms and agents by name and parameters, and the composition root
//! (the CLI or the SDK) resolves the description into concrete values. That resolution living
//! at the edge is what lets a new mechanism crate appear without this crate changing shape.
//!
//! Validation is fail-fast at the boundary: a scenario that refers to nothing, or asks for an
//! empty world, is an error before the first tick, never a silently empty result.
//!
//! Quantities travel as plain base units under names that say what they measure. The unit types
//! of a mechanism are that mechanism's vocabulary, and a scenario names mechanisms rather than
//! importing them — the composition root is where a number becomes a quantity of something.

mod knob;

pub use knob::{Knob, UnknownKnob};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A scenario that cannot be run. Raised at load time, with the field that broke.
#[derive(Debug, Error)]
#[error("invalid scenario: {reason}")]
pub struct InvalidScenario {
    /// What broke, naming the field, in words a user can act on without reading the code.
    pub reason: String,
}

impl InvalidScenario {
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        InvalidScenario {
            reason: reason.into(),
        }
    }
}

/// The full description of a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioConfig {
    /// Names the run wherever it is reported, and names the artifacts it writes.
    pub name: String,
    /// The scenario's own seed. A seed given on the command line overrides it, and whichever
    /// was used is printed with the result.
    pub seed: u64,
    /// Length of one run, in logical steps. What a step is worth in seconds is not the
    /// engine's business.
    pub n_ticks: u64,
    /// Which mechanism the world runs, and the state it opens from.
    pub mechanism: MechanismConfig,
    /// The population, described as homogeneous groups rather than one agent at a time.
    pub agents: Vec<AgentGroup>,
    /// The futures a sweep divides its runs between. Empty means one future, exactly as written.
    #[serde(default)]
    pub regimes: Vec<Regime>,
}

/// Which mechanism the world runs, with its initial state parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MechanismConfig {
    /// A constant-product pool, opened at these reserves.
    Cpamm {
        /// Opening token reserve, in that asset's base units.
        #[serde(with = "sting_karman_types::u128_str")]
        token_reserve: u128,
        /// Opening quote reserve, in that asset's base units.
        #[serde(with = "sting_karman_types::u128_str")]
        quote_reserve: u128,
    },
    /// A showcase, opened from a census of a market that is held rather than traded.
    ///
    /// What the market *is* — how many collections, how big each is, what share of it sits at an
    /// issuer, how thinly the rest is held and what its sales settle at — is not stated here. It
    /// comes out of the census, because it is a measurement rather than a choice this scenario
    /// gets to make. Three numbers below are fitted and say so; everything else is either the
    /// mechanism's own posted terms or a size.
    Showcase {
        /// The census this world is generated from, by path. A run reads a fixture and never a
        /// chain.
        census: String,
        /// Which class of the census to cover, by the path its descriptions are served under.
        ///
        /// A census covers gifts, names, numbers and domains at once and they behave nothing
        /// alike: one keeps half its stock at an issuer and the others keep none. Absent means all
        /// of them, which is a world of no market in particular.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        covering: Option<String>,
        /// Whether to leave out collections an issuer did not deploy, or holds no stock of.
        #[serde(default)]
        only_issued: bool,
        /// How many items the world holds, across every collection it covers.
        items: u32,
        /// What each participant opens with, in base units.
        #[serde(with = "sting_karman_types::u128_str")]
        capital: u128,
        /// What one tick is worth in seconds, which is what turns an annual charge into one.
        seconds_per_tick: u64,
        /// How far apart the things inside one collection really are, at `1 << 16`. **Fitted.**
        shape: u32,
        /// How far one participant's opinion of a thing reaches, in basis points. **Fitted.**
        spread_bps: u32,
        /// What the showcase charges a listing, per year, in basis points.
        charge_bps: u32,
        /// How many ticks a raise or a withdrawal waits before it binds.
        lag: u64,
        /// What the venue takes from a buyout, in basis points.
        commission_bps: u32,
        /// What one action costs whoever takes it, in base units. It leaves the system.
        #[serde(with = "sting_karman_types::u128_str")]
        execution: u128,
        /// What happens when the charge cannot be paid.
        arrears: ArrearsConfig,
        /// How the pool is shared out.
        distribution: DistributionConfig,
        /// How much of the pool is shared out, in basis points; the rest is the venue's.
        share_bps: u32,
        /// How many ticks pass between one sharing out and the next. Zero never shares out.
        period: u64,
    },
    /// A guarantee layer, opened over a population that has money and no history.
    ///
    /// Nothing here is fitted, and the reason is worth stating rather than hiding: no market of
    /// this shape has been measured, so there is nothing to fit to. Every number below is a term a
    /// venue would post, and the experiment is a sweep over the one that decides everything —
    /// `floor_bps`, the least collateral the system will ever ask of anybody.
    Vouch {
        /// How many participants stand behind one another honestly.
        honest: u32,
        /// How many of those honest participants never repay.
        ///
        /// Not attackers: ordinary bad borrowers, backed by people who lose real money over them.
        /// They are what makes a guarantor's stake risky at all, and without them standing behind
        /// somebody would be a fee with no exposure attached.
        bad: u32,
        /// How many seats a ring of confederates occupies.
        ring: u32,
        /// What each participant opens with, in base units. The same for everybody: a ring that
        /// had to be richer than an honest participant would be a different question.
        #[serde(with = "sting_karman_types::u128_str")]
        capital: u128,
        /// What the fund holds to lend out, in base units.
        #[serde(with = "sting_karman_types::u128_str")]
        fund: u128,
        /// How many people one participant is willing to stand behind.
        circle: u32,
        /// What one tick is worth in seconds, which is what turns an annual premium into one.
        seconds_per_tick: u64,
        /// What share of a draw somebody with no history must have standing behind it, in basis
        /// points.
        opening_bps: u32,
        /// What each settled loan takes off that share, in basis points.
        step_bps: u32,
        /// The least that share may ever fall to, in basis points. **The swept parameter.**
        floor_bps: u32,
        /// What a guarantee counts for.
        backing: BackingConfig,
        /// The annual premium on drawn principal, in basis points.
        premium_bps: u32,
        /// What share of that premium the venue keeps, in basis points.
        venue_share_bps: u32,
        /// How many ticks a guarantee must stand before it backs anything.
        seasoning: u64,
        /// How many ticks a withdrawal of a guarantee waits before it binds.
        release_lag: u64,
        /// What one action costs whoever takes it, in base units. It leaves the system.
        #[serde(with = "sting_karman_types::u128_str")]
        execution: u128,
        /// What each participant locks behind each guarantee it writes, in base units.
        #[serde(with = "sting_karman_types::u128_str")]
        stake: u128,
        /// What each keeps free and will not stake, in base units.
        #[serde(with = "sting_karman_types::u128_str")]
        reserve: u128,
        /// How many ticks a loan runs for.
        term: u64,
        /// How many ticks before a due date a borrower starts trying to repay.
        notice: u64,
        /// One tick in this many is one a participant acts on.
        cadence: u64,
    },
    /// A credit market, opened from a market pulled out of a chain.
    ///
    /// What the market *is* — the pair, the threshold it is listed at, what its oracle answered
    /// — is not stated here. It comes out of the fixture, because it is a fact about that market
    /// and not a choice this scenario gets to make. A scenario that could restate it could quote
    /// one market at another market's price, and every run under it would look entirely ordinary.
    Lending {
        /// The pulled market this opens from, by path. A run reads a fixture and never a node.
        market: String,
        /// What the market charges its borrowers, per second, at WAD.
        ///
        /// A choice, not a fact: the curve the reference charges by is not modelled here
        /// (karman/docs/ARITHMETIC.md §6), so a run states the rate it is driven at.
        #[serde(with = "sting_karman_types::u128_str")]
        borrow_rate: u128,
        /// A threshold other than the one the market is listed at, at WAD.
        ///
        /// Absent means the listing, which is the ordinary case. Stating it asks what this
        /// market would have done had it been listed somewhere else, and a report that runs one
        /// says which of the two it ran.
        #[serde(default, with = "sting_karman_types::option_u128_str")]
        lltv: Option<u128>,
        /// Who is in the market when a run opens.
        #[serde(default)]
        population: Population,
        /// What one tick is worth in seconds. Interest compounds per second, so a market that
        /// did not know this would charge for the wrong amount of time.
        seconds_per_tick: u64,
    },
}

/// What a guarantee counts for when credit is drawn against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackingConfig {
    /// The money locked behind it, whatever its writer owes elsewhere.
    Stake,
    /// The money locked behind it, scaled down where its writer has staked more than it is worth
    /// net of its own debts. Credit staked again backs nothing.
    NetWorth,
}

/// What a showcase does when a charge cannot be paid out of a purse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrearsConfig {
    /// Take what the purse holds, leave the item off the showcase with its holder, write off the
    /// rest.
    Forfeit,
    /// Sell the item to the venue at its declared price, the debt coming out of that price.
    ForcedSale,
}

/// How a showcase shares its pool out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistributionConfig {
    /// Equally between every participant but the venue.
    Even,
    /// In proportion to the buyouts each was a party to.
    ByTrades,
    /// In proportion to the charge each paid.
    ByCharge,
}

/// Who holds the positions a run opens with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Population {
    /// The market's own holders, as the pull found them.
    #[default]
    Pulled,
    /// The market's size held by one lender, with the borrowers this scenario states.
    ///
    /// A pull names the holders it can reach and puts everything else into one residual holder
    /// with no collateral, because the reference keeps no market-wide total of collateral to
    /// reconcile a remainder against and inventing one would be inventing the answer. Where that
    /// residual holds most of the debt, what happens to the borrowers of that market is not
    /// something a run can observe — so a study states borrowers of its own, at distances from
    /// the threshold it chose, and keeps the market only for its terms and its size.
    Stated,
}

/// One future the runs of a sweep are divided between, and how much of the sweep it takes.
///
/// A mixture rather than a grid, and the weights are the assumption a reader is entitled to see
/// first: what comes out of a sweep is a distribution conditional on these, and calling it a
/// probability without naming them would be reporting the assumption as the finding.
///
/// Which runs belong to which regime is decided by slicing rather than by drawing. A sweep of
/// ten thousand runs at a fifth weight contains exactly two thousand of that regime, where a
/// draw would contain about that many — and a mixture that is itself noisy adds variance to
/// every number downstream for nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Regime {
    /// Names it wherever a report groups by it.
    pub name: String,
    /// Its share of the sweep's runs, in basis points.
    pub weight_bps: u32,
    /// What the price does in this future.
    pub shock: Shock,
    /// How far the depth of that shock may sit from the one written down, in basis points,
    /// drawn from the run's own stream. Zero is a regime with no uncertainty in it.
    #[serde(default)]
    pub depth_spread_bps: u32,
}

/// What the price does over a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shock {
    /// Nothing. The price it opened at is the price it closes at.
    Steady,
    /// It walks down to `to_bps` of where it opened, arriving at `over_ticks`.
    Slide {
        /// Where it ends, as a share of where it began, in basis points.
        to_bps: u32,
        /// How long the walk takes.
        over_ticks: u64,
    },
    /// It is one price until `at_tick` and another from then on.
    Gap {
        /// Where it lands, as a share of where it began, in basis points.
        to_bps: u32,
        /// The tick it lands on.
        at_tick: u64,
    },
}

/// A homogeneous group of agents: one behaviour, one parameter set, `count` copies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentGroup {
    /// How many copies of the behaviour join the population.
    pub count: u32,
    /// The behaviour every member of the group runs, with its parameters.
    pub behaviour: BehaviourConfig,
}

/// Which behaviour, with its parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BehaviourConfig {
    /// Uninformed flow: a fair coin between the two sides, and a uniform size below the cap.
    NoiseTrader {
        /// Largest single trade, in the base units of whichever side the trade takes.
        trade_cap: u64,
    },
    /// Informed flow: trades the pool toward a reference price the pool cannot see.
    Arbitrageur {
        /// The reference price as a ratio, quote side, in base units.
        #[serde(with = "sting_karman_types::u128_str")]
        reference_quote: u128,
        /// The reference price as a ratio, token side, in base units.
        #[serde(with = "sting_karman_types::u128_str")]
        reference_token: u128,
        /// How far the reference may move in one tick, in basis points.
        volatility_bps: u32,
        /// The deviation that must open before a round trip pays, in basis points.
        entry_band_bps: u32,
        /// Share of the incoming reserve committed when it acts, in basis points.
        aggression_bps: u32,
    },
    /// Somebody in a guarantee layer, standing behind others and borrowing against those who
    /// stand behind it.
    ///
    /// It carries no parameters of its own. Everything one of these needs — what it stakes, how
    /// long it borrows for, how often it acts — is a term of the layer, stated once in the
    /// mechanism rather than once per group, and who it stands behind and when it stops paying are
    /// decided by which group it is in. A group of these that could restate the terms could put a
    /// ring on different terms from the market it is attacking.
    Member,
    /// Somebody who owns things on a showcase and decides what to say they are worth.
    ///
    /// A collector is this behaviour with a devotion and a patience, and not a second kind: two
    /// kinds differing only in their constants drift apart the first time either is touched.
    Holder {
        /// What every seller declares above its own valuation when it first puts something up, in
        /// basis points. **Fitted.** One number for the market, because what a market opens at is
        /// a convention of the market; what separates one seller from another is how long they
        /// hold to it.
        opening_bps: u32,
        /// What share of its standing markup a seller keeps each time it looks at a listing of its
        /// own that nobody has taken, in basis points, before the charge is taken off it.
        ///
        /// **Fitted**, against the gap between what this market asks and what it pays. A
        /// population that never came down could not produce that gap and a turnover at the same
        /// time: asking three and seven tenths of what the market pays and selling a third of the
        /// standing inventory in a quarter are the same sellers at different points of one walk.
        patience_bps: u32,
        /// The charge at which it declares its valuation exactly, in basis points per year.
        ///
        /// A measurement rather than a fit: by Weyl and Zhang's Theorem 1 it is the observed
        /// chance of selling, which is the measured turnover. Below it a holder declares above its
        /// valuation and above it declares below, which is what gives the curve of a sweep an
        /// interior best rate at all.
        truthful_at_bps: u32,
        /// How much more than everybody this one wants what it holds, in basis points. Zero is an
        /// ordinary holder; a collector is somebody for whom the same thing is simply worth more.
        devotion_bps: u32,
        /// The share of the group that would sell at any price at all, in basis points.
        ///
        /// **Fitted**, against the share of a market's items that carry a price. The rest hold
        /// what they have and never put it up, which is most of the market this models.
        sells_bps: u32,
        /// How many items one of them looks at in a tick.
        ///
        /// Not a fit. It says what a tick is worth, and how many ticks make a quarter is this
        /// scenario's to say; a population looking at twice as much half as often is the same
        /// population.
        attention: u32,
    },
    /// A leveraged position, opened once and put back together after part of it is taken.
    Borrower {
        /// What it pledges, in the collateral's base units.
        #[serde(with = "sting_karman_types::u128_str")]
        collateral: u128,
        /// How far it opens from the value of that collateral, in basis points.
        ///
        /// A distance and not an amount. What a given debt means depends on the price the
        /// collateral is quoted at, and a study of a threshold is a study of distances from it —
        /// so the scenario says where the position stands and the market's own price says what
        /// that is worth. Written as an amount, the same document would mean a different
        /// experiment against a different market.
        ltv_bps: u32,
        /// The share of what it owes it pays down after collateral is taken, in basis points.
        repair_bps: u32,
    },
    /// Whoever takes an uncovered position over, with the two parameters that decide whether
    /// anybody got there in time.
    Liquidator {
        /// How many ticks pass between one attempt and the next.
        every: u64,
        /// The most of a position it takes in one go, in basis points of that position.
        capacity_bps: u32,
        /// How many holders it watches.
        watching: u32,
    },
    /// The supply side: lends while the market is paying for it and leaves while it is not.
    Depositor {
        /// What it moves in one go, in the loan asset's base units.
        #[serde(with = "sting_karman_types::u128_str")]
        step: u128,
        /// The utilisation it is content with, in basis points.
        target_utilisation_bps: u32,
    },
    /// Technical flow: follows the pool's own move over a window.
    MomentumTrader {
        /// How many ticks one window lasts.
        lookback_ticks: u32,
        /// The move that must have happened before it follows, in basis points.
        entry_band_bps: u32,
        /// Share of the incoming reserve committed when it acts, in basis points.
        aggression_bps: u32,
    },
}

/// One whole, in basis points. A share of the whole is the largest any of these parameters can
/// honestly be, and a band at or beyond it is a trader that never trades.
const BPS: u32 = 10_000;

/// One whole at the scale a threshold and a rate are quoted against.
const WAD: u128 = 1_000_000_000_000_000_000;

/// The bounds every behaviour quoted in basis points shares.
///
/// A band at the whole would make the lower edge of the deviation test zero, which is a
/// comparison against nothing; an aggression above the whole would commit a reserve the pool
/// does not hold.
fn check_band(
    behaviour: &str,
    volatility_bps: u32,
    entry_band_bps: u32,
    aggression_bps: u32,
) -> Result<(), InvalidScenario> {
    if entry_band_bps >= BPS {
        return Err(InvalidScenario::new(format!(
            "{behaviour} entry_band_bps is a whole or more: no deviation could ever reach it"
        )));
    }
    if aggression_bps == 0 || aggression_bps > BPS {
        return Err(InvalidScenario::new(format!(
            "{behaviour} aggression_bps is outside 1..={BPS}: a trade is a share of a reserve"
        )));
    }
    if volatility_bps >= BPS {
        return Err(InvalidScenario::new(format!(
            "{behaviour} volatility_bps is a whole or more: a step could erase the reference"
        )));
    }
    Ok(())
}

impl ScenarioConfig {
    /// The futures a sweep is divided between, and what makes a division a division.
    ///
    /// Weights that do not come to a whole are refused rather than normalised. A mixture is the
    /// assumption a reader is told about first, and one quietly rescaled underneath them is not
    /// the one they were told.
    fn check_regimes(&self) -> Result<(), InvalidScenario> {
        if self.regimes.is_empty() {
            return Ok(());
        }
        let mut total = 0u32;
        for regime in &self.regimes {
            if regime.name.is_empty() {
                return Err(InvalidScenario::new(
                    "a regime is unnamed: a report groups by it",
                ));
            }
            if regime.weight_bps == 0 {
                return Err(InvalidScenario::new(format!(
                    "regime `{}` takes no share of the sweep: it would never run",
                    regime.name
                )));
            }
            if let Shock::Slide { to_bps, over_ticks } = regime.shock {
                if over_ticks == 0 {
                    return Err(InvalidScenario::new(format!(
                        "regime `{}` slides over no ticks: that is a gap and should say so",
                        regime.name
                    )));
                }
                if to_bps > BPS {
                    return Err(InvalidScenario::new(format!(
                        "regime `{}` slides upward: a shock this searches over is a fall",
                        regime.name
                    )));
                }
            }
            if let Shock::Gap { to_bps, .. } = regime.shock
                && to_bps > BPS
            {
                return Err(InvalidScenario::new(format!(
                    "regime `{}` gaps upward: a shock this searches over is a fall",
                    regime.name
                )));
            }
            total = total
                .checked_add(regime.weight_bps)
                .ok_or_else(|| InvalidScenario::new("the regime weights do not fit a whole"))?;
        }
        if total != BPS {
            return Err(InvalidScenario::new(format!(
                "the regime weights come to {total} of {BPS}: a mixture is not rescaled to fit"
            )));
        }
        Ok(())
    }

    /// The checks a scenario must pass before a world is built from it.
    ///
    /// # Errors
    /// [`InvalidScenario`], naming the first thing that would have made the run meaningless: an
    /// empty run, a population that never acts, a mixture whose weights are not a whole.
    pub fn validate(&self) -> Result<(), InvalidScenario> {
        self.check_run()?;
        self.check_agents()?;
        self.check_mechanism()?;
        self.check_regimes()?;
        Ok(())
    }

    /// The run itself: a length, and somebody to act during it.
    fn check_run(&self) -> Result<(), InvalidScenario> {
        if self.n_ticks == 0 {
            return Err(InvalidScenario::new(
                "n_ticks is zero: the run would be empty",
            ));
        }
        if self.agents.iter().map(|g| u64::from(g.count)).sum::<u64>() == 0 {
            return Err(InvalidScenario::new("no agents: nothing would ever act"));
        }
        Ok(())
    }

    /// Every agent group describes a behaviour that can act.
    fn check_agents(&self) -> Result<(), InvalidScenario> {
        for group in &self.agents {
            check_behaviour(group)?;
        }
        Ok(())
    }

    /// The mechanism describes a market that can open.
    fn check_mechanism(&self) -> Result<(), InvalidScenario> {
        match &self.mechanism {
            MechanismConfig::Cpamm {
                token_reserve,
                quote_reserve,
            } => {
                if *token_reserve == 0 || *quote_reserve == 0 {
                    return Err(InvalidScenario::new("cpamm reserves must be positive"));
                }
            }
            MechanismConfig::Showcase { .. } => check_showcase(&self.mechanism)?,
            MechanismConfig::Vouch { .. } => check_vouch(&self.mechanism)?,
            MechanismConfig::Lending {
                market,
                lltv,
                seconds_per_tick,
                ..
            } => {
                if market.is_empty() {
                    return Err(InvalidScenario::new(
                        "lending market is unnamed: a run opens from a pulled market",
                    ));
                }
                if lltv.is_some_and(|stated| stated == 0 || stated >= WAD) {
                    return Err(InvalidScenario::new(
                        "lending lltv is outside 1..WAD: a threshold at the collateral's full \
                         value leaves nothing to pay a liquidator out of",
                    ));
                }
                if *seconds_per_tick == 0 {
                    return Err(InvalidScenario::new(
                        "lending seconds_per_tick is zero: no interest could ever accrue",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A showcase: a census to open from, and terms a venue could actually post.
fn check_showcase(mechanism: &MechanismConfig) -> Result<(), InvalidScenario> {
    let MechanismConfig::Showcase {
        census,
        items,
        seconds_per_tick,
        spread_bps,
        charge_bps,
        commission_bps,
        share_bps,
        capital,
        ..
    } = mechanism
    else {
        return Ok(());
    };
    if census.is_empty() {
        return Err(InvalidScenario::new(
            "showcase census is unnamed: a world opens from a measured market",
        ));
    }
    if *items == 0 {
        return Err(InvalidScenario::new(
            "showcase items is zero: the world would hold nothing",
        ));
    }
    if *seconds_per_tick == 0 {
        return Err(InvalidScenario::new(
            "showcase seconds_per_tick is zero: no charge could ever accrue",
        ));
    }
    if *capital == 0 {
        return Err(InvalidScenario::new(
            "showcase capital is zero: nobody could buy anything",
        ));
    }
    if *spread_bps > BPS {
        return Err(InvalidScenario::new(
            "showcase spread_bps is more than a whole: an opinion that wide would \
                 take a valuation through zero",
        ));
    }
    if *commission_bps > BPS || *share_bps > BPS {
        return Err(InvalidScenario::new(
            "a showcase share is more than a whole: no fee takes more than the \
                 price and no distribution shares more than the pool",
        ));
    }
    if *charge_bps > 100 * BPS {
        return Err(InvalidScenario::new(
            "showcase charge_bps is more than a hundredfold a year: past the cap on \
                 what a listing owes, every rate above this is the same rate",
        ));
    }
    Ok(())
}

/// A guarantee layer: a population that can stand behind itself, and terms that leave a stake at risk.
fn check_vouch(mechanism: &MechanismConfig) -> Result<(), InvalidScenario> {
    let MechanismConfig::Vouch {
        honest,
        ring,
        bad,
        capital,
        fund,
        circle,
        seconds_per_tick,
        opening_bps,
        floor_bps,
        venue_share_bps,
        premium_bps,
        stake,
        term,
        cadence,
        ..
    } = mechanism
    else {
        return Ok(());
    };
    if honest.saturating_add(*ring) == 0 {
        return Err(InvalidScenario::new(
            "a vouch layer with nobody in it: no guarantee could ever be written",
        ));
    }
    if bad > honest {
        return Err(InvalidScenario::new(
            "vouch bad is more than honest: a bad borrower is one of the honest                          population, not an addition to it",
        ));
    }
    if *capital == 0 || *stake == 0 {
        return Err(InvalidScenario::new(
            "vouch capital or stake is zero: nobody could stand behind anybody",
        ));
    }
    if *fund == 0 {
        return Err(InvalidScenario::new(
            "vouch fund is zero: there would be no credit to draw",
        ));
    }
    if *circle == 0 {
        return Err(InvalidScenario::new(
            "vouch circle is zero: nobody is willing to stand behind anybody",
        ));
    }
    if *seconds_per_tick == 0 || *term == 0 || *cadence == 0 {
        return Err(InvalidScenario::new(
            "vouch seconds_per_tick, term and cadence must all be positive: a tick                          worth no time, a loan due when it is drawn and a participant that never                          acts are each a run that measures nothing",
        ));
    }
    if *floor_bps == 0 {
        return Err(InvalidScenario::new(
            "vouch floor_bps is zero: credit standing on nothing is not credit                          standing on somebody",
        ));
    }
    if *floor_bps > *opening_bps {
        return Err(InvalidScenario::new(
            "vouch floor_bps is above opening_bps: history would make credit dearer                          rather than cheaper, and nothing in the layer would ever be earned",
        ));
    }
    if *opening_bps > 100 * BPS {
        return Err(InvalidScenario::new(
            "vouch opening_bps asks a hundredfold of what is drawn: nobody would                          ever draw, and the run would measure an empty market",
        ));
    }
    if *venue_share_bps > BPS {
        return Err(InvalidScenario::new(
            "vouch venue_share_bps is more than a whole: the venue cannot keep more                          of a premium than was paid",
        ));
    }
    if *premium_bps > 100 * BPS {
        return Err(InvalidScenario::new(
            "vouch premium_bps is more than a hundredfold a year: no borrower would                          reach a second loan",
        ));
    }
    Ok(())
}

/// One agent group: a behaviour that could act, with parameters that let it.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per behaviour: the length is the enum's, and a behaviour missing from it is what this must not allow"
)]
fn check_behaviour(group: &AgentGroup) -> Result<(), InvalidScenario> {
    match group.behaviour {
        BehaviourConfig::NoiseTrader { trade_cap } => {
            if trade_cap == 0 {
                return Err(InvalidScenario::new(
                    "noise_trader trade_cap is zero: the agent could never act",
                ));
            }
        }
        BehaviourConfig::Arbitrageur {
            reference_quote,
            reference_token,
            volatility_bps,
            entry_band_bps,
            aggression_bps,
        } => {
            if reference_quote == 0 || reference_token == 0 {
                return Err(InvalidScenario::new(
                    "arbitrageur reference price has a zero side: it prices nothing",
                ));
            }
            check_band(
                "arbitrageur",
                volatility_bps,
                entry_band_bps,
                aggression_bps,
            )?;
        }
        BehaviourConfig::Borrower {
            collateral,
            ltv_bps,
            repair_bps,
        } => {
            if collateral == 0 {
                return Err(InvalidScenario::new(
                    "borrower collateral is zero: there is nothing to borrow against",
                ));
            }
            if ltv_bps == 0 || ltv_bps >= 10_000 {
                return Err(InvalidScenario::new(
                    "borrower ltv_bps is outside 1..10000: a position is not leveraged \
                         at nothing and cannot open owing more than it pledged",
                ));
            }
            if repair_bps > BPS {
                return Err(InvalidScenario::new(
                    "borrower repair_bps is more than a whole: it cannot repay more debt \
                         than it has",
                ));
            }
        }
        BehaviourConfig::Liquidator {
            every,
            capacity_bps,
            watching,
        } => {
            if every == 0 {
                return Err(InvalidScenario::new(
                    "liquidator every is zero: a reaction time of no ticks is not one",
                ));
            }
            if capacity_bps == 0 || capacity_bps > BPS {
                return Err(InvalidScenario::new(
                    "liquidator capacity_bps is outside 1..=10000: a seizure is a share \
                         of a position",
                ));
            }
            if watching == 0 {
                return Err(InvalidScenario::new(
                    "liquidator watching is zero: it looks at nobody",
                ));
            }
        }
        BehaviourConfig::Depositor {
            step,
            target_utilisation_bps,
        } => {
            if step == 0 {
                return Err(InvalidScenario::new(
                    "depositor step is zero: it could never move anything",
                ));
            }
            if target_utilisation_bps >= BPS {
                return Err(InvalidScenario::new(
                    "depositor target_utilisation_bps is a whole or more: no market ever \
                         reaches it, so it would only ever lend",
                ));
            }
        }
        BehaviourConfig::MomentumTrader {
            lookback_ticks,
            entry_band_bps,
            aggression_bps,
        } => {
            if lookback_ticks == 0 {
                return Err(InvalidScenario::new(
                    "momentum_trader lookback_ticks is zero: no window ever closes",
                ));
            }
            check_band("momentum_trader", 0, entry_band_bps, aggression_bps)?;
        }
        // A member carries no parameters of its own: everything it needs is a term of
        // the layer, checked where the layer is checked.
        BehaviourConfig::Member => {}
        BehaviourConfig::Holder {
            opening_bps,
            patience_bps,
            truthful_at_bps,
            devotion_bps,
            sells_bps,
            attention,
        } => {
            if sells_bps > BPS {
                return Err(InvalidScenario::new(
                    "holder sells_bps is more than a whole: a share of a group cannot be \
                         more than the group",
                ));
            }
            if truthful_at_bps == 0 {
                return Err(InvalidScenario::new(
                    "holder truthful_at_bps is zero: no charge would ever bring a \
                         declared price to what the thing is worth, and the curve a sweep \
                         draws would have no interior best rate on it",
                ));
            }
            if attention == 0 {
                return Err(InvalidScenario::new(
                    "holder attention is zero: it would look at nothing and never act",
                ));
            }
            if patience_bps > BPS {
                return Err(InvalidScenario::new(
                    "holder patience_bps is more than a whole: a seller cannot take more                              off its markup than the markup it is standing on",
                ));
            }
            if opening_bps > 100 * BPS || devotion_bps > 100 * BPS {
                return Err(InvalidScenario::new(
                    "a holder wants a hundredfold of what a thing is worth: that is not a \
                         market participant, and a population of them measures nothing",
                ));
            }
        }
    }
    Ok(())
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

    fn scenario() -> ScenarioConfig {
        ScenarioConfig {
            name: "smoke".into(),
            seed: 42,
            n_ticks: 100,
            mechanism: MechanismConfig::Cpamm {
                token_reserve: 1_000_000,
                quote_reserve: 1_000_000,
            },
            regimes: Vec::new(),
            agents: vec![AgentGroup {
                count: 3,
                behaviour: BehaviourConfig::NoiseTrader { trade_cap: 1_000 },
            }],
        }
    }

    #[test]
    fn valid_scenario_round_trips_through_json() {
        let s = scenario();
        s.validate().unwrap();
        let json = serde_json::to_string(&s).unwrap();
        let back: ScenarioConfig = serde_json::from_str(&json).unwrap();
        back.validate().unwrap();
    }

    #[test]
    fn a_run_of_zero_ticks_is_rejected() {
        let mut s = scenario();
        s.n_ticks = 0;
        assert!(s.validate().is_err());
    }

    #[test]
    fn an_invalid_scenario_names_the_field_that_broke() {
        let mut s = scenario();
        s.agents[0].behaviour = BehaviourConfig::NoiseTrader { trade_cap: 0 };
        assert!(s.validate().unwrap_err().to_string().contains("trade_cap"));
    }

    #[test]
    fn money_crosses_the_wire_as_a_string() {
        let json = serde_json::to_value(scenario()).unwrap();
        assert_eq!(
            json["mechanism"]["token_reserve"],
            serde_json::json!("1000000")
        );
    }

    fn with(behaviour: BehaviourConfig) -> ScenarioConfig {
        let mut s = scenario();
        s.agents[0].behaviour = behaviour;
        s
    }

    fn arbitrageur() -> BehaviourConfig {
        BehaviourConfig::Arbitrageur {
            reference_quote: 1_000_000,
            reference_token: 1_000_000,
            volatility_bps: 50,
            entry_band_bps: 30,
            aggression_bps: 100,
        }
    }

    fn momentum() -> BehaviourConfig {
        BehaviourConfig::MomentumTrader {
            lookback_ticks: 8,
            entry_band_bps: 40,
            aggression_bps: 100,
        }
    }

    #[test]
    fn the_informed_and_technical_behaviours_validate() {
        with(arbitrageur()).validate().unwrap();
        with(momentum()).validate().unwrap();
    }

    #[test]
    fn a_reference_price_with_a_zero_side_prices_nothing() {
        let BehaviourConfig::Arbitrageur {
            reference_token, ..
        } = arbitrageur()
        else {
            unreachable!("built just above")
        };
        let _ = reference_token;
        assert!(
            with(BehaviourConfig::Arbitrageur {
                reference_quote: 0,
                reference_token: 1_000_000,
                volatility_bps: 50,
                entry_band_bps: 30,
                aggression_bps: 100,
            })
            .validate()
            .is_err()
        );
    }

    /// A band at the whole would make the lower edge of every deviation test zero, so a trader
    /// carrying one is not cautious, it is broken.
    #[test]
    fn a_band_of_a_whole_is_rejected() {
        assert!(
            with(BehaviourConfig::MomentumTrader {
                lookback_ticks: 8,
                entry_band_bps: 10_000,
                aggression_bps: 100,
            })
            .validate()
            .is_err()
        );
    }

    #[test]
    fn a_share_larger_than_the_reserve_is_rejected() {
        assert!(
            with(BehaviourConfig::MomentumTrader {
                lookback_ticks: 8,
                entry_band_bps: 40,
                aggression_bps: 10_001,
            })
            .validate()
            .is_err()
        );
        assert!(
            with(BehaviourConfig::MomentumTrader {
                lookback_ticks: 8,
                entry_band_bps: 40,
                aggression_bps: 0,
            })
            .validate()
            .is_err()
        );
    }

    #[test]
    fn a_window_that_never_closes_is_rejected() {
        assert!(
            with(BehaviourConfig::MomentumTrader {
                lookback_ticks: 0,
                entry_band_bps: 40,
                aggression_bps: 100,
            })
            .validate()
            .is_err()
        );
    }

    #[test]
    fn an_unknown_mechanism_kind_fails_to_load() {
        let json =
            r#"{"name":"x","seed":1,"n_ticks":1,"mechanism":{"kind":"orderbook"},"agents":[]}"#;
        assert!(serde_json::from_str::<ScenarioConfig>(json).is_err());
    }

    #[test]
    fn empty_world_is_rejected() {
        let mut s = scenario();
        s.agents.clear();
        assert!(s.validate().is_err());
    }

    #[test]
    fn zero_trade_cap_is_rejected() {
        let mut s = scenario();
        s.agents[0].behaviour = BehaviourConfig::NoiseTrader { trade_cap: 0 };
        assert!(s.validate().is_err());
    }

    /// A pool whose product needs more than 128 bits is a legal scenario: the mechanism compares
    /// products at full width (karman/docs/ARITHMETIC.md §2), so there is nothing here to reject.
    #[test]
    fn reserves_at_full_width_are_accepted() {
        let mut s = scenario();
        s.mechanism = MechanismConfig::Cpamm {
            token_reserve: u128::MAX,
            quote_reserve: u128::MAX,
        };
        s.validate().unwrap();
    }

    #[test]
    fn an_empty_pool_is_rejected() {
        let mut s = scenario();
        s.mechanism = MechanismConfig::Cpamm {
            token_reserve: 0,
            quote_reserve: 1,
        };
        assert!(s.validate().is_err());
    }
}
