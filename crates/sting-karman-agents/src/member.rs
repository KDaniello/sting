//! Somebody who stands behind other people, borrows against the people standing behind them, and
//! either pays it back or does not.
//!
//! One behaviour and not two, and here that is a claim about the world rather than a preference
//! about code. A ring of fake participants is not a different kind of actor from an ordinary
//! borrower who fails: both write guarantees, both draw, both stop repaying. What separates them
//! is **who stands behind them** — a bad borrower is backed by people who lose real money over it,
//! a ring is backed by itself — and **when they stop** — a bad borrower fails early, a ring pays
//! its way down to the collateral floor first and fails at the bottom. Writing them as two types
//! would put that distinction in the code, where the mechanism cannot see it, instead of in the
//! graph, where the mechanism has to survive it.

use std::sync::Arc;

use sting_karman_core::{Agent, RunRng};
use sting_karman_mech_vouch::{Cash, LoanId, Vouch, VouchAction, VouchState};
use sting_karman_types::{AgentId, Amount, Tick};

/// A participant in a guarantee layer.
///
/// **What it does, in this order.** It repays a loan that is coming due, if it means to and can
/// afford to. Failing that it writes a guarantee for somebody in its circle it has not written for
/// yet. Failing that it draws whatever stands behind it.
///
/// The order is this behaviour's one real assumption, and it is deliberate: repaying beats
/// everything, because a participant that let a due date pass while it was busy staking would be
/// modelling forgetfulness, and what this population exists to measure is intent.
///
/// **Where the parameters come from.** None is fitted, because there is nothing measured to fit
/// them to: no market of this shape has ever been run. They are stated, and a sweep moves the one
/// that matters. What a run measures is insensitive to the cadences — a population acting twice as
/// often for half as long is the same population — and entirely sensitive to
/// [`Member::defects_after`], which is what the experiment varies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The posted terms of the layer it is acting in.
    ///
    /// The mechanism is asked what it would allow rather than second-guessed. A borrower that
    /// computed its own headroom would be a second implementation of the rule the experiment is
    /// about, and the first time the two disagreed a run would be measuring the disagreement. One
    /// handle between the whole population, because the terms are one thing and a population of
    /// thousands holding a copy each is a copy of the rules per participant.
    pub terms: Arc<Vouch>,
    /// Whom it is willing to stand behind, and nobody else.
    ///
    /// This is the whole of the graph. An honest participant's circle is drawn from the honest
    /// population; a ring's circle is the ring. Nothing else about the two differs.
    pub circle: Vec<AgentId>,
    /// What it locks behind each guarantee it writes.
    pub stake: Amount<Cash>,
    /// What it keeps free and will not stake, so that it can still act and still repay.
    pub reserve: Amount<Cash>,
    /// How many ticks it borrows for.
    pub term: u64,
    /// How many ticks before a due date it starts trying to repay.
    ///
    /// Not a taste: a borrower that repaid on the tick its loan fell due would be racing the
    /// mechanism's own default sweep, and which of the two went first would be a fact about the
    /// engine's tick order rather than about the borrower.
    pub notice: u64,
    /// One tick in this many, on average, is one it acts on. Never zero.
    pub cadence: u64,
    /// How many loans it settles before it stops repaying, where it ever stops.
    ///
    /// `None` is somebody who always pays. `Some(0)` is somebody who never does — an ordinary bad
    /// borrower, and the reason a guarantor's stake is at genuine risk. `Some(k)` for a larger `k`
    /// is the attack: pay for `k` settlements to buy the collateral floor, then draw the most the
    /// floor allows and stop.
    pub defects_after: Option<u32>,
}

impl Member {
    /// Whether it means to repay, given what it has settled so far.
    fn honours(&self, settled: u32) -> bool {
        self.defects_after.is_none_or(|after| settled < after)
    }

    /// The loan of its own that is nearest to falling due and still open.
    ///
    /// Its own, and found through the mechanism's index rather than by scanning: a population that
    /// each walked every loan ever drawn would make one tick cost the square of the run.
    fn coming_due(state: &VouchState, me: AgentId) -> Option<(LoanId, Amount<Cash>)> {
        state
            .drawn_by(me)
            .iter()
            .map(|loan| (*loan, &state.loans[loan.0 as usize]))
            .filter(|(_, held)| held.open)
            .min_by_key(|(_, held)| held.due.0)
            .map(|(loan, held)| (loan, held.principal))
    }

    /// Somebody in its circle it has not already written a guarantee for.
    ///
    /// One guarantee per person, ever. A second for the same person would be one relationship
    /// counted twice, and in a ring it would be a way to manufacture standing out of repetition.
    fn unbacked(&self, state: &VouchState, me: AgentId) -> Option<AgentId> {
        self.circle.iter().copied().find(|&of| {
            of != me
                && !state
                    .written_for(of)
                    .iter()
                    .any(|line| state.lines[line.0 as usize].by == me)
        })
    }
}

impl Agent<Vouch> for Member {
    fn decide(
        &mut self,
        me: AgentId,
        state: &VouchState,
        now: Tick,
        rng: &mut RunRng,
    ) -> Option<VouchAction> {
        let mine = state.members.get(me.0 as usize)?;
        // A participant that has already failed can do nothing here: the mechanism refuses it
        // every action, and an agent that kept asking would spend the run being rejected.
        if mine.failed > 0 {
            return None;
        }

        if let Some((loan, principal)) = Self::coming_due(state, me) {
            if !self.honours(mine.settled) {
                return None;
            }
            let due = state.loans[loan.0 as usize].due;
            if due.0.saturating_sub(now.0) > self.notice {
                return None;
            }
            return (mine.balance >= principal).then_some(VouchAction::Repay { loan });
        }

        // Everything below is occasional. A population that acted every tick would say more about
        // how long a run is than about what its rules do.
        if rng.below(self.cadence.max(1)) != 0 {
            return None;
        }

        if let Some(of) = self.unbacked(state, me)
            && mine.balance >= self.stake.checked_add(self.reserve).ok()?
        {
            return Some(VouchAction::Write {
                of,
                stake: self.stake,
            });
        }

        // The most that stands behind it, and no more than the fund can hand over. Asking for more
        // than either is an action the mechanism refuses, and a population that spent a run being
        // refused would report rejection pressure of its own making.
        let headroom = self.terms.headroom(state, me, now).ok()?;
        let amount = headroom.min(state.fund);
        (!amount.is_zero() && mine.balance >= self.reserve).then_some(VouchAction::Draw {
            amount,
            term: self.term,
        })
    }
}
