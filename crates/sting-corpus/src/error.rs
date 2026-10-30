//! What resolving a contract's code through a [`crate::ContractSource`] can fail as.

/// A failed resolution.
///
/// The transport's own failures are one variant rather than four of their own:
/// a request that did not complete, was refused, or answered a status nobody
/// reads belongs to `sting-chain`, and repeating its shapes here would be the
/// same enum written twice. What stays is what is about a contract — a missing
/// field, a field in the wrong encoding, an address that is not one.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// The request itself failed. See `sting_chain::ChainError`.
    #[error(transparent)]
    Chain(#[from] sting_chain::ChainError),
    /// The answer parsed, and the field this crate needs is not in it — an
    /// account that does not exist on-chain, or one with no code deployed.
    #[error(
        "{provider}: response is missing expected field {field:?} (account may not exist or have no code deployed)"
    )]
    MissingField {
        /// Which service answered.
        provider: &'static str,
        /// The field that was not there.
        field: &'static str,
    },
    /// A field expected to be hex was not hex. `tonapi` answers in hex where
    /// `toncenter` answers in base64 — see [`TonapiClient`](crate::TonapiClient).
    #[error("{provider}: {field} field was not valid hex: {source}")]
    Hex {
        /// Which service answered.
        provider: &'static str,
        /// The field that failed to decode.
        field: &'static str,
        /// The underlying decode error.
        #[source]
        source: hex::FromHexError,
    },
    /// The address carries a character outside every real TON address format,
    /// refused locally rather than after a round trip that answers something
    /// harder to read back to a typo.
    #[error(
        "{provider}: {address:?} is not a valid TON address (expected raw \
         `<workchain>:<64 hex chars>` or a base64/base64url \"friendly\" address)"
    )]
    InvalidAddress {
        /// Which service the address was going to be asked of.
        provider: &'static str,
        /// The rejected address text.
        address: String,
    },
}
