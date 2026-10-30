//! A guarantee layer as the world a run starts from.
//!
//! What is built here is a population and a graph, and the graph is the whole experiment. Everyone
//! opens with the same money and the same terms; what differs between an honest participant, a bad
//! borrower and a member of a ring is only **whom they are willing to stand behind** and **when
//! they stop repaying**. The mechanism is told none of it and cannot be: a hundred confederates
//! look exactly like a hundred ordinary participants who happen to know each other, which is what
//! makes the question worth simulating rather than ruling on.
//!
//! Three circles, drawn once and never re-drawn during a run:
//!
//! - **Honest** — a circle of other honest participants, chosen by a stride around the honest seats
//!   so that the graph is a ring of acquaintances rather than a clique or a star. Most of them
//!   repay; a stated share never do, and those are what put a guarantor's stake at real risk.
//! - **Bad** — honest seats that never repay. They are not attackers. They are the reason standing
//!   behind somebody is an exposure and not a fee.
//! - **The ring** — seats whose circle is the ring and nobody else. They repay exactly as long as
//!   repaying still buys them a lower collateral requirement, and stop the moment it does not.
//!
//! Nothing about the ring is privileged. It has the same capital per seat, pays the same premium
//! and posts the same stake. What it is buying with all that repaying is the collateral floor, and
//! whether the floor is worth more than it cost is the whole of the measurement.

use std::{num::NonZeroU64, sync::Arc};

use sting_karman_agents::Member as MemberAgent;
use sting_karman_core::{Agent, World};
use sting_karman_mech_vouch::{Backing, Cash, Member, Vouch, VouchState};
use sting_karman_scenario::{BackingConfig, BehaviourConfig, MechanismConfig, ScenarioConfig};
use sting_karman_types::{AgentId, Amount, SecondsPerTick};

use crate::AppError;

/// The venue: it takes its share of every premium and never acts.
pub const VENUE: AgentId = AgentId(0);

/// The first seat a participant can occupy, the venue having the one before it.
const FIRST_MEMBER: u64 = 1;

/// A guarantee layer as a scenario describes it, and the world it opens.
#[derive(Debug)]
pub struct Opened {
    /// The mechanism with its posted terms.
    pub mechanism: Arc<Vouch>,
    /// The state the run starts from.
    pub state: VouchState,
    /// Which seats the ring occupies, so that what it walked away with can be read apart from
    /// what everybody else did.
    pub ring: Vec<AgentId>,
    /// Which seats the honest population occupies, bad borrowers among them.
    pub honest: Vec<AgentId>,
    /// What each seat opened holding, which every net result here is measured against.
    pub capital: Amount<Cash>,
    /// What the fund opened holding, which is what a draw and a venue's take are read against.
    pub fund: Amount<Cash>,
}

/// Reads a scenario's guarantee layer and opens the world it describes.
///
/// # Errors
///
/// Fails when the scenario is not a guarantee layer, when its population does not fit the range a
/// seat can be named in, or when its opening balances are wider than money is carried in.
pub fn opened(scenario: &ScenarioConfig) -> Result<Opened, AppError> {
    let MechanismConfig::Vouch {
        honest,
        ring,
        capital,
        fund,
        seconds_per_tick,
        opening_bps,
        step_bps,
        floor_bps,
        backing,
        premium_bps,
        venue_share_bps,
        seasoning,
        release_lag,
        execution,
        ..
    } = scenario.mechanism
    else {
        return Err(AppError::WrongFamily {
            wanted: "a guarantee layer",
        });
    };
    // One group of members and no more: the scenario's own counts decide who is who, and a
    // population that could also be described group by group could describe two different worlds.
    if !scenario
        .agents
        .iter()
        .all(|group| matches!(group.behaviour, BehaviourConfig::Member))
    {
        return Err(AppError::WrongFamily {
            wanted: "a population of members",
        });
    }
    // The mechanism's counts and the population's have to agree, because both are written down.
    // A scenario seating nine hundred and describing a thousand would run one of the two, and
    // which one would be a fact about the order this function happens to read them in.
    let described: u64 = scenario
        .agents
        .iter()
        .map(|group| u64::from(group.count))
        .sum();
    if described != u64::from(honest) + u64::from(ring) {
        return Err(AppError::Ungenerable(
            "the population and the layer disagree about how many participants there are".into(),
        ));
    }

    let seats = u64::from(honest) + u64::from(ring) + FIRST_MEMBER;
    let seats = u32::try_from(seats)
        .map_err(|_| AppError::Ungenerable("more participants than can be named".into()))?;
    let capital = Amount::<Cash>::new(capital);

    // The venue opens with nothing. What it ends with is what the layer earned, and a venue that
    // opened holding money would have that money mixed into the answer.
    let mut members = vec![Member::default()];
    members.extend((FIRST_MEMBER..u64::from(seats)).map(|_| Member::opening(capital)));
    let fund = Amount::<Cash>::new(fund);
    let state = VouchState::open(members, fund)?;

    let mechanism = Vouch {
        venue: VENUE,
        seconds_per_tick: SecondsPerTick(
            NonZeroU64::new(seconds_per_tick)
                .ok_or_else(|| AppError::Ungenerable("a tick worth no time".into()))?,
        ),
        opening: Amount::new(u128::from(opening_bps)),
        step: Amount::new(u128::from(step_bps)),
        floor: Amount::new(u128::from(floor_bps)),
        backing: match backing {
            BackingConfig::Stake => Backing::Stake,
            BackingConfig::NetWorth => Backing::NetWorth,
        },
        premium: Amount::new(u128::from(premium_bps)),
        venue_share: Amount::new(u128::from(venue_share_bps)),
        seasoning,
        release_lag,
        execution: Amount::new(execution),
    };

    // The honest seats come first and the ring takes the tail. Which end is which does not matter
    // to the mechanism — it reads no seat number — and fixing it here is what lets a measurement
    // tell the two populations apart afterwards without the run having been told.
    let honest_seats: Vec<AgentId> = (FIRST_MEMBER..FIRST_MEMBER + u64::from(honest))
        .map(|at| AgentId(u32::try_from(at).unwrap_or(u32::MAX)))
        .collect();
    let ring_seats: Vec<AgentId> = (FIRST_MEMBER + u64::from(honest)
        ..FIRST_MEMBER + u64::from(honest) + u64::from(ring))
        .map(|at| AgentId(u32::try_from(at).unwrap_or(u32::MAX)))
        .collect();

    Ok(Opened {
        mechanism: Arc::new(mechanism),
        state,
        ring: ring_seats,
        honest: honest_seats,
        capital,
        fund,
    })
}

/// Gives every seat a behaviour and a circle.
///
/// # Errors
///
/// Fails when the scenario is not a guarantee layer.
pub fn populate(
    scenario: &ScenarioConfig,
    opened: &Opened,
) -> Result<Vec<Box<dyn Agent<Vouch>>>, AppError> {
    let MechanismConfig::Vouch {
        bad,
        circle,
        stake,
        reserve,
        term,
        notice,
        cadence,
        opening_bps,
        step_bps,
        floor_bps,
        ..
    } = scenario.mechanism
    else {
        return Err(AppError::WrongFamily {
            wanted: "a guarantee layer",
        });
    };

    // How many settlements it takes to walk the opening requirement down to the floor. This is
    // what a ring is buying, and therefore also exactly how long it pays before it stops.
    let earned = u128::from(opening_bps).saturating_sub(u128::from(floor_bps));
    let to_the_floor = if step_bps == 0 {
        u32::MAX
    } else {
        u32::try_from(earned.div_ceil(u128::from(step_bps))).unwrap_or(u32::MAX)
    };

    let terms = Arc::clone(&opened.mechanism);
    let member = |circle: Vec<AgentId>, defects_after: Option<u32>| MemberAgent {
        terms: Arc::clone(&terms),
        circle,
        stake: Amount::new(stake),
        reserve: Amount::new(reserve),
        term,
        notice,
        cadence,
        defects_after,
    };

    let mut population: Vec<Box<dyn Agent<Vouch>>> = Vec::with_capacity(opened.state.members.len());
    // The venue acts as nothing: it stands behind nobody, so nobody stands behind it and the
    // mechanism refuses it in any case. A seat that sat out would be a hole in the acting order.
    population.push(Box::new(member(Vec::new(), None)));

    for at in 0..opened.honest.len() {
        // A bad borrower is taken from the front of the honest seats rather than drawn, so a
        // stated count is that count exactly, whatever the seed and whatever the population size.
        //
        // Where in its history it fails is its seat number modulo the length of that history,
        // which spreads them evenly over every stage of it. All of them failing at the same point
        // would decide the answer by itself: fail on the first loan and every default is fully
        // covered, because that is what full collateral means; fail at the floor and every one of
        // them is an attacker in all but name.
        let seat = u32::try_from(at).unwrap_or(u32::MAX);
        let defects_after = (at < bad as usize).then(|| seat % to_the_floor.max(1));
        population.push(Box::new(member(
            acquaintances(&opened.honest, at, circle),
            defects_after,
        )));
    }
    for at in 0..opened.ring.len() {
        // The same circle shape as an honest participant's, and the same size: what separates the
        // ring from the honest population is not how it is wired but that it is closed and that it
        // stops paying. A ring given a denser graph than everybody else would be an experiment
        // about density.
        //
        // It pays for exactly as many settlements as the floor costs and not one more. Fewer and
        // it attacks before it has bought anything; more and it is paying for standing it will
        // never spend, which is the same as handing the venue money.
        population.push(Box::new(member(
            acquaintances(&opened.ring, at, circle),
            Some(to_the_floor),
        )));
    }
    Ok(population)
}

/// Whom one honest seat is willing to stand behind.
///
/// A stride around the honest seats rather than a draw: what an honest graph should not be is a
/// clique, because a clique of honest people is a ring that happens to pay, and a measurement that
/// could not tell the two apart would be measuring its own construction. A stride gives every
/// participant a circle of the same size, none of them overlapping more than the stride forces,
/// and it is the same circle at every seed — the graph is a fact about the design being tested and
/// not one more thing drawn from the run's stream.
fn acquaintances(seats: &[AgentId], from: usize, circle: u32) -> Vec<AgentId> {
    let span = seats.len();
    if span <= 1 {
        return Vec::new();
    }
    (1..=circle as usize)
        .map(|step| seats[(from + step) % span])
        .filter(|&of| of != seats[from])
        .collect()
}

/// Builds the world a run starts from, as many times as a sweep needs it.
///
/// # Errors
///
/// Fails when the scenario would not open a guarantee layer.
///
/// # Panics
///
/// Panics when a scenario that opened a layer would not populate one, which cannot happen: both
/// read the same fields off the same validated scenario. A world built from an unvalidated one is
/// the caller's mistake and not a condition a run should discover halfway through.
pub fn world_builder(
    scenario: &ScenarioConfig,
) -> Result<impl Fn() -> World<Vouch> + Sync + '_, AppError> {
    let opened = opened(scenario)?;
    // Refused here, where a refusal is still a value. The closure populates again on every call
    // because a population is a fresh set of agents rather than a value to clone, and by then the
    // only way it can fail has already been ruled out — which is why the second call may say so.
    drop(populate(scenario, &opened)?);
    Ok(move || {
        #[allow(
            clippy::expect_used,
            reason = "the same call was made above, where its failure was returned"
        )]
        let population = populate(scenario, &opened)
            .expect("the scenario populated a guarantee layer a moment ago");
        World::new(
            (*opened.mechanism).clone(),
            opened.state.clone(),
            population,
        )
    })
}

/// What one run of a guarantee layer leaves behind, in the numbers a claim is stated in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landed {
    /// What the ring walked away with, over what it opened holding, in basis points, signed.
    ///
    /// **The number the claim is stated in.** Negative is an attack that cost more than it took. It
    /// counts everything a ring seat still has — free money and stake that was never taken — so
    /// there is no accounting in which the attack looks worse than it was.
    pub ring_net_bps: i128,
    /// The same for the honest population, bad borrowers among them.
    ///
    /// Registered beside the ring's because a floor that ruins an attacker by ruining everybody is
    /// not a defence, and a report that showed only the first number could not tell the two apart.
    pub honest_net_bps: i128,
    /// Principal drawn over the run, over what the fund opened with, in basis points.
    ///
    /// The liveness reading. A collateral floor high enough that nobody borrows makes any ring
    /// unprofitable and makes the layer pointless, and that outcome has to be visible rather than
    /// scored as a success.
    pub drawn_bps: i128,
    /// Principal that defaulted and no stake covered, over what was drawn, in basis points.
    pub written_off_bps: i128,
    /// What the venue took in premium, over what the fund opened with, in basis points.
    pub venue_take_bps: i128,
    /// The most credit any one participant held against the stake standing behind it, in basis
    /// points.
    ///
    /// **The number the other half of the claim is stated in.** Ten thousand is somebody who borrowed
    /// exactly what was locked up for them, which is credit that buys nothing money would not have
    /// bought. Anything at or below it says the floor that stops a ring is a floor at which the
    /// layer has no reason to exist.
    pub leverage_bps: i128,
    /// How many loans the median ring seat settled before it stopped paying.
    pub ring_settled: i128,
    /// Whether the ring ever drew anything at all. A ring that never got started measures nothing
    /// about whether attacking pays.
    pub ring_acted: bool,
}

/// The names the numbers above are reported under, in their order.
pub const LANDED: [&str; 7] = [
    "ring_net_bps",
    "honest_net_bps",
    "drawn_bps",
    "written_off_bps",
    "venue_take_bps",
    "leverage_bps",
    "ring_settled",
];

/// What a group of seats is worth at the end of a run.
///
/// Free money, plus stake nobody took, less what is still owed. The last term is not a detail: a
/// participant holding an open loan is holding money that is not its own, and counting it would
/// report anybody who happened to be mid-loan when the run ended as that much richer — which is
/// most of a population that borrows, and none of a ring that has already defaulted and had its
/// debt cleared against the stake.
fn worth(state: &VouchState, seats: &[AgentId]) -> u128 {
    seats
        .iter()
        .map(|who| state.members[who.0 as usize].net_worth().base_units())
        .sum()
}

/// `part` against `whole`, in basis points, signed and centred on nothing.
fn against(part: u128, whole: u128) -> i128 {
    if whole == 0 {
        return 0;
    }
    let scaled = part.saturating_mul(10_000) / whole;
    i128::try_from(scaled).unwrap_or(i128::MAX)
}

/// Reads one finished run into the numbers a claim is stated in.
#[must_use]
pub fn landed(state: &VouchState, opened: &Opened) -> Landed {
    let capital = opened.capital.base_units();
    let ring_opened = capital.saturating_mul(opened.ring.len() as u128);
    let honest_opened = capital.saturating_mul(opened.honest.len() as u128);

    // A net result is a change, so it is the only reading here that may be negative, and it is
    // taken as a difference of two totals rather than as a ratio of them: a ring wiped out has no
    // ratio at all, and reporting a zero there would read as a ring that broke even.
    let net = |now: u128, then: u128| -> i128 {
        if then == 0 {
            return 0;
        }
        let now = i128::try_from(now).unwrap_or(i128::MAX);
        let then = i128::try_from(then).unwrap_or(i128::MAX);
        (now - then).saturating_mul(10_000) / then
    };

    let mut settled: Vec<i128> = opened
        .ring
        .iter()
        .map(|who| i128::from(state.members[who.0 as usize].settled))
        .collect();
    settled.sort_unstable();

    // Leverage is what somebody owes against the stake standing behind *them* — not against what
    // they staked for other people, which is a different relationship that happens to be the same
    // size in a graph where everybody backs as many as back them.
    //
    // Read at its worst rather than at its middle: what matters is not what the ordinary
    // participant reached but what the most-levered one did, because that is the position an
    // attacker is buying its way toward.
    let leverage = state
        .members
        .iter()
        .enumerate()
        .map(|(at, member)| {
            let behind: u128 = state
                .written_for(AgentId(u32::try_from(at).unwrap_or(u32::MAX)))
                .iter()
                .map(|line| state.lines[line.0 as usize].stake.base_units())
                .sum();
            against(member.owed.base_units(), behind)
        })
        .max()
        .unwrap_or(0);

    Landed {
        ring_net_bps: net(worth(state, &opened.ring), ring_opened),
        honest_net_bps: net(worth(state, &opened.honest), honest_opened),
        drawn_bps: against(state.drawn.base_units(), opened.fund.base_units()),
        written_off_bps: against(state.written_off.base_units(), state.drawn.base_units()),
        venue_take_bps: against(
            state.members[VENUE.0 as usize].balance.base_units(),
            opened.fund.base_units(),
        ),
        leverage_bps: leverage,
        ring_settled: settled.get(settled.len() / 2).copied().unwrap_or(0),
        ring_acted: opened
            .ring
            .iter()
            .any(|who| !state.drawn_by(*who).is_empty()),
    }
}
