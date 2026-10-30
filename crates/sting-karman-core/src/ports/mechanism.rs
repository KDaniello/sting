use std::borrow::Cow;

use sting_karman_types::{AgentId, ArithError, Tick};
use thiserror::Error;

use crate::ports::SourceError;

/// A rejected action. Rejection is a normal outcome — real venues reject orders — so it is a
/// value, not a panic, and agents may observe it.
///
/// The reason borrows when it is a fixed sentence, which it almost always is: a mechanism under
/// rejection pressure produces one of these per refused action, and copying text already in the
/// binary would be an allocation per refusal.
#[derive(Debug, Error)]
#[error("action rejected: {reason}")]
pub struct MechError {
    /// Why the action was refused, in the words an agent or a report would repeat.
    pub reason: Cow<'static, str>,
}

impl MechError {
    /// Refuses an action, saying why.
    #[must_use]
    pub fn new(reason: impl Into<Cow<'static, str>>) -> Self {
        MechError {
            reason: reason.into(),
        }
    }
}

/// Arithmetic with no representable answer rejects the action that asked for it. The reference
/// venues do the same — an amount that overflows their accounting reverts the call — and the
/// alternative, aborting the run, would blame the mechanism for an input it correctly refused.
impl From<ArithError> for MechError {
    fn from(error: ArithError) -> Self {
        MechError::new(error.to_string())
    }
}

/// A broken invariant. Unlike [`MechError`] this is never a normal outcome: it means the
/// mechanism implementation itself is wrong, and the engine aborts the run loudly.
#[derive(Debug, Error)]
#[error("invariant violated: {invariant}")]
pub struct InvariantViolation {
    /// Which promise the state broke, named as the mechanism states it.
    pub invariant: Cow<'static, str>,
}

impl InvariantViolation {
    /// Declares a state the mechanism said could not be reached.
    #[must_use]
    pub fn new(invariant: impl Into<Cow<'static, str>>) -> Self {
        InvariantViolation {
            invariant: invariant.into(),
        }
    }
}

/// Arithmetic that fails with no actor to refuse is the mechanism claiming a state it cannot
/// represent. Accrual is where this happens: nobody asked for it, and skipping it would leave a
/// state that looks valid and is wrong.
impl From<ArithError> for InvariantViolation {
    fn from(error: ArithError) -> Self {
        InvariantViolation::new(format!("state is not representable: {error}"))
    }
}

/// A run that cannot honestly continue.
///
/// Two ways that happens, and they are diagnosed differently: the mechanism reached a state it
/// declares impossible, or an input the run needed could not be supplied. Neither is a rejection
/// — no action is being refused — so neither leaves the run a way forward, and the alternative
/// to stopping is a number computed from a world that did not happen.
#[derive(Debug, Error)]
pub enum Abort {
    /// The mechanism is wrong about itself: look at the mechanism crate.
    #[error(transparent)]
    Invariant(#[from] InvariantViolation),
    /// The mechanism was not given what it needed: look at the scenario and its fixtures.
    #[error(transparent)]
    Source(#[from] SourceError),
}

impl From<ArithError> for Abort {
    fn from(error: ArithError) -> Self {
        Abort::Invariant(error.into())
    }
}

/// The central port: a crypto-economic mechanism as a deterministic state machine.
///
/// A mechanism owns its state shape, its action vocabulary and its event vocabulary; the engine
/// is generic over all three, which is what keeps the core free of `if lending`-style special
/// cases. Determinism is part of the contract: two calls with equal inputs produce equal outputs
/// on any platform — no randomness, no clocks, no floats in the transition.
///
/// Whatever a mechanism consumes but does not generate — an oracle price, a rate, an external
/// feed — it holds as a [`crate::ports::PathSource`] and reads where it needs it. The engine
/// never learns what those inputs are, which is what lets a replayed recording and a synthetic
/// stress path be swapped at a composition root without the mechanism noticing.
///
/// Events are pushed into a buffer the engine owns and reuses for the whole run. Returning a
/// fresh collection would put one allocation behind every action a sweep ever applies, and a
/// sweep is millions of runs of this loop.
///
/// The contract is enforced, not assumed: every implementation must pass the suite in
/// [`crate::conformance`], and level 0 of the verification ladder (karman/docs/VERIFICATION.md) checks
/// the transition against the reference on-chain implementation where one exists.
pub trait Mechanism {
    /// Full market state at one tick.
    type State;
    /// What an agent can do to the mechanism.
    type Action;
    /// What the mechanism reports happened: a fill, a liquidation, an unlock.
    type Event;

    /// Applies one agent action. `Err` is a rejection; the state must then be unchanged.
    ///
    /// `actor` comes from the engine, which is the only party that knows who is acting. A
    /// mechanism that took the identity from the action would be trusting the actor's own claim
    /// about itself, and one agent could then move another's position.
    ///
    /// `events` arrives empty and is never cleared by the mechanism. Nothing is pushed before
    /// the transition is committed, so a rejection leaves the buffer as it found it — the same
    /// rule that leaves the state untouched.
    ///
    /// # Errors
    /// [`MechError`] when the mechanism refuses the action. A refusal leaves the state and the
    /// event buffer exactly as they were.
    fn apply(
        &self,
        state: &mut Self::State,
        actor: AgentId,
        action: Self::Action,
        now: Tick,
        events: &mut Vec<Self::Event>,
    ) -> Result<(), MechError>;

    /// Time-driven transitions with no actor: interest accrual, funding, vesting unlocks.
    ///
    /// Fallible, because there is nobody here to reject: an accrual that cannot be represented,
    /// or an exogenous input the path cannot supply, leaves no third path. Skipping the
    /// transition would produce a state that looks valid and is wrong.
    ///
    /// `events` follows the same rule as in [`Mechanism::apply`].
    ///
    /// # Errors
    /// [`Abort`] when the transition cannot be made: an accrual with no representation, or an
    /// exogenous input the path cannot supply. There is nobody here to reject, so a failure
    /// stops the run rather than being refused to an actor.
    fn tick(
        &self,
        state: &mut Self::State,
        now: Tick,
        events: &mut Vec<Self::Event>,
    ) -> Result<(), Abort>;

    /// The invariants every reachable state must satisfy; the engine calls this after every
    /// transition when invariant checking is on, and the conformance suite always does.
    ///
    /// Called once per applied action, so what belongs here is what can be established without
    /// walking the state. A mechanism whose state is a handful of numbers may check all of it;
    /// one holding a million items may not, because a check that walks them turns a run into
    /// quadratic work and a simulator nobody can afford to run is a simulator that checks
    /// nothing. What such a mechanism cannot afford here belongs in [`Mechanism::audit`].
    ///
    /// # Errors
    /// [`InvariantViolation`] when the state is one the mechanism declares impossible.
    fn check_invariants(&self, state: &Self::State) -> Result<(), InvariantViolation>;

    /// The full sweep, called once per tick rather than once per action.
    ///
    /// The same contract as [`Mechanism::check_invariants`] and never a weaker one: everything
    /// stated here must hold in every reachable state. What differs is only how often the engine
    /// can afford to ask. A mechanism that overrides this is saying that some of what it
    /// guarantees costs a walk of its state to establish, and that catching a break within the
    /// tick that made it is worth the walk not happening after every action inside that tick.
    ///
    /// It defaults to the per-action check, so a mechanism that says nothing here is checked
    /// exactly as it always was, twice at a tick boundary rather than once.
    ///
    /// # Errors
    ///
    /// Returns the violation that a reachable state must never exhibit.
    fn audit(&self, state: &Self::State) -> Result<(), InvariantViolation> {
        self.check_invariants(state)
    }
}
