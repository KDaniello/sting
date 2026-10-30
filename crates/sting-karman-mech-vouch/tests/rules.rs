//! What the guarantee layer promises, pinned one rule at a time.
//!
//! Each test here names a rule the mechanism would still compile without, and which the whole
//! claim turns on. The arithmetic ones matter most: the whole finding is that a ring's profit is the
//! leverage the floor extends, so a floor that quietly extended a different amount of leverage
//! than it says would be a wrong answer that looked entirely ordinary.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::num::NonZeroU64;

use sting_karman_core::{Agent, MechError, Mechanism, RunRng, World, conformance};
use sting_karman_mech_vouch::{
    Backing, Cash, LineId, LoanId, Member, Vouch, VouchAction, VouchEvent, VouchState, WHOLE,
};
use sting_karman_types::{AgentId, Amount, SecondsPerTick, Tick};

const VENUE: AgentId = AgentId(0);
const ALICE: AgentId = AgentId(1);
const BOB: AgentId = AgentId(2);
const CARA: AgentId = AgentId(3);

/// A year in ticks, so a premium quoted per year is the premium a term of this length pays.
const A_YEAR: u64 = 365;

fn cash(units: u128) -> Amount<Cash> {
    Amount::new(units)
}

/// A layer whose terms are round numbers, so that what a test asserts is the rule and not the
/// rounding: one tick is a day, a year is a year, and full collateral is stake for stake.
fn layer(floor: u128) -> Vouch {
    Vouch {
        venue: VENUE,
        seconds_per_tick: SecondsPerTick(NonZeroU64::new(24 * 60 * 60).expect("a day is not zero")),
        opening: WHOLE,
        step: Amount::new(1_000),
        floor: Amount::new(floor),
        backing: Backing::Stake,
        premium: Amount::new(1_000),
        venue_share: Amount::new(5_000),
        seasoning: 0,
        release_lag: 2,
        execution: Amount::ZERO,
    }
}

fn world(fund: u128, each: u128) -> VouchState {
    VouchState::open(
        vec![
            Member::default(),
            Member::opening(cash(each)),
            Member::opening(cash(each)),
            Member::opening(cash(each)),
        ],
        cash(fund),
    )
    .expect("a world of four members and a fund opens")
}

/// Applies an action that is expected to be accepted, and hands back what it reported.
fn act(
    mechanism: &Vouch,
    state: &mut VouchState,
    actor: AgentId,
    action: VouchAction,
    now: u64,
) -> Vec<VouchEvent> {
    let mut events = Vec::new();
    mechanism
        .apply(state, actor, action, Tick(now), &mut events)
        .unwrap_or_else(|refused| panic!("{action:?} was refused: {refused}"));
    mechanism
        .audit(state)
        .expect("an accepted action left a state the mechanism disallows");
    events
}

/// Ticks once, and hands back what the tick reported.
fn tick(mechanism: &Vouch, state: &mut VouchState, now: u64) -> Vec<VouchEvent> {
    let mut events = Vec::new();
    mechanism
        .tick(state, Tick(now), &mut events)
        .expect("a tick aborted the run");
    mechanism
        .audit(state)
        .expect("a tick left a state the mechanism disallows");
    events
}

/// Stake for stake is a draw of exactly what stands behind it, and a third of that is three times
/// it. This is the arithmetic the whole claim rests on, so it is pinned rather than assumed.
#[test]
fn the_floor_is_the_leverage() {
    for (floor, expected) in [(10_000u128, 1_000u128), (5_000, 2_000), (3_333, 3_000)] {
        let mechanism = layer(floor);
        let mut state = world(1_000_000, 10_000);
        act(
            &mechanism,
            &mut state,
            BOB,
            VouchAction::Write {
                of: ALICE,
                stake: cash(1_000),
            },
            0,
        );
        // Ten settlements at a step of a thousand take the opening requirement to the floor
        // whatever the floor is, so every reading below is taken at the floor and not on the way.
        state.members[ALICE.0 as usize].settled = 10;
        let headroom = mechanism
            .headroom(&state, ALICE, Tick(1))
            .expect("a headroom of a thousand-unit guarantee is representable");
        assert_eq!(
            headroom,
            cash(expected),
            "a floor of {floor} extended the wrong leverage"
        );
    }
}

/// The rule that makes a ring's arithmetic what it is: what a defaulter keeps is the principal, and
/// what it loses is the stake that stood behind it. Nothing else moves.
#[test]
fn a_default_takes_the_stake_and_leaves_the_principal() {
    let mechanism = layer(5_000);
    let mut state = world(1_000_000, 10_000);
    act(
        &mechanism,
        &mut state,
        BOB,
        VouchAction::Write {
            of: ALICE,
            stake: cash(1_000),
        },
        0,
    );
    state.members[ALICE.0 as usize].settled = 10;
    act(
        &mechanism,
        &mut state,
        ALICE,
        VouchAction::Draw {
            amount: cash(2_000),
            term: 4,
        },
        1,
    );

    let before = state.members[ALICE.0 as usize].balance;
    let events = tick(&mechanism, &mut state, 5);
    assert!(
        matches!(
            events[0],
            VouchEvent::Defaulted { seized, written_off, .. }
                if seized == cash(1_000) && written_off == cash(1_000)
        ),
        "a default of two thousand behind one thousand of stake takes the stake and writes off \
         the rest, and reported {:?}",
        events[0]
    );
    assert_eq!(
        state.members[ALICE.0 as usize].balance, before,
        "the defaulter's money moved, and nothing here pursues it"
    );
    assert_eq!(
        state.members[BOB.0 as usize].staked,
        Amount::ZERO,
        "the guarantor kept its stake"
    );
    assert_eq!(
        state.members[ALICE.0 as usize].owed,
        Amount::ZERO,
        "a defaulted debt is not owed"
    );
    assert_eq!(state.written_off, cash(1_000));
}

/// Under [`Backing::NetWorth`] a guarantee is worth no more than its writer is worth net of its
/// own debts, so a guarantor whose debts have overtaken what it holds stands behind nothing.
///
/// The boundary is the whole of the rule: it bites exactly when free money falls below what is
/// owed, which is what happens to somebody who has borrowed and parted with the proceeds. Nothing
/// in this crate lets a borrower part with anything, so this is pinned here rather than left to a
/// population to reach — and that the population cannot reach it is stated as a gap and not as a
/// result.
#[test]
fn a_guarantee_is_worth_no_more_than_its_writer() {
    let mut mechanism = layer(5_000);
    mechanism.backing = Backing::NetWorth;
    let mut state = world(1_000_000, 10_000);
    act(
        &mechanism,
        &mut state,
        ALICE,
        VouchAction::Write {
            of: CARA,
            stake: cash(2_000),
        },
        0,
    );
    state.members[CARA.0 as usize].settled = 10;

    let full = mechanism
        .headroom(&state, CARA, Tick(1))
        .expect("a headroom of a two-thousand-unit guarantee is representable");
    assert_eq!(
        full,
        cash(4_000),
        "a whole guarantee at half cover is twice its face"
    );

    // Alice now owes more than she holds free: nine thousand against the eight she has left.
    state.members[ALICE.0 as usize].owed = cash(9_000);
    let scaled = mechanism
        .headroom(&state, CARA, Tick(1))
        .expect("a scaled headroom is representable");
    assert_eq!(
        scaled,
        cash(2_000),
        "Alice is worth one thousand net of her debts against two thousand staked, so the          guarantee is worth half its face and the credit standing on it is half as much"
    );

    state.members[ALICE.0 as usize].owed = cash(1_000_000);
    assert_eq!(
        mechanism
            .headroom(&state, CARA, Tick(1))
            .expect("a headroom of nothing is representable"),
        Amount::ZERO,
        "a guarantor worth nothing net of its debts stood behind somebody anyway"
    );
}

/// A guarantee cannot be withdrawn out from under a loan it is standing behind. Without the lag a
/// guarantor watches a due date approach and steps out of the way, and a guarantee means nothing.
#[test]
fn a_withdrawal_cannot_outrun_a_default() {
    let mechanism = layer(10_000);
    let mut state = world(1_000_000, 10_000);
    act(
        &mechanism,
        &mut state,
        BOB,
        VouchAction::Write {
            of: ALICE,
            stake: cash(1_000),
        },
        0,
    );
    act(
        &mechanism,
        &mut state,
        ALICE,
        VouchAction::Draw {
            amount: cash(1_000),
            term: 4,
        },
        1,
    );
    act(
        &mechanism,
        &mut state,
        BOB,
        VouchAction::Release { line: LineId(0) },
        2,
    );

    tick(&mechanism, &mut state, 4);
    assert_eq!(
        state.members[BOB.0 as usize].staked,
        cash(1_000),
        "a matured withdrawal released stake a live loan was standing on"
    );

    tick(&mechanism, &mut state, 5);
    assert_eq!(
        state.members[BOB.0 as usize].staked,
        Amount::ZERO,
        "the default did not take the stake the withdrawal was waiting on"
    );
    assert_eq!(state.members[BOB.0 as usize].seized, cash(1_000));
}

/// The premium is a year's rate over the part of a year the loan runs for, split at the stated
/// share, and what the division cannot place goes to the venue rather than into existence.
#[test]
fn a_premium_is_split_and_never_invented() {
    let mechanism = layer(10_000);
    let mut state = world(1_000_000, 10_000);
    act(
        &mechanism,
        &mut state,
        BOB,
        VouchAction::Write {
            of: ALICE,
            stake: cash(1_000),
        },
        0,
    );
    let events = act(
        &mechanism,
        &mut state,
        ALICE,
        VouchAction::Draw {
            amount: cash(1_000),
            term: A_YEAR,
        },
        1,
    );
    assert!(
        matches!(events[0], VouchEvent::Drawn { premium, .. } if premium == cash(100)),
        "a tenth a year over a year on a thousand is a hundred, and it reported {:?}",
        events[0]
    );
    assert_eq!(
        state.members[VENUE.0 as usize].balance,
        cash(50),
        "the venue's half of a hundred"
    );
    assert_eq!(
        state.members[BOB.0 as usize].earned,
        cash(50),
        "the guarantor's half of a hundred"
    );
}

/// Repaying releases what stood behind the loan and buys a step off the next requirement, which is
/// the only thing in this mechanism anybody can accumulate.
#[test]
fn settling_releases_the_stake_and_buys_a_step() {
    let mechanism = layer(5_000);
    let mut state = world(1_000_000, 10_000);
    act(
        &mechanism,
        &mut state,
        BOB,
        VouchAction::Write {
            of: ALICE,
            stake: cash(1_000),
        },
        0,
    );
    act(
        &mechanism,
        &mut state,
        ALICE,
        VouchAction::Draw {
            amount: cash(1_000),
            term: 40,
        },
        1,
    );
    assert_eq!(
        state.lines[0].committed,
        cash(1_000),
        "a draw at full cover took the whole stake"
    );

    act(
        &mechanism,
        &mut state,
        ALICE,
        VouchAction::Repay { loan: LoanId(0) },
        2,
    );
    assert_eq!(
        state.lines[0].committed,
        Amount::ZERO,
        "a repaid loan kept its backing committed"
    );
    assert_eq!(state.members[ALICE.0 as usize].settled, 1);
    assert_eq!(
        mechanism.collateral(1),
        Amount::new(9_000),
        "one settlement did not buy its step off the requirement"
    );
}

/// Rejection is not corruption: every refused action leaves the state exactly as it found it and
/// reports nothing.
#[test]
fn a_refused_action_moves_nothing() {
    let mechanism = layer(10_000);
    let mut state = world(1_000, 10_000);
    act(
        &mechanism,
        &mut state,
        BOB,
        VouchAction::Write {
            of: ALICE,
            stake: cash(1_000),
        },
        0,
    );
    let before = state.clone();

    for action in [
        // Nobody stands behind themselves, and nobody stands behind anybody with nothing.
        VouchAction::Write {
            of: ALICE,
            stake: cash(1),
        },
        VouchAction::Write {
            of: CARA,
            stake: Amount::ZERO,
        },
        // A draw wider than the fund, and one nothing stands behind.
        VouchAction::Draw {
            amount: cash(100_000),
            term: 4,
        },
        VouchAction::Draw {
            amount: cash(1),
            term: 0,
        },
        // A loan that is not this one's, and a guarantee that is not this one's.
        VouchAction::Repay { loan: LoanId(0) },
        VouchAction::Release { line: LineId(0) },
    ] {
        let mut events = Vec::new();
        let refused = mechanism.apply(&mut state, ALICE, action, Tick(1), &mut events);
        assert!(
            refused.is_err(),
            "{action:?} was accepted where it should have been refused"
        );
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

/// The venue takes what rules send it and is the actor of nothing.
#[test]
fn the_venue_does_not_act() {
    let mechanism = layer(10_000);
    let mut state = world(1_000_000, 10_000);
    let mut events = Vec::new();
    let refused = mechanism.apply(
        &mut state,
        VENUE,
        VouchAction::Write {
            of: ALICE,
            stake: cash(1),
        },
        Tick(0),
        &mut events,
    );
    assert!(matches!(refused, Err(MechError { .. })), "the venue acted");
}

/// Somebody who writes guarantees, draws against the ones written for them, and repays; twice from
/// one seed, reaching the same state and having done the same work.
#[test]
fn conformance_determinism() {
    conformance::assert_deterministic(64, 11, || {
        World::new(
            layer(5_000),
            world(1_000_000, 10_000),
            vec![
                Box::new(Idle) as Box<dyn Agent<Vouch>>,
                Box::new(Busy { of: BOB }),
                Box::new(Busy { of: CARA }),
                Box::new(Busy { of: ALICE }),
            ],
        )
    });
}

/// The venue, which never acts.
struct Idle;

impl Agent<Vouch> for Idle {
    fn decide(
        &mut self,
        _me: AgentId,
        _state: &VouchState,
        _now: Tick,
        _rng: &mut RunRng,
    ) -> Option<VouchAction> {
        None
    }
}

/// Somebody who stands behind one other, then borrows and repays for the rest of the run.
struct Busy {
    of: AgentId,
}

impl Agent<Vouch> for Busy {
    fn decide(
        &mut self,
        me: AgentId,
        state: &VouchState,
        now: Tick,
        rng: &mut RunRng,
    ) -> Option<VouchAction> {
        if state.written_for(self.of).is_empty() {
            return Some(VouchAction::Write {
                of: self.of,
                stake: cash(1_000),
            });
        }
        let open = state
            .drawn_by(me)
            .iter()
            .copied()
            .find(|loan| state.loans[loan.0 as usize].open);
        match open {
            Some(loan) if state.loans[loan.0 as usize].due.0 <= now.0 + 2 => {
                Some(VouchAction::Repay { loan })
            }
            Some(_) => None,
            None => {
                let headroom = state
                    .written_for(me)
                    .iter()
                    .map(|line| state.lines[line.0 as usize].stake.base_units())
                    .sum::<u128>();
                (headroom > 0).then(|| VouchAction::Draw {
                    amount: cash(headroom.min(state.fund.base_units())),
                    term: 4 + rng.below(4),
                })
            }
        }
    }
}
