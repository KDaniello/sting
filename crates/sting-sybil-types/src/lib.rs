//! The chain facts every crate of the clustering contour shares, and nothing else.
//!
//! These are not a domain model: they are the vocabulary the chain answers in — logical time, a
//! transaction hash, an amount, whether a block can still be taken back — plus the two records the
//! funding graph is built out of. A type used by exactly one crate stays in that crate; what lands
//! here is what two crates that do not depend on each other would otherwise define twice.
//!
//! # What this crate does not do
//!
//! It models no address: [`sting_address`] holds the one address model, and this crate re-exports
//! the names the contour reads rather than restating them. It reaches no network, opens no file
//! and validates nothing — a value here is a fact a caller has already established.

mod chain;
mod date;
mod graph;

pub use chain::{Finality, JettonAmount, Lt, Nanoton, TxHash};
pub use date::epoch_seconds_to_ymd;
pub use graph::{FundingEdge, TransferTrace};
pub use sting_address::{AccountKind, AddressError, JettonWalletAddr, OwnerId, TonAddress};
