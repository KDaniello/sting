use serde::{Deserialize, Serialize};
use sting_karman_core::{PathSource, SourceError};
use sting_karman_types::Tick;

/// One recorded observation: what the value was, and from which tick it held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathSample<T> {
    /// The tick from which this value holds, until the next observation replaces it.
    pub at: Tick,
    /// What was observed.
    pub value: T,
}

/// The closed range of ticks a recording can answer for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathSpan {
    /// The earliest tick the recording answers for.
    pub first: Tick,
    /// The latest tick the recording answers for; beyond it there is no value and no guess.
    pub last: Tick,
}

/// A recording could not be turned into a replayable path.
#[derive(Debug, thiserror::Error)]
#[error("invalid recorded path: {reason}")]
pub struct InvalidPath {
    /// What made the recording unreplayable, naming the observations that disagree.
    pub reason: String,
}

/// An exogenous path replayed from a recording.
///
/// Samples hold forward: the value at a tick between two observations is the earlier one, which
/// is how an oracle behaves between its updates. Outside the recorded span there is no value and
/// no guess — a run that would read past its data stops instead of extrapolating, because
/// invented numbers behind a measured result are the failure this whole boundary exists to
/// prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPath<T> {
    samples: Vec<PathSample<T>>,
    // Held rather than derived, so reading the span depends on no invariant of the vector.
    span: PathSpan,
}

impl<T> RecordedPath<T> {
    /// Builds a path from observations that must be non-empty and strictly increasing in tick.
    ///
    /// Order is checked rather than imposed: sorting a recording silently would hide a mangled
    /// fixture, and two observations at one tick mean the recording disagrees with itself.
    ///
    /// # Errors
    /// [`InvalidPath`] when the recording is empty, or when two observations share a tick or go
    /// backwards.
    pub fn new(samples: Vec<PathSample<T>>) -> Result<Self, InvalidPath> {
        let Some(first) = samples.first() else {
            return Err(InvalidPath {
                reason: "a path with no samples answers nothing".into(),
            });
        };
        let span = PathSpan {
            first: first.at,
            last: first.at,
        };
        let mut previous = first.at;
        for sample in &samples[1..] {
            if sample.at <= previous {
                return Err(InvalidPath {
                    reason: format!(
                        "samples must rise in tick: {:?} follows {:?}",
                        sample.at, previous
                    ),
                });
            }
            previous = sample.at;
        }
        Ok(RecordedPath {
            samples,
            span: PathSpan {
                last: previous,
                ..span
            },
        })
    }

    /// The range of ticks this recording can answer for.
    #[must_use]
    pub fn span(&self) -> PathSpan {
        self.span
    }
}

impl<T: Clone> PathSource<T> for RecordedPath<T> {
    fn value_at(&self, now: Tick) -> Result<T, SourceError> {
        let PathSpan { first, last } = self.span;
        if now < first || now > last {
            return Err(SourceError::new(format!(
                "recording covers {first:?}..={last:?} and was asked for {now:?}"
            )));
        }
        // The last observation at or before `now`; `partition_point` gives the count of samples
        // strictly earlier than the first one after `now`, and the span check above makes that
        // count at least one.
        let index = self.samples.partition_point(|sample| sample.at <= now) - 1;
        Ok(self.samples[index].value.clone())
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

    fn path(samples: &[(u64, i64)]) -> RecordedPath<i64> {
        let samples = samples
            .iter()
            .map(|&(at, value)| PathSample {
                at: Tick(at),
                value,
            })
            .collect();
        RecordedPath::new(samples).unwrap()
    }

    #[test]
    fn a_value_holds_until_the_next_observation() {
        let recorded = path(&[(0, 10), (5, 20), (9, 30)]);
        let seen: Vec<i64> = (0..=9)
            .map(|t| recorded.value_at(Tick(t)).unwrap())
            .collect();
        assert_eq!(seen, vec![10, 10, 10, 10, 10, 20, 20, 20, 20, 30]);
    }

    #[test]
    fn a_tick_before_the_recording_has_no_value() {
        let recorded = path(&[(3, 10)]);
        let error = recorded.value_at(Tick(2)).unwrap_err();
        assert!(
            error.to_string().contains("Tick(3)"),
            "the span is missing from: {error}"
        );
    }

    #[test]
    fn a_span_reports_the_range_the_recording_covers() {
        assert_eq!(
            path(&[(2, 1), (9, 2)]).span(),
            PathSpan {
                first: Tick(2),
                last: Tick(9)
            }
        );
        assert_eq!(
            path(&[(4, 1)]).span(),
            PathSpan {
                first: Tick(4),
                last: Tick(4)
            }
        );
    }

    #[test]
    fn a_tick_after_the_recording_is_not_extrapolated() {
        let recorded = path(&[(0, 10), (3, 20)]);
        assert!(recorded.value_at(Tick(3)).is_ok());
        assert!(
            recorded.value_at(Tick(4)).is_err(),
            "the last value was carried past its data"
        );
    }

    #[test]
    fn an_empty_recording_is_refused() {
        assert!(RecordedPath::<i64>::new(Vec::new()).is_err());
    }

    #[test]
    fn samples_out_of_order_are_refused_rather_than_sorted() {
        let out_of_order = vec![
            PathSample {
                at: Tick(5),
                value: 1,
            },
            PathSample {
                at: Tick(2),
                value: 2,
            },
        ];
        assert!(RecordedPath::new(out_of_order).is_err());
    }

    #[test]
    fn two_samples_at_one_tick_are_a_disagreement() {
        let doubled = vec![
            PathSample {
                at: Tick(1),
                value: 1,
            },
            PathSample {
                at: Tick(1),
                value: 2,
            },
        ];
        assert!(RecordedPath::new(doubled).is_err());
    }

    #[test]
    fn a_recording_survives_the_fixture_boundary() {
        let dir = std::env::temp_dir().join("karman-path-tests");
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("oracle.json");
        let samples = vec![
            PathSample {
                at: Tick(0),
                value: 10i64,
            },
            PathSample {
                at: Tick(4),
                value: 20,
            },
        ];
        crate::write_json_fixture(&file, &samples).unwrap();

        let read: Vec<PathSample<i64>> = crate::read_json_fixture(&file).unwrap();
        let recorded = RecordedPath::new(read).unwrap();
        assert_eq!(recorded.value_at(Tick(4)).unwrap(), 20);
    }
}
