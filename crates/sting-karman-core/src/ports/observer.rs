use sting_karman_types::{AgentId, Tick};

use crate::ports::{MechError, Mechanism};

/// The engine's only voice: everything a run reports — metrics, traces, progress — leaves
/// through this port. The core itself never prints, never writes a file, never buffers more
/// than the observer asked for.
pub trait Observer<M: Mechanism> {
    /// Called after every applied action with the events it produced.
    fn on_events(&mut self, now: Tick, events: &[M::Event]);

    /// Called for every action the mechanism refused, with who asked and why.
    ///
    /// The reason exists nowhere else: the engine counts rejections in its statistics, but a
    /// count answers how much pressure a mechanism was under and never what it was refusing.
    /// Dropping the reason here would make that question unanswerable for every report
    /// downstream.
    fn on_rejected(&mut self, now: Tick, actor: AgentId, reason: &MechError);

    /// Called once per tick after all agents have acted and the mechanism has ticked.
    fn on_tick_end(&mut self, now: Tick, state: &M::State);
}

/// Discards everything. The observer for sweeps where only the final state matters.
#[derive(Debug, Default)]
pub struct NullObserver;

impl<M: Mechanism> Observer<M> for NullObserver {
    fn on_events(&mut self, _now: Tick, _events: &[M::Event]) {}

    fn on_rejected(&mut self, _now: Tick, _actor: AgentId, _reason: &MechError) {}

    fn on_tick_end(&mut self, _now: Tick, _state: &M::State) {}
}
