//! `sting-abi-service` — the public, permissionless HTTP layer over
//! `sting-abi-pipeline::ExtractController`
//! (`docs/domains/service.md`,
//! `docs/domains/service.md`).
//!
//! `sting-abi-pipeline` stays synchronous
//! (`docs/domains/abi-ir.md`) — every extraction call in this
//! crate is wrapped in `tokio::task::spawn_blocking`, never invoked
//! directly from an async handler. `tokio`/`axum` are scoped to this one
//! crate in the whole workspace, for exactly this reason: real,
//! multi-connection HTTP concurrency, not just the CPU-bound parallelism
//! the rest of the platform needs.
//!
//! Router-building logic lives here, not in `main.rs`, so integration
//! tests can drive the whole app via `tower::ServiceExt::oneshot` without
//! a real TCP socket (`docs/TESTING.md`).

mod config;
mod error;
mod extract_source;
mod quota;
mod rate_limit;
mod routes;
mod state;

pub use config::{REQUEST_TIMEOUT_HEADROOM, ServiceConfig};
pub use error::ApiError;
pub use quota::API_KEY_HEADER;
pub use rate_limit::RateLimiter;
pub use state::{AppState, Tier3Config};

use axum::Router;

/// Builds the full application router — the single thing both `main.rs`
/// (a real `tokio::net::TcpListener`) and this crate's own tests drive.
pub fn build_router(state: AppState) -> Router {
    routes::router(state)
}
