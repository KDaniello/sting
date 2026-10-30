use sting_karman_types::{AgentId, Tick};
use thiserror::Error;

use crate::{
    engine::{
        probe::{NoProbe, Phase, Probe},
        rng::RunRng,
    },
    ports::{Abort, Agent, InvariantViolation, Mechanism, Observer},
};

/// A run stopped because it could not honestly continue. The tick number is the start of the
/// reproduction, and the cause says whether to look at the mechanism or at its inputs.
#[derive(Debug, Error)]
#[error("run aborted at tick {at:?}: {cause}")]
pub struct RunAborted {
    /// The tick the run stopped on, and the first tick a reproduction has to reach.
    pub at: Tick,
    /// Whether to look at the mechanism or at what was fed to it.
    pub cause: Abort,
}

/// What one run did, counted: the activity a report owes its reader. Rejections are counted,
/// not judged — heavy rejection pressure can be the finding itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStats {
    /// Transitions the mechanism accepted and the invariant guard then cleared.
    pub actions_applied: u64,
    /// Actions the mechanism refused. High against `actions_applied` is a finding, not noise.
    pub actions_rejected: u64,
    /// Events reported across every transition, whether or not an observer kept them.
    pub events_emitted: u64,
}

/// One simulated world: a mechanism, its state, and a population of agents.
///
/// The tick order is fixed and part of the reproducibility contract: agents act in [`AgentId`]
/// order, each action is applied immediately, then the mechanism ticks once. A rejected action
/// changes nothing, is counted in [`RunStats`], and is announced to the observer with its
/// reason.
pub struct World<M: Mechanism> {
    mechanism: M,
    state: M::State,
    agents: Vec<Box<dyn Agent<M>>>,
    check_invariants: bool,
    stats: RunStats,
}

impl<M: Mechanism> std::fmt::Debug for World<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("agents", &self.agents.len())
            .finish_non_exhaustive()
    }
}

impl<M: Mechanism> World<M> {
    /// Builds a world. An agent's position in `agents` is its [`AgentId`] for the whole run.
    ///
    /// # Panics
    ///
    /// Panics when the population does not fit [`AgentId`]'s range: a position without an
    /// identifier would leave the acting order inexpressible.
    #[must_use]
    pub fn new(mechanism: M, state: M::State, agents: Vec<Box<dyn Agent<M>>>) -> Self {
        assert!(
            u32::try_from(agents.len()).is_ok(),
            "population exceeds AgentId's range"
        );
        World {
            mechanism,
            state,
            agents,
            check_invariants: true,
            stats: RunStats::default(),
        }
    }

    /// Disables per-transition invariant checks. Sweeps may buy speed this way once the
    /// mechanism has passed conformance; the conformance suite itself never does.
    #[must_use]
    pub fn without_invariant_checks(mut self) -> Self {
        self.check_invariants = false;
        self
    }

    /// The state as the run left it.
    #[must_use]
    pub fn state(&self) -> &M::State {
        &self.state
    }

    /// What the run did, for a report that has to account for activity and not only for outcome.
    #[must_use]
    pub fn stats(&self) -> RunStats {
        self.stats
    }

    /// Runs `n_ticks` steps. A pure function of the initial state, the agent population, `rng`
    /// and whatever the mechanism reads through its own ports: equal inputs, equal final state,
    /// on any platform.
    ///
    /// # Errors
    /// [`RunAborted`] when a transition leaves a state the mechanism declares impossible, or when
    /// the actor-less half of a tick refuses. A run that stopped is not a shorter run.
    pub fn run(
        &mut self,
        n_ticks: u64,
        rng: &mut RunRng,
        observer: &mut dyn Observer<M>,
    ) -> Result<(), RunAborted> {
        self.run_with_probe(n_ticks, rng, observer, &mut NoProbe)
    }

    /// [`World::run`] with a phase probe attached. The probe only listens: a profiled run and
    /// an unprofiled run of the same inputs produce the same state, stats and events.
    ///
    /// A transition reaches the observer only once the invariant guard has cleared it, so the
    /// events of the transition that aborted a run are never announced as though the run had
    /// continued past them. A refused action reaches the observer immediately, because nothing
    /// about the state moved and there is nothing left to guard.
    ///
    /// # Errors
    /// See [`World::run`]: the probe changes nothing about when a run stops.
    pub fn run_with_probe(
        &mut self,
        n_ticks: u64,
        rng: &mut RunRng,
        observer: &mut dyn Observer<M>,
        probe: &mut impl Probe,
    ) -> Result<(), RunAborted> {
        // One buffer for the whole run: the mechanism reports into it and the observer reads it
        // before the next transition reuses the same capacity.
        let mut events: Vec<M::Event> = Vec::new();
        for t in 0..n_ticks {
            let now = Tick(t);
            // Indexed rather than iterated: the agent borrow must end before the mechanism takes
            // the state mutably, and the position is the agent's identity for the whole run.
            for position in 0..self.agents.len() {
                // An `AgentId` addresses the population, and a population larger than a `u32`
                // counts is one this engine cannot address. Truncating would give two agents one
                // identity, which is the one thing the acting order may never do.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "the population's length is checked against u32::MAX in `new`"
                )]
                let me = AgentId(position as u32);
                probe.enter(Phase::Decide);
                let decision = self.agents[position].decide(me, &self.state, now, rng);
                probe.exit(Phase::Decide);
                let Some(action) = decision else { continue };
                events.clear();
                probe.enter(Phase::Apply);
                let applied = self
                    .mechanism
                    .apply(&mut self.state, me, action, now, &mut events);
                probe.exit(Phase::Apply);
                if let Err(rejection) = applied {
                    self.stats.actions_rejected += 1;
                    observer.on_rejected(now, me, &rejection);
                    continue;
                }
                self.stats.actions_applied += 1;
                self.stats.events_emitted += events.len() as u64;
                probe.enter(Phase::Invariants);
                let guarded = self.guard(now);
                probe.exit(Phase::Invariants);
                guarded?;
                observer.on_events(now, &events);
            }
            events.clear();
            probe.enter(Phase::Tick);
            let ticked = self.mechanism.tick(&mut self.state, now, &mut events);
            probe.exit(Phase::Tick);
            ticked.map_err(|cause| RunAborted { at: now, cause })?;
            self.stats.events_emitted += events.len() as u64;
            probe.enter(Phase::Invariants);
            let guarded = self.sweep(now);
            probe.exit(Phase::Invariants);
            guarded?;
            observer.on_events(now, &events);
            observer.on_tick_end(now, &self.state);
        }
        Ok(())
    }

    fn guard(&self, now: Tick) -> Result<(), RunAborted> {
        if !self.check_invariants {
            return Ok(());
        }
        Self::aborting(now, self.mechanism.check_invariants(&self.state))
    }

    /// The tick-boundary guard: everything the per-action one asks, and whatever else the
    /// mechanism can only establish by walking its state.
    fn sweep(&self, now: Tick) -> Result<(), RunAborted> {
        if !self.check_invariants {
            return Ok(());
        }
        Self::aborting(now, self.mechanism.audit(&self.state))
    }

    fn aborting(now: Tick, checked: Result<(), InvariantViolation>) -> Result<(), RunAborted> {
        match checked {
            Ok(()) => Ok(()),
            Err(violation) => Err(RunAborted {
                at: now,
                cause: violation.into(),
            }),
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
    use crate::{
        ports::NullObserver,
        testing::{
            Add, Added, Adder, AddsItsOwnId, ConstantPath, Counter, CounterState, CountingProbe,
            Drip, Idle, RecordingObserver, ShortPath,
        },
    };

    fn world(cap: i64, agents: Vec<Box<dyn Agent<Counter>>>) -> World<Counter> {
        World::new(Counter { cap }, CounterState::default(), agents)
    }

    #[test]
    fn agents_act_in_agent_id_order() {
        let mut w = world(
            i64::MAX,
            vec![
                Box::new(AddsItsOwnId),
                Box::new(AddsItsOwnId),
                Box::new(AddsItsOwnId),
            ],
        );
        let mut observer = RecordingObserver::default();
        w.run(1, &mut RunRng::new(1, 0), &mut observer).unwrap();
        let order: Vec<i64> = observer
            .events
            .iter()
            .map(|(_, event)| event.amount)
            .collect();
        assert_eq!(
            order,
            vec![0, 1, 2],
            "the population acted out of AgentId order"
        );
    }

    /// The identity the mechanism credits is the engine's, not the action's: an action carries no
    /// identity at all, so one agent has no way to move another's account.
    #[test]
    fn the_mechanism_credits_the_agent_the_engine_says_acted() {
        let mut w = world(i64::MAX, vec![Box::new(Adder(2)), Box::new(Adder(5))]);
        let mut observer = RecordingObserver::default();
        w.run(3, &mut RunRng::new(1, 0), &mut observer).unwrap();
        assert_eq!(w.state().credited[&AgentId(0)], 6);
        assert_eq!(w.state().credited[&AgentId(1)], 15);
        assert!(observer.events.iter().all(|(_, event)| match event.actor {
            AgentId(0) => event.amount == 2,
            AgentId(1) => event.amount == 5,
            other => panic!("an event named an agent that never acted: {other:?}"),
        }));
    }

    #[test]
    fn the_observer_hears_why_an_action_was_refused() {
        let mut w = world(i64::MAX, vec![Box::new(Idle), Box::new(Adder(-1))]);
        let mut observer = RecordingObserver::default();
        w.run(2, &mut RunRng::new(1, 0), &mut observer).unwrap();
        let refused: Vec<(Tick, AgentId)> = observer
            .rejections
            .iter()
            .map(|(now, actor, _)| (*now, *actor))
            .collect();
        assert_eq!(refused, vec![(Tick(0), AgentId(1)), (Tick(1), AgentId(1))]);
        assert!(
            observer
                .rejections
                .iter()
                .all(|(_, _, reason)| reason.contains("negative"))
        );
    }

    /// A mechanism that consumes an exogenous path stops when the path runs out, rather than
    /// carrying a guessed value into a result.
    #[test]
    fn a_path_that_runs_out_aborts_the_run_at_the_tick_it_ran_out() {
        let mut w = World::new(
            Drip::fed_by(Box::new(ShortPath {
                value: 3,
                last: Tick(4),
            })),
            0i64,
            Vec::new(),
        );
        let aborted = w
            .run(10, &mut RunRng::new(1, 0), &mut NullObserver)
            .unwrap_err();
        assert_eq!(aborted.at, Tick(5));
        assert!(
            matches!(aborted.cause, Abort::Source(_)),
            "a missing input is not an invariant"
        );
        assert_eq!(*w.state(), 15, "the ticks before the gap still happened");
    }

    #[test]
    fn a_mechanism_with_no_actions_still_advances_on_its_path() {
        let mut w = World::new(Drip::fed_by(Box::new(ConstantPath(7))), 0i64, Vec::new());
        w.run(3, &mut RunRng::new(1, 0), &mut NullObserver).unwrap();
        assert_eq!(*w.state(), 21);
        assert_eq!(
            w.stats().events_emitted,
            3,
            "a tick's events are counted like any other"
        );
    }

    #[test]
    fn a_rejected_action_leaves_the_state_untouched_and_is_counted() {
        // The only agent asks for a total below zero, which the counter refuses outright.
        let mut w = world(i64::MAX, vec![Box::new(Adder(-1))]);
        w.run(4, &mut RunRng::new(1, 0), &mut NullObserver).unwrap();
        assert_eq!(w.state().total, 0);
        assert_eq!(
            w.stats(),
            RunStats {
                actions_applied: 0,
                actions_rejected: 4,
                events_emitted: 0
            }
        );
    }

    #[test]
    fn stats_count_applications_and_the_events_they_produced() {
        let mut w = world(i64::MAX, vec![Box::new(Adder(2)), Box::new(Adder(-3))]);
        w.run(3, &mut RunRng::new(1, 0), &mut NullObserver).unwrap();
        let stats = w.stats();
        assert_eq!(
            stats.actions_applied + stats.actions_rejected,
            6,
            "every decision is counted"
        );
        assert_eq!(
            stats.events_emitted, stats.actions_applied,
            "the counter emits one per action"
        );
    }

    #[test]
    fn an_idle_population_still_advances_the_clock() {
        let mut w = world(i64::MAX, vec![Box::new(Idle)]);
        w.run(5, &mut RunRng::new(1, 0), &mut NullObserver).unwrap();
        assert_eq!(w.state().ticks, 5);
        assert_eq!(w.stats(), RunStats::default());
    }

    #[test]
    fn an_invariant_violation_aborts_at_the_tick_that_caused_it() {
        // Two per tick against a cap of five: the total passes it on the third tick.
        let mut w = world(5, vec![Box::new(Adder(2))]);
        let aborted = w
            .run(10, &mut RunRng::new(1, 0), &mut NullObserver)
            .unwrap_err();
        assert_eq!(aborted.at, Tick(2));
    }

    #[test]
    fn without_invariant_checks_the_same_run_completes() {
        let mut w = world(5, vec![Box::new(Adder(2))]).without_invariant_checks();
        w.run(10, &mut RunRng::new(1, 0), &mut NullObserver)
            .unwrap();
        assert_eq!(w.state().total, 20);
    }

    #[test]
    fn the_observer_hears_one_tick_end_per_tick_after_the_mechanism_ticked() {
        let mut w = world(i64::MAX, vec![Box::new(Adder(1))]);
        let mut observer = RecordingObserver::default();
        w.run(3, &mut RunRng::new(1, 0), &mut observer).unwrap();
        let seen: Vec<(u64, u64)> = observer
            .tick_ends
            .iter()
            .map(|(now, state)| (now.0, state.ticks))
            .collect();
        assert_eq!(seen, vec![(0, 1), (1, 2), (2, 3)]);
    }

    #[test]
    fn a_probe_sees_every_phase_and_changes_nothing() {
        let build = || world(i64::MAX, vec![Box::new(Adder(1)), Box::new(Adder(1))]);
        let mut plain = build();
        plain
            .run(4, &mut RunRng::new(9, 0), &mut NullObserver)
            .unwrap();

        let mut probed = build();
        let mut probe = CountingProbe::default();
        probed
            .run_with_probe(4, &mut RunRng::new(9, 0), &mut NullObserver, &mut probe)
            .unwrap();

        assert_eq!(probed.state(), plain.state(), "the probe moved the run");
        assert_eq!(
            probed.stats(),
            plain.stats(),
            "the probe moved the counters"
        );
        assert!(!probe.nested, "the loop nested two phases");
        assert_eq!(
            probe.entered, probe.exited,
            "a phase was entered and never left"
        );
        assert_eq!(probe.entered[Phase::Decide.index()], 8);
        assert_eq!(probe.entered[Phase::Apply.index()], 8);
        assert_eq!(probe.entered[Phase::Tick.index()], 4);
        assert_eq!(probe.entered[Phase::Invariants.index()], 12);
    }

    #[test]
    fn the_counter_maps_one_action_to_one_event() {
        // Guards the fixture the order and counting tests read their conclusions from.
        let mut state = CounterState::default();
        let mut events = Vec::new();
        Counter::unbounded()
            .apply(&mut state, AgentId(4), Add(3), Tick(0), &mut events)
            .unwrap();
        assert_eq!(
            events,
            vec![Added {
                actor: AgentId(4),
                amount: 3
            }]
        );
    }
}
