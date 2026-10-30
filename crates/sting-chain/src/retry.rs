//! Exponential backoff, and a server's own `Retry-After` above it.
//!
//! Pure: the decision of how long to wait is separated from the call that
//! waits, so the policy is tested without a request and without a sleep. What
//! a status code means for a given provider is not here — this module knows
//! only that an attempt was refused.

use std::time::Duration;

use crate::error::ChainError;

/// End-to-end timeout (DNS lookup through finishing reading the response
/// body — `ureq::config::ConfigBuilder::timeout_global`) applied to every
/// single HTTP call attempt by both [`the toncenter client`] and
/// [`the tonapi client`]. Found missing during a full project review:
/// `ureq::Agent`'s own default is unbounded (`None`), so a stalled TCP
/// connection or a slow-drip response could hang `sting fetch`/`sting scan`
/// indefinitely — [`RetryPolicy`]'s backoff only ever engages *after* an
/// attempt actually completes (with a response or a transport error), never
/// as a substitute for a per-attempt ceiling. 20 seconds: generous relative
/// to every real `toncenter`/`tonapi` response observed during this
/// project's development (low hundreds of milliseconds), but short enough
/// that 5 stalled attempts (`RetryPolicy::default`'s `max_attempts`) do not
/// compound into minutes of an apparently-frozen CLI.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// Retry behavior for one [`crate::ChainClient`]. `Default` is exponential
/// backoff, and the values are chosen rather than mandated: 5 attempts
/// tops out around 8s of total backoff even without server guidance, short
/// enough not to make a single `sting fetch` invocation feel hung.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Total attempts before giving up (including the first, non-retry, attempt).
    pub max_attempts: u32,
    /// Backoff before the second attempt; doubles each subsequent attempt.
    pub initial_backoff: Duration,
    /// Ceiling on any single backoff, whether computed or server-specified.
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(8),
        }
    }
}

/// Outcome of one attempt inside [`with_retries`].
pub(crate) enum Attempt<T> {
    /// The call succeeded, or failed in a way retrying cannot fix — the
    /// caller signals the second through `Err` from the attempt closure.
    Done(T),
    /// The service refused the call. `retry_after` is its own requested
    /// delay, when it sent one.
    RateLimited { retry_after: Option<Duration> },
    /// The call never reached an answer: a dropped connection, a TLS
    /// failure, a timeout. Retried on the same count as a refusal, because
    /// both are transient and a caller wanting neither retried sets
    /// `max_attempts` to 1. `cause` is what is returned if the attempts run
    /// out — the transport's own error rather than a rate limit that never
    /// happened.
    Unreachable { cause: ChainError },
}

/// The delay before the next attempt after attempt number `attempt` (1-based)
/// was rate-limited. `retry_after` — parsed from the server's own response
/// (e.g. a `Retry-After` header) — takes precedence over the exponential
/// guess when present: a server that states its own delay is respected
/// (`docs/DATA.md`). Both paths are
/// still capped at `policy.max_backoff` — a server-specified delay longer
/// than our own ceiling should not make a single fetch block indefinitely.
pub(crate) fn backoff_delay(
    policy: &RetryPolicy,
    attempt: u32,
    retry_after: Option<Duration>,
) -> Duration {
    if let Some(server_delay) = retry_after {
        return server_delay.min(policy.max_backoff);
    }
    let exponent = attempt.saturating_sub(1).min(20);
    let multiplier = 2u32.saturating_pow(exponent);
    policy
        .initial_backoff
        .saturating_mul(multiplier)
        .min(policy.max_backoff)
}

/// Runs `attempt_fn` up to `policy.max_attempts` times, waiting between
/// attempts per [`backoff_delay`].
///
/// `attempt_fn` receives the 1-based attempt number and returns
/// [`Attempt::Done`] to stop with a value, [`Attempt::RateLimited`] or
/// [`Attempt::Unreachable`] to try again, or `Err` for a failure retrying
/// cannot fix. That last distinction is the whole point: a body that does not
/// parse and a status naming a malformed request will read the same on the
/// fifth attempt as on the first.
pub(crate) fn with_retries<T>(
    policy: &RetryPolicy,
    provider: &'static str,
    hint: Option<&'static str>,
    mut attempt_fn: impl FnMut(u32) -> Result<Attempt<T>, ChainError>,
) -> Result<T, ChainError> {
    for attempt in 1..=policy.max_attempts {
        let (retry_after, exhausted) = match attempt_fn(attempt)? {
            Attempt::Done(value) => return Ok(value),
            Attempt::RateLimited { retry_after } => (
                retry_after,
                ChainError::RateLimited {
                    provider,
                    attempts: attempt,
                    hint,
                },
            ),
            Attempt::Unreachable { cause } => (None, cause),
        };
        if attempt == policy.max_attempts {
            return Err(exhausted);
        }
        std::thread::sleep(backoff_delay(policy, attempt, retry_after));
    }
    // Only reachable if `policy.max_attempts == 0` — a misconfiguration, not
    // a real code path, but still returns a value rather than panicking
    // (docs/ARCHITECTURE.md §3.2: no `panic!`/`unwrap`/`expect` outside tests).
    Err(ChainError::RateLimited {
        provider,
        attempts: 0,
        hint,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::time::Duration;

    use super::{RetryPolicy, backoff_delay};

    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(2),
        }
    }

    #[test]
    fn backoff_delay_doubles_each_attempt_without_server_guidance() {
        let p = policy();
        assert_eq!(backoff_delay(&p, 1, None), Duration::from_millis(100));
        assert_eq!(backoff_delay(&p, 2, None), Duration::from_millis(200));
        assert_eq!(backoff_delay(&p, 3, None), Duration::from_millis(400));
        assert_eq!(backoff_delay(&p, 4, None), Duration::from_millis(800));
    }

    #[test]
    fn backoff_delay_is_capped_at_max_backoff() {
        let p = policy();
        assert_eq!(backoff_delay(&p, 10, None), Duration::from_secs(2));
    }

    #[test]
    fn backoff_delay_prefers_server_retry_after_over_the_exponential_guess() {
        let p = policy();
        // Attempt 1's exponential guess would be 100ms, but the server said
        // 1500ms explicitly — the server's number wins.
        assert_eq!(
            backoff_delay(&p, 1, Some(Duration::from_millis(1500))),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn backoff_delay_still_caps_an_excessive_server_retry_after() {
        let p = policy();
        assert_eq!(
            backoff_delay(&p, 1, Some(Duration::from_secs(600))),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn with_retries_returns_immediately_on_success_without_sleeping() {
        use super::{Attempt, with_retries};
        let mut calls = 0;
        let result = with_retries(&policy(), "test", None, |_attempt| {
            calls += 1;
            Ok::<_, crate::ChainError>(Attempt::Done(42))
        });
        assert_eq!(result.expect("succeeds"), 42);
        assert_eq!(calls, 1);
    }

    #[test]
    fn with_retries_gives_up_after_max_attempts_of_rate_limiting() {
        use super::{Attempt, with_retries};
        let mut calls = 0;
        let result: Result<(), _> = with_retries(
            &RetryPolicy {
                max_attempts: 3,
                initial_backoff: Duration::from_millis(1),
                max_backoff: Duration::from_millis(2),
            },
            "test",
            None,
            |_attempt| {
                calls += 1;
                Ok::<_, crate::ChainError>(Attempt::RateLimited { retry_after: None })
            },
        );
        assert!(matches!(
            result,
            Err(crate::ChainError::RateLimited { attempts: 3, .. })
        ));
        assert_eq!(calls, 3);
    }
}
