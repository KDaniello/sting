//! What can go wrong between a masterchain block and a row, stated once.
//!
//! Every variant here is a fact the caller can act on: a page that did not parse names the field,
//! a value read back from storage names the value and the column it came from. An error that only
//! says "ingestion failed" tells an operator to go and read the logs, which is what an error is
//! supposed to save them from.

use std::fmt;

/// Everything the ingestion path returns instead of a row.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// The chain client refused, or the host did.
    #[error("chain request failed: {0}")]
    Chain(#[from] sting_chain::ChainError),
    /// The storage backend refused.
    #[error("storage refused: {0}")]
    Storage(#[from] clickhouse::error::Error),
    /// A response body was not the JSON this source expects.
    #[error("malformed response: {0}")]
    Json(#[from] serde_json::Error),
    /// A base64 field did not decode.
    #[error("base64 field did not decode: {0}")]
    Base64(#[from] base64::DecodeError),
    /// A snapshot could not be written, read, or held against what was committed.
    ///
    /// One variant for the three, because they are one failure from a caller's side: the check that
    /// says a fixed range replays to one file could not be carried out, or was carried out and the
    /// two files differ.
    #[error("{0}")]
    Snapshot(String),
    /// A hex field did not decode.
    #[error("hex field did not decode: {0}")]
    Hex(#[from] hex::FromHexError),
    /// An address could not be parsed out of a response or a row.
    #[error("address {text:?} did not parse: {source}")]
    Address {
        /// What was in the field.
        text: String,
        /// Why the address model refused it.
        source: sting_sybil_types::AddressError,
    },
    /// A hash field held the wrong number of bytes.
    #[error("{field} must be 32 bytes, got {len}")]
    HashWidth {
        /// Which field.
        field: &'static str,
        /// How many bytes it actually held.
        len: usize,
    },
    /// A stored enumeration held a value this schema never writes.
    #[error("{column}={value} is not a value this schema writes")]
    UnknownCode {
        /// Which column.
        column: &'static str,
        /// What was in it.
        value: u8,
    },
    /// The writer was handed a finality the schema deliberately does not store.
    #[error(
        "the store keeps only confirmed and finalized rows, and was handed {state}: a provisional \
         row that reached storage is a row nothing can take back"
    )]
    UnstorableFinality {
        /// The state that was refused.
        state: &'static str,
    },
    /// A background mutation did not finish inside the window the caller waited.
    #[error("mutations on {table} did not finish within {waited_ms} ms")]
    MutationTimeout {
        /// Which table was being waited on.
        table: String,
        /// How long the wait lasted.
        waited_ms: u64,
    },
    /// The consumer of the follower's channel went away.
    #[error("nobody is reading the block channel any more")]
    ChannelClosed,
    /// A range was asked for backwards.
    #[error("range {from}..={to} runs backwards")]
    BackwardsRange {
        /// The lower bound as given.
        from: u32,
        /// The upper bound as given.
        to: u32,
    },
    /// A block the source was asked for is not one it holds.
    #[error("no block {seqno} in this source")]
    NoSuchBlock {
        /// Which masterchain sequence number.
        seqno: u32,
    },
    /// A fetch failed in a way the caller is expected to retry.
    #[error("transient failure fetching block {seqno}")]
    Transient {
        /// Which masterchain sequence number.
        seqno: u32,
    },
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for IngestError {
    fn from(_: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Self::ChannelClosed
    }
}

/// What a caller gets back from every fallible step of ingestion.
pub type Result<T> = std::result::Result<T, IngestError>;

/// Names a finality state for [`IngestError::UnstorableFinality`], so the message says which one
/// was refused rather than that one was.
pub(crate) fn state_name(state: sting_sybil_types::Finality) -> &'static str {
    match state {
        sting_sybil_types::Finality::Pending => "pending",
        sting_sybil_types::Finality::Confirmed => "confirmed",
        sting_sybil_types::Finality::Finalized => "finalized",
        sting_sybil_types::Finality::Invalidated => "invalidated",
    }
}

impl fmt::Display for crate::Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::In => "in",
            Self::Out => "out",
        })
    }
}
