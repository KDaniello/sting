//! Local coverage tracking for Tier 3 (`sting-abi-dictionary`) mainnet
//! sweeps — `docs/domains/method-dictionary.md`
//!
//! Two independent pieces, composed by `sting-abi-cli`'s `sweep`/
//! `coverage-report` subcommands (the orchestration itself lives there,
//! matching this workspace's existing `dictionary-build`/`dictionary-probe`
//! convention in `sting-abi-cli/src/dictionary.rs` — this crate stays a
//! library of building blocks, not a driver):
//!
//! - [`db::CoverageDb`] — the local `SQLite` persistence layer: every address
//!   seen, deduplicated by real on-chain code hash, plus each unique code's
//!   Tier 3 probe result once probed.
//! - [`walker::TransactionAddressWalker`] — discovers real addresses by
//!   paging `toncenter.com/api/v3/transactions`.
//! - [`messages::MessageReader`] — reads the incoming message of each of
//!   those same transactions, which is what a decode is measured over.
//!
//! See both modules' own doc comments for two load-bearing findings made
//! while implementing this crate (not assumed from the written plan): no
//! public API supports filtering by code hash or listing every account, and
//! every transaction record already carries its account's real code hash
//! for free, which changes how `code_hash` is sourced from what the
//! original plan specified (`sting_abi_coverage::db`'s own doc comment).

pub mod db;
pub mod messages;
pub mod walker;

pub use db::{CodeSample, CoverageDb, CoverageError, CoverageStats, CrawlProgress, GapCandidate};
pub use messages::{MessagePage, MessageReader, ObservedMessage};
pub use walker::{DiscoveredAddress, TransactionAddressWalker, TransactionsPage, WalkerError};
