//! Route table — see each submodule for its handlers.

mod decode;
mod extract;
mod health;
mod probe;
mod schema;
mod verify;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};

use crate::quota::quota_middleware;
use crate::rate_limit::rate_limit_middleware;
use crate::state::AppState;

/// Builds the whole app. Split into two sub-routers merged back together
/// so the rate-limit middleware (`crate::rate_limit`) applies only to the
/// extraction routes, never to `/v1/health`/`/v1/version` — see
/// `rate_limit_middleware`'s own doc comment for why.
pub fn router(state: AppState) -> Router {
    let rate_limited = Router::new()
        .route("/v1/extract", post(extract::extract_single))
        .route("/v1/extract/batch", post(extract::extract_batch))
        .route("/v1/verify", post(verify::verify))
        .route("/v1/probe", post(probe::probe_address))
        .route("/v1/decode", post(decode::decode_message))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        // Applied after the rate limit and therefore checked before it: a key this deployment does
        // not know should be told so rather than spending an anonymous budget first.
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            quota_middleware,
        ));

    let unlimited = Router::new()
        .route("/v1/health", get(health::health))
        .route("/v1/version", get(health::version))
        .route("/v1/schema/{address}", get(schema::schema_by_address));

    rate_limited
        .merge(unlimited)
        .layer(DefaultBodyLimit::max(state.config.max_body_bytes))
        .with_state(state)
}
