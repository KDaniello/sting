use std::borrow::Cow;

use sting_karman_types::Tick;
use thiserror::Error;

/// A data port failed to produce what the scenario promised. Fail-fast: a run never continues
/// on partial inputs, because a number computed from silently truncated data still looks like a
/// number.
#[derive(Debug, Error)]
#[error("data source error: {reason}")]
pub struct SourceError {
    /// What was asked for and why it could not be given, in enough detail to fix the fixture.
    pub reason: Cow<'static, str>,
}

impl SourceError {
    /// States what the source could not supply.
    #[must_use]
    pub fn new(reason: impl Into<Cow<'static, str>>) -> Self {
        SourceError {
            reason: reason.into(),
        }
    }
}

/// An exogenous path: the sequence of values a run replays but does not generate — an oracle
/// price, an external market feed, a funding reference.
///
/// One implementation replays a recorded path (verification levels 1 and up); another generates
/// a synthetic stress path (adversarial search). The mechanism cannot tell them apart, which is
/// exactly the point, and the swap happens at a composition root rather than inside a run.
///
/// Generic over the value it carries, so a rate path and a price path are different types and
/// cannot be handed to each other's consumer.
pub trait PathSource<T> {
    /// The value at `now`.
    ///
    /// A path that has nothing to say for a tick reports it rather than guessing: extrapolating
    /// past a recording would put invented data behind a measured result.
    ///
    /// # Errors
    /// [`SourceError`] when the path has no value for `now`.
    fn value_at(&self, now: Tick) -> Result<T, SourceError>;
}
