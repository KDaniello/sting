//! A block source over TON Center's v3 REST endpoint.
//!
//! The streaming API was tried first and does not answer this question: a subscription requires
//! an `addresses` field, which makes it a watchlist rather than a firehose. What ingestion needs
//! is every transaction of a block regardless of who is in it, and `transactionsByMasterchainBlock`
//! is the endpoint that gives it.
//!
//! Everything from here is [`Finality::Finalized`]: v3 REST answers with committed history and
//! has no notion of a pending transaction. The provisional half of the store's contract is
//! exercised against synthetic blocks instead, which is the only place it can be exercised
//! deliberately.
//!
//! The field shapes below were read off real responses rather than off documentation, and they
//! are not what the documentation implies: `lt`, `value` and `created_lt` arrive as strings
//! rather than JSON numbers — a large `u64` does not survive a double — hashes and message bodies
//! arrive base64 rather than hex, and `opcode` is a hex string or `null`.

use base64::Engine;
use serde::Deserialize;
use sting_chain::{AsyncChainClient, Endpoint, Network, Provider};
use sting_sybil_types::{Finality, TonAddress, TxHash};

use crate::error::{IngestError, Result};
use crate::{BlockBatch, Direction, RawMessage, RawTx};

/// Transactions per page. A block under load holds more than one page, so the caller paginates
/// rather than trusting a single request: a quiet block answered with 13 transactions at this
/// limit, which proves nothing about a busy one.
const PAGE_LIMIT: u32 = 256;

/// Reads masterchain blocks from TON Center v3.
pub struct TonCenterV3Source {
    client: AsyncChainClient,
}

impl TonCenterV3Source {
    /// Mainnet, with whichever key the environment carries for it.
    ///
    /// The key, the pace and the retries are `sting-chain`'s: this source used to hold all three
    /// itself, and three copies of a retry policy answer a refusal three different ways.
    #[must_use]
    pub fn new() -> Self {
        Self::for_network(Network::Mainnet)
    }

    /// The same source against `network`.
    #[must_use]
    pub fn for_network(network: Network) -> Self {
        Self::over(Endpoint::public(Provider::ToncenterV3, network))
    }

    /// Against another host — a stub server, or a self-hosted instance.
    #[must_use]
    pub fn with_base_url(base_url: String) -> Self {
        Self::over(Endpoint::at(
            Provider::ToncenterV3,
            Network::Mainnet,
            base_url,
        ))
    }

    fn over(endpoint: Endpoint) -> Self {
        Self {
            client: AsyncChainClient::keyed(endpoint, &sting_chain::ApiKeys::from_env()),
        }
    }

    /// Parses one page of `transactionsByMasterchainBlock` with no network involved.
    ///
    /// The page length comes back beside the rows because it is what decides whether to ask for
    /// another page, and counting the rows afterwards answers a different question the moment a
    /// transaction is ever dropped during the fold.
    ///
    /// # Errors
    /// [`IngestError`] when the body is not this endpoint's JSON, or a field in it does not hold
    /// what its shape promises.
    pub fn parse_transactions_page(
        body: &str,
        mc_seqno: u32,
    ) -> Result<(Vec<RawTx>, Vec<RawMessage>, usize)> {
        Self::fold_transactions_page(serde_json::from_str(body)?, mc_seqno)
    }

    /// The same fold over an already-parsed page, which is what the live path has: the client
    /// hands back a value rather than a body.
    fn fold_transactions_page(
        parsed: TxListResponse,
        mc_seqno: u32,
    ) -> Result<(Vec<RawTx>, Vec<RawMessage>, usize)> {
        let page_len = parsed.transactions.len();
        let mut txs = Vec::with_capacity(page_len);
        let mut messages = Vec::new();

        for tx in parsed.transactions {
            let account = parse_account(&tx.account)?;
            let tx_hash = parse_hash_b64(&tx.hash)?;

            if let Some(in_msg) = tx.in_msg {
                messages.push(msg_dto_to_raw(in_msg, tx_hash, Direction::In, mc_seqno)?);
            }
            for out_msg in tx.out_msgs {
                messages.push(msg_dto_to_raw(out_msg, tx_hash, Direction::Out, mc_seqno)?);
            }

            txs.push(RawTx {
                account,
                lt: tx.lt,
                tx_hash,
                mc_seqno,
                now: tx.now,
                finality: Finality::Finalized,
            });
        }
        Ok((txs, messages, page_len))
    }
}

impl Default for TonCenterV3Source {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::BlockSource for TonCenterV3Source {
    async fn masterchain_info(&self) -> Result<crate::MasterchainInfo> {
        let answer: MasterchainInfoResponse =
            self.client.get_json("/masterchainInfo", Vec::new()).await?;
        Ok(crate::MasterchainInfo {
            last_seqno: answer.last.seqno,
        })
    }

    async fn fetch_masterchain_block(&self, seqno: u32) -> Result<BlockBatch> {
        let mut txs = Vec::new();
        let mut messages = Vec::new();
        let mut offset = 0u32;

        loop {
            let page: TxListResponse = self
                .client
                .get_json(
                    "/transactionsByMasterchainBlock",
                    vec![
                        ("seqno".to_string(), seqno.to_string()),
                        ("limit".to_string(), PAGE_LIMIT.to_string()),
                        ("offset".to_string(), offset.to_string()),
                    ],
                )
                .await?;

            let (page_txs, page_messages, page_len) = Self::fold_transactions_page(page, seqno)?;
            txs.extend(page_txs);
            messages.extend(page_messages);

            if u32::try_from(page_len).unwrap_or(u32::MAX) < PAGE_LIMIT {
                break;
            }
            offset += PAGE_LIMIT;
        }

        Ok(BlockBatch {
            seqno,
            finality: Finality::Finalized,
            txs,
            messages,
        })
    }
}

fn parse_account(text: &str) -> Result<TonAddress> {
    // v3 answers with upper-case hex ("0:06395D8D..."). Parsing delegates to tonlib-core, which
    // is almost certainly case-insensitive, and lower-casing first costs nothing to be sure.
    TonAddress::from_raw_string(&text.to_lowercase()).map_err(|source| IngestError::Address {
        text: text.to_string(),
        source,
    })
}

fn parse_hash_b64(text: &str) -> Result<TxHash> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(text)?;
    bytes
        .try_into()
        .map_err(|wrong: Vec<u8>| IngestError::HashWidth {
            field: "hash",
            len: wrong.len(),
        })
}

fn msg_dto_to_raw(
    msg: MsgDto,
    tx_hash: TxHash,
    direction: Direction,
    mc_seqno: u32,
) -> Result<RawMessage> {
    // No captured response has ever carried a null source or destination, but the shape allows
    // one — an external-in message has no on-chain sender — so the zero address stands in for it
    // rather than the parse failing on a message that is genuinely addressed from nowhere.
    let zero_addr = || TonAddress::new(0, [0u8; 32]);
    let src = msg
        .source
        .as_deref()
        .map(parse_account)
        .transpose()?
        .unwrap_or_else(zero_addr);
    let dst = msg
        .destination
        .as_deref()
        .map(parse_account)
        .transpose()?
        .unwrap_or_else(zero_addr);
    let body_boc = base64::engine::general_purpose::STANDARD.decode(msg.message_content.body)?;

    Ok(RawMessage {
        tx_hash,
        direction,
        src,
        dst,
        value_ton: msg.value,
        op_code: msg.opcode.unwrap_or(0),
        body_boc,
        created_lt: msg.created_lt,
        mc_seqno,
        finality: Finality::Finalized,
    })
}

#[derive(Deserialize)]
struct MasterchainInfoResponse {
    last: BlockRefDto,
}

#[derive(Deserialize)]
struct BlockRefDto {
    seqno: u32,
}

#[derive(Deserialize)]
struct TxListResponse {
    transactions: Vec<TxDto>,
}

#[derive(Deserialize)]
struct TxDto {
    account: String,
    hash: String,
    #[serde(deserialize_with = "de_u64_str")]
    lt: u64,
    now: u32,
    #[serde(default)]
    in_msg: Option<MsgDto>,
    #[serde(default)]
    out_msgs: Vec<MsgDto>,
}

/// An external-in message really does answer with `null` for `value` and `created_lt` — it is a
/// function call rather than a transfer, and it has no place in a chain of internal messages — so
/// zero here means "carried none", not "lost during parsing".
#[derive(Deserialize)]
struct MsgDto {
    source: Option<String>,
    destination: Option<String>,
    #[serde(deserialize_with = "de_u64_str_or_zero")]
    value: u64,
    #[serde(deserialize_with = "de_u64_str_or_zero")]
    created_lt: u64,
    #[serde(deserialize_with = "de_opt_hex_u32")]
    opcode: Option<u32>,
    message_content: MsgContentDto,
}

#[derive(Deserialize)]
struct MsgContentDto {
    body: String,
}

fn de_u64_str<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    text.parse().map_err(serde::de::Error::custom)
}

/// As `de_u64_str`, with `null` reading as zero for the reason given on [`MsgDto`].
fn de_u64_str_or_zero<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let field = Option::<String>::deserialize(deserializer)?;
    field
        .map(|text| text.parse().map_err(serde::de::Error::custom))
        .transpose()
        .map(|value| value.unwrap_or(0))
}

fn de_opt_hex_u32<'de, D>(deserializer: D) -> std::result::Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let field = Option::<String>::deserialize(deserializer)?;
    field
        .map(|text| {
            let digits = text.strip_prefix("0x").unwrap_or(&text);
            u32::from_str_radix(digits, 16).map_err(serde::de::Error::custom)
        })
        .transpose()
}
