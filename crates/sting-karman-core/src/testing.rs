//! Reference mechanisms and stub agents for exercising code that is generic over the ports.
//!
//! The core cannot drive its engine against a real mechanism without depending on one, which the
//! layering forbids — and the same problem meets every crate above the ports that owns no
//! mechanism knowledge. These are deliberately trivial: an assertion about the tick loop is then
//! about the loop, and not about the mechanism underneath it.
//!
//! Behind the `testing` feature, because it is scaffolding for tests and never part of the
//! simulation surface.

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicI64, Ordering},
};

use sting_karman_types::{AgentId, Tick};

use crate::{
    engine::{
        probe::{Phase, Probe},
        rng::RunRng,
    },
    ports::{
        Abort, Agent, InvariantViolation, MechError, Mechanism, Observer, PathSource, SourceError,
    },
};

/// The counter's whole state: a running total, how many times the clock advanced it, and who
/// contributed what — the ledger that makes the acting identity observable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CounterState {
    /// Everything added so far, across every account.
    pub total: i64,
    /// How many times the clock advanced, counted apart from actions so a test can tell them
    /// apart.
    pub ticks: u64,
    /// What each agent contributed, which is what makes the acting identity observable.
    pub credited: BTreeMap<AgentId, i64>,
}

/// Move the total. Negative amounts are legal; driving the total below zero is not.
///
/// The action carries no identity: whoever acted is the engine's knowledge, and a mechanism that
/// read it from here would be trusting the actor's claim about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Add(pub i64);

/// What the counter reports: the amount that landed, and whose account it landed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Added {
    /// Whose account it landed in, as the engine named them.
    pub actor: AgentId,
    /// How much landed.
    pub amount: i64,
}

/// A mechanism with one number in it and a ledger beside it. It rejects what would take the
/// total below zero, declares any total above `cap` unreachable, and counts clock advances
/// separately from actions so a test can tell the two apart.
#[derive(Debug, Clone, Copy)]
pub struct Counter {
    /// The total above which the mechanism declares its own state unreachable.
    pub cap: i64,
}

impl Counter {
    /// A counter whose invariant no accepted action can break.
    #[must_use]
    pub fn unbounded() -> Counter {
        Counter { cap: i64::MAX }
    }
}

impl Mechanism for Counter {
    type State = CounterState;
    type Action = Add;
    type Event = Added;

    fn apply(
        &self,
        state: &mut CounterState,
        actor: AgentId,
        Add(delta): Add,
        _now: Tick,
        events: &mut Vec<Added>,
    ) -> Result<(), MechError> {
        let total = state
            .total
            .checked_add(delta)
            .ok_or_else(|| MechError::new("total overflows"))?;
        if total < 0 {
            return Err(MechError::new("total would go negative"));
        }
        state.total = total;
        *state.credited.entry(actor).or_default() += delta;
        events.push(Added {
            actor,
            amount: delta,
        });
        Ok(())
    }

    fn tick(
        &self,
        state: &mut CounterState,
        _now: Tick,
        _events: &mut Vec<Added>,
    ) -> Result<(), Abort> {
        state.ticks += 1;
        Ok(())
    }

    fn check_invariants(&self, state: &CounterState) -> Result<(), InvariantViolation> {
        if state.total > self.cap {
            return Err(InvariantViolation::new("total rose above the cap"));
        }
        Ok(())
    }
}

/// A mechanism with no actions at all: every tick it adds whatever its exogenous path reports.
///
/// The seam where a run consumes something it did not generate, reduced to its smallest form —
/// including the case that matters most, where the path has nothing to say and the run has to
/// stop rather than invent a value.
pub struct Drip {
    path: Box<dyn PathSource<i64>>,
}

impl Drip {
    /// A mechanism that adds whatever this path reports, and stops when it reports nothing.
    #[must_use]
    pub fn fed_by(path: Box<dyn PathSource<i64>>) -> Drip {
        Drip { path }
    }
}

impl std::fmt::Debug for Drip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drip").finish_non_exhaustive()
    }
}

/// A mechanism nobody can act on. Uninhabited, so an agent cannot produce one and the engine's
/// action path is provably unreachable for this mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoAction {}

impl Mechanism for Drip {
    type State = i64;
    type Action = NoAction;
    type Event = i64;

    fn apply(
        &self,
        _state: &mut i64,
        _actor: AgentId,
        action: NoAction,
        _now: Tick,
        _events: &mut Vec<i64>,
    ) -> Result<(), MechError> {
        match action {}
    }

    fn tick(&self, state: &mut i64, now: Tick, events: &mut Vec<i64>) -> Result<(), Abort> {
        let drop = self.path.value_at(now)?;
        *state = state
            .checked_add(drop)
            .ok_or_else(|| InvariantViolation::new("total overflows"))?;
        events.push(drop);
        Ok(())
    }

    fn check_invariants(&self, _state: &i64) -> Result<(), InvariantViolation> {
        Ok(())
    }
}

/// The same value at every tick.
#[derive(Debug, Clone, Copy)]
pub struct ConstantPath(pub i64);

impl PathSource<i64> for ConstantPath {
    fn value_at(&self, _now: Tick) -> Result<i64, SourceError> {
        Ok(self.0)
    }
}

/// A path that runs out: it answers up to `last` and then reports that it has nothing more.
#[derive(Debug, Clone, Copy)]
pub struct ShortPath {
    /// What it answers with, everywhere it answers.
    pub value: i64,
    /// The last tick it can answer for.
    pub last: Tick,
}

impl PathSource<i64> for ShortPath {
    fn value_at(&self, now: Tick) -> Result<i64, SourceError> {
        if now > self.last {
            return Err(SourceError::new(format!("path ends at {:?}", self.last)));
        }
        Ok(self.value)
    }
}

/// Adds the same amount every tick.
#[derive(Debug, Clone, Copy)]
pub struct Adder(pub i64);

impl Agent<Counter> for Adder {
    fn decide(
        &mut self,
        _me: AgentId,
        _s: &CounterState,
        _now: Tick,
        _rng: &mut RunRng,
    ) -> Option<Add> {
        Some(Add(self.0))
    }
}

/// Adds its own identifier, so the sequence of events is a transcript of the acting order.
#[derive(Debug, Clone, Copy)]
pub struct AddsItsOwnId;

impl Agent<Counter> for AddsItsOwnId {
    fn decide(
        &mut self,
        me: AgentId,
        _s: &CounterState,
        _now: Tick,
        _rng: &mut RunRng,
    ) -> Option<Add> {
        Some(Add(i64::from(me.0)))
    }
}

/// Never acts.
#[derive(Debug, Clone, Copy)]
pub struct Idle;

impl Agent<Counter> for Idle {
    fn decide(
        &mut self,
        _me: AgentId,
        _s: &CounterState,
        _now: Tick,
        _rng: &mut RunRng,
    ) -> Option<Add> {
        None
    }
}

/// Draws its amount from the run stream, so a run's outcome depends on its seed and stream.
#[derive(Debug, Clone, Copy)]
pub struct RandomAdder {
    /// Exclusive upper bound on each draw from the run stream.
    pub bound: u64,
}

impl Agent<Counter> for RandomAdder {
    fn decide(
        &mut self,
        _me: AgentId,
        _s: &CounterState,
        _now: Tick,
        rng: &mut RunRng,
    ) -> Option<Add> {
        // The bound is this test agent's own, and a test that set one past `i64::MAX` would be
        // testing the cast rather than the engine.
        #[allow(
            clippy::cast_possible_wrap,
            reason = "the bound is a test's own constant, well inside i64"
        )]
        Some(Add(rng.below(self.bound) as i64))
    }
}

/// Nondeterminism, deliberately injected: the amount comes from a process-global counter rather
/// than the run stream, so a repeat of the same seed continues where the previous run stopped.
/// The determinism harness exists to fail on exactly this.
#[derive(Debug, Clone, Copy)]
pub struct DriftingAdder;

static DRIFT: AtomicI64 = AtomicI64::new(0);

impl Agent<Counter> for DriftingAdder {
    fn decide(
        &mut self,
        _me: AgentId,
        _s: &CounterState,
        _now: Tick,
        _rng: &mut RunRng,
    ) -> Option<Add> {
        Some(Add(DRIFT.fetch_add(1, Ordering::Relaxed) % 8))
    }
}

/// Records everything the engine says, in the order it says it.
#[derive(Debug, Default)]
pub struct RecordingObserver {
    /// Every event, paired with the tick it was announced on, in announcement order.
    pub events: Vec<(Tick, Added)>,
    /// Every refusal: when, who asked, and what the mechanism said.
    pub rejections: Vec<(Tick, AgentId, String)>,
    /// The state as each tick left it.
    pub tick_ends: Vec<(Tick, CounterState)>,
}

impl Observer<Counter> for RecordingObserver {
    fn on_events(&mut self, now: Tick, events: &[Added]) {
        self.events.extend(events.iter().map(|event| (now, *event)));
    }

    fn on_rejected(&mut self, now: Tick, actor: AgentId, reason: &MechError) {
        self.rejections
            .push((now, actor, reason.reason.to_string()));
    }

    fn on_tick_end(&mut self, now: Tick, state: &CounterState) {
        self.tick_ends.push((now, state.clone()));
    }
}

/// Counts phase boundaries, and notices a phase entered while another is still open — the
/// nesting the [`Probe`] contract rules out.
#[derive(Debug, Default)]
pub struct CountingProbe {
    /// Entries per phase, indexed by [`Phase::index`].
    pub entered: [u64; Phase::ALL.len()],
    /// Exits per phase. Any difference from `entered` is a phase the loop never closed.
    pub exited: [u64; Phase::ALL.len()],
    /// Whether a phase was ever entered while another was still open, which the contract rules
    /// out.
    pub nested: bool,
    open: Option<Phase>,
}

impl Probe for CountingProbe {
    fn enter(&mut self, phase: Phase) {
        self.nested |= self.open.is_some();
        self.open = Some(phase);
        self.entered[phase.index()] += 1;
    }

    fn exit(&mut self, phase: Phase) {
        self.open = None;
        self.exited[phase.index()] += 1;
    }
}
