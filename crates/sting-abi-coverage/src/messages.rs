//! `MessageReader` — the bodies of real mainnet messages, by transaction or by the page.
//!
//! The same endpoint [`crate::walker`] pages for addresses carries, on every record, the message
//! that caused the transaction: its opcode as the endpoint read it, and the body itself as a bag of
//! cells. One walk answers two questions, so this module reads the second one out of the same
//! response rather than opening a second way to the chain.
//!
//! **The opcode is carried, not trusted.** The endpoint's `opcode` field and the body it comes with
//! are two claims about one message, and this module hands both to its caller: what decodes a body
//! reads the opcode out of the body itself. What the field is good for is counting — how much of
//! the traffic carries an operation at all — without parsing a cell for every record.
//!
//! **What this does not do.** It does not decode, does not cache, and does not deduplicate: a
//! measurement over traffic is a measurement over occurrences, and a reader that quietly collapsed
//! two identical messages would be answering a different question than the one asked.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use sting_chain::{ChainClient, Endpoint, Network, Provider};

// The same policy the address walk uses, and the same one on purpose: both make many requests of
// one endpoint, and two retry policies against one service are two policies waiting to disagree.
use crate::walker::{PAGE_RETRY, WalkerError};

/// One real incoming message, as the endpoint returned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedMessage {
    /// The transaction this message caused, by its hash as the endpoint spells it.
    pub transaction: String,
    /// The account that received it, raw form.
    pub destination: String,
    /// The opcode the endpoint read, when it read one. A message with no opcode is a message with
    /// fewer than 32 bits of body — a plain transfer of value, which is not an operation.
    pub opcode: Option<u32>,
    /// The body as a bag of cells, when the record carried one.
    pub body: Option<Vec<u8>>,
}

/// One page of transactions, as the messages they carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePage {
    /// The incoming messages found, one per transaction that had one.
    pub messages: Vec<ObservedMessage>,
    /// How many transaction records the page held, message or not.
    ///
    /// Counted separately because the difference is the answer to a question: a transaction with no
    /// incoming message is a transaction nobody sent anything to, and calling those "read" or
    /// "unread" would both be wrong.
    pub record_count: usize,
}

/// Reads real messages off mainnet, by the page or by transaction hash.
pub struct MessageReader {
    client: ChainClient,
}

impl MessageReader {
    /// A reader against mainnet.
    #[must_use]
    pub fn new() -> Self {
        Self::for_network(Network::Mainnet)
    }

    /// A reader against `network`, keyed from the environment.
    #[must_use]
    pub fn for_network(network: Network) -> Self {
        Self::over(Endpoint::public(Provider::ToncenterV3, network))
    }

    /// A reader against another host — this module's own tests point it at a stub server.
    #[must_use]
    pub fn with_base_url(base_url: String) -> Self {
        Self::over(Endpoint::at(
            Provider::ToncenterV3,
            Network::Mainnet,
            base_url,
        ))
    }

    /// A reader over `endpoint`.
    #[must_use]
    pub fn over(endpoint: Endpoint) -> Self {
        Self {
            client: ChainClient::keyed(endpoint, &sting_chain::ApiKeys::from_env())
                .with_retry_policy(PAGE_RETRY),
        }
    }

    /// One page of up to `limit` transactions from `offset`, newest first, as their incoming
    /// messages.
    ///
    /// # Errors
    /// See [`WalkerError`].
    pub fn page(&self, limit: u32, offset: u32) -> Result<MessagePage, WalkerError> {
        let body: TransactionsResponse = self.client.get_json(
            "/transactions",
            &[
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
                ("sort", "desc"),
            ],
        )?;
        page_of(body)
    }

    /// The incoming message of one transaction, by hash.
    ///
    /// Returns nothing when the endpoint knows no such transaction, rather than an error: a hash
    /// nobody has is an answer to the question asked.
    ///
    /// # Errors
    /// See [`WalkerError`].
    pub fn by_transaction(&self, hash: &str) -> Result<Option<ObservedMessage>, WalkerError> {
        let body: TransactionsResponse = self
            .client
            .get_json("/transactions", &[("hash", hash), ("limit", "1")])?;
        Ok(page_of(body)?.messages.into_iter().next())
    }
}

impl Default for MessageReader {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads one response into the messages it carries, and how many records it held.
fn page_of(response: TransactionsResponse) -> Result<MessagePage, WalkerError> {
    let record_count = response.transactions.len();
    let mut messages = Vec::new();
    for transaction in response.transactions {
        let Some(incoming) = transaction.in_msg else {
            continue;
        };
        let body = match incoming.message_content.and_then(|content| content.body) {
            Some(text) if !text.is_empty() => Some(
                BASE64
                    .decode(text.as_bytes())
                    .map_err(|source| WalkerError::Base64("toncenter v3 (messages)", source))?,
            ),
            _ => None,
        };
        messages.push(ObservedMessage {
            transaction: transaction.hash,
            destination: incoming.destination.unwrap_or_default(),
            opcode: incoming.opcode.as_deref().and_then(read_opcode),
            body,
        });
    }
    Ok(MessagePage {
        messages,
        record_count,
    })
}

/// The endpoint spells an opcode as hex text, and signed on occasion — `0xffffffff` arrives as
/// `-0x1`. Both are the same 32 bits, and both are read as those bits.
fn read_opcode(text: &str) -> Option<u32> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let digits = digits.strip_prefix("0x").unwrap_or(digits);
    let magnitude = i64::from_str_radix(digits, 16).ok()?;
    let signed = if negative { -magnitude } else { magnitude };
    u32::try_from(signed & 0xffff_ffff).ok()
}

/// The slice of `toncenter`'s transaction record this module reads.
#[derive(serde::Deserialize)]
struct TransactionsResponse {
    transactions: Vec<TransactionRecord>,
}

#[derive(serde::Deserialize)]
struct TransactionRecord {
    hash: String,
    in_msg: Option<IncomingMessage>,
}

#[derive(serde::Deserialize)]
struct IncomingMessage {
    destination: Option<String>,
    opcode: Option<String>,
    message_content: Option<MessageContent>,
}

#[derive(serde::Deserialize)]
struct MessageContent {
    body: Option<String>,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::read_opcode;

    #[test]
    fn an_opcode_reads_the_same_bits_whichever_sign_the_endpoint_gives_it() {
        assert_eq!(read_opcode("0x0f8a7ea5"), Some(0x0f8a_7ea5));
        assert_eq!(read_opcode("-0x1"), Some(0xffff_ffff));
        assert_eq!(read_opcode("0x00000000"), Some(0));
        assert_eq!(read_opcode("not an opcode"), None);
    }
}
