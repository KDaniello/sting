//! A market pulled from a chain, as the state a run starts from.
//!
//! The two halves that meet here have to: the boundary crate knows how to read a node's answers
//! and nothing about credit, and the mechanism knows about credit and never reads anything. A
//! composition root is the only place where both are in scope, which is where the translation
//! belongs.

use std::{num::NonZeroU64, path::Path};

use sting_karman_agents::{Borrower, Depositor, Liquidator};
use sting_karman_core::{Agent, MechError, Observer, PathSource, RunRng, SourceError, World};
use sting_karman_data::{PulledMarket, Terms as Listed, read_json_fixture};
use sting_karman_mech_lending::{
    BorrowRate, Collateral, FixedRate, Lending, LendingAction, LendingEvent, LendingState, Loan,
    Market, MarketParams, Position, Price,
};
use sting_karman_scenario::{
    AgentGroup, BehaviourConfig, MechanismConfig, Population, Regime, ScenarioConfig, Shock,
};
use sting_karman_types::{AgentId, Amount, MarketId, Rounding, SecondsPerTick, Tick, Wide};

use crate::AppError;

/// The market a pulled fixture opens under. One market, because a pull is of one market.
pub const PULLED: MarketId = MarketId(0);

/// A pulled market as a world's opening state, and who its holders became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    /// The state, with every share the market issued held by somebody.
    pub state: LendingState,
    /// What each named holder became, in the order the fixture lists them.
    pub holders: Vec<AgentId>,
    /// The agent standing for everybody the pull did not name.
    pub unnamed: AgentId,
}

/// Rebuilds a runnable opening state from a pulled market.
///
/// The pull's `last_update` is a wall clock and a run's is a tick, so the market opens at tick
/// zero: the moment it was pulled is the moment the run begins, and every elapsed second after
/// that is the run's own. Nothing else about the fixture is reinterpreted.
///
/// The holders a pull could not name are one agent holding exactly what the totals do not
/// account for. Without it the shares held would not add up to the shares issued, and the state
/// would be one the mechanism declares impossible — a real market read in part is still a whole
/// market, and this is what says so.
///
/// # Errors
///
/// Fails when the fixture does not reconcile — the named holders holding more than the market
/// issued — or when it carries a quantity wider than the market can be opened with.
pub fn opening(pulled: &PulledMarket) -> Result<Opening, AppError> {
    let unnamed_holder = pulled.unaccounted("unnamed")?;
    let market = Market {
        total_supply_assets: Amount::new(pulled.total_supply_assets),
        total_supply_shares: Amount::new(pulled.total_supply_shares),
        total_borrow_assets: Amount::new(pulled.total_borrow_assets),
        total_borrow_shares: Amount::new(pulled.total_borrow_shares),
        last_update: Tick(0),
        fee: Amount::new(pulled.fee),
    };
    let mut state = LendingState::new().with_market(PULLED, market)?;

    let mut holders = Vec::with_capacity(pulled.holders.len());
    for (at, held) in pulled.holders.iter().enumerate() {
        let who = AgentId(u32::try_from(at).map_err(|_| AppError::TooManyHolders)?);
        holders.push(who);
        state = state.with_position(
            PULLED,
            who,
            Position {
                supply_shares: Amount::new(held.supply_shares),
                borrow_shares: Amount::new(held.borrow_shares),
                collateral: Amount::new(held.collateral),
            },
        )?;
    }

    let unnamed = AgentId(u32::try_from(holders.len()).map_err(|_| AppError::TooManyHolders)?);
    state = state.with_position(
        PULLED,
        unnamed,
        Position {
            supply_shares: Amount::new(unnamed_holder.supply_shares),
            borrow_shares: Amount::new(unnamed_holder.borrow_shares),
            collateral: Amount::new(unnamed_holder.collateral),
        },
    )?;
    Ok(Opening {
        state,
        holders,
        unnamed,
    })
}

/// A market with the fixture's terms and its size, and one lender holding all of it.
///
/// Every share the market issued is still held by somebody — the invariant a state is built to
/// keep — but by one holder who never acts, so that what a run exhibits is the borrowers the
/// scenario placed and not the ones a pull could not see.
///
/// # Errors
///
/// Fails when the fixture carries a quantity the market cannot be opened with.
fn lent(pulled: &PulledMarket) -> Result<LendingState, AppError> {
    let market = Market {
        total_supply_assets: Amount::new(pulled.total_supply_assets),
        total_supply_shares: Amount::new(pulled.total_supply_shares),
        total_borrow_assets: Amount::ZERO,
        total_borrow_shares: Amount::ZERO,
        last_update: Tick(0),
        fee: Amount::new(pulled.fee),
    };
    Ok(LendingState::new()
        .with_market(PULLED, market)?
        .with_position(
            PULLED,
            AgentId(0),
            Position {
                supply_shares: Amount::new(pulled.total_supply_shares),
                borrow_shares: Amount::ZERO,
                collateral: Amount::ZERO,
            },
        )?)
}

/// The price a run is quoted, as the shock its future describes.
///
/// A path rather than a number, because that is what a shock is: the mechanism reads it through
/// the same port a recording would arrive on, and cannot tell the two apart.
#[derive(Debug, Clone, Copy)]
pub struct Shocked {
    opening: Wide,
    shock: Shock,
}

impl Shocked {
    /// A price that opens at `opening` and does what `shock` says.
    #[must_use]
    pub fn new(opening: Wide, shock: Shock) -> Self {
        Shocked { opening, shock }
    }

    /// What it is quoted at `now`, in basis points of where it opened.
    fn depth_bps(self, now: Tick) -> u128 {
        match self.shock {
            Shock::Steady => 10_000,
            Shock::Gap { to_bps, at_tick } => {
                if now.0 < at_tick {
                    10_000
                } else {
                    u128::from(to_bps)
                }
            }
            Shock::Slide { to_bps, over_ticks } => {
                let elapsed = u128::from(now.0.min(over_ticks));
                let fallen = (10_000 - u128::from(to_bps)) * elapsed / u128::from(over_ticks);
                10_000 - fallen
            }
        }
    }
}

impl PathSource<Price<Collateral, Loan>> for Shocked {
    fn value_at(&self, now: Tick) -> Result<Price<Collateral, Loan>, SourceError> {
        self.opening
            .mul_div(self.depth_bps(now), 10_000, Rounding::Down)
            .map(Price::wider)
            .map_err(|e| SourceError::new(format!("a shocked price has no answer: {e}")))
    }
}

/// The listing a study needs, refused rather than replaced when a pull did not record it.
///
/// # Errors
///
/// Fails when the market was pulled without its terms.
pub(crate) fn listing(pulled: &PulledMarket) -> Result<&Listed, AppError> {
    pulled.terms.as_ref().ok_or_else(|| {
        AppError::Unopenable(MechError::new(
            "this market was pulled without its listing: a study cannot supply the threshold and              the price itself, because a scenario that could would be free to quote one market at              another's",
        ))
    })
}

/// The fixture a lending scenario names, as it was pulled.
///
/// What a report states about the market it ran on comes from here rather than from the
/// scenario, for the same reason the run itself does.
///
/// # Errors
///
/// Fails when the fixture cannot be read, or the scenario is not a lending one.
pub(crate) fn listed(scenario: &ScenarioConfig) -> Result<PulledMarket, AppError> {
    let MechanismConfig::Lending { market, .. } = &scenario.mechanism else {
        return Err(AppError::Unopenable(MechError::new(
            "this scenario is not a lending one",
        )));
    };
    Ok(read_json_fixture(Path::new(market))?)
}

/// The market a lending scenario opens from, read out of the fixture it names.
///
/// # Errors
///
/// Fails when the fixture cannot be read, does not reconcile, or the scenario is not a lending
/// one.
pub(crate) fn opening_market(scenario: &ScenarioConfig) -> Result<Market, AppError> {
    opened(scenario)?
        .0
        .market(PULLED)
        .ok_or(AppError::Ungenerable(
            "the pulled market opened without the market it was pulled for".to_string(),
        ))
}

/// The world a lending scenario builds, and the shock it is quoted under.
///
/// # Errors
///
/// Fails when the fixture cannot be read or the scenario is not a lending one.
pub(crate) fn world_builder(
    scenario: &ScenarioConfig,
    shock: Shock,
) -> Result<impl Fn() -> World<Lending<Shocked, FixedRate>> + Sync + '_, AppError> {
    let (state, terms) = opened(scenario)?;
    let quoted = Shocked::new(terms.price, shock);
    let owed = debts(Price::wider(terms.price), &scenario.agents)?;
    let mechanism = Lending::new(SecondsPerTick(
        NonZeroU64::new(terms.seconds_per_tick)
            .ok_or_else(|| AppError::Unopenable(MechError::new("a tick worth no seconds")))?,
    ))
    .with_market(
        PULLED,
        MarketParams {
            lltv: Amount::new(terms.lltv),
            fee_recipient: AgentId(0),
        },
        quoted,
        FixedRate(Amount::new(terms.borrow_rate)),
    )?;
    // Every holder the pull named, and the one standing for those it did not.
    let inherited = state.holdings().count();
    Ok(move || {
        World::new(
            mechanism.clone(),
            state.clone(),
            lending_agents(inherited, &scenario.agents, &owed),
        )
    })
}

/// What a lending run opens under: the market's own terms, and the scenario's choices about how
/// to drive it.
///
/// The two are kept apart deliberately. Everything a chain decided arrives from the fixture, so
/// a scenario cannot describe one market with another's numbers; everything this workspace does
/// not model — the rate curve, the length of a tick — is the scenario's to state.
struct Terms {
    lltv: u128,
    price: Wide,
    borrow_rate: u128,
    seconds_per_tick: u64,
}

fn opened(scenario: &ScenarioConfig) -> Result<(LendingState, Terms), AppError> {
    let MechanismConfig::Lending {
        market,
        borrow_rate,
        lltv,
        population,
        seconds_per_tick,
    } = &scenario.mechanism
    else {
        return Err(AppError::Unopenable(MechError::new(
            "this scenario is not a lending one",
        )));
    };
    let pulled: PulledMarket = read_json_fixture(Path::new(market))?;
    let listed = listing(&pulled)?.clone();
    let state = match population {
        Population::Pulled => opening(&pulled)?.state,
        Population::Stated => lent(&pulled)?,
    };
    Ok((
        state,
        Terms {
            lltv: lltv.unwrap_or(listed.lltv),
            price: listed.price,
            borrow_rate: *borrow_rate,
            seconds_per_tick: *seconds_per_tick,
        },
    ))
}

/// A holder the pulled market already had.
///
/// Their positions are in the state and their behaviour is not, because what they would have
/// done is not something this workspace knows. They act first in the order, so the identifiers a
/// pulled market handed out keep meaning what they meant — a scenario's own agents come after
/// them and inherit nothing.
struct Inherited;

impl<O, R> Agent<Lending<O, R>> for Inherited
where
    O: PathSource<Price<Collateral, Loan>>,
    R: BorrowRate,
{
    fn decide(
        &mut self,
        _me: AgentId,
        _state: &LendingState,
        _now: Tick,
        _rng: &mut RunRng,
    ) -> Option<LendingAction> {
        None
    }
}

/// The population a lending scenario describes, behind the holders the market already had.
/// What each stated borrower group opens owing, at the price its collateral is quoted at.
///
/// One entry per group, so a group that borrows nothing still has a place and the two lists stay
/// aligned. The valuation happens here rather than in the agent because an agent that could read
/// the venue's oracle would not be a borrower (karman/docs/ARCHITECTURE.md §9.11).
///
/// # Errors
///
/// Fails when a stated position has no representable size at this market's price.
fn debts(price: Price<Collateral, Loan>, groups: &[AgentGroup]) -> Result<Vec<u128>, AppError> {
    groups
        .iter()
        .map(|group| match group.behaviour {
            BehaviourConfig::Borrower {
                collateral,
                ltv_bps,
                ..
            } => price
                .quote(Amount::<Collateral>::new(collateral), Rounding::Down)
                .and_then(|worth| {
                    worth.mul_div(
                        Amount::<Loan>::new(u128::from(ltv_bps)),
                        Amount::<Loan>::new(10_000),
                        Rounding::Down,
                    )
                })
                .map(Amount::base_units)
                .map_err(|e| {
                    AppError::Unopenable(MechError::new(format!(
                        "a stated position has no size at this market's price: {e}"
                    )))
                }),
            _ => Ok(0),
        })
        .collect()
}

fn lending_agents(
    inherited: usize,
    groups: &[AgentGroup],
    owed: &[u128],
) -> Vec<Box<dyn Agent<Lending<Shocked, FixedRate>>>> {
    let mut population: Vec<Box<dyn Agent<Lending<Shocked, FixedRate>>>> = (0..inherited)
        .map(|_| Box::new(Inherited) as Box<dyn Agent<_>>)
        .collect();
    for (group, debt) in groups.iter().zip(owed) {
        for _ in 0..group.count {
            match group.behaviour {
                BehaviourConfig::Borrower {
                    collateral,
                    repair_bps,
                    ..
                } => {
                    population.push(Box::new(Borrower::new(
                        PULLED,
                        Amount::new(collateral),
                        Amount::new(*debt),
                        repair_bps,
                    )));
                }
                BehaviourConfig::Liquidator {
                    every,
                    capacity_bps,
                    watching,
                } => {
                    population.push(Box::new(Liquidator::new(
                        PULLED,
                        every,
                        capacity_bps,
                        watching,
                    )));
                }
                BehaviourConfig::Depositor {
                    step,
                    target_utilisation_bps,
                } => {
                    population.push(Box::new(Depositor::new(
                        PULLED,
                        Amount::new(step),
                        target_utilisation_bps,
                    )));
                }
                // A behaviour of the other family joins no lending world. The scenario schema
                // cannot express that on its own, so the population it produces is the population
                // it described minus whatever could not act here.
                _ => {}
            }
        }
    }
    population
}

/// How many depths one regime's spread is swept across.
///
/// Swept rather than drawn. A spread is uncertainty about the shock, and drawing it puts that
/// uncertainty into the answer twice — once because the shock is uncertain and once because the
/// sample of it is. Nine depths cover the stated range exactly, at every weight, in every piece
/// of the sweep.
const DEPTHS: u64 = 9;

/// What one run watches for while it happens.
///
/// A state at the end of a run does not say whether a loss was realised on the way: a write-off
/// leaves the totals lower and the interest that accrued alongside it leaves them higher, and
/// nothing in the final numbers separates the two. The events do, so they are counted as they
/// are reported.
#[derive(Debug, Default, Clone, Copy)]
pub struct Losses {
    /// Positions taken over.
    pub liquidations: u64,
    /// What was written off against the supply side, in the loan asset's base units.
    pub written_off: u128,
}

impl Observer<Lending<Shocked, FixedRate>> for Losses {
    fn on_events(&mut self, _now: Tick, events: &[LendingEvent]) {
        for event in events {
            if let LendingEvent::Liquidated { bad_debt, .. } = event {
                self.liquidations += 1;
                self.written_off = self.written_off.saturating_add(bad_debt.base_units());
            }
        }
    }

    fn on_rejected(&mut self, _now: Tick, _actor: AgentId, _reason: &MechError) {}

    fn on_tick_end(&mut self, _now: Tick, _state: &LendingState) {}
}

/// One future of a sweep: what the price does in it, how much of the sweep it takes, and what a
/// report calls it.
///
/// The name carries the depth as well as the regime, because a regime with a spread is several
/// experiments and a reader comparing them needs to know which one a row is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Future {
    /// The regime, and the depth this stratum was swept at.
    pub(crate) name: String,
    /// What the price does.
    pub(crate) shock: Shock,
    /// Runs spent here.
    pub(crate) runs: u64,
}

/// What one shock is called in a report.
fn named(regime: &str, shock: Shock) -> String {
    match shock {
        Shock::Steady => format!("{regime} (steady)"),
        Shock::Gap { to_bps, .. } => format!("{regime} @ {to_bps} bps"),
        // A slide's depth is where it ends rather than where it sits, and the word says which.
        Shock::Slide { to_bps, .. } => format!("{regime} to {to_bps} bps"),
    }
}

/// The futures a sweep of `runs` is divided between, and how many runs each takes.
///
/// The division is exact. Weights come to a whole and the remainder of the last one is given to
/// it, so a sweep of ten thousand runs at a fifth weight is two thousand runs of that regime and
/// not about that many — which is the difference between a stated mixture and a sampled one.
///
/// A regime with a spread is divided again, across [`DEPTHS`] depths of its shock.
pub(crate) fn futures(scenario: &ScenarioConfig, runs: u64) -> Vec<Future> {
    if scenario.regimes.is_empty() {
        let MechanismConfig::Lending { .. } = scenario.mechanism else {
            return Vec::new();
        };
        return vec![Future {
            name: "unshocked".into(),
            shock: Shock::Steady,
            runs,
        }];
    }
    let mut divided = Vec::new();
    let mut spent = 0u64;
    let last = scenario.regimes.len() - 1;
    for (at, regime) in scenario.regimes.iter().enumerate() {
        let share = if at == last {
            runs - spent
        } else {
            runs * u64::from(regime.weight_bps) / 10_000
        };
        spent += share;
        divided.extend(strata(regime, share));
    }
    divided
}

/// One regime's runs, across the depths its spread covers.
fn strata(regime: &Regime, runs: u64) -> Vec<Future> {
    if runs == 0 {
        return Vec::new();
    }
    let spread = u64::from(regime.depth_spread_bps);
    if spread == 0 || runs < DEPTHS {
        return vec![Future {
            name: named(&regime.name, regime.shock),
            shock: regime.shock,
            runs,
        }];
    }
    let mut across = Vec::with_capacity(usize::try_from(DEPTHS).unwrap_or(0));
    let mut spent = 0;
    for step in 0..DEPTHS {
        let share = if step + 1 == DEPTHS {
            runs - spent
        } else {
            runs / DEPTHS
        };
        spent += share;
        // Evenly across the stated range, ends included, so the two edges of the uncertainty are
        // in the sweep rather than only its middle.
        let offset = 2 * spread * step / (DEPTHS - 1);
        let shock = deepened(regime.shock, spread, offset);
        across.push(Future {
            name: named(&regime.name, shock),
            shock,
            runs: share,
        });
    }
    across
}

/// One shock moved to a depth `offset − spread` basis points from the one written down.
fn deepened(shock: Shock, spread: u64, offset: u64) -> Shock {
    let moved = |to_bps: u32| -> u32 {
        let shifted = i64::from(to_bps) + i64::try_from(offset).unwrap_or(0)
            - i64::try_from(spread).unwrap_or(0);
        u32::try_from(shifted.clamp(0, 10_000)).unwrap_or(0)
    };
    match shock {
        Shock::Steady => Shock::Steady,
        Shock::Gap { to_bps, at_tick } => Shock::Gap {
            to_bps: moved(to_bps),
            at_tick,
        },
        Shock::Slide { to_bps, over_ticks } => Shock::Slide {
            to_bps: moved(to_bps),
            over_ticks,
        },
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

    use sting_karman_core::{
        Agent, Mechanism, NullObserver, PathSource, RunRng, SourceError, World,
    };
    use sting_karman_mech_lending::{
        Collateral, FixedRate, Given, Lending, LendingAction, Loan, MarketParams,
        ORACLE_PRICE_SCALE, Price,
    };
    use sting_karman_types::SecondsPerTick;

    use super::*;

    /// The market as it was pulled: one live Morpho Blue market, and every holder its own pull
    /// could name. Read at compile time, because a test that reached for a file would be the one
    /// thing this whole discipline exists to prevent.
    const PULLED_MARKET: &str = include_str!("../../../corpus/karman/fixtures/morpho-market.json");

    #[derive(Debug, Clone, Copy)]
    struct Steady;

    impl PathSource<Price<Collateral, Loan>> for Steady {
        fn value_at(&self, _now: Tick) -> Result<Price<Collateral, Loan>, SourceError> {
            Ok(Price::new(ORACLE_PRICE_SCALE))
        }
    }

    fn pulled() -> PulledMarket {
        serde_json::from_str(PULLED_MARKET).expect("the committed fixture is not readable")
    }

    fn mechanism() -> Lending<Steady, FixedRate> {
        let params = MarketParams {
            lltv: Amount::new(860_000_000_000_000_000),
            fee_recipient: AgentId(0),
        };
        Lending::new(SecondsPerTick(NonZeroU64::new(12).unwrap()))
            .with_market(
                PULLED,
                params,
                Steady,
                FixedRate(Amount::new(3_170_979_198)),
            )
            .unwrap()
    }

    /// The gate: a market pulled from a chain opens as a state the mechanism accepts, with no
    /// network anywhere near it.
    #[test]
    fn a_pulled_market_opens_as_a_state_the_mechanism_accepts() {
        let pulled = pulled();
        assert_eq!(pulled.origin.chain_id, 1);
        assert_eq!(pulled.origin.protocol, "morpho-blue");
        assert!(
            pulled.holders.len() >= 10,
            "a pull of a live market named almost nobody"
        );

        let opened = opening(&pulled).expect("the fixture does not open");
        assert!(mechanism().check_invariants(&opened.state).is_ok());
    }

    /// Every share the market issued is held by somebody, which is what makes a market read in
    /// part a whole market. The holders a pull could not name are the difference, and here that
    /// difference is most of the debt.
    #[test]
    fn the_holders_a_pull_could_not_name_hold_the_rest() {
        let pulled = pulled();
        let opened = opening(&pulled).unwrap();
        let market = opened.state.market(PULLED).unwrap();

        let mut supply = Amount::ZERO;
        let mut borrow = Amount::ZERO;
        for (_, held) in opened.state.holders(PULLED) {
            supply = supply.checked_add(held.supply_shares).unwrap();
            borrow = borrow.checked_add(held.borrow_shares).unwrap();
        }
        assert_eq!(supply, market.total_supply_shares);
        assert_eq!(borrow, market.total_borrow_shares);

        let unnamed = opened.state.position(PULLED, opened.unnamed);
        assert!(
            !unnamed.borrow_shares.is_zero(),
            "a live market's borrowers were all named"
        );
        assert!(
            unnamed.collateral.is_zero(),
            "a residual invented collateral nobody measured"
        );
    }

    /// Runnable means a run: the state goes into a world, agents act on it, and the mechanism
    /// carries it for as long as it is asked to.
    #[test]
    fn a_run_starts_from_a_pulled_market() {
        struct Lender;
        impl Agent<Lending<Steady, FixedRate>> for Lender {
            fn decide(
                &mut self,
                _me: AgentId,
                _state: &LendingState,
                _now: Tick,
                rng: &mut RunRng,
            ) -> Option<LendingAction> {
                Some(LendingAction::Supply {
                    market: PULLED,
                    given: Given::Assets(Amount::new(u128::from(rng.below(1_000_000) + 1))),
                })
            }
        }

        let opened = opening(&pulled()).unwrap();
        let population = opened.holders.len() + 1;

        let agents: Vec<Box<dyn Agent<Lending<Steady, FixedRate>>>> = (0..population)
            .map(|_| Box::new(Lender) as Box<dyn Agent<Lending<Steady, FixedRate>>>)
            .collect();
        let mut world = World::new(mechanism(), opened.state, agents);
        world
            .run(200, &mut RunRng::new(42, 0), &mut NullObserver)
            .expect("the run stopped");

        let after = world.state().market(PULLED).unwrap();
        assert!(
            after.total_borrow_assets > Amount::new(287_815_521_258_445),
            "no interest accrued"
        );
        assert_eq!(after.last_update, Tick(199));
    }

    fn regime(name: &str, weight_bps: u32, to_bps: u32, spread: u32) -> Regime {
        Regime {
            name: name.into(),
            weight_bps,
            shock: Shock::Gap {
                to_bps,
                at_tick: 50,
            },
            depth_spread_bps: spread,
        }
    }

    fn swept(regimes: Vec<Regime>) -> ScenarioConfig {
        ScenarioConfig {
            name: "swept".into(),
            seed: 1,
            n_ticks: 100,
            mechanism: MechanismConfig::Lending {
                market: "corpus/karman/fixtures/morpho-market.json".into(),
                borrow_rate: 3_170_979_198,
                lltv: None,
                population: Population::Pulled,
                seconds_per_tick: 12,
            },
            regimes,
            agents: Vec::new(),
        }
    }

    /// A market opened for its terms alone still issues exactly the shares it issued, and one
    /// holder has all of them. The invariant a state is built to keep does not weaken because a
    /// study replaced the population — it is the reason the lender is there at all.
    #[test]
    fn a_market_opened_for_its_terms_is_still_wholly_held() {
        let pulled: PulledMarket = sting_karman_data::read_json_fixture(Path::new(
            "../../corpus/karman/fixtures/morpho-market.json",
        ))
        .expect("the pulled market is committed beside this crate");
        let state = lent(&pulled).unwrap();
        let market = state.market(PULLED).unwrap();
        assert_eq!(
            market.total_supply_shares,
            Amount::new(pulled.total_supply_shares)
        );
        assert_eq!(market.total_borrow_assets, Amount::ZERO);
        assert_eq!(state.holdings().count(), 1);

        let lender = state.position(PULLED, AgentId(0));
        assert_eq!(lender.supply_shares, market.total_supply_shares);
        assert_eq!(lender.borrow_shares, Amount::ZERO);
        assert_eq!(lender.collateral, Amount::ZERO);
    }

    /// The division is exact and not approximate, which is the whole difference between a stated
    /// mixture and a sampled one: a fifth weight of ten thousand runs is two thousand runs.
    #[test]
    fn the_futures_divide_a_sweep_exactly() {
        let scenario = swept(vec![
            regime("calm", 6_000, 10_000, 0),
            regime("stress", 2_500, 9_000, 0),
            regime("break", 1_500, 8_000, 0),
        ]);
        let divided = futures(&scenario, 10_000);
        assert_eq!(
            divided.iter().map(|future| future.runs).sum::<u64>(),
            10_000
        );
        assert_eq!(divided[0].runs, 6_000);
        assert_eq!(divided[1].runs, 2_500);
        assert_eq!(divided[2].runs, 1_500);
    }

    /// Weights that do not divide the run count evenly still spend every run: the remainder goes
    /// to the last future rather than being dropped, so a sweep of a thousand and one runs is a
    /// thousand and one runs.
    #[test]
    fn every_run_is_spent_however_the_weights_fall() {
        let scenario = swept(vec![
            regime("a", 3_333, 9_000, 0),
            regime("b", 3_333, 8_000, 0),
            regime("c", 3_334, 7_000, 0),
        ]);
        for runs in [1, 2, 7, 1_001, 9_999] {
            let divided = futures(&scenario, runs);
            assert_eq!(
                divided.iter().map(|future| future.runs).sum::<u64>(),
                runs,
                "at {runs} runs"
            );
        }
    }

    /// A spread is swept rather than drawn, ends included, so the two edges of the uncertainty
    /// are in the sweep and not only its middle.
    #[test]
    fn a_spread_is_swept_across_its_whole_range() {
        let scenario = swept(vec![regime("break", 10_000, 8_000, 500)]);
        let divided = futures(&scenario, 900);
        assert_eq!(divided.len(), 9);
        assert_eq!(divided.iter().map(|future| future.runs).sum::<u64>(), 900);

        let depths: Vec<u32> = divided
            .iter()
            .map(|future| match future.shock {
                Shock::Gap { to_bps, .. } => to_bps,
                _ => unreachable!("the regime states a gap"),
            })
            .collect();
        assert_eq!(
            depths,
            vec![
                7_500, 7_625, 7_750, 7_875, 8_000, 8_125, 8_250, 8_375, 8_500
            ]
        );
    }

    /// Too few runs to cover a spread is not a reason to cover part of it: the regime runs at the
    /// depth it was written down at, and a reader is not shown a range that was never swept.
    #[test]
    fn a_regime_with_too_few_runs_keeps_the_depth_it_states() {
        let scenario = swept(vec![regime("break", 10_000, 8_000, 500)]);
        let divided = futures(&scenario, 4);
        assert_eq!(divided.len(), 1);
        assert_eq!(
            divided[0].shock,
            Shock::Gap {
                to_bps: 8_000,
                at_tick: 50
            }
        );
        assert_eq!(divided[0].runs, 4);
        assert_eq!(divided[0].name, "break @ 8000 bps");
    }

    /// With no futures stated there is one, exactly as written.
    #[test]
    fn a_scenario_with_no_futures_has_one() {
        let only = futures(&swept(Vec::new()), 250);
        assert_eq!(only.len(), 1);
        assert_eq!((only[0].shock, only[0].runs), (Shock::Steady, 250));
    }

    /// A shock deepened past the whole is a price gone to nothing rather than below it.
    #[test]
    fn a_spread_cannot_take_a_shock_past_the_ends() {
        let scenario = swept(vec![regime("break", 10_000, 200, 9_000)]);
        for future in futures(&scenario, 90) {
            let Shock::Gap { to_bps, .. } = future.shock else {
                unreachable!()
            };
            assert!(to_bps <= 10_000);
        }
    }

    /// A fixture whose holders hold more than the market issued came from two different blocks,
    /// and opening it would put a market that never existed under a run.
    #[test]
    fn a_fixture_that_does_not_reconcile_is_refused() {
        let mut pulled = pulled();
        pulled.total_supply_shares = 1;
        assert!(matches!(opening(&pulled), Err(AppError::Pulled(_))));
    }
}
