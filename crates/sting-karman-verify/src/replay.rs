//! Level 1: replay. Feed a mechanism the recorded actions of a real episode and compare what
//! it did against what the real venue did.

use serde::{Deserialize, Serialize};
use sting_karman_core::Mechanism;
use sting_karman_types::{AgentId, Tick};

use crate::Verdict;

/// One recorded step of a real episode: who acted, when, with what, and what the venue reported.
///
/// The actor is recorded because the mechanism is told who acts rather than asked — a replay
/// that dropped the identity could not reproduce anything a venue settles per account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedAction<A, E> {
    /// When the venue accepted it.
    pub at: Tick,
    /// Whose account the venue settled it against.
    pub actor: AgentId,
    /// What was asked for.
    pub action: A,
    /// What the venue reported happened, in the order it reported them.
    pub events: Vec<E>,
}

/// Drives a recorded action sequence through a mechanism and compares the produced events with
/// the recorded ones, one action at a time.
///
/// Rejections count: an action the real venue accepted must not be rejected here, and the first
/// divergence — different events, or an unexpected rejection — ends the replay with its
/// position, because everything after a divergence happens in a counterfactual world.
///
/// An empty recording confirms vacuously; what a replay is worth is a property of the episode
/// it was given, and the caller owns that choice.
pub fn replay<M: Mechanism>(
    mechanism: &M,
    state: &mut M::State,
    recorded: impl IntoIterator<Item = RecordedAction<M::Action, M::Event>>,
) -> Verdict
where
    M::Event: PartialEq + std::fmt::Debug,
{
    // One buffer for the episode, like the engine keeps one for a run.
    let mut produced = Vec::new();
    for (index, step) in recorded.into_iter().enumerate() {
        let at = step.at;
        produced.clear();
        match mechanism.apply(state, step.actor, step.action, at, &mut produced) {
            Ok(()) if produced == step.events => {}
            Ok(()) => {
                return Verdict::Diverged {
                    detail: format!(
                        "action {index} at {at:?}: produced {produced:?}, recorded {:?}",
                        step.events
                    ),
                };
            }
            Err(rejection) => {
                return Verdict::Diverged {
                    detail: format!(
                        "action {index} at {at:?}: rejected ({rejection}), but the venue accepted it"
                    ),
                };
            }
        }
    }
    Verdict::Confirmed
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use sting_karman_core::testing::{Add, Added, Counter, CounterState};

    use super::*;

    type Episode = Vec<RecordedAction<Add, Added>>;

    fn episode(steps: &[(u64, u32, i64, i64)]) -> Episode {
        steps
            .iter()
            .map(|&(tick, actor, action, recorded)| RecordedAction {
                at: Tick(tick),
                actor: AgentId(actor),
                action: Add(action),
                events: vec![Added {
                    actor: AgentId(actor),
                    amount: recorded,
                }],
            })
            .collect()
    }

    fn replayed(recorded: Episode) -> (Verdict, CounterState) {
        let mut state = CounterState::default();
        let verdict = replay(&Counter::unbounded(), &mut state, recorded);
        (verdict, state)
    }

    #[test]
    fn an_episode_the_mechanism_reproduces_is_confirmed() {
        let (verdict, state) = replayed(episode(&[(0, 1, 3, 3), (1, 2, 4, 4)]));
        assert_eq!(verdict, Verdict::Confirmed);
        assert_eq!(
            state.total, 7,
            "replay must leave the state the episode drove it to"
        );
        assert_eq!(
            state.credited[&AgentId(1)],
            3,
            "the recorded actor must be the credited one"
        );
        assert_eq!(state.credited[&AgentId(2)], 4);
    }

    #[test]
    fn an_empty_episode_confirms_vacuously() {
        assert_eq!(replayed(Vec::new()).0, Verdict::Confirmed);
    }

    #[test]
    fn the_first_divergence_ends_the_replay_and_names_its_position() {
        let (verdict, _) = replayed(episode(&[(0, 1, 3, 3), (7, 1, 4, 99), (2, 1, 5, 5)]));
        let Verdict::Diverged { detail } = verdict else {
            panic!("a mismatched event was accepted as agreement");
        };
        assert!(
            detail.contains("action 1"),
            "the position is missing from: {detail}"
        );
        assert!(
            detail.contains("Tick(7)"),
            "the tick is missing from: {detail}"
        );
    }

    #[test]
    fn a_replay_that_credits_the_wrong_account_diverges() {
        // The venue recorded agent 1 acting; a mechanism crediting anyone else disagrees here.
        let mut steps = episode(&[(0, 1, 3, 3)]);
        steps[0].events = vec![Added {
            actor: AgentId(2),
            amount: 3,
        }];
        assert!(matches!(replayed(steps).0, Verdict::Diverged { .. }));
    }

    #[test]
    fn an_action_the_venue_accepted_may_not_be_rejected_here() {
        // The counter refuses a total below zero; the recording says the venue allowed it.
        let (verdict, _) = replayed(episode(&[(0, 1, -1, -1)]));
        let Verdict::Diverged { detail } = verdict else {
            panic!("an unexpected rejection was accepted as agreement");
        };
        assert!(
            detail.contains("but the venue accepted it"),
            "unclear detail: {detail}"
        );
    }
}
