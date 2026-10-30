//! The parameters of a scenario that a sweep is allowed to vary.
//!
//! A closed set rather than a path into the document. A query language over a scenario would
//! let a sweep reach anything, including combinations the validation was never written for, and
//! every value it produced would have to be re-validated against rules it could have stepped
//! around. These are the knobs the schema knows how to turn, and turning one produces a scenario
//! that goes through the same front door as any other.

use std::{fmt, str::FromStr};

use crate::{BehaviourConfig, InvalidScenario, MechanismConfig, ScenarioConfig, Shock};

/// One parameter a sweep can vary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Knob {
    /// The length of each run, in logical steps.
    NTicks,
    /// The pool's opening token reserve, in base units.
    TokenReserve,
    /// The pool's opening quote reserve, in base units.
    QuoteReserve,
    /// How many agents one group contributes, by the group's position in the scenario.
    GroupCount(usize),
    /// A threshold other than the one a credit market is listed at, at WAD.
    ///
    /// Turning it states a counterfactual rather than correcting a value: unturned, a scenario
    /// takes the market's own listing and has no threshold of its own to read.
    Lltv,
    /// How far one regime's shock takes the price, in basis points, by the regime's position.
    ShockDepth(usize),
    /// How many ticks pass between one liquidator's attempts, by the group's position.
    ///
    /// One of the two axes a search over a credit market runs along, and the one that decides
    /// whether a liquidation ladder keeps pace with a falling price.
    LiquidatorEvery(usize),
    /// What a showcase charges a listing, per year, in basis points.
    ///
    /// The main curve of this family: what a sweep over it draws is whether a rate exists at which
    /// a market starts telling rare from ordinary, and where it is.
    Charge,
    /// How many ticks a raise or a withdrawal waits before it binds.
    ///
    /// Swept to find the least value at which a holder cannot see a buyout coming and defeat it,
    /// which is a property of the mechanism rather than a preference.
    Lag,
    /// What the venue takes from a buyout, in basis points.
    Commission,
    /// How much of the pool is shared out, in basis points.
    Share,
    /// How far apart the things inside one collection really are, at `1 << 16`.
    ///
    /// One of the three the world is fitted on, and the one the measured dispersion of paid prices
    /// solves for in closed form.
    Shape,
    /// How far one participant's opinion of a thing reaches, in basis points.
    Spread,
    /// How many items a world holds.
    Items,
    /// The share of one group that would sell at any price at all, in basis points.
    ///
    /// The third of the three the world is fitted on, and what decides how much of a market is
    /// reachable at all.
    SellsShare(usize),
    /// How much of a position one liquidator takes per attempt, in basis points.
    ///
    /// The other axis, and the one that decides which boundary binds. A liquidator that can take
    /// the whole of a position cannot take any of it until the position is insolvent, because
    /// anything less than the whole would repay more than is owed; one that takes a part can act
    /// from the moment the position is liquidatable, and never reaches the last unit of
    /// collateral — which is the only thing that realises a loss against the supply side.
    LiquidatorCapacity(usize),
    /// The least collateral a guarantee layer will ever ask of anybody, in basis points.
    ///
    /// The main curve of that family, and the one a claim about a ring is stated against: what a
    /// sweep over it draws is how much leverage a system can extend against a person before
    /// standing behind oneself starts to pay.
    Floor,
    /// The annual premium a guarantee layer charges on drawn principal, in basis points.
    Premium,
    /// What share of that premium the venue keeps, in basis points.
    ///
    /// It moves more than its size suggests: a premium's guarantor share paid by a ring never
    /// leaves the ring, so this is the only part of it that costs an attacker anything.
    VenueShare,
    /// How many seats a ring of confederates occupies.
    Ring,
}

/// A knob asked of a scenario whose mechanism has no such thing.
fn wrong_mechanism(knob: Knob) -> InvalidScenario {
    InvalidScenario::new(format!(
        "`{knob}` is not a knob this scenario's mechanism has"
    ))
}

impl Knob {
    /// The value this knob currently holds.
    #[must_use]
    pub fn read(self, scenario: &ScenarioConfig) -> Option<u128> {
        match self {
            Knob::NTicks => Some(u128::from(scenario.n_ticks)),
            Knob::TokenReserve => match scenario.mechanism {
                MechanismConfig::Cpamm { token_reserve, .. } => Some(token_reserve),
                _ => None,
            },
            Knob::QuoteReserve => match scenario.mechanism {
                MechanismConfig::Cpamm { quote_reserve, .. } => Some(quote_reserve),
                _ => None,
            },
            Knob::Lltv => match scenario.mechanism {
                MechanismConfig::Lending { lltv, .. } => lltv,
                _ => None,
            },
            Knob::Charge => match scenario.mechanism {
                MechanismConfig::Showcase { charge_bps, .. } => Some(u128::from(charge_bps)),
                _ => None,
            },
            Knob::Lag => match scenario.mechanism {
                MechanismConfig::Showcase { lag, .. } => Some(u128::from(lag)),
                _ => None,
            },
            Knob::Commission => match scenario.mechanism {
                MechanismConfig::Showcase { commission_bps, .. } => {
                    Some(u128::from(commission_bps))
                }
                _ => None,
            },
            Knob::Share => match scenario.mechanism {
                MechanismConfig::Showcase { share_bps, .. } => Some(u128::from(share_bps)),
                _ => None,
            },
            Knob::Shape => match scenario.mechanism {
                MechanismConfig::Showcase { shape, .. } => Some(u128::from(shape)),
                _ => None,
            },
            Knob::Spread => match scenario.mechanism {
                MechanismConfig::Showcase { spread_bps, .. } => Some(u128::from(spread_bps)),
                _ => None,
            },
            Knob::Items => match scenario.mechanism {
                MechanismConfig::Showcase { items, .. } => Some(u128::from(items)),
                _ => None,
            },
            Knob::SellsShare(group) => match scenario.agents.get(group)?.behaviour {
                BehaviourConfig::Holder { sells_bps, .. } => Some(u128::from(sells_bps)),
                _ => None,
            },
            Knob::GroupCount(group) => scenario
                .agents
                .get(group)
                .map(|group| u128::from(group.count)),
            Knob::ShockDepth(at) => match scenario.regimes.get(at)?.shock {
                Shock::Steady => None,
                Shock::Slide { to_bps, .. } | Shock::Gap { to_bps, .. } => Some(u128::from(to_bps)),
            },
            Knob::LiquidatorCapacity(group) => match scenario.agents.get(group)?.behaviour {
                BehaviourConfig::Liquidator { capacity_bps, .. } => Some(u128::from(capacity_bps)),
                _ => None,
            },
            Knob::LiquidatorEvery(group) => match scenario.agents.get(group)?.behaviour {
                BehaviourConfig::Liquidator { every, .. } => Some(u128::from(every)),
                _ => None,
            },
            Knob::Floor => match scenario.mechanism {
                MechanismConfig::Vouch { floor_bps, .. } => Some(u128::from(floor_bps)),
                _ => None,
            },
            Knob::Premium => match scenario.mechanism {
                MechanismConfig::Vouch { premium_bps, .. } => Some(u128::from(premium_bps)),
                _ => None,
            },
            Knob::VenueShare => match scenario.mechanism {
                MechanismConfig::Vouch {
                    venue_share_bps, ..
                } => Some(u128::from(venue_share_bps)),
                _ => None,
            },
            Knob::Ring => match scenario.mechanism {
                MechanismConfig::Vouch { ring, .. } => Some(u128::from(ring)),
                _ => None,
            },
        }
    }

    /// The scenario this one would be with the knob turned to `value`.
    ///
    /// The result is validated like any other scenario, so a value that produces a world nobody
    /// could run is refused here rather than at the first tick of the point it belongs to.
    ///
    /// # Errors
    ///
    /// Fails when the value does not fit the field, when the group does not exist, or when the
    /// scenario it produces would not have been accepted had it been written by hand.
    pub fn turned_to(
        self,
        scenario: &ScenarioConfig,
        value: u128,
    ) -> Result<ScenarioConfig, InvalidScenario> {
        let mut turned = scenario.clone();
        self.turn(&mut turned, value)?;
        turned.validate()?;
        Ok(turned)
    }

    /// Which field this knob names, and the value written into it.
    ///
    /// A table rather than a decision: what the caller reads is [`Knob::turned_to`], and what
    /// a reader consults here is one row, with a knob already in hand.
    #[allow(
        clippy::too_many_lines,
        reason = "one row per knob: the length is the enum's, and splitting the table hides that it is complete"
    )]
    fn turn(self, turned: &mut ScenarioConfig, value: u128) -> Result<(), InvalidScenario> {
        match self {
            Knob::NTicks => {
                turned.n_ticks = u64::try_from(value)
                    .map_err(|_| InvalidScenario::new("n_ticks does not fit its field"))?;
            }
            Knob::TokenReserve => match &mut turned.mechanism {
                MechanismConfig::Cpamm { token_reserve, .. } => *token_reserve = value,
                _ => return Err(wrong_mechanism(self)),
            },
            Knob::QuoteReserve => match &mut turned.mechanism {
                MechanismConfig::Cpamm { quote_reserve, .. } => *quote_reserve = value,
                _ => return Err(wrong_mechanism(self)),
            },
            Knob::Lltv => match &mut turned.mechanism {
                MechanismConfig::Lending { lltv, .. } => *lltv = Some(value),
                _ => return Err(wrong_mechanism(self)),
            },
            Knob::Charge | Knob::Commission | Knob::Share | Knob::Shape | Knob::Spread => {
                let narrow = u32::try_from(value).map_err(|_| {
                    InvalidScenario::new(format!("`{self}` does not fit its field"))
                })?;
                match &mut turned.mechanism {
                    MechanismConfig::Showcase {
                        charge_bps,
                        commission_bps,
                        share_bps,
                        shape,
                        spread_bps,
                        ..
                    } => match self {
                        Knob::Charge => *charge_bps = narrow,
                        Knob::Commission => *commission_bps = narrow,
                        Knob::Share => *share_bps = narrow,
                        Knob::Shape => *shape = narrow,
                        _ => *spread_bps = narrow,
                    },
                    _ => return Err(wrong_mechanism(self)),
                }
            }
            Knob::Items => {
                let narrow = u32::try_from(value)
                    .map_err(|_| InvalidScenario::new("a stock does not fit its field"))?;
                match &mut turned.mechanism {
                    MechanismConfig::Showcase { items, .. } => *items = narrow,
                    _ => return Err(wrong_mechanism(self)),
                }
            }
            Knob::Lag => {
                let ticks = u64::try_from(value)
                    .map_err(|_| InvalidScenario::new("a lag does not fit its field"))?;
                match &mut turned.mechanism {
                    MechanismConfig::Showcase { lag, .. } => *lag = ticks,
                    _ => return Err(wrong_mechanism(self)),
                }
            }
            Knob::SellsShare(group) => {
                let share = u32::try_from(value).map_err(|_| {
                    InvalidScenario::new("a share of a group does not fit its field")
                })?;
                let group = turned.agents.get_mut(group).ok_or_else(|| {
                    InvalidScenario::new(format!("the scenario has no agent group {self}"))
                })?;
                match &mut group.behaviour {
                    BehaviourConfig::Holder { sells_bps, .. } => *sells_bps = share,
                    _ => return Err(wrong_mechanism(self)),
                }
            }
            Knob::ShockDepth(at) => {
                let depth = u32::try_from(value)
                    .map_err(|_| InvalidScenario::new("a shock depth does not fit its field"))?;
                let regime = turned.regimes.get_mut(at).ok_or_else(|| {
                    InvalidScenario::new(format!("the scenario has no regime {at}"))
                })?;
                match &mut regime.shock {
                    Shock::Slide { to_bps, .. } | Shock::Gap { to_bps, .. } => *to_bps = depth,
                    Shock::Steady => {
                        return Err(InvalidScenario::new(format!(
                            "regime `{}` has no shock to deepen",
                            regime.name
                        )));
                    }
                }
            }
            Knob::LiquidatorEvery(group) => {
                let ticks = u64::try_from(value).map_err(|_| {
                    InvalidScenario::new("a liquidator's reaction time does not fit its field")
                })?;
                let group = turned.agents.get_mut(group).ok_or_else(|| {
                    InvalidScenario::new(format!("the scenario has no agent group {self}"))
                })?;
                match &mut group.behaviour {
                    BehaviourConfig::Liquidator { every, .. } => *every = ticks,
                    _ => return Err(wrong_mechanism(self)),
                }
            }
            Knob::LiquidatorCapacity(group) => {
                let share = u32::try_from(value).map_err(|_| {
                    InvalidScenario::new("a liquidator's capacity does not fit its field")
                })?;
                let group = turned.agents.get_mut(group).ok_or_else(|| {
                    InvalidScenario::new(format!("the scenario has no agent group {self}"))
                })?;
                match &mut group.behaviour {
                    BehaviourConfig::Liquidator { capacity_bps, .. } => *capacity_bps = share,
                    _ => return Err(wrong_mechanism(self)),
                }
            }
            Knob::Floor | Knob::Premium | Knob::VenueShare | Knob::Ring => {
                let narrow = u32::try_from(value).map_err(|_| {
                    InvalidScenario::new(format!("`{self}` does not fit its field"))
                })?;
                match &mut turned.mechanism {
                    MechanismConfig::Vouch {
                        floor_bps,
                        premium_bps,
                        venue_share_bps,
                        ring,
                        ..
                    } => match self {
                        Knob::Floor => *floor_bps = narrow,
                        Knob::Premium => *premium_bps = narrow,
                        Knob::VenueShare => *venue_share_bps = narrow,
                        _ => *ring = narrow,
                    },
                    _ => return Err(wrong_mechanism(self)),
                }
            }
            Knob::GroupCount(group) => {
                let count = u32::try_from(value)
                    .map_err(|_| InvalidScenario::new("a group count does not fit its field"))?;
                let group = turned.agents.get_mut(group).ok_or_else(|| {
                    InvalidScenario::new(format!("the scenario has no agent group {self}"))
                })?;
                group.count = count;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Knob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Knob::NTicks => f.write_str("n-ticks"),
            Knob::TokenReserve => f.write_str("token-reserve"),
            Knob::QuoteReserve => f.write_str("quote-reserve"),
            Knob::GroupCount(group) => write!(f, "group-count:{group}"),
            Knob::Lltv => f.write_str("lltv"),
            Knob::ShockDepth(at) => write!(f, "shock-depth:{at}"),
            Knob::LiquidatorEvery(group) => write!(f, "liquidator-every:{group}"),
            Knob::LiquidatorCapacity(group) => write!(f, "liquidator-capacity:{group}"),
            Knob::Charge => f.write_str("charge"),
            Knob::Lag => f.write_str("lag"),
            Knob::Commission => f.write_str("commission"),
            Knob::Share => f.write_str("share"),
            Knob::Shape => f.write_str("shape"),
            Knob::Spread => f.write_str("spread"),
            Knob::Items => f.write_str("items"),
            Knob::SellsShare(group) => write!(f, "sells-share:{group}"),
            Knob::Floor => f.write_str("floor"),
            Knob::Premium => f.write_str("premium"),
            Knob::VenueShare => f.write_str("venue-share"),
            Knob::Ring => f.write_str("ring"),
        }
    }
}

/// The name given was not a knob this schema knows how to turn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{given}` is not a knob: expected n-ticks, token-reserve, quote-reserve, lltv, charge, lag, \
     commission, share, shape, spread, items, group-count:N, shock-depth:N, sells-share:N, \
     liquidator-every:N or liquidator-capacity:N"
)]
pub struct UnknownKnob {
    /// What was asked for.
    pub given: String,
}

impl FromStr for Knob {
    type Err = UnknownKnob;

    fn from_str(name: &str) -> Result<Self, UnknownKnob> {
        let unknown = || UnknownKnob {
            given: name.to_owned(),
        };
        match name {
            "n-ticks" => Ok(Knob::NTicks),
            "token-reserve" => Ok(Knob::TokenReserve),
            "quote-reserve" => Ok(Knob::QuoteReserve),
            "lltv" => Ok(Knob::Lltv),
            "charge" => Ok(Knob::Charge),
            "lag" => Ok(Knob::Lag),
            "commission" => Ok(Knob::Commission),
            "share" => Ok(Knob::Share),
            "shape" => Ok(Knob::Shape),
            "spread" => Ok(Knob::Spread),
            "items" => Ok(Knob::Items),
            "floor" => Ok(Knob::Floor),
            "premium" => Ok(Knob::Premium),
            "venue-share" => Ok(Knob::VenueShare),
            "ring" => Ok(Knob::Ring),
            _ => {
                for (prefix, build) in [
                    ("group-count:", Knob::GroupCount as fn(usize) -> Knob),
                    ("shock-depth:", Knob::ShockDepth as fn(usize) -> Knob),
                    (
                        "liquidator-every:",
                        Knob::LiquidatorEvery as fn(usize) -> Knob,
                    ),
                    (
                        "liquidator-capacity:",
                        Knob::LiquidatorCapacity as fn(usize) -> Knob,
                    ),
                    ("sells-share:", Knob::SellsShare as fn(usize) -> Knob),
                ] {
                    if let Some(at) = name.strip_prefix(prefix) {
                        return at.parse().map(build).map_err(|_| unknown());
                    }
                }
                Err(unknown())
            }
        }
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
    use crate::{AgentGroup, BehaviourConfig};

    fn scenario() -> ScenarioConfig {
        ScenarioConfig {
            name: "knobs".into(),
            seed: 1,
            n_ticks: 100,
            mechanism: MechanismConfig::Cpamm {
                token_reserve: 1_000_000,
                quote_reserve: 2_000_000,
            },
            regimes: Vec::new(),
            agents: vec![AgentGroup {
                count: 3,
                behaviour: BehaviourConfig::NoiseTrader { trade_cap: 1_000 },
            }],
        }
    }

    #[test]
    fn every_knob_reads_what_it_names() {
        let s = scenario();
        assert_eq!(Knob::NTicks.read(&s), Some(100));
        assert_eq!(Knob::TokenReserve.read(&s), Some(1_000_000));
        assert_eq!(Knob::QuoteReserve.read(&s), Some(2_000_000));
        assert_eq!(Knob::GroupCount(0).read(&s), Some(3));
        assert_eq!(Knob::GroupCount(1).read(&s), None);
    }

    #[test]
    fn turning_a_knob_changes_that_knob_and_nothing_else() {
        let before = scenario();
        let after = Knob::TokenReserve.turned_to(&before, 7_000_000).unwrap();
        assert_eq!(Knob::TokenReserve.read(&after), Some(7_000_000));
        assert_eq!(
            Knob::QuoteReserve.read(&after),
            Knob::QuoteReserve.read(&before)
        );
        assert_eq!(Knob::NTicks.read(&after), Knob::NTicks.read(&before));
        assert_eq!(after.name, before.name);
    }

    /// A knob goes through the same front door as a hand-written scenario, so a value that
    /// produces an unrunnable world is refused where it was asked for.
    #[test]
    fn a_value_that_produces_an_unrunnable_world_is_refused() {
        let s = scenario();
        assert!(
            Knob::NTicks.turned_to(&s, 0).is_err(),
            "a run of no ticks was accepted"
        );
        assert!(
            Knob::TokenReserve.turned_to(&s, 0).is_err(),
            "an empty pool was accepted"
        );
        assert!(
            Knob::GroupCount(0).turned_to(&s, 0).is_err(),
            "an empty population was accepted"
        );
    }

    #[test]
    fn a_value_too_wide_for_its_field_is_refused() {
        let s = scenario();
        assert!(Knob::NTicks.turned_to(&s, u128::MAX).is_err());
        assert!(Knob::GroupCount(0).turned_to(&s, u128::MAX).is_err());
    }

    #[test]
    fn a_group_the_scenario_does_not_have_is_refused() {
        assert!(Knob::GroupCount(9).turned_to(&scenario(), 5).is_err());
    }

    #[test]
    fn a_knob_round_trips_through_the_name_it_is_asked_for_by() {
        for knob in [
            Knob::NTicks,
            Knob::TokenReserve,
            Knob::QuoteReserve,
            Knob::GroupCount(2),
        ] {
            assert_eq!(knob.to_string().parse::<Knob>().unwrap(), knob);
        }
    }

    #[test]
    fn a_name_that_is_not_a_knob_says_what_the_knobs_are() {
        let error = "reserves".parse::<Knob>().unwrap_err();
        assert!(
            error.to_string().contains("token-reserve"),
            "unclear error: {error}"
        );
        assert!("group-count:".parse::<Knob>().is_err());
        assert!("group-count:x".parse::<Knob>().is_err());
    }
}
