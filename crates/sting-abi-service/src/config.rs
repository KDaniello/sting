//! Runtime-tunable limits — every field has an env-var override, read once
//! at startup by `main.rs` and documented in this crate's own README.

use std::time::Duration;

/// The margin [`ServiceConfig::default`] (and `main.rs`'s `config_from_env`,
/// which must stay in sync — both add this to whatever `compile_timeout`
/// resolves to) adds on top of `compile_timeout` to get the
/// [`ServiceConfig::request_timeout`] default. Was `5s`, empirically found
/// too tight by `docs/domains/service.md` Round 1: a
/// `batch_size=3` Tact batch under concurrent load reliably hit `504`
/// against the old `compile_timeout=20s`/`request_timeout=25s` defaults,
/// because `request_timeout` wraps the *entire* `spawn_blocking` call
/// (every item in a batch, not one compile), while `compile_timeout` only
/// bounds a single compiler subprocess. Round 2's `60s`/`90s` pairing (a
/// `+30s` margin) saw zero timeouts on the same `batch_size=3` scenario —
/// this constant encodes that validated margin, not a guess. It is still
/// only validated at `batch_size=3`, not [`ServiceConfig::batch_max_items`]'s
/// full 100 — a deployment expecting large, concurrent Tact batches should
/// still size `REQUEST_TIMEOUT_SECS` explicitly rather than trust this
/// default, since the margin does not scale with batch size.
pub const REQUEST_TIMEOUT_HEADROOM: Duration = Duration::from_secs(30);

/// Server configuration, read once at startup — plain fields, not a
/// builder: this isn't a fluent API other code constructs repeatedly, it's
/// one value built from env vars (or defaults) at process start.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Maximum accepted request body size, in bytes. `POST /v1/extract`'s
    /// `source` field is realistically a few KB to a few hundred KB of
    /// Tolk/Tact text; this bounds a client from sending something huge
    /// and exhausting server memory before extraction even starts.
    pub max_body_bytes: usize,
    /// Timeout passed straight to `TolkCompiler`/`TactCompiler` — the
    /// *real* enforcement (kills the compiler subprocess, frees the
    /// blocking thread it ran on), not just an async-level "stop waiting".
    /// `60s` default — `docs/domains/service.md` measured real
    /// Tact cold-start compiles (a fresh Node.js process per call) at
    /// p99 ≈ 11.8s under concurrency 8; `60s` leaves real headroom above
    /// that, not just above the median.
    pub compile_timeout: Duration,
    /// The async-level safety-net timeout wrapping the whole
    /// `spawn_blocking` call. Deliberately a separate field, not derived
    /// from [`Self::compile_timeout`] on every read — tests want to set
    /// this directly and short, independent of `compile_timeout`
    /// (`docs/TESTING.md`'s `oneshot`-based
    /// integration tests need this path to trip in milliseconds, not
    /// seconds). [`Self::default`] sets it to
    /// `compile_timeout + `[`REQUEST_TIMEOUT_HEADROOM`] so the compiler's
    /// own process-kill timeout is the one that normally fires in
    /// production (and is precisely classified as `timeout`, not
    /// `compilation_failed`, by downcasting in `error.rs`) — this one is
    /// the backstop for if that mechanism itself somehow fails to return,
    /// or (see [`REQUEST_TIMEOUT_HEADROOM`]'s own doc comment) for a batch
    /// whose aggregate duration the per-item `compile_timeout` alone
    /// doesn't bound.
    pub request_timeout: Duration,
    /// Maximum items accepted in one `POST /v1/extract/batch` request
    /// (`docs/domains/abi-ir.md`: a hard limit,
    /// not a soft recommendation — otherwise one request could out-run the
    /// per-request timeout simply by aggregating enough compiles).
    pub batch_max_items: usize,
    /// Requests allowed per IP per [`Self::rate_limit_window`] before
    /// `429 rate_limited`.
    pub rate_limit_requests: u32,
    /// The token-bucket refill window for [`Self::rate_limit_requests`].
    pub rate_limit_window: Duration,
    /// The keys this deployment recognises. Empty leaves the service open, which is what it has
    /// been since it was written: keys turn on when a deployment names them.
    pub api_keys: Vec<String>,
    /// Requests one key may spend per [`Self::quota_window`] before `429 quota_exhausted`.
    ///
    /// A different limit from [`Self::rate_limit_requests`] and not a substitute for it: that one
    /// bounds a burst from one address, this one bounds a period for one caller however many
    /// addresses it comes from.
    pub quota_requests: u32,
    /// The refill window for [`Self::quota_requests`].
    pub quota_window: Duration,
    /// The largest response this service will send, in bytes.
    ///
    /// The third limit, and the one a body size and a timeout cannot stand in for: a small request
    /// that is answered quickly can still name a message whose reading is long, and a caller that
    /// asked for a decode did not ask to be handed a megabyte. Over this, the request fails with a
    /// named error rather than being answered with a truncated reading — a cut-off answer that
    /// looks whole is worse than no answer.
    pub max_result_bytes: usize,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        let compile_timeout = Duration::from_secs(60);
        Self {
            max_body_bytes: 1_000_000,
            compile_timeout,
            request_timeout: compile_timeout + REQUEST_TIMEOUT_HEADROOM,
            batch_max_items: 100,
            rate_limit_requests: 60,
            rate_limit_window: Duration::from_secs(60),
            api_keys: Vec::new(),
            quota_requests: 10_000,
            quota_window: Duration::from_hours(24),
            max_result_bytes: 1_000_000,
        }
    }
}
