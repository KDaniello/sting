//! Level 0: differential. Put the same transition through this workspace and through the
//! implementation it claims to model, and demand the same answer down to the last unit.
//!
//! Distinct from replay, which is level 1: replay follows one recorded episode in sequence and
//! asks whether the mechanism keeps up, while this asks a corpus of individual transitions,
//! chosen to sit on the edges where two implementations part company. Where a reference exists,
//! this is the strongest claim a mechanism can make, and it is the only one that catches a
//! rounding direction taken the wrong way.

use serde::{Deserialize, Serialize};
use sting_karman_core::Mechanism;
use sting_karman_types::{AgentId, Tick};

use crate::Verdict;

/// What the reference did with one transition.
///
/// Serialisable, because a corpus is generated once against the implementation being modelled
/// and then committed: a differential test that re-derived its own expectations on every run
/// would be tied to whatever produced them, and could not be read by anyone checking the claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome<S, E> {
    /// The reference accepted it, reaching this state and reporting these events.
    Applied {
        /// The state the reference left behind.
        state: S,
        /// What it reported, in the order it reported them.
        events: Vec<E>,
    },
    /// The reference refused it. Which is as much a fact about the implementation as a fill is:
    /// a mechanism that accepts what the reference reverts has diverged.
    Rejected,
}

/// One transition, and what the reference made of it.
///
/// The state travels with the case rather than accumulating across the corpus, because a
/// differential corpus is chosen for coverage rather than for continuity: the case that probes
/// a rounding boundary has no reason to follow the case that probes an overflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentialCase<S, A, E> {
    /// What this case probes, quoted back whenever it is the one that diverges.
    pub probes: String,
    /// The state the transition starts from.
    pub state: S,
    /// Who the reference settled it against.
    pub actor: AgentId,
    /// When, for a mechanism whose transition depends on the clock.
    pub at: Tick,
    /// What was asked for.
    pub action: A,
    /// What the reference answered.
    pub outcome: Outcome<S, E>,
}

/// Runs every case through the mechanism and compares against the reference, stopping at the
/// first disagreement.
///
/// Stopping early is not impatience: once two implementations differ, every later case is being
/// asked of a mechanism already known to be wrong, and the position of the first divergence is
/// what a fix is written against.
pub fn differential<M: Mechanism>(
    mechanism: &M,
    corpus: impl IntoIterator<Item = DifferentialCase<M::State, M::Action, M::Event>>,
) -> Verdict
where
    M::State: Clone + PartialEq + std::fmt::Debug,
    M::Event: PartialEq + std::fmt::Debug,
{
    let mut events = Vec::new();
    for case in corpus {
        let probes = case.probes;
        let before = case.state.clone();
        let mut state = case.state;
        events.clear();
        let applied = mechanism.apply(&mut state, case.actor, case.action, case.at, &mut events);

        let detail = match (applied, &case.outcome) {
            (
                Ok(()),
                Outcome::Applied {
                    state: expected,
                    events: recorded,
                },
            ) => {
                if state != *expected {
                    Some(format!(
                        "state is {state:?}, the reference reached {expected:?}"
                    ))
                } else if events != *recorded {
                    Some(format!(
                        "reported {events:?}, the reference reported {recorded:?}"
                    ))
                } else {
                    None
                }
            }
            (Err(rejection), Outcome::Rejected) => {
                // Both refused. The reasons are each implementation's own words and are not
                // compared: what level 0 claims is that the same inputs are refused, not that
                // two codebases phrase a refusal alike.
                let _ = rejection;
                None
            }
            (Ok(()), Outcome::Rejected) => Some(format!(
                "accepted, reaching {state:?}, where the reference refused"
            )),
            (Err(rejection), Outcome::Applied { .. }) => {
                if state == before {
                    Some(format!("refused ({rejection}) what the reference accepted"))
                } else {
                    Some(format!(
                        "refused ({rejection}) what the reference accepted, and moved the state \
                         to {state:?} while refusing"
                    ))
                }
            }
        };

        if let Some(detail) = detail {
            return Verdict::Diverged {
                detail: format!("case `{probes}`: {detail}"),
            };
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
    use std::collections::BTreeMap;

    use sting_karman_core::testing::{Add, Added, Counter, CounterState};

    use super::*;

    type Case = DifferentialCase<CounterState, Add, Added>;

    fn state(total: i64) -> CounterState {
        let mut state = CounterState {
            total,
            ticks: 0,
            credited: BTreeMap::default(),
        };
        if total != 0 {
            state.credited.insert(AgentId(1), total);
        }
        state
    }

    fn case(probes: &str, from: i64, add: i64, outcome: Outcome<CounterState, Added>) -> Case {
        DifferentialCase {
            probes: probes.into(),
            state: state(from),
            actor: AgentId(1),
            at: Tick(0),
            action: Add(add),
            outcome,
        }
    }

    fn applied(from: i64, add: i64) -> Outcome<CounterState, Added> {
        Outcome::Applied {
            state: state(from + add),
            events: vec![Added {
                actor: AgentId(1),
                amount: add,
            }],
        }
    }

    fn run(corpus: Vec<Case>) -> Verdict {
        differential(&Counter::unbounded(), corpus)
    }

    #[test]
    fn a_corpus_the_mechanism_matches_is_confirmed() {
        let corpus = vec![
            case("a plain credit", 10, 5, applied(10, 5)),
            case("a debit that stays above zero", 10, -3, applied(10, -3)),
            case("a debit through zero", 10, -20, Outcome::Rejected),
        ];
        assert_eq!(run(corpus), Verdict::Confirmed);
    }

    #[test]
    fn an_empty_corpus_confirms_vacuously() {
        assert_eq!(run(Vec::new()), Verdict::Confirmed);
    }

    #[test]
    fn a_state_the_reference_did_not_reach_diverges_and_names_the_case() {
        let corpus = vec![case("an off-by-one credit", 10, 5, applied(10, 6))];
        let Verdict::Diverged { detail } = run(corpus) else {
            panic!("a different final state was accepted as agreement");
        };
        assert!(
            detail.contains("an off-by-one credit"),
            "the case is missing from: {detail}"
        );
        assert!(
            detail.contains("the reference reached"),
            "unclear detail: {detail}"
        );
    }

    #[test]
    fn events_the_reference_did_not_report_diverge() {
        let corpus = vec![case(
            "a credit to the wrong account",
            10,
            5,
            Outcome::Applied {
                state: state(15),
                events: vec![Added {
                    actor: AgentId(2),
                    amount: 5,
                }],
            },
        )];
        assert!(matches!(run(corpus), Verdict::Diverged { .. }));
    }

    /// The asymmetry level 0 exists to catch: a mechanism that is more permissive than what it
    /// models is exactly as wrong as one that computes a different number.
    #[test]
    fn accepting_what_the_reference_refused_diverges() {
        let corpus = vec![case(
            "a credit the reference reverts",
            10,
            5,
            Outcome::Rejected,
        )];
        let Verdict::Diverged { detail } = run(corpus) else {
            panic!("being more permissive than the reference was accepted as agreement");
        };
        assert!(
            detail.contains("where the reference refused"),
            "unclear detail: {detail}"
        );
    }

    #[test]
    fn refusing_what_the_reference_accepted_diverges() {
        let corpus = vec![case(
            "a debit the reference allows",
            10,
            -20,
            applied(10, -20),
        )];
        let Verdict::Diverged { detail } = run(corpus) else {
            panic!("being stricter than the reference was accepted as agreement");
        };
        assert!(
            detail.contains("the reference accepted"),
            "unclear detail: {detail}"
        );
    }

    #[test]
    fn the_first_divergence_is_the_one_reported() {
        let corpus = vec![
            case("this one agrees", 10, 5, applied(10, 5)),
            case("this one is the first to differ", 10, 5, applied(10, 6)),
            case("this one differs too", 0, 1, applied(0, 9)),
        ];
        let Verdict::Diverged { detail } = run(corpus) else {
            panic!("nothing diverged")
        };
        assert!(
            detail.contains("the first to differ"),
            "the wrong case was reported: {detail}"
        );
    }
}
