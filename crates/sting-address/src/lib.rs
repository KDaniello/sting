//! One address model for the platform: the raw form, the two kinds of address
//! that must never be confused, and what a contract at an address is.
//!
//! An address existed twice in this tree — once in the clustering contour,
//! where it is a graph node, and once in the security contour, where it is
//! text about to go into a URL. Neither was wrong and both were partial: the
//! first parsed and could not validate, the second validated and could not
//! parse. Two models of one chain fact drift, and the drift shows up as a
//! contract fetched under one spelling and cached under another.
//!
//! # An address is raw, and its friendly form is a rendering
//!
//! One account has four valid user-friendly spellings — bounceable or not,
//! mainnet or testnet — and comparing two of them as text answers a question
//! nobody asked. [`TonAddress`] therefore holds the workchain and the hash,
//! and every friendly form is parsed into that.
//!
//! # What this crate does not do
//!
//! It computes no checksum and implements no base64: parsing a friendly form
//! goes through `tonlib-core`, because a hand-rolled CRC16 is a source of bugs
//! that surface as somebody else's money. It does not classify an address by
//! reaching the chain — [`AccountKind`] is a value a caller has established,
//! not a lookup this crate performs.

mod kind;
mod raw;
mod validate;

pub use kind::{AccountKind, JettonWalletAddr, OwnerId};
pub use raw::{AddressError, TonAddress};
pub use validate::{InvalidAddress, validate_text};
