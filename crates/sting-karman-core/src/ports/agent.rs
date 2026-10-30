use sting_karman_types::{AgentId, Tick};

use crate::{engine::rng::RunRng, ports::Mechanism};

/// A unit of behaviour, generic over the mechanism it acts on.
///
/// Agents are where randomness lives: an agent draws from the run stream it is handed, and from
/// nowhere else. The mechanism it acts on stays deterministic, so a run is reproducible even
/// though its population is stochastic.
///
/// An agent observing the state through `&M::State` sees everything. Information asymmetry, when
/// a scenario needs it, is the agent's own discipline — encoded in the implementation, not in
/// the port.
pub trait Agent<M: Mechanism> {
    /// One decision at one tick. `None` means the agent sits this tick out.
    ///
    /// `me` is the agent's own identifier: state is shared and seen in full, so an agent that
    /// owns part of it — a lender's position, a trader's margin — needs a handle to find
    /// itself. It is also the agent's place in the acting order, which the engine fixes.
    fn decide(
        &mut self,
        me: AgentId,
        state: &M::State,
        now: Tick,
        rng: &mut RunRng,
    ) -> Option<M::Action>;
}
