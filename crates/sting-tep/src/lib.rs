//! The message bodies the standards define, read from cells.
//!
//! A jetton transfer is a shape written down in TEP-74, and reading one is the
//! same work whether a graph is being built from it, a risk signal computed, or
//! an operation named for a person. This crate is that reading, once — the
//! decoding used to sit inside the crate that builds the funding graph, where
//! nothing else could reach it.
//!
//! # What this crate does not do
//!
//! It reaches nothing: a body arrives as bytes and leaves as a value. It reads
//! only the fields the standard's consumers use, and says so per message rather
//! than pretending to a round trip — a body it decodes does not re-encode to
//! itself. It knows no contract's own schema: a message a project invented is
//! not a message here, and asking for one is an error rather than a guess.

pub mod jetton;

pub use jetton::{
    JettonOp, OP_EXCESSES, OP_INTERNAL_TRANSFER, OP_TRANSFER, OP_TRANSFER_NOTIFICATION,
    decode_jetton_body,
};
