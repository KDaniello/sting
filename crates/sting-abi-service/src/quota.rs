//! Keys and quotas — who is asking, and how much they have left.
//!
//! The blanket rate limit answers a different question from this one. It is keyed by the address a
//! connection came from and bounds a burst; a quota is keyed by the key a caller presents and bounds
//! a period. A service with only the first is a service where one client with many addresses has no
//! budget at all, and a service with only the second is one where an unkeyed flood costs nothing.
//!
//! **A deployment with no keys configured stays open.** This service was built permissionless and
//! most of it still is: keys turn on when a deployment names them, and until then every request is
//! anonymous and only the rate limit applies. What is never allowed is the middle state — a key
//! being optional, so that presenting none is cheaper than presenting one.
//!
//! **What this does not do.** It does not persist: a restart forgets what everyone has spent, the
//! same accepted limitation the rate limiter carries. It does not coordinate between instances. And
//! it does not price by cost — a quota counts requests, so a deployment that wants a heavy route to
//! cost more charges it explicitly, as the batch route already does against the rate limit.

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;
use crate::state::AppState;

/// The header a caller presents its key in.
///
/// The same spelling the two chain providers this platform reads use, so that an operator holding
/// keys for both is not asked to remember a third convention.
pub const API_KEY_HEADER: &str = "X-Api-Key";

/// Rejects a request with no key, an unknown key, or a key with nothing left.
///
/// The three are three answers, not one: `unauthorized` says the caller is not known,
/// `quota_exhausted` says the caller is known and has spent its period's budget. Collapsing them
/// into one code would leave a caller unable to tell "your key is wrong" from "come back tomorrow".
pub async fn quota_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    if state.config.api_keys.is_empty() {
        return next.run(request).await;
    }
    let Some(key) = headers
        .get(API_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
    else {
        return ApiError::Unauthorized.into_response();
    };
    if !state.config.api_keys.contains(&key) {
        return ApiError::Unauthorized.into_response();
    }
    if state.quotas.check(key) {
        next.run(request).await
    } else {
        ApiError::QuotaExhausted.into_response()
    }
}
