//! A minimal in-memory, per-IP token bucket
//! (`docs/domains/service.md`). Does not survive a restart, does
//! not coordinate across instances if this service is ever scaled
//! horizontally — both explicitly accepted MVP limitations
//! (`docs/domains/service.md`'s own risk table), not a
//! silent gap.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

/// Guards both the map of buckets and the sweep timestamp together — a
/// sweep reads/mutates the same map a `check()` call does, so one lock
/// covers both without a separate atomic.
#[derive(Debug)]
struct LimiterState<K> {
    buckets: HashMap<K, Bucket>,
    last_sweep: Instant,
}

/// Per-IP token bucket, guarded by a plain [`Mutex`] — request rates here
/// are far below the contention threshold where anything fancier would
/// matter, and every critical section is a handful of float ops plus an
/// occasional sweep, never a blocking call.
/// The budget is keyed by whatever identifies a client: the address a
/// connection came from for the blanket limit, the key it presented for a
/// quota. One bucket implementation for both, because two copies of a
/// token bucket are two places for a refill rule to drift.
#[derive(Debug)]
pub struct RateLimiter<K = IpAddr> {
    capacity: f64,
    refill_per_sec: f64,
    /// An IP idle for longer than this is swept from the map (real gap
    /// found by review: without eviction, a public, permissionless service
    /// facing many distinct/rotating source IPs grows this map without
    /// bound for the entire process lifetime — a genuinely different
    /// problem from the already-documented "doesn't survive a restart"
    /// MVP limitation). A bucket idle this long has long since refilled to
    /// full capacity anyway, so evicting it changes no client's observed
    /// behavior — it just stops remembering IPs with nothing left to
    /// track.
    sweep_after: Duration,
    state: Mutex<LimiterState<K>>,
}

impl<K: Eq + Hash> RateLimiter<K> {
    /// `capacity` tokens, refilled continuously so the bucket is full
    /// again after `window` — e.g. `new(60, Duration::from_secs(60))` is
    /// "60 requests per minute, smoothed", not "60 at the start of every
    /// wall-clock minute". Idle IPs are swept from memory after `10 *
    /// window` of inactivity, checked opportunistically on every `check()`
    /// call rather than a background task (no implicit dependency on being
    /// constructed inside a `tokio` runtime).
    #[must_use]
    pub fn new(capacity: u32, window: Duration) -> Self {
        Self {
            capacity: f64::from(capacity),
            refill_per_sec: f64::from(capacity) / window.as_secs_f64(),
            sweep_after: window * 10,
            state: Mutex::new(LimiterState {
                buckets: HashMap::new(),
                last_sweep: Instant::now(),
            }),
        }
    }

    /// `true` and consumes one token if `ip` still has budget; `false` if
    /// it should be rejected with `429`. The `cost = 1.0` case of
    /// [`Self::check_n`] — see that method's doc comment for why a batch
    /// route needs the general form.
    pub fn check(&self, client: K) -> bool {
        self.check_n(client, 1.0)
    }

    /// `true` and consumes `cost` tokens if `ip` has that much budget;
    /// `false` (consuming nothing) if it should be rejected with `429`.
    ///
    /// Real gap found by review : the
    /// original limiter only ever charged a flat 1 token per *request*,
    /// so a `POST /v1/extract/batch` with up to `batch_max_items` items
    /// cost exactly the same as a single-item request — a
    /// rate-limit-compliant client could aggregate far more compiler work
    /// per token than the limit was meant to bound. Batch routes charge
    /// `cost = items.len()` (or the marginal cost above whatever the
    /// blanket per-request middleware already charged) through this method
    /// instead of [`Self::check`]. One consequence, worth knowing before
    /// tuning `RATE_LIMIT_REQUESTS`/`BATCH_MAX_ITEMS`: since a bucket never
    /// holds more than `capacity` tokens, a batch whose cost exceeds
    /// `capacity` can never succeed no matter how idle the bucket is — by
    /// design, not a bug, but it means the two limits should be sized
    /// together (`capacity >= batch_max_items` if full-size batches are
    /// meant to ever succeed).
    pub fn check_n(&self, client: K, cost: f64) -> bool {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if now.saturating_duration_since(state.last_sweep) >= self.sweep_after {
            let sweep_after = self.sweep_after;
            state.buckets.retain(|_, bucket| {
                now.saturating_duration_since(bucket.last_refill) < sweep_after
            });
            state.last_sweep = now;
        }

        let bucket = state.buckets.entry(client).or_insert(Bucket {
            tokens: self.capacity,
            last_refill: now,
        });
        let elapsed = now
            .saturating_duration_since(bucket.last_refill)
            .as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        bucket.last_refill = now;
        if bucket.tokens >= cost {
            bucket.tokens -= cost;
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    fn tracked_ip_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .buckets
            .len()
    }
}

/// Rejects with `429 rate_limited` once the requesting IP exceeds
/// [`AppState`]'s configured [`RateLimiter`] budget. Applied only to the
/// extraction routes (`routes::router`) — never to `/v1/health`, so an
/// orchestrator's own health-check polling can never itself trip it.
///
/// Reads the client IP from [`ConnectInfo`], not an `X-Forwarded-For`
/// header — a client-supplied header is trivially spoofable per request
/// (an attacker just varies it to dodge the limit entirely), which would
/// make the whole mechanism pointless; `ConnectInfo` reflects the real TCP
/// peer address the server actually accepted the connection from.
pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if state.rate_limiter.check(addr.ip()) {
        next.run(request).await
    } else {
        ApiError::RateLimited.into_response()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    use super::RateLimiter;

    /// Real gap found by review: without eviction, a per-IP `HashMap` grows
    /// without bound for the whole process lifetime on a public service
    /// facing many distinct/rotating source IPs.
    #[test]
    fn idle_ips_are_evicted_after_the_sweep_window() {
        let limiter = RateLimiter::new(5, Duration::from_millis(10));
        let idle_ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
        assert!(limiter.check(idle_ip));
        assert_eq!(limiter.tracked_ip_count(), 1);

        // sweep_after == window * 10 == 100ms here.
        std::thread::sleep(Duration::from_millis(150));

        // A check from a different IP is what triggers the opportunistic
        // sweep — it should evict the now long-idle first IP, leaving only
        // the fresh one.
        let fresh_ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 2));
        assert!(limiter.check(fresh_ip));
        assert_eq!(
            limiter.tracked_ip_count(),
            1,
            "idle IP should have been swept, only the fresh one should remain"
        );
    }

    #[test]
    fn budget_is_still_enforced_correctly_around_a_sweep() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 3));
        assert!(limiter.check(ip), "first request within budget must pass");
        assert!(
            !limiter.check(ip),
            "second immediate request must be rejected — budget is exhausted"
        );
    }

    /// Regression test for the batch-amplification gap found by review
    /// : a big-cost request must not be able
    /// to hide behind the same one-token price as a small one.
    #[test]
    fn check_n_charges_proportionally_to_cost() {
        let limiter = RateLimiter::new(10, Duration::from_secs(60));
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4));
        assert!(
            limiter.check_n(ip, 7.0),
            "7 of 10 tokens should still be within budget"
        );
        assert!(
            !limiter.check_n(ip, 7.0),
            "only 3 tokens remain — a second cost-7 request must be rejected"
        );
        assert!(
            limiter.check_n(ip, 3.0),
            "exactly the remaining 3 tokens should still succeed"
        );
    }

    /// A cost above the bucket's own capacity can never succeed, even at
    /// full budget — documented as intentional in `check_n`'s own doc
    /// comment, not a bug; this pins that behavior down.
    #[test]
    fn check_n_rejects_a_cost_that_exceeds_capacity_even_when_full() {
        let limiter = RateLimiter::new(5, Duration::from_secs(60));
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 5));
        assert!(
            !limiter.check_n(ip, 6.0),
            "a cost above capacity must never succeed, regardless of how full the bucket is"
        );
    }
}
