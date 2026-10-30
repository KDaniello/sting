//! Writing funding edges out, in the same shape and with the same simplifications as the raw
//! store.
//!
//! Two of them are worth naming. Enumerations are stored as small integers and addresses as text,
//! for the reason the raw store gives: those types map onto the client's own types in any version
//! of it, and the narrower ones do not. An amount is stored as a decimal string rather than a
//! 128-bit column, because a jetton amount is 128 bits wide and the client's handling of that
//! width is not something this crate has verified.

use clickhouse::{Client, Row};
use serde::Serialize;

use crate::Result;
use sting_sybil_types::FundingEdge;

/// Which of the two graphs an edge belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphKind {
    /// The value graph.
    Value,
    /// The gas graph.
    Gas,
}

impl GraphKind {
    fn as_u8(self) -> u8 {
        match self {
            GraphKind::Value => 1,
            GraphKind::Gas => 2,
        }
    }
}

/// Funding edges in a column store.
pub struct FundingEdgeStore {
    client: Client,
}

#[derive(Row, Serialize)]
struct FundingEdgeRow {
    graph: u8,
    src_owner: String,
    dst_owner: String,
    amount: String,
    first_ts: u32,
    jetton: String,
    tx_hash: String,
}

impl FundingEdgeStore {
    /// Connects to `url` as the development user, the same one the raw store uses and for the
    /// same reason.
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            client: Client::default()
                .with_url(url)
                .with_user("default")
                .with_password("dev"),
        }
    }

    /// Creates the table, and does nothing when it is already there.
    ///
    /// # Errors
    /// [`crate::GraphError::Storage`] when the backend refuses.
    pub async fn migrate(&self) -> Result<()> {
        self.client
            .query(
                "CREATE TABLE IF NOT EXISTS funding_edges (
                    graph     UInt8,
                    src_owner String,
                    dst_owner String,
                    amount    String,
                    first_ts  UInt32,
                    jetton    String,
                    tx_hash   String
                ) ENGINE = ReplacingMergeTree
                ORDER BY (graph, src_owner, dst_owner, tx_hash)",
            )
            .execute()
            .await?;
        Ok(())
    }

    /// Empties the table.
    ///
    /// # Errors
    /// [`crate::GraphError::Storage`] when the backend refuses.
    pub async fn truncate(&self) -> Result<()> {
        self.client
            .query("TRUNCATE TABLE funding_edges")
            .execute()
            .await?;
        Ok(())
    }

    /// Writes `edges` as rows of `graph`.
    ///
    /// # Errors
    /// [`crate::GraphError::Storage`] when the backend refuses.
    pub async fn write_edges(&self, graph: GraphKind, edges: &[FundingEdge]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut insert = self
            .client
            .insert::<FundingEdgeRow>("funding_edges")
            .await?;
        for edge in edges {
            insert
                .write(&FundingEdgeRow {
                    graph: graph.as_u8(),
                    src_owner: edge.src.0.to_raw_string(),
                    dst_owner: edge.dst.0.to_raw_string(),
                    amount: edge.amount.to_string(),
                    first_ts: edge.first_ts,
                    jetton: edge
                        .jetton_master
                        .map(|master| master.to_raw_string())
                        .unwrap_or_default(),
                    tx_hash: hex::encode(edge.tx_hash),
                })
                .await?;
        }
        insert.end().await?;
        Ok(())
    }

    /// How many edges are stored.
    ///
    /// # Errors
    /// [`crate::GraphError::Storage`] when the backend refuses.
    pub async fn count(&self) -> Result<u64> {
        #[derive(Row, serde::Deserialize)]
        struct CountRow {
            c: u64,
        }
        Ok(self
            .client
            .query("SELECT count() AS c FROM funding_edges FINAL")
            .fetch_one::<CountRow>()
            .await?
            .c)
    }
}
