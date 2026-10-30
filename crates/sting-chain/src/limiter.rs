//! The pace, kept before a request rather than after a refusal.
//!
//! A retry answers a limit that has already been hit; a limiter keeps it from
//! being hit. Both are needed and they are not the same: the sweep that walks
//! twenty-four thousand addresses spends its time in the limiter and never
//! reaches the retry, and a run that only ever backs off after a `429` has
//! already cost the service a refusal it had to compute.
//!
//! # One limiter per service, not per client
//!
//! A limiter held by a client is a limiter per client, and a caller that builds
//! a client per call — which two risk signals in this tree did — is a caller
//! with no pace at all. [`shared`] hands out one limiter per service and
//! network for the life of the process, so building ten clients against one
//! service builds ten clients that queue behind each other.
//!
//! # What this does not do
//!
//! It paces one process, not a machine. Two processes running at ten requests a
//! second are twenty requests a second, and nothing here knows about the other
//! process. It is a floor on the interval between calls, not a token bucket
//! that lets a burst through after an idle minute: a burst is what the public
//! tier refuses.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::endpoint::{Network, Provider};

/// The pace a key buys, and the pace this tree asks for: ten requests a
/// second, stated by the provider for a keyed tier.
///
/// Without a key the public tier answers about one a second, so a keyless
/// caller sets its own slower pace rather than being given this one silently.
pub const DEFAULT_REQUESTS_PER_SECOND: u32 = 10;

/// A minimum interval between calls, shared by whoever holds it.
///
/// `Mutex` rather than an atomic because the wait is decided and recorded as
/// one step: two threads reading the same last-call instant would both decide
/// they may go now.
#[derive(Debug)]
pub struct RateLimiter {
    interval: Duration,
    /// When the next call may go out. A queue rather than a record of the
    /// last call: ten calls arriving together have to leave one interval
    /// apart, and a limiter remembering only the last one lets nine of them
    /// leave at once after a single wait.
    next_free: Mutex<Option<Instant>>,
}

impl RateLimiter {
    /// A limiter admitting `per_second` calls a second.
    ///
    /// Zero is treated as one: a limiter that admits nothing would hang the
    /// caller rather than pace it, and a misconfiguration must not look like a
    /// deadlock.
    #[must_use]
    pub fn per_second(per_second: u32) -> Self {
        let per_second = per_second.max(1);
        Self {
            interval: Duration::from_secs(1) / per_second,
            next_free: Mutex::new(None),
        }
    }

    /// A limiter that admits everything — a stub server in a container, where
    /// pacing would only make the test slower.
    #[must_use]
    pub fn unlimited() -> Self {
        Self {
            interval: Duration::ZERO,
            next_free: Mutex::new(None),
        }
    }

    /// The minimum interval between two calls.
    #[must_use]
    pub const fn interval(&self) -> Duration {
        self.interval
    }

    /// Blocks until the next call may go out, then records it as gone.
    pub fn acquire(&self) {
        let Some(wait) = self.reserve(Instant::now()) else {
            return;
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }

    /// How long a call arriving at `now` has to wait, recording it as
    /// admitted at the end of that wait.
    ///
    /// Separate from [`Self::acquire`] and taking the clock as an argument, so
    /// the pacing itself is tested without a test that sleeps.
    fn reserve(&self, now: Instant) -> Option<Duration> {
        if self.interval.is_zero() {
            return None;
        }
        // A poisoned lock means another thread panicked while holding it. The
        // value behind it is one instant, and pacing off a stale instant is a
        // slower run rather than a wrong one, so the guard is taken either way.
        let mut next_free = self
            .next_free
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let starts_at = match *next_free {
            Some(free) if free > now => free,
            _ => now,
        };
        *next_free = Some(starts_at + self.interval);
        Some(starts_at.duration_since(now))
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::per_second(DEFAULT_REQUESTS_PER_SECOND)
    }
}

/// The limiter for one host, shared by every client built against it in this
/// process.
///
/// Keyed by the host and not by the service alone, for two reasons that point
/// the same way: two networks of one service are two quotas, and a stub server
/// in a test is not the service it imitates. A registry keyed by service would
/// hand a test the pace of the real host, and every test against a stub would
/// wait out a quota nobody is spending.
///
/// `per_second` decides the pace the first time a host is asked for. A second
/// caller asking for a different one joins the first, because two paces against
/// one host is the defect this function exists to remove.
pub fn shared(
    provider: Provider,
    network: Network,
    base_url: &str,
    per_second: u32,
) -> Arc<RateLimiter> {
    /// One limiter per host, for the life of the process.
    type Limiters = Mutex<HashMap<(Provider, Network, String), Arc<RateLimiter>>>;

    static LIMITERS: OnceLock<Limiters> = OnceLock::new();
    let limiters = LIMITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut limiters = limiters
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Arc::clone(
        limiters
            .entry((provider, network, base_url.to_string()))
            .or_insert_with(|| Arc::new(RateLimiter::per_second(per_second))),
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::{DEFAULT_REQUESTS_PER_SECOND, RateLimiter};

    #[test]
    fn ten_a_second_is_a_hundred_milliseconds_apart() {
        let limiter = RateLimiter::per_second(DEFAULT_REQUESTS_PER_SECOND);
        assert_eq!(limiter.interval(), Duration::from_millis(100));
    }

    #[test]
    fn the_first_call_waits_for_nothing() {
        let limiter = RateLimiter::per_second(10);
        assert_eq!(limiter.reserve(Instant::now()), Some(Duration::ZERO));
    }

    #[test]
    fn a_call_arriving_at_once_waits_out_the_interval() {
        let limiter = RateLimiter::per_second(10);
        let now = Instant::now();
        assert_eq!(limiter.reserve(now), Some(Duration::ZERO));
        assert_eq!(limiter.reserve(now), Some(Duration::from_millis(100)));
        // The third waits behind the second, not behind the first: the queue
        // is what keeps ten calls in a burst from all waiting one interval and
        // then leaving together.
        assert_eq!(limiter.reserve(now), Some(Duration::from_millis(200)));
    }

    #[test]
    fn a_call_arriving_after_the_interval_waits_for_nothing() {
        let limiter = RateLimiter::per_second(10);
        let now = Instant::now();
        assert_eq!(limiter.reserve(now), Some(Duration::ZERO));
        assert_eq!(
            limiter.reserve(now + Duration::from_millis(150)),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn a_limiter_admitting_everything_reserves_nothing() {
        let limiter = RateLimiter::unlimited();
        assert_eq!(limiter.reserve(Instant::now()), None);
        assert_eq!(limiter.interval(), Duration::ZERO);
    }

    #[test]
    fn zero_a_second_paces_rather_than_hangs() {
        let limiter = RateLimiter::per_second(0);
        assert_eq!(limiter.interval(), Duration::from_secs(1));
    }

    #[test]
    fn one_service_has_one_limiter_however_many_clients_ask_for_it() {
        use super::shared;
        use crate::endpoint::{Network, Provider};

        let host = "https://testnet.toncenter.com/api/v2";
        let first = shared(Provider::ToncenterV2, Network::Testnet, host, 10);
        let second = shared(Provider::ToncenterV2, Network::Testnet, host, 10);
        assert!(Arc::ptr_eq(&first, &second));

        // A different host is a different quota — including a stub server,
        // which is not the service it imitates.
        let stub = shared(
            Provider::ToncenterV2,
            Network::Testnet,
            "http://127.0.0.1:9",
            10,
        );
        assert!(!Arc::ptr_eq(&first, &stub));
    }

    #[test]
    fn the_pace_a_service_was_first_asked_for_is_the_pace_it_keeps() {
        use super::shared;
        use crate::endpoint::{Network, Provider};

        // Two paces against one service is the defect the registry removes, so
        // the second caller joins the first rather than replacing it.
        let host = "https://testnet.tonapi.io/v2";
        let first = shared(Provider::TonapiV2, Network::Testnet, host, 1);
        let second = shared(Provider::TonapiV2, Network::Testnet, host, 10);
        assert_eq!(second.interval(), first.interval());
    }
}
