//! Who may ask, and how often.
//!
//! A key names a tier, and the tier carries the limit. Storing the limit on the key instead would
//! let two keys of one tier drift apart, which is a support ticket nobody can reproduce.
//!
//! The bucket is hand-written, and that is a deliberate exception to the rule that formats go to
//! libraries. A token bucket is arithmetic with no edge cases hiding in it; a checksum or a
//! serialisation format is where a hand-rolled version goes quietly wrong.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// What a tier allows.
///
/// Two limits, because they answer two questions. The rate bounds a burst — how hard a key may
/// press at one moment — and the quota bounds a period: how much it may spend before the window
/// turns over. A tier with only the first lets one key spend a day's worth of a service every day
/// forever, and a tier with only the second lets it spend the whole day's worth in a second.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct TierConfig {
    /// Requests a second, sustained.
    pub rps_limit: f64,
    /// How much of that can be spent at once.
    pub capacity: f64,
    /// Requests allowed per [`Self::quota_window_secs`]. Absent means the tier has no period cap,
    /// which is what every tier had before this field existed.
    #[serde(default)]
    pub quota: Option<f64>,
    /// The window the quota refills over, in seconds. A day unless a tier says otherwise.
    #[serde(default = "default_quota_window_secs")]
    pub quota_window_secs: f64,
}

/// The window a quota refills over when a tier does not name one.
fn default_quota_window_secs() -> f64 {
    24.0 * 60.0 * 60.0
}

#[derive(Debug, serde::Deserialize)]
struct KeyConfig {
    tier: String,
}

#[derive(Debug, serde::Deserialize)]
struct ApiKeysFile {
    tiers: HashMap<String, TierConfig>,
    keys: HashMap<String, KeyConfig>,
}

/// The keys that exist and the tiers they belong to.
pub struct ApiKeyStore {
    tiers: HashMap<String, TierConfig>,
    key_to_tier: HashMap<String, String>,
}

impl ApiKeyStore {
    /// Reads the keys and tiers from the configuration format.
    ///
    /// # Errors
    /// [`toml::de::Error`] when the text is not that format.
    pub fn from_toml_str(text: &str) -> Result<Self, toml::de::Error> {
        let file: ApiKeysFile = toml::from_str(text)?;
        let key_to_tier = file
            .keys
            .into_iter()
            .map(|(key, config)| (key, config.tier))
            .collect();
        Ok(Self {
            tiers: file.tiers,
            key_to_tier,
        })
    }

    /// A store with nothing in it, which refuses every key.
    ///
    /// Refusing rather than waving requests through: a service that started without its key file
    /// and served anyway is a service with no authentication and no sign of it.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            tiers: HashMap::new(),
            key_to_tier: HashMap::new(),
        }
    }

    fn tier_for(&self, key: &str) -> Option<&TierConfig> {
        let tier_name = self.key_to_tier.get(key)?;
        self.tiers.get(tier_name)
    }
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

/// What a refusal was: which of the two limits the key ran into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The key pressed harder than its tier allows at one moment.
    RateLimited,
    /// The key has spent its tier's allowance for the period.
    QuotaExhausted,
}

/// One bucket per key per limit, made on that key's first request.
#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    quotas: Mutex<HashMap<String, TokenBucket>>,
}

impl RateLimiter {
    /// A limiter with no buckets yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this request is allowed, spending a token if it is.
    ///
    /// Timed on the monotonic clock rather than the wall clock: a clock correction that stepped
    /// backwards would make the refill arithmetic produce a negative interval, and the limit
    /// would either vanish or lock everybody out until the clock caught up.
    fn try_acquire(&self, key: &str, tier: &TierConfig) -> Result<(), Refusal> {
        self.try_acquire_at(key, tier, Instant::now())
    }

    /// The same decision, at a moment the caller names.
    ///
    /// The moment is a parameter because a test that reads the real clock is timing the machine it
    /// runs on: with a bucket that refills a thousand a second, two calls a millisecond apart are
    /// one refused request on an idle host and two served ones on a busy host, and neither outcome
    /// says anything about the limiter.
    fn try_acquire_at(&self, key: &str, tier: &TierConfig, now: Instant) -> Result<(), Refusal> {
        // Both maps are locked before either is spent, always in this order. Checking one and then
        // the other would let two requests pass a period's last token together, and the whole point
        // of a period cap is that it is not exceeded by however many callers happen to arrive at
        // once.
        let mut quotas = self
            .quotas
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(quota) = tier.quota {
            let refill = quota / tier.quota_window_secs;
            if !refill_to(&mut quotas, key, quota, refill, now) {
                return Err(Refusal::QuotaExhausted);
            }
        }
        if !refill_to(&mut buckets, key, tier.capacity, tier.rps_limit, now) {
            // The period is not charged: this request was refused rather than served, and charging
            // a caller's allowance for a request it never got would make pressing too hard cost
            // twice.
            return Err(Refusal::RateLimited);
        }
        take_from(&mut buckets, key);
        if tier.quota.is_some() {
            take_from(&mut quotas, key);
        }
        Ok(())
    }
}

/// Refills one key's bucket and answers whether it now holds a token.
///
/// Timed on the monotonic clock rather than the wall clock: a clock correction that stepped
/// backwards would make the refill arithmetic produce a negative interval, and the limit would
/// either vanish or lock everybody out until the clock caught up.
fn refill_to(
    buckets: &mut HashMap<String, TokenBucket>,
    key: &str,
    capacity: f64,
    refill_per_sec: f64,
    now: Instant,
) -> bool {
    let bucket = buckets.entry(key.to_string()).or_insert(TokenBucket {
        tokens: capacity,
        last_refill: now,
    });
    let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
    bucket.tokens = (bucket.tokens + elapsed * refill_per_sec).min(capacity);
    bucket.last_refill = now;
    bucket.tokens >= 1.0
}

/// Spends the token [`refill_to`] just confirmed.
fn take_from(buckets: &mut HashMap<String, TokenBucket>, key: &str) {
    if let Some(bucket) = buckets.get_mut(key) {
        bucket.tokens -= 1.0;
    }
}

/// What the middleware checks against.
pub struct AuthState {
    /// The keys that exist.
    pub api_keys: ApiKeyStore,
    /// What each of them has spent.
    pub rate_limiter: RateLimiter,
}

/// Refuses a request with no key or an unknown one, and refuses a known key that has spent its
/// allowance.
pub async fn auth_middleware(
    State(auth): State<std::sync::Arc<AuthState>>,
    request: Request,
    next: Next,
) -> Response {
    let key = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let Some(key) = key else {
        return (
            StatusCode::UNAUTHORIZED,
            "missing Authorization: Bearer <key>",
        )
            .into_response();
    };
    let Some(tier) = auth.api_keys.tier_for(key) else {
        return (StatusCode::UNAUTHORIZED, "unknown API key").into_response();
    };
    // Two refusals rather than one: a caller told only `429` cannot tell "slow down" from "come
    // back tomorrow", and the two ask for opposite behaviour.
    match auth.rate_limiter.try_acquire(key, tier) {
        Ok(()) => {}
        Err(Refusal::RateLimited) => {
            return (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
        }
        Err(Refusal::QuotaExhausted) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "quota exhausted for this period",
            )
                .into_response();
        }
    }

    next.run(request).await
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    reason = "AGENTS.md §5 scopes the no-unwrap rule to library code outside tests, and these compare limits read straight back out of a file"
)]
mod tests {
    use super::*;

    #[test]
    fn a_key_reads_with_the_tier_it_names() {
        let toml_str = r#"
            [tiers.free]
            rps_limit = 5.0
            capacity = 5.0
            [tiers.pro]
            rps_limit = 50.0
            capacity = 50.0

            [keys."sk_live_abc"]
            tier = "pro"
        "#;
        let store = ApiKeyStore::from_toml_str(toml_str).unwrap();
        let tier = store.tier_for("sk_live_abc").unwrap();
        assert_eq!(tier.rps_limit, 50.0);
        assert!(store.tier_for("unknown_key").is_none());
    }

    #[test]
    fn two_keys_of_one_tier_cannot_have_different_limits() {
        let toml_str = r#"
            [tiers.pro]
            rps_limit = 50.0
            capacity = 50.0
            [keys."key_a"]
            tier = "pro"
            [keys."key_b"]
            tier = "pro"
        "#;
        let store = ApiKeyStore::from_toml_str(toml_str).unwrap();
        assert_eq!(
            store.tier_for("key_a").unwrap().rps_limit,
            store.tier_for("key_b").unwrap().rps_limit
        );
    }

    /// A tier with a rate and no period cap, which is what every tier was before quotas existed.
    fn tier(rps_limit: f64, capacity: f64) -> TierConfig {
        TierConfig {
            rps_limit,
            capacity,
            quota: None,
            quota_window_secs: 24.0 * 60.0 * 60.0,
        }
    }

    /// The bucket empties and then refuses, with no assumption about what refills it: the refill
    /// over the microseconds between these calls is far below one token.
    #[test]
    fn a_bucket_runs_out() {
        let limiter = RateLimiter::new();
        let tier = tier(1.0, 2.0);

        assert_eq!(limiter.try_acquire("k", &tier), Ok(()));
        assert_eq!(limiter.try_acquire("k", &tier), Ok(()));
        assert_eq!(limiter.try_acquire("k", &tier), Err(Refusal::RateLimited));
    }

    #[test]
    fn one_key_running_out_does_not_affect_another() {
        let limiter = RateLimiter::new();
        let tier = tier(1.0, 1.0);

        assert_eq!(limiter.try_acquire("a", &tier), Ok(()));
        assert_eq!(limiter.try_acquire("a", &tier), Err(Refusal::RateLimited));
        assert_eq!(limiter.try_acquire("b", &tier), Ok(()));
    }

    /// The path a paying caller actually hits, and the one nobody tests until it fires.
    #[test]
    fn a_spent_quota_is_a_different_refusal_from_a_burst() {
        let limiter = RateLimiter::new();
        let tier = TierConfig {
            rps_limit: 1000.0,
            capacity: 1000.0,
            quota: Some(2.0),
            quota_window_secs: 24.0 * 60.0 * 60.0,
        };

        assert_eq!(limiter.try_acquire("k", &tier), Ok(()));
        assert_eq!(limiter.try_acquire("k", &tier), Ok(()));
        assert_eq!(
            limiter.try_acquire("k", &tier),
            Err(Refusal::QuotaExhausted),
            "the rate still allows a thousand a second; what ran out is the period"
        );
    }

    /// A request the rate limit refused was never served, so it does not spend the period's
    /// allowance — pressing too hard would otherwise cost a caller twice.
    #[test]
    fn a_request_refused_by_the_rate_does_not_spend_the_quota() {
        let limiter = RateLimiter::new();
        // One request at a time, refilled a thousand a second: a burst of two is refused, and a
        // few milliseconds later the rate is available again while the period is not refilled in
        // any measurable amount.
        let tier = TierConfig {
            rps_limit: 1000.0,
            capacity: 1.0,
            quota: Some(3.0),
            quota_window_secs: 24.0 * 60.0 * 60.0,
        };

        let start = Instant::now();
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);

        assert_eq!(limiter.try_acquire_at("k", &tier, at(0)), Ok(()));
        assert_eq!(
            limiter.try_acquire_at("k", &tier, at(0)),
            Err(Refusal::RateLimited)
        );
        assert_eq!(
            limiter.try_acquire_at("k", &tier, at(0)),
            Err(Refusal::RateLimited)
        );

        // Two more of the three remain, and they are spent one refill apart.
        for step in 1..=2 {
            assert_eq!(limiter.try_acquire_at("k", &tier, at(step * 5)), Ok(()));
        }
        assert_eq!(
            limiter.try_acquire_at("k", &tier, at(15)),
            Err(Refusal::QuotaExhausted),
            "three were served and three was the period's allowance; the two refusals cost nothing"
        );
    }
}
