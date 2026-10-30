//! The request path: an address in, a risk score out, inside a few milliseconds.
//!
//! Everything the answer needs is already in memory. The model is the distilled linear one, the
//! features come from a cache built off what the pipeline has already written, and neither the
//! chain nor another process is reached while a request is open — a hot path that waits on a
//! network call has a latency nobody can promise.
//!
//! A cache miss is answered rather than resolved. The response says `partial` and returns what it
//! has, because an address nobody has computed features for is a fact about the pipeline, and
//! going to look for the answer would trade a bounded latency for an unbounded one.
//!
//! # What this crate does not do
//!
//! It computes no feature and trains no model. It resolves no unknown address live. It stores
//! nothing: the cache is loaded at startup and replaced by restarting.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::get,
};
use sting_sybil_scorer::LinearScorer;

pub mod auth;
pub mod feature_cache;
pub mod metrics;

use metrics::Metrics;

/// What starting or serving answers with when it cannot.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// A file could not be read, or the port could not be bound.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// A configuration file is not the format it should be.
    #[error("{path} is not a valid configuration file: {source}")]
    Config {
        /// Which file.
        path: String,
        /// Why it was refused.
        source: toml::de::Error,
    },
    /// The warm cache could not be loaded.
    #[error("{0}")]
    Cache(#[from] feature_cache::CacheError),
}

/// What a caller gets back from every fallible step of starting the service.
pub type Result<T> = std::result::Result<T, ApiError>;

/// Which version of the answer's shape this service speaks.
///
/// Stated in every response rather than in a document a caller may not have read: a client parsing
/// a field that moved needs to learn it from the answer, and a version nobody sends is a version
/// nobody checks.
pub const RESPONSE_SCHEMA_VERSION: &str = "1.0";

/// What a score request answers with.
///
/// The same shape the batch side returns, field for field: a caller that switched between them
/// should not have to parse two things.
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct ScoreResponse {
    /// Which version of this shape the answer is in.
    pub schema_version: &'static str,
    /// The address asked about.
    pub address: String,
    /// The calibrated probability.
    pub risk: f64,
    /// Which band that falls in.
    pub band: String,
    /// The few features that moved the score most.
    pub reasons: Vec<String>,
    /// Which model produced it.
    pub model_version: String,
    /// Whether the answer was given without features for this address.
    pub partial: bool,
    /// How long the request took, in milliseconds.
    pub latency_ms: u64,
}

/// What a request is served from: the model, the warm cache, and the counters.
pub struct AppState {
    /// The model every score comes from.
    pub scorer: LinearScorer,
    /// Features per address, loaded from what the pipeline wrote.
    pub feature_cache: HashMap<String, HashMap<String, f64>>,
    /// What the service reports about itself.
    pub metrics: Metrics,
}

/// The features that moved this score the most, largest contribution first.
///
/// A score with no reasons attached is a number somebody has to take on trust, and the first
/// question anybody asks about a flagged address is which of its behaviour caused it.
fn top_reasons(features: &HashMap<String, f64>, scorer: &LinearScorer, n: usize) -> Vec<String> {
    let mut contributions: Vec<(String, f64)> = scorer
        .weights
        .iter()
        .filter_map(|(name, weight)| {
            features
                .get(name)
                .map(|value| (name.clone(), weight * value))
        })
        .collect();
    contributions.sort_by(|a, b| {
        b.1.abs()
            .partial_cmp(&a.1.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    contributions
        .into_iter()
        .take(n)
        .map(|(name, contribution)| format!("{name} (contributes {contribution:.3})"))
        .collect()
}

async fn score_handler(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Json<ScoreResponse> {
    let started = Instant::now();
    // A miss answers with what it has rather than going to look: the latency this path promises
    // is only promisable because nothing here waits on anything.
    //
    // Read by reference rather than cloned. Nothing on this path mutates a cached row, and a copy
    // of thirty-one keyed values per request is an allocation in exactly the place whose cost is
    // the thing being promised.
    let empty = HashMap::new();
    let (features, partial) = match state.feature_cache.get(&address) {
        Some(cached) => (cached, false),
        None => (&empty, true),
    };
    let risk = state.scorer.score(features);
    let band = state.scorer.band(risk).to_string();
    let reasons = top_reasons(features, &state.scorer, 3);
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    // A hit that carries no cluster signal is the gap the cache documents, counted rather than
    // described: the day the gap closes, this counter goes to zero on its own.
    let missing_cluster_signal = !partial && !features.contains_key("cluster_agreement");
    state
        .metrics
        .record(latency_ms, !partial, false, missing_cluster_signal);

    Json(ScoreResponse {
        schema_version: RESPONSE_SCHEMA_VERSION,
        address,
        risk,
        band,
        reasons,
        model_version: state.scorer.model_version.clone(),
        partial,
        latency_ms,
    })
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render_prometheus_text(),
    )
}

/// The routes with no authentication in front of them.
///
/// Used where authentication is not what is being exercised. What actually serves wraps the score
/// route in the layer below.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/score/{address}", get(score_handler))
        .route("/metrics", get(metrics_handler))
        .with_state(state)
}

/// Scoring behind authentication, and metrics in front of it.
///
/// The metrics route is deliberately unauthenticated: a scraper lives on the same private network
/// as the service, and putting a key in a scrape config is a key in a config file.
pub fn router_with_auth(state: Arc<AppState>, auth_state: Arc<auth::AuthState>) -> Router {
    let scored = Router::new()
        .route("/score/{address}", get(score_handler))
        .layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth::auth_middleware,
        ))
        .with_state(state.clone());
    let metrics_route = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(state);
    scored.merge(metrics_route)
}
