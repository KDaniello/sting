//! The same store contract against a real column store.
//!
//! The invariants are not re-proven here. [`crate::InMemoryStore`] is written to be this store's
//! twin — the same upsert keys, the same checkpoint rule, the same refusal to take back a
//! finalized row — and the acceptance suite proves them there, where no database is needed. What
//! the tests against this backend answer is narrower and cannot be answered anywhere else:
//! whether the twin's semantics survive the mapping onto columns.
//!
//! Three simplifications against the literal schema, none of which change what a row means:
//!
//! | In the schema | Here | Why |
//! |---|---|---|
//! | `FixedString(64)`, `Enum8` | `String`, `UInt8` | both map onto `String`/`u8` in any client version; the narrower types do not |
//! | raw `body_boc` bytes | hex text | the same treatment addresses and hashes already get, so one encoding covers every binary column |
//! | `PARTITION BY` | omitted | partitioning is a storage decision and no acceptance threshold reads it |
//!
//! Pending and invalidated rows are never written. Those two states exist to model what is
//! provisional while it is still provisional, and a provisional row that reached storage is a row
//! nothing can take back — so the writer refuses one rather than storing it and hoping.

use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use sting_sybil_types::{Finality, TonAddress, TxHash};

use crate::error::{IngestError, Result, state_name};
use crate::{BlockBatch, Direction, RawMessage, RawTx, Store, StoreSummary};

/// The one stream this ingestion writes; a second follower would carry its own name.
const CHECKPOINT_STREAM: &str = "main";

/// How long to wait between asking whether background mutations have finished, and how many
/// times. Fifty polls at 100 ms is five seconds: long enough for a mutation over a test-sized
/// table, short enough that a caller waiting on a stuck one is told rather than hung.
const MUTATION_POLL_MS: u64 = 100;
/// The same interval as a [`Duration`].
const MUTATION_POLL: Duration = Duration::from_millis(MUTATION_POLL_MS);
/// How many polls before the wait is refused.
const MUTATION_POLLS: u32 = 50;

/// Raw rows in a column store.
pub struct ClickHouseStore {
    client: Client,
}

impl ClickHouseStore {
    /// Connects to `url` as the development user.
    ///
    /// The password is the local development one the compose file already sets, and it is not a
    /// secret. It is spelled out because the image disables network access for a passwordless
    /// `default` account entirely, so omitting it fails at connect time with a message about
    /// something else.
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            client: Client::default()
                .with_url(url)
                .with_user("default")
                .with_password("dev"),
        }
    }

    /// Creates the schema, and does nothing when it is already there.
    ///
    /// # Errors
    /// [`IngestError::Storage`] when the backend refuses a statement.
    pub async fn migrate(&self) -> Result<()> {
        self.client
            .query(
                "CREATE TABLE IF NOT EXISTS transactions (
                    account     String,
                    lt          UInt64,
                    tx_hash     String,
                    mc_seqno    UInt32,
                    now         UInt32,
                    finality    UInt8,
                    ingested_at DateTime DEFAULT now()
                ) ENGINE = ReplacingMergeTree(ingested_at)
                ORDER BY (account, lt)",
            )
            .execute()
            .await?;

        self.client
            .query(
                "CREATE TABLE IF NOT EXISTS messages (
                    tx_hash    String,
                    direction  UInt8,
                    src        String,
                    dst        String,
                    value_ton  UInt64,
                    op_code    UInt32,
                    body_boc   String,
                    created_lt UInt64,
                    mc_seqno   UInt32,
                    finality   UInt8
                ) ENGINE = ReplacingMergeTree
                ORDER BY (tx_hash, direction, created_lt)",
            )
            .execute()
            .await?;

        self.client
            .query(
                "CREATE TABLE IF NOT EXISTS checkpoint (
                    stream     String,
                    seqno      UInt32,
                    finality   UInt8,
                    updated_at DateTime DEFAULT now()
                ) ENGINE = ReplacingMergeTree(updated_at)
                ORDER BY stream",
            )
            .execute()
            .await?;

        Ok(())
    }

    /// Empties all three tables.
    ///
    /// Tests need it because the volume outlives the run: rows from a previous run would
    /// otherwise be counted by the next one and the count would drift upwards forever.
    ///
    /// # Errors
    /// [`IngestError::Storage`] when the backend refuses.
    pub async fn truncate_all(&self) -> Result<()> {
        for table in ["transactions", "messages", "checkpoint"] {
            self.client
                .query(&format!("TRUNCATE TABLE {table}"))
                .execute()
                .await?;
        }
        Ok(())
    }

    /// Waits until the background mutations on `table` have finished.
    ///
    /// `ALTER TABLE ... DELETE`, which is how [`Store::invalidate`] works, is applied
    /// asynchronously: the statement returns before the rows are gone. Anyone who needs the
    /// invalidation to have happened — not only a test — has to be able to wait for it, and a
    /// wait with no bound would hang on a mutation that is stuck rather than slow.
    ///
    /// # Errors
    /// [`IngestError::MutationTimeout`] when they have not finished within the window, and
    /// [`IngestError::Storage`] when the backend refuses the question.
    pub async fn wait_for_pending_mutations(&self, table: &str) -> Result<()> {
        #[derive(Row, Deserialize)]
        struct PendingRow {
            c: u64,
        }

        for _ in 0..MUTATION_POLLS {
            let mut cursor = self
                .client
                .query("SELECT count() AS c FROM system.mutations WHERE table = ? AND is_done = 0")
                .bind(table)
                .fetch::<PendingRow>()?;
            let pending = cursor.next().await?.map_or(0, |row| row.c);
            if pending == 0 {
                return Ok(());
            }
            tokio::time::sleep(MUTATION_POLL).await;
        }
        Err(IngestError::MutationTimeout {
            table: table.to_string(),
            waited_ms: u64::from(MUTATION_POLLS) * MUTATION_POLL_MS,
        })
    }

    /// Reads transactions back over a range of masterchain blocks.
    ///
    /// Feature extraction reads raw rows, and the column mapping lives here and only here: a
    /// second mapping in the crate that consumes them would drift from this one silently, and the
    /// first sign of it would be a feature computed off a column that means something else.
    ///
    /// # Errors
    /// [`IngestError`] when the backend refuses, or a stored value is one this schema never
    /// writes.
    pub async fn fetch_txs_in_seqno_range(&self, from: u32, to: u32) -> Result<Vec<RawTx>> {
        let mut cursor = self
            .client
            .query(
                "SELECT account, lt, tx_hash, mc_seqno, now, finality FROM transactions FINAL \
                 WHERE mc_seqno BETWEEN ? AND ?",
            )
            .bind(from)
            .bind(to)
            .fetch::<TxRow>()?;

        let mut out = Vec::new();
        while let Some(row) = cursor.next().await? {
            out.push(RawTx {
                account: parse_stored_address(&row.account)?,
                lt: row.lt,
                tx_hash: hex_to_tx_hash(&row.tx_hash)?,
                mc_seqno: row.mc_seqno,
                now: row.now,
                finality: finality_from_u8(row.finality)?,
            });
        }
        Ok(out)
    }

    /// The same for messages, for the reason given on [`Self::fetch_txs_in_seqno_range`].
    ///
    /// # Errors
    /// [`IngestError`] when the backend refuses, or a stored value is one this schema never
    /// writes.
    pub async fn fetch_messages_in_seqno_range(
        &self,
        from: u32,
        to: u32,
    ) -> Result<Vec<RawMessage>> {
        let mut cursor = self
            .client
            .query(
                "SELECT tx_hash, direction, src, dst, value_ton, op_code, body_boc, created_lt, \
                 mc_seqno, finality FROM messages FINAL WHERE mc_seqno BETWEEN ? AND ?",
            )
            .bind(from)
            .bind(to)
            .fetch::<MessageRow>()?;

        let mut out = Vec::new();
        while let Some(row) = cursor.next().await? {
            out.push(RawMessage {
                tx_hash: hex_to_tx_hash(&row.tx_hash)?,
                direction: direction_from_u8(row.direction)?,
                src: parse_stored_address(&row.src)?,
                dst: parse_stored_address(&row.dst)?,
                value_ton: row.value_ton,
                op_code: row.op_code,
                body_boc: hex::decode(&row.body_boc)?,
                created_lt: row.created_lt,
                mc_seqno: row.mc_seqno,
                finality: finality_from_u8(row.finality)?,
            });
        }
        Ok(out)
    }
}

fn parse_stored_address(text: &str) -> Result<TonAddress> {
    TonAddress::from_raw_string(text).map_err(|source| IngestError::Address {
        text: text.to_string(),
        source,
    })
}

fn finality_to_u8(state: Finality) -> Result<u8> {
    match state {
        Finality::Confirmed => Ok(1),
        Finality::Finalized => Ok(2),
        other => Err(IngestError::UnstorableFinality {
            state: state_name(other),
        }),
    }
}

fn direction_to_u8(direction: Direction) -> u8 {
    match direction {
        Direction::In => 1,
        Direction::Out => 2,
    }
}

fn finality_from_u8(value: u8) -> Result<Finality> {
    match value {
        1 => Ok(Finality::Confirmed),
        2 => Ok(Finality::Finalized),
        other => Err(IngestError::UnknownCode {
            column: "finality",
            value: other,
        }),
    }
}

fn direction_from_u8(value: u8) -> Result<Direction> {
    match value {
        1 => Ok(Direction::In),
        2 => Ok(Direction::Out),
        other => Err(IngestError::UnknownCode {
            column: "direction",
            value: other,
        }),
    }
}

fn hex_hash(hash: &TxHash) -> String {
    hex::encode(hash)
}

fn hex_to_tx_hash(text: &str) -> Result<TxHash> {
    let bytes = hex::decode(text)?;
    bytes
        .try_into()
        .map_err(|wrong: Vec<u8>| IngestError::HashWidth {
            field: "tx_hash",
            len: wrong.len(),
        })
}

/// One shape for both directions of the `transactions` mapping — written by `write_batch`, read
/// by `fetch_txs_in_seqno_range` — so that the column list exists once rather than as two sets
/// that drift.
#[derive(Row, Serialize, Deserialize)]
struct TxRow {
    account: String,
    lt: u64,
    tx_hash: String,
    mc_seqno: u32,
    now: u32,
    finality: u8,
}

/// The same, for `messages`.
#[derive(Row, Serialize, Deserialize)]
struct MessageRow {
    tx_hash: String,
    direction: u8,
    src: String,
    dst: String,
    value_ton: u64,
    op_code: u32,
    body_boc: String,
    created_lt: u64,
    mc_seqno: u32,
    finality: u8,
}

#[derive(Row, Serialize)]
struct CheckpointRow {
    stream: String,
    seqno: u32,
    finality: u8,
}

#[derive(Row, Deserialize)]
struct SeqnoRow {
    seqno: u32,
}

#[derive(Row, Deserialize)]
struct CountRow {
    c: u64,
}

#[derive(Row, Deserialize)]
struct OpCodeCountRow {
    op_code: u32,
    c: u64,
}

impl Store for ClickHouseStore {
    async fn write_batch(&self, batch: BlockBatch) -> Result<()> {
        if !batch.txs.is_empty() {
            let mut insert = self.client.insert::<TxRow>("transactions").await?;
            for tx in &batch.txs {
                insert
                    .write(&TxRow {
                        account: tx.account.to_raw_string(),
                        lt: tx.lt,
                        tx_hash: hex_hash(&tx.tx_hash),
                        mc_seqno: tx.mc_seqno,
                        now: tx.now,
                        finality: finality_to_u8(tx.finality)?,
                    })
                    .await?;
            }
            insert.end().await?;
        }

        if !batch.messages.is_empty() {
            let mut insert = self.client.insert::<MessageRow>("messages").await?;
            for msg in &batch.messages {
                insert
                    .write(&MessageRow {
                        tx_hash: hex_hash(&msg.tx_hash),
                        direction: direction_to_u8(msg.direction),
                        src: msg.src.to_raw_string(),
                        dst: msg.dst.to_raw_string(),
                        value_ton: msg.value_ton,
                        op_code: msg.op_code,
                        body_boc: hex::encode(&msg.body_boc),
                        created_lt: msg.created_lt,
                        mc_seqno: msg.mc_seqno,
                        finality: finality_to_u8(msg.finality)?,
                    })
                    .await?;
            }
            insert.end().await?;
        }

        let mut insert = self.client.insert::<CheckpointRow>("checkpoint").await?;
        insert
            .write(&CheckpointRow {
                stream: CHECKPOINT_STREAM.to_string(),
                seqno: batch.seqno,
                finality: finality_to_u8(batch.finality)?,
            })
            .await?;
        insert.end().await?;

        Ok(())
    }

    async fn checkpoint(&self) -> Result<Option<u32>> {
        let mut cursor = self
            .client
            .query("SELECT seqno FROM checkpoint WHERE stream = ? ORDER BY updated_at DESC LIMIT 1")
            .bind(CHECKPOINT_STREAM)
            .fetch::<SeqnoRow>()?;
        Ok(cursor.next().await?.map(|row| row.seqno))
    }

    async fn invalidate(&self, tx_hashes: &[TxHash]) -> Result<()> {
        if tx_hashes.is_empty() {
            return Ok(());
        }
        // `hex::encode` produces only `[0-9a-f]`, so the literal below cannot carry anything a
        // parser would read as syntax: the alphabet rules injection out by construction rather
        // than by trusting where the hashes came from.
        let list = tx_hashes
            .iter()
            .map(|hash| format!("'{}'", hex_hash(hash)))
            .collect::<Vec<_>>()
            .join(",");
        for table in ["transactions", "messages"] {
            self.client
                .query(&format!(
                    "ALTER TABLE {table} DELETE WHERE tx_hash IN ({list}) AND finality != 2"
                ))
                .execute()
                .await?;
        }
        Ok(())
    }

    async fn summary(&self) -> Result<StoreSummary> {
        let tx_count = self
            .client
            .query("SELECT count() AS c FROM transactions FINAL")
            .fetch_one::<CountRow>()
            .await?
            .c;
        let message_count = self
            .client
            .query("SELECT count() AS c FROM messages FINAL")
            .fetch_one::<CountRow>()
            .await?
            .c;

        let mut op_code_counts = BTreeMap::new();
        let mut cursor = self
            .client
            .query("SELECT op_code, count() AS c FROM messages FINAL GROUP BY op_code")
            .fetch::<OpCodeCountRow>()?;
        while let Some(row) = cursor.next().await? {
            op_code_counts.insert(row.op_code, usize::try_from(row.c).unwrap_or(usize::MAX));
        }

        let checkpoint = self.checkpoint().await?;

        Ok(StoreSummary {
            tx_count: usize::try_from(tx_count).unwrap_or(usize::MAX),
            message_count: usize::try_from(message_count).unwrap_or(usize::MAX),
            checkpoint,
            op_code_counts,
        })
    }
}
