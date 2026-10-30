//! Contract source resolution: local file, local `BoC`, or on-chain address
//! (`docs/DATA.md`).
//!
//! This crate turns an address into a contract's code: the two providers that
//! answer with one ([`ToncenterClient`]/[`TonapiClient`]), the
//! [`ContractSource`] trait that makes them mockable, the disk cache, and the
//! one controller that orders them — [`CorpusController`], cache first, then a
//! fetch with a fallback.
//!
//! Reaching a service is not here. Pacing, retrying, honouring a `Retry-After`
//! and carrying a key belong to `sting-chain`, and this crate is one of its
//! callers. Chain access existed five times in this tree and each copy had its
//! own idea of what being refused means.
//!
//! That controller was first written for `sting fetch` and lived in
//! `sting-pipeline` — the security contour's orchestration crate — which
//! made four abi-layer crates depend on the whole Tolk defect taxonomy for
//! a bytecode loader. It was moved here, and `sting-pipeline` re-exports it
//! so its own callers are unaffected.

mod cache;
mod controller;
mod error;
mod library;
mod source;
mod tonapi;
mod toncenter;

pub use cache::{CacheError, DEFAULT_TTL, DiskCache, cache_key};
pub use controller::{CorpusController, CorpusError, FetchOptions};
pub use error::FetchError;
pub use library::{LibraryError, LibraryResolver};
pub use source::{ContractCode, ContractSource, ContractTarget};
// Re-exported rather than re-declared: a caller tuning a retry is tuning
// `sting-chain`'s policy, and two names for one type is how they drift.
pub use sting_chain::{Network, RetryPolicy};
pub use tonapi::TonapiClient;
pub use toncenter::ToncenterClient;
