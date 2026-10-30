//! The guarantee layer: credit that stands on somebody else's stake.
//!
//! One participant writes a guarantee for another and locks money behind it. The one guaranteed
//! may then draw credit — more than the stake, which is the whole point — and pays a premium for
//! it. Repaying on time buys the borrower a lower collateral requirement next time; failing to
//! repay takes the guarantee's stake and hands it to the fund the credit came from. Nothing here
//! prices an asset and nothing here is secured by one: what stands behind a loan is somebody who
//! was willing to lose money over somebody else.
//!
//! Three properties of this implementation are worth stating where they cannot be missed.
//!
//! **Standing is the only thing that accrues.** A newcomer covers a draw stake for stake. Each
//! settled loan takes a step off what the next one demands, down to a floor. That floor is the
//! parameter the design turns on: it is the most leverage the system will ever extend against a
//! person rather than against money, and it is therefore also the prize an attacker is paying
//! premiums to reach.
//!
//! **A ring is not a special case.** Nothing here knows what a fake participant is. A hundred
//! seats that stand behind one another are a hundred ordinary participants, and whether that pays
//! is arithmetic about the floor, the premium and the venue's share of it — never a rule that
//! names them.
//!
//! **Rounding runs against the mechanism.** Where a direction has to be chosen, the one taken
//! extends *more* credit per unit of stake and collects *less* premium. There is no reference
//! implementation to agree with here, and the conservative direction is the one that refuses to
//! flatter the design's own defences
//! ([`ARCHITECTURE.md`](https://github.com/karman-dev/karman/blob/main/karman/docs/ARCHITECTURE.md) §5
//! invariant 6).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sting_karman_core::{Abort, InvariantViolation, MechError, Mechanism};
use sting_karman_types::{AgentId, Amount, ArithError, Rounding, SecondsPerTick, Tick, Unit};

/// The asset everything here is settled in: what is staked, drawn, repaid and seized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cash;

impl Unit for Cash {
    const SYMBOL: &'static str = "cash";
}

/// A share, in ten-thousandths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bps;

impl Unit for Bps {
    const SYMBOL: &'static str = "bps";
}

/// Elapsed time, in the unit a rate is quoted against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seconds;

impl Unit for Seconds {
    const SYMBOL: &'static str = "s";
}

/// The whole of a share.
pub const WHOLE: Amount<Bps> = Amount::new(10_000);

/// A year, and always this many seconds: a rate needs a denominator that never moves, or two runs
/// of one scenario disagree about what a year was.
pub const SECONDS_PER_YEAR: Amount<Seconds> = Amount::new(365 * 24 * 60 * 60);

/// One written guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LineId(pub u32);

/// One drawn loan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LoanId(pub u32);

/// What a guarantee is worth to the credit that stands on it.
///
/// The two are simulated rather than ranked because the difference between them is the whole
/// question a ring asks. Under the first, money drawn on credit can be staked again and the stake
/// it creates is as good as any other; under the second it is not, because what a guarantor is
/// worth is netted against what it owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backing {
    /// A guarantee is worth the money locked behind it.
    Stake,
    /// A guarantee is worth the money locked behind it, scaled down where its writer has staked
    /// more than it is worth net of its own debts. Credit staked again backs nothing.
    NetWorth,
}

/// What one participant holds and what it has done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Member {
    /// What it can spend.
    pub balance: Amount<Cash>,
    /// What it has locked behind guarantees it wrote.
    pub staked: Amount<Cash>,
    /// Principal drawn and not yet repaid.
    pub owed: Amount<Cash>,
    /// Loans repaid on time. The only quantity here that buys anything.
    pub settled: u32,
    /// Loans that came due unpaid. One is enough to end a participant's credit.
    pub failed: u32,
    /// Premium taken for standing behind somebody.
    pub earned: Amount<Cash>,
    /// Premium paid for credit.
    pub premium_paid: Amount<Cash>,
    /// Stake lost because somebody it stood behind did not pay.
    pub seized: Amount<Cash>,
}

impl Member {
    /// A participant opening with this much and no history.
    #[must_use]
    pub fn opening(balance: Amount<Cash>) -> Self {
        Member {
            balance,
            ..Member::default()
        }
    }

    /// What it is worth: what it holds, free and locked, less what it owes.
    ///
    /// Zero rather than a negative: a participant that owes more than it holds is worth nothing to
    /// stand behind anybody, and there is no weaker statement to make about it.
    #[must_use]
    pub fn net_worth(&self) -> Amount<Cash> {
        self.balance
            .checked_add(self.staked)
            .and_then(|held| held.checked_sub(self.owed))
            .unwrap_or(Amount::ZERO)
    }
}

/// One guarantee: who wrote it, for whom, and what stands behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// Who is liable.
    pub by: AgentId,
    /// Who may draw against it.
    pub of: AgentId,
    /// What is locked behind it and can be taken.
    pub stake: Amount<Cash>,
    /// How much of that stake stands behind loans that are still open.
    pub committed: Amount<Cash>,
    /// When it was written, which is what a seasoning period is counted from.
    pub written: Tick,
    /// When a withdrawal of it binds, where one has been asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub releasing: Option<Tick>,
    /// Whether it still stands.
    pub live: bool,
}

/// One drawn loan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loan {
    /// Who drew it.
    pub to: AgentId,
    /// What was drawn, and what is owed back.
    pub principal: Amount<Cash>,
    /// The tick it falls due on.
    pub due: Tick,
    /// The guarantees standing behind it and how much of each, in the order they were taken.
    pub backing: Vec<(LineId, Amount<Cash>)>,
    /// Whether it is still outstanding.
    pub open: bool,
}

/// The whole state of one guarantee layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VouchState {
    /// Every participant, by agent identifier. The venue has one of these like anybody.
    pub members: Vec<Member>,
    /// Every guarantee ever written, live or not, by [`LineId`].
    pub lines: Vec<Line>,
    /// Every loan ever drawn, open or not, by [`LoanId`].
    pub loans: Vec<Loan>,
    /// Which guarantees stand for a participant, by agent identifier.
    ///
    /// Kept beside the lines rather than found by walking them: a draw consults every guarantee
    /// written for one participant, and a run makes many draws, so walking every guarantee ever
    /// written would make one draw cost the history of the world.
    written_for: Vec<Vec<LineId>>,
    /// Which loans a participant drew, by agent identifier.
    ///
    /// Kept for the same reason as `written_for`, and for a second one: a participant deciding
    /// whether to repay has to find its own loans, and a population that each scanned every loan
    /// ever drawn would make one tick cost the square of the run.
    drawn_by: Vec<Vec<LoanId>>,
    /// Which loans fall due at which tick.
    ///
    /// An ordered map, and for the workspace's first rule rather than for speed alone: a tick that
    /// walked every open loan would cost that history over again, and a hash map's iteration order
    /// is not something a deterministic result may depend on.
    due: BTreeMap<u64, Vec<LoanId>>,
    /// What credit is drawn from and repaid into.
    pub fund: Amount<Cash>,
    /// What execution has consumed. It leaves the system and does not come back.
    pub spent: Amount<Cash>,
    /// Principal that defaulted and no stake covered. A number and not cash: the loss is the
    /// fund's, and this says what the fund is short by.
    pub written_off: Amount<Cash>,
    /// Principal drawn over the whole run, whether or not it came back.
    pub drawn: Amount<Cash>,
    /// What the world opened with, so that conservation is checkable rather than asserted.
    opened_with: Amount<Cash>,
    /// What the members hold between them, free and locked, kept as it moves rather than counted
    /// when asked. Conservation is checked after every applied action, and a population of
    /// thousands counted there would make a run quadratic in its own size.
    held: Amount<Cash>,
}

impl VouchState {
    /// Opens a layer over a population, with a fund to lend out of.
    ///
    /// # Errors
    ///
    /// Fails when the opening balances and the fund together are wider than money is carried in.
    pub fn open(members: Vec<Member>, fund: Amount<Cash>) -> Result<Self, MechError> {
        let mut held = Amount::ZERO;
        for member in &members {
            held = held
                .checked_add(member.balance)?
                .checked_add(member.staked)?;
        }
        let seats = members.len();
        Ok(VouchState {
            members,
            lines: Vec::new(),
            loans: Vec::new(),
            written_for: vec![Vec::new(); seats],
            drawn_by: vec![Vec::new(); seats],
            due: BTreeMap::new(),
            fund,
            spent: Amount::ZERO,
            written_off: Amount::ZERO,
            drawn: Amount::ZERO,
            opened_with: held.checked_add(fund)?,
            held,
        })
    }

    /// What the world opened with: every member and the fund, before anything happened.
    #[must_use]
    pub const fn opened_with(&self) -> Amount<Cash> {
        self.opened_with
    }

    /// The guarantees written for one participant, in the order they were written.
    #[must_use]
    pub fn written_for(&self, who: AgentId) -> &[LineId] {
        self.written_for
            .get(who.0 as usize)
            .map_or(&[], Vec::as_slice)
    }

    /// The loans one participant drew, in the order it drew them.
    #[must_use]
    pub fn drawn_by(&self, who: AgentId) -> &[LoanId] {
        self.drawn_by.get(who.0 as usize).map_or(&[], Vec::as_slice)
    }

    /// Moves cash into one member, keeping the running total in step.
    fn credit(&mut self, who: usize, amount: Amount<Cash>) -> Result<(), ArithError> {
        self.members[who].balance = self.members[who].balance.checked_add(amount)?;
        self.held = self.held.checked_add(amount)?;
        Ok(())
    }

    /// Moves cash out of one member, keeping the running total in step.
    fn debit(&mut self, who: usize, amount: Amount<Cash>) -> Result<(), ArithError> {
        self.members[who].balance = self.members[who].balance.checked_sub(amount)?;
        self.held = self.held.checked_sub(amount)?;
        Ok(())
    }

    /// Locks cash behind a guarantee. The member still holds it, so the total does not move.
    fn lock(&mut self, who: usize, amount: Amount<Cash>) -> Result<(), ArithError> {
        self.members[who].balance = self.members[who].balance.checked_sub(amount)?;
        self.members[who].staked = self.members[who].staked.checked_add(amount)?;
        Ok(())
    }

    /// Frees cash from behind a guarantee that no longer needs it.
    fn unlock(&mut self, who: usize, amount: Amount<Cash>) -> Result<(), ArithError> {
        self.members[who].staked = self.members[who].staked.checked_sub(amount)?;
        self.members[who].balance = self.members[who].balance.checked_add(amount)?;
        Ok(())
    }

    /// Takes locked cash away from a member for good.
    fn seize(&mut self, who: usize, amount: Amount<Cash>) -> Result<(), ArithError> {
        self.members[who].staked = self.members[who].staked.checked_sub(amount)?;
        self.members[who].seized = self.members[who].seized.checked_add(amount)?;
        self.held = self.held.checked_sub(amount)?;
        Ok(())
    }
}

/// What a participant can do to a guarantee layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VouchAction {
    /// Stand behind somebody, locking money that can be taken if they do not pay.
    Write {
        /// Who may draw against it.
        of: AgentId,
        /// What to lock behind it.
        stake: Amount<Cash>,
    },
    /// Take back a guarantee, after the lag and only as far as nothing stands on it.
    Release {
        /// Which guarantee.
        line: LineId,
    },
    /// Draw credit against the guarantees standing for you.
    Draw {
        /// How much.
        amount: Amount<Cash>,
        /// How many ticks until it falls due.
        term: u64,
    },
    /// Repay a loan, which releases what stood behind it.
    Repay {
        /// Which loan.
        loan: LoanId,
    },
}

/// What the guarantee layer reports happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VouchEvent {
    /// A guarantee was written.
    Written {
        /// Which guarantee.
        line: LineId,
        /// Who is liable.
        by: AgentId,
        /// Who may draw against it.
        of: AgentId,
        /// What was locked.
        stake: Amount<Cash>,
    },
    /// A withdrawal was asked for, and binds at `effective`.
    Releasing {
        /// Which guarantee.
        line: LineId,
        /// When it binds.
        effective: Tick,
    },
    /// A guarantee ended and what nothing stood on came back.
    Released {
        /// Which guarantee.
        line: LineId,
        /// Who wrote it.
        by: AgentId,
        /// What came back.
        stake: Amount<Cash>,
    },
    /// Credit was drawn.
    Drawn {
        /// Which loan.
        loan: LoanId,
        /// Who drew it.
        by: AgentId,
        /// How much.
        principal: Amount<Cash>,
        /// What it cost up front.
        premium: Amount<Cash>,
        /// What share of the principal had to stand behind it, in basis points.
        collateral: Amount<Bps>,
        /// When it falls due.
        due: Tick,
    },
    /// A loan was repaid before it fell due.
    Repaid {
        /// Which loan.
        loan: LoanId,
        /// Who repaid it.
        by: AgentId,
        /// How much came back.
        principal: Amount<Cash>,
    },
    /// A loan fell due unpaid, and what stood behind it was taken.
    Defaulted {
        /// Which loan.
        loan: LoanId,
        /// Who did not pay.
        by: AgentId,
        /// What was owed.
        principal: Amount<Cash>,
        /// What the guarantees covered.
        seized: Amount<Cash>,
        /// What nothing covered.
        written_off: Amount<Cash>,
    },
}

/// The guarantee layer: every parameter of one run, and no mutable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vouch {
    /// The participant that takes the venue's share of every premium. It never acts.
    pub venue: AgentId,
    /// What one tick is worth, which is what turns an annual premium into one.
    pub seconds_per_tick: SecondsPerTick,
    /// What share of a draw a participant with no history must have standing behind it, in basis
    /// points. Ten thousand is stake for stake: credit that buys nothing money would not have.
    pub opening: Amount<Bps>,
    /// What each settled loan takes off that share, in basis points.
    pub step: Amount<Bps>,
    /// The least that share may ever fall to, in basis points.
    ///
    /// **The parameter the design turns on**, and the one a sweep of this mechanism varies.
    pub floor: Amount<Bps>,
    /// What a guarantee counts for.
    pub backing: Backing,
    /// The annual premium on drawn principal, in basis points.
    pub premium: Amount<Bps>,
    /// What share of that premium the venue keeps; the rest reaches the guarantors.
    ///
    /// It matters more than its size suggests. The guarantors' share of a premium paid by a
    /// participant that its own confederates stand behind never leaves the group that paid it; the
    /// venue's share does. This is therefore the only part of a premium that costs a ring
    /// anything at all.
    pub venue_share: Amount<Bps>,
    /// How many ticks a guarantee must stand before it backs anything.
    pub seasoning: u64,
    /// How many ticks a withdrawal waits before it binds.
    pub release_lag: u64,
    /// What one action costs whoever takes it. It leaves the system.
    pub execution: Amount<Cash>,
}

/// What a default took, and what it could not.
struct Recovered {
    seized: Amount<Cash>,
    short: Amount<Cash>,
}

impl Vouch {
    /// What share of a draw must stand behind it, for a borrower with this much history.
    ///
    /// A step subtracted rather than a curve, because what is being modelled is a schedule a venue
    /// could publish: settle one, and the next asks for this much less. A curve would put a second
    /// fitted thing between the floor and the result the floor is here to produce.
    #[must_use]
    pub fn collateral(&self, settled: u32) -> Amount<Bps> {
        let earned = self.step.base_units().saturating_mul(u128::from(settled));
        let asked = self.opening.base_units().saturating_sub(earned);
        Amount::new(asked.max(self.floor.base_units()))
    }

    /// What one guarantee is worth to the credit standing on it.
    ///
    /// Under [`Backing::NetWorth`] a guarantor that has staked more than it is worth has every
    /// guarantee it wrote scaled down in the same proportion, so that what it stands behind can
    /// never exceed what it would actually lose. Scaling all of them rather than exhausting them
    /// in order is what keeps the answer independent of which guarantee is being asked about, and
    /// therefore independent of the order draws happen to arrive in.
    fn worth(&self, state: &VouchState, line: &Line) -> Result<Amount<Cash>, MechError> {
        match self.backing {
            Backing::Stake => Ok(line.stake),
            Backing::NetWorth => {
                let guarantor = &state.members[line.by.0 as usize];
                if guarantor.staked.is_zero() {
                    return Ok(Amount::ZERO);
                }
                let net = guarantor.net_worth();
                if net >= guarantor.staked {
                    return Ok(line.stake);
                }
                Ok(line.stake.mul_div(net, guarantor.staked, Rounding::Down)?)
            }
        }
    }

    /// How much of a guarantee is free to stand behind a new draw.
    fn free(&self, state: &VouchState, line: &Line, now: Tick) -> Result<Amount<Cash>, MechError> {
        if !line.live || line.releasing.is_some() {
            return Ok(Amount::ZERO);
        }
        if now.0 < line.written.0.saturating_add(self.seasoning) {
            return Ok(Amount::ZERO);
        }
        // A guarantor that has defaulted stands behind nothing further. Its stake is still there
        // to be taken for the loans it already backs; what ends is its ability to back another.
        if state.members[line.by.0 as usize].failed > 0 {
            return Ok(Amount::ZERO);
        }
        Ok(self
            .worth(state, line)?
            .checked_sub(line.committed)
            .unwrap_or(Amount::ZERO))
    }

    /// The premium on a principal held for a term.
    ///
    /// Rounds down, against the venue, for the reason the module header gives: a premium rounded
    /// up would collect dust the design may not collect in practice, and it would do it exactly
    /// where it flatters the design most — on the small, frequent draws a ring makes while it is
    /// buying its way down to the floor.
    fn premium_on(&self, principal: Amount<Cash>, term: u64) -> Result<Amount<Cash>, MechError> {
        let seconds = Amount::<Seconds>::new(
            u128::from(term).saturating_mul(u128::from(self.seconds_per_tick.0.get())),
        );
        let a_year = principal.mul_div(self.premium, WHOLE, Rounding::Down)?;
        Ok(a_year.mul_div(seconds, SECONDS_PER_YEAR, Rounding::Down)?)
    }

    /// Takes the cost of acting out of a member's balance.
    fn charge_execution(&self, state: &mut VouchState, actor: AgentId) -> Result<(), MechError> {
        if self.execution.is_zero() {
            return Ok(());
        }
        state.debit(actor.0 as usize, self.execution)?;
        state.spent = state.spent.checked_add(self.execution)?;
        Ok(())
    }

    /// Takes what stood behind a loan and hands it to the fund.
    ///
    /// The stake leaves its writer for good: it is the price of having been wrong about somebody,
    /// and it is the only thing here that makes a guarantee mean anything.
    fn recover(state: &mut VouchState, loan: LoanId) -> Result<Recovered, MechError> {
        let backing = std::mem::take(&mut state.loans[loan.0 as usize].backing);
        let principal = state.loans[loan.0 as usize].principal;
        let mut seized = Amount::ZERO;
        for &(line, committed) in &backing {
            let at = line.0 as usize;
            let take = committed.min(state.lines[at].stake);
            let by = state.lines[at].by;
            state.lines[at].stake = state.lines[at].stake.checked_sub(take)?;
            state.lines[at].committed = state.lines[at].committed.checked_sub(committed)?;
            state.seize(by.0 as usize, take)?;
            state.fund = state.fund.checked_add(take)?;
            seized = seized.checked_add(take)?;
            // A guarantee with nothing left behind it stands for nothing, and is not left in the
            // way of the ones that still do.
            if state.lines[at].stake.is_zero() {
                state.lines[at].live = false;
            }
        }
        state.loans[loan.0 as usize].backing = backing;
        Ok(Recovered {
            seized,
            short: principal.checked_sub(seized).unwrap_or(Amount::ZERO),
        })
    }

    /// Releases what a repaid loan had standing behind it.
    fn uncommit(state: &mut VouchState, loan: LoanId) -> Result<(), MechError> {
        let backing = std::mem::take(&mut state.loans[loan.0 as usize].backing);
        for &(line, committed) in &backing {
            let at = line.0 as usize;
            state.lines[at].committed = state.lines[at].committed.checked_sub(committed)?;
        }
        state.loans[loan.0 as usize].backing = backing;
        Ok(())
    }

    /// The most a participant could draw right now, given what stands behind it.
    ///
    /// A quote and not a transition: a population deciding what to ask for needs the same answer
    /// the mechanism would give, and a second implementation of it inside the agents would be a
    /// second answer waiting to disagree with this one.
    ///
    /// # Errors
    ///
    /// Fails when what stands behind the participant is wider than money is carried in.
    pub fn headroom(
        &self,
        state: &VouchState,
        who: AgentId,
        now: Tick,
    ) -> Result<Amount<Cash>, MechError> {
        let at = who.0 as usize;
        let Some(member) = state.members.get(at) else {
            return Ok(Amount::ZERO);
        };
        if member.failed > 0 {
            return Ok(Amount::ZERO);
        }
        let mut standing = Amount::ZERO;
        for line in &state.written_for[at] {
            let held = state.lines[line.0 as usize];
            standing = standing.checked_add(self.free(state, &held, now)?)?;
        }
        let collateral = self.collateral(member.settled);
        if collateral.is_zero() {
            return Ok(Amount::ZERO);
        }
        Ok(standing.mul_div(WHOLE, collateral, Rounding::Down)?)
    }

    /// The guarantees a draw of this size would stand on, and how much of each it would take.
    ///
    /// Reads and does not write, so a draw it cannot cover is refused with nothing moved. The
    /// guarantees are taken in the order they were written, which makes which stake covers which
    /// loan a fact about the run rather than about how a population was built.
    fn assemble(
        &self,
        state: &VouchState,
        who: usize,
        need: Amount<Cash>,
        now: Tick,
    ) -> Result<Vec<(LineId, Amount<Cash>)>, MechError> {
        let mut taken = Vec::new();
        let mut left = need;
        for at in 0..state.written_for[who].len() {
            if left.is_zero() {
                break;
            }
            let line = state.written_for[who][at];
            let held = state.lines[line.0 as usize];
            let free = self.free(state, &held, now)?;
            if free.is_zero() {
                continue;
            }
            let take = free.min(left);
            taken.push((line, take));
            left = left.checked_sub(take)?;
        }
        if left.is_zero() {
            Ok(taken)
        } else {
            Err(MechError::new("not enough stands behind this participant"))
        }
    }
}

impl Vouch {
    /// A draw: the terms a member must meet to borrow, and what standing behind them costs.
    ///
    /// The longest of the four actions and the only one that both reads a member's history and
    /// commits somebody else's stake — which is why it is here rather than inside the match that
    /// dispatches to it.
    #[allow(
        clippy::too_many_arguments,
        reason = "an action's own parameters, the actor, the tick and the event buffer: bundling them into a struct would name the call site's arguments twice"
    )]
    fn draw(
        &self,
        state: &mut VouchState,
        actor: AgentId,
        amount: Amount<Cash>,
        term: u64,
        now: Tick,
        events: &mut Vec<VouchEvent>,
    ) -> Result<(), MechError> {
        // The caller checked that the actor is a participant and not the venue; this is the same
        // index it checked.
        let who = actor.0 as usize;
        if amount.is_zero() {
            return Err(MechError::new("a draw of nothing is not a draw"));
        }
        if term == 0 {
            return Err(MechError::new("a loan due the tick it is drawn is not one"));
        }
        if state.members[who].failed > 0 {
            return Err(MechError::new("one who did not pay draws nothing"));
        }
        if state.fund < amount {
            return Err(MechError::new("the fund does not hold that much"));
        }
        let loan = LoanId(
            u32::try_from(state.loans.len())
                .map_err(|_| MechError::new("more loans than can be named"))?,
        );
        let collateral = self.collateral(state.members[who].settled);
        // Rounds down: less stake locked per unit drawn is more credit extended per unit
        // staked, which is the direction that does not flatter the mechanism.
        let need = amount.mul_div(collateral, WHOLE, Rounding::Down)?;
        if need.is_zero() {
            return Err(MechError::new("a draw needs something standing behind it"));
        }
        let taken = self.assemble(state, who, need, now)?;
        let premium = self.premium_on(amount, term)?;
        if state.members[who].balance < self.execution {
            return Err(MechError::new("cannot afford to act"));
        }
        // The premium comes out of the principal, which the borrower is about to hold. A
        // mechanism that asked for it beforehand would extend credit only to those who
        // already had money, which is the condition this layer exists to relax.
        let after = state.members[who]
            .balance
            .checked_sub(self.execution)?
            .checked_add(amount)?;
        if after < premium {
            return Err(MechError::new("cannot afford the premium"));
        }

        self.charge_execution(state, actor)?;
        state.fund = state.fund.checked_sub(amount)?;
        state.credit(who, amount)?;
        state.debit(who, premium)?;
        state.members[who].premium_paid = state.members[who].premium_paid.checked_add(premium)?;

        let to_venue = premium.mul_div(self.venue_share, WHOLE, Rounding::Down)?;
        state.credit(self.venue.0 as usize, to_venue)?;
        // Shared between the guarantors in proportion to what each stands behind, and what
        // the division cannot place goes to the venue rather than being invented.
        let to_guarantors = premium.checked_sub(to_venue)?;
        let mut handed = Amount::ZERO;
        for &(line, committed) in &taken {
            let by = state.lines[line.0 as usize].by;
            let share = to_guarantors.mul_div(committed, need, Rounding::Down)?;
            state.credit(by.0 as usize, share)?;
            state.members[by.0 as usize].earned =
                state.members[by.0 as usize].earned.checked_add(share)?;
            handed = handed.checked_add(share)?;
            state.lines[line.0 as usize].committed = state.lines[line.0 as usize]
                .committed
                .checked_add(committed)?;
        }
        state.credit(self.venue.0 as usize, to_guarantors.checked_sub(handed)?)?;

        state.members[who].owed = state.members[who].owed.checked_add(amount)?;
        state.drawn = state.drawn.checked_add(amount)?;
        let due = Tick(now.0.saturating_add(term));
        state.loans.push(Loan {
            to: actor,
            principal: amount,
            due,
            backing: taken,
            open: true,
        });
        state.drawn_by[who].push(loan);
        state.due.entry(due.0).or_default().push(loan);
        events.push(VouchEvent::Drawn {
            loan,
            by: actor,
            principal: amount,
            premium,
            collateral,
            due,
        });
        Ok(())
    }
}

impl Mechanism for Vouch {
    type State = VouchState;
    type Action = VouchAction;
    type Event = VouchEvent;

    #[allow(
        clippy::too_many_lines,
        reason = "one arm per action: the length is the action enum's, and the arms that are long are the ones split out already"
    )]
    fn apply(
        &self,
        state: &mut VouchState,
        actor: AgentId,
        action: VouchAction,
        now: Tick,
        events: &mut Vec<VouchEvent>,
    ) -> Result<(), MechError> {
        let who = actor.0 as usize;
        if who >= state.members.len() {
            return Err(MechError::new("no such participant"));
        }
        if actor == self.venue {
            return Err(MechError::new("the venue does not act"));
        }
        // Every branch below establishes what it needs before it moves anything, so that a refusal
        // leaves the state exactly as it found it — the rule that makes rejection a normal outcome
        // rather than a partial write.
        match action {
            VouchAction::Write { of, stake } => {
                if of == actor {
                    return Err(MechError::new("nobody stands behind themselves"));
                }
                if of.0 as usize >= state.members.len() {
                    return Err(MechError::new("no such participant to stand behind"));
                }
                if stake.is_zero() {
                    return Err(MechError::new(
                        "a guarantee with nothing behind it is not one",
                    ));
                }
                if state.members[who].failed > 0 {
                    return Err(MechError::new("one who did not pay stands behind nobody"));
                }
                let line = LineId(
                    u32::try_from(state.lines.len())
                        .map_err(|_| MechError::new("more guarantees than can be named"))?,
                );
                let outlay = stake.checked_add(self.execution)?;
                if state.members[who].balance < outlay {
                    return Err(MechError::new("cannot afford to stake that much"));
                }
                self.charge_execution(state, actor)?;
                state.lock(who, stake)?;
                state.lines.push(Line {
                    by: actor,
                    of,
                    stake,
                    committed: Amount::ZERO,
                    written: now,
                    releasing: None,
                    live: true,
                });
                state.written_for[of.0 as usize].push(line);
                events.push(VouchEvent::Written {
                    line,
                    by: actor,
                    of,
                    stake,
                });
                Ok(())
            }
            VouchAction::Release { line } => {
                let held = *state
                    .lines
                    .get(line.0 as usize)
                    .ok_or_else(|| MechError::new("no such guarantee"))?;
                if held.by != actor {
                    return Err(MechError::new("only the writer may take back a guarantee"));
                }
                if !held.live {
                    return Err(MechError::new("this guarantee no longer stands"));
                }
                if held.releasing.is_some() {
                    return Err(MechError::new("this guarantee is already being taken back"));
                }
                if state.members[who].balance < self.execution {
                    return Err(MechError::new("cannot afford to act"));
                }
                self.charge_execution(state, actor)?;
                // A withdrawal always waits. Without the lag a guarantor could watch a borrower
                // approach a due date it will not meet and step out of the way, which is the same
                // as there having been no guarantee at all.
                let effective = Tick(now.0.saturating_add(self.release_lag));
                state.lines[line.0 as usize].releasing = Some(effective);
                events.push(VouchEvent::Releasing { line, effective });
                Ok(())
            }
            VouchAction::Draw { amount, term } => {
                self.draw(state, actor, amount, term, now, events)
            }
            VouchAction::Repay { loan } => {
                let held = state
                    .loans
                    .get(loan.0 as usize)
                    .ok_or_else(|| MechError::new("no such loan"))?;
                if held.to != actor {
                    return Err(MechError::new("only the borrower repays"));
                }
                if !held.open {
                    return Err(MechError::new("this loan is already closed"));
                }
                let principal = held.principal;
                let outlay = principal.checked_add(self.execution)?;
                if state.members[who].balance < outlay {
                    return Err(MechError::new("cannot afford to repay"));
                }
                self.charge_execution(state, actor)?;
                state.debit(who, principal)?;
                state.fund = state.fund.checked_add(principal)?;
                state.members[who].owed = state.members[who].owed.checked_sub(principal)?;
                state.members[who].settled = state.members[who].settled.saturating_add(1);
                state.loans[loan.0 as usize].open = false;
                Self::uncommit(state, loan)?;
                events.push(VouchEvent::Repaid {
                    loan,
                    by: actor,
                    principal,
                });
                Ok(())
            }
        }
    }

    fn tick(
        &self,
        state: &mut VouchState,
        now: Tick,
        events: &mut Vec<VouchEvent>,
    ) -> Result<(), Abort> {
        // Defaults before releases, and the order is a rule rather than an accident: a withdrawal
        // maturing on the tick a loan it stands behind falls due must not be the way out of paying
        // for it.
        if let Some(falling) = state.due.remove(&now.0) {
            for loan in falling {
                let at = loan.0 as usize;
                if !state.loans[at].open {
                    continue;
                }
                let borrower = state.loans[at].to;
                let principal = state.loans[at].principal;
                state.loans[at].open = false;
                let recovered = Self::recover(state, loan)
                    .map_err(|refused| InvariantViolation::new(refused.reason))?;
                let who = borrower.0 as usize;
                state.members[who].owed = state.members[who].owed.checked_sub(principal)?;
                state.members[who].failed = state.members[who].failed.saturating_add(1);
                state.written_off = state.written_off.checked_add(recovered.short)?;
                events.push(VouchEvent::Defaulted {
                    loan,
                    by: borrower,
                    principal,
                    seized: recovered.seized,
                    written_off: recovered.short,
                });
            }
        }

        for at in 0..state.lines.len() {
            let line = state.lines[at];
            if !line.live || line.releasing.is_none_or(|effective| effective > now) {
                continue;
            }
            // Only what nothing stands on comes back. A guarantee behind a live loan keeps
            // standing behind it, however long ago its writer asked to be let out.
            let back = line
                .stake
                .checked_sub(line.committed)
                .unwrap_or(Amount::ZERO);
            if back.is_zero() {
                continue;
            }
            state.lines[at].stake = line.stake.checked_sub(back)?;
            state.unlock(line.by.0 as usize, back)?;
            if state.lines[at].stake.is_zero() {
                state.lines[at].live = false;
                state.lines[at].releasing = None;
            }
            events.push(VouchEvent::Released {
                // `at` indexes `state.lines`, which a `LineId` addresses: a layer with more lines
                // than a `u32` counts is one no event could name.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a LineId indexes this very vector, so the index fits by construction"
                )]
                line: LineId(at as u32),
                by: line.by,
                stake: back,
            });
        }
        Ok(())
    }

    fn check_invariants(&self, state: &VouchState) -> Result<(), InvariantViolation> {
        // Cash is conserved: what the members hold between them, free and locked, plus the fund,
        // plus what execution consumed, is what the world opened with. Debt is not cash and does
        // not appear — a draw moves money out of the fund and a repayment moves it back, and a
        // default moves stake rather than principal.
        let total = state
            .held
            .checked_add(state.fund)?
            .checked_add(state.spent)?;
        if total != state.opened_with {
            return Err(InvariantViolation::new(
                "cash is not conserved across the members, the fund and what execution took",
            ));
        }
        Ok(())
    }

    fn audit(&self, state: &VouchState) -> Result<(), InvariantViolation> {
        self.check_invariants(state)?;

        // What every member says it has locked is what the guarantees it wrote say it has locked.
        let mut staked = vec![Amount::<Cash>::ZERO; state.members.len()];
        for line in &state.lines {
            if line.committed > line.stake {
                return Err(InvariantViolation::new(
                    "a guarantee stands behind more than is locked in it",
                ));
            }
            let at = line.by.0 as usize;
            staked[at] = staked[at].checked_add(line.stake)?;
        }

        // What every member owes is the principal of its open loans, and nothing else.
        let mut owed = vec![Amount::<Cash>::ZERO; state.members.len()];
        for loan in &state.loans {
            if loan.open {
                let at = loan.to.0 as usize;
                owed[at] = owed[at].checked_add(loan.principal)?;
            }
        }

        let mut counted = Amount::<Cash>::ZERO;
        for (at, member) in state.members.iter().enumerate() {
            if member.staked != staked[at] {
                return Err(InvariantViolation::new(
                    "a member's locked money is not what its guarantees hold",
                ));
            }
            if member.owed != owed[at] {
                return Err(InvariantViolation::new(
                    "a member owes something no open loan accounts for",
                ));
            }
            counted = counted
                .checked_add(member.balance)?
                .checked_add(member.staked)?;
        }
        if counted != state.held {
            return Err(InvariantViolation::new(
                "the running total of what members hold is not what they hold",
            ));
        }
        Ok(())
    }
}
