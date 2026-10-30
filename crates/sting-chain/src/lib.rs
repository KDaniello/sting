//! One path to the chain: one retry policy, one rate limiter, one place a
//! `Retry-After` is honoured, one place an API key is read.
//!
//! Chain access existed eight times in this tree — five blocking clients under
//! the security contour, an asynchronous one under the clustering contour, a
//! 325-line shell script under the mechanism contour, and a Python tool. Eight
//! copies are eight ideas of what being refused means, and two of them retried
//! nothing at all. That is the defect this crate exists to remove, and the
//! `tools/ton-res` Python tool is the one exception, recorded rather
//! than implied: it runs a few times a year against a liteserver, and what it
//! is worth is a proven state rather than throughput.
//!
//! # The core is synchronous
//!
//! Every caller but one is synchronous, so the core is, and that one gets a
//! thin adapter above it rather than the core getting a runtime. A library that drags `tokio` into a linter's dependency
//! path to make one HTTP call has decided for its callers. The adapter is
//! behind the `async` feature, off by default, and it is
//! [`AsyncChainClient`] — one client, one pace, awaited.
//!
//! # What this crate does not do
//!
//! It does not know what a contract is. It carries requests and answers as
//! JSON, and every meaning above that — an address resolved to code and data,
//! a block walked into transactions — belongs to the crate asking. It is not
//! an indexer and it holds no chain state. It does not cache: an answer's
//! freshness depends on what was asked, and the caller is what knows that.

#[cfg(feature = "async")]
mod asynchronous;
mod client;
mod credentials;
mod endpoint;
mod error;
mod limiter;
mod retry;
mod retry_after;

#[cfg(feature = "async")]
pub use asynchronous::AsyncChainClient;
pub use client::{ChainClient, Query};
pub use credentials::{ApiKey, ApiKeys};
pub use endpoint::{Endpoint, Network, Provider};
pub use error::ChainError;
pub use limiter::{DEFAULT_REQUESTS_PER_SECOND, RateLimiter};
pub use retry::{DEFAULT_CALL_TIMEOUT, RetryPolicy};
