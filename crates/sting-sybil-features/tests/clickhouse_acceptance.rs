//! The whole extraction over a real column store, in the order the terminal surface runs it.
//!
//! Rows are written, then read back rather than reused — reading is a separate mapping and the
//! only way to catch a column that round-trips wrong. From there it is the same sequence the
//! command performs: build the gas graph, take the hot wallets out, extract, assemble the batch,
//! write the partition.
//!
//! Ignored by default and needs a running column store:
//! `docker compose up -d clickhouse && cargo test -p sting-sybil-features --test
//! clickhouse_acceptance -- --ignored --test-threads=1`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use sting_sybil_features::persist::write_partition;
use sting_sybil_features::schema::wallet_features_schema;
use sting_sybil_features::{
    FeatureExtractionConfig, TopologyThresholds, build_record_batch, extract_features_for_range,
};
use sting_sybil_graph::{
    ExclusionCfg, FundingGraphs, apply_exclusion, ingest_block_into_gas_graph,
};
use sting_sybil_ingestion::{BlockBatch, ClickHouseStore, Direction, RawMessage, RawTx, Store};
use sting_sybil_types::{AccountKind, Finality, OwnerId, TonAddress};

fn clickhouse_url() -> String {
    std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".to_string())
}

async fn fresh_store() -> ClickHouseStore {
    let store = ClickHouseStore::new(&clickhouse_url());
    store
        .migrate()
        .await
        .expect("the column store is up: docker compose up -d clickhouse");
    store.truncate_all().await.unwrap();
    store
}

fn addr(byte: u8) -> TonAddress {
    TonAddress::new(0, [byte; 32])
}

const SEQNO: u32 = 777;
const HUB: u8 = 2;
const CHILDREN: [u8; 4] = [20, 21, 22, 23];

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn the_whole_extraction_runs_against_a_real_store() {
    let store = fresh_store().await;

    let mut seed_txs = Vec::new();
    let mut seed_messages = Vec::new();
    for (i, &child) in CHILDREN.iter().enumerate() {
        let ts = 1_000 + u32::try_from(i).unwrap_or(0) * 10;
        let hash = [u8::try_from(i + 1).unwrap_or(0); 32];
        for direction in [Direction::Out, Direction::In] {
            seed_messages.push(RawMessage {
                tx_hash: hash,
                direction,
                src: addr(HUB),
                dst: addr(child),
                value_ton: 5_000_000,
                op_code: 0,
                body_boc: vec![],
                created_lt: u64::from(ts),
                mc_seqno: SEQNO,
                finality: Finality::Finalized,
            });
        }
        seed_txs.push(RawTx {
            account: addr(child),
            lt: 1,
            tx_hash: hash,
            mc_seqno: SEQNO,
            now: ts,
            finality: Finality::Finalized,
        });
    }
    store
        .write_batch(BlockBatch {
            seqno: SEQNO,
            finality: Finality::Finalized,
            txs: seed_txs,
            messages: seed_messages,
        })
        .await
        .unwrap();

    // Read back rather than reused: writing and reading are two mappings, and only the round
    // trip says they agree.
    let fetched_txs = store.fetch_txs_in_seqno_range(SEQNO, SEQNO).await.unwrap();
    let fetched_messages = store
        .fetch_messages_in_seqno_range(SEQNO, SEQNO)
        .await
        .unwrap();
    assert_eq!(fetched_txs.len(), 4);
    assert_eq!(
        fetched_messages.len(),
        8,
        "four transfers, each seen from both sides"
    );

    let batch = BlockBatch {
        seqno: SEQNO,
        finality: Finality::Finalized,
        txs: fetched_txs,
        messages: fetched_messages,
    };
    let mut graphs = FundingGraphs::default();
    ingest_block_into_gas_graph(&batch, &mut graphs);
    assert_eq!(graphs.gas_node_count(), 5, "the hub and the four it funded");

    // The hub threshold is deliberately above four: at the default of zero the hub would be
    // excluded as a hub before anything had a chance to read its shape.
    let exclusion_cfg = ExclusionCfg {
        hub_degree_threshold: 10,
        ..ExclusionCfg::default()
    };
    let account_kind = |_owner: OwnerId| AccountKind::Wallet;
    graphs.gas = apply_exclusion(&graphs.gas, &exclusion_cfg, account_kind);
    graphs.value = apply_exclusion(&graphs.value, &exclusion_cfg, account_kind);

    let cfg = FeatureExtractionConfig {
        feature_version: "ch-live-test".to_string(),
        snapshot_ts: batch.txs.iter().map(|tx| tx.now).max().unwrap_or(0),
        burst_factor: 0.5,
        topology: TopologyThresholds {
            star_fan_threshold: 4,
            tree_fan_threshold: 4,
        },
    };
    let rows = extract_features_for_range(&batch.txs, &batch.messages, &[], &graphs, &cfg);
    assert_eq!(rows.len(), 5);

    let record_batch = build_record_batch(&rows).unwrap();
    assert_eq!(
        record_batch.schema().as_ref(),
        wallet_features_schema().as_ref(),
        "the canonical schema holds on the live path too, not only over synthetic rows"
    );

    let hub_row = rows
        .iter()
        .find(|row| row.owner == OwnerId(addr(HUB)))
        .expect("the hub is in the graph and gets a row");
    assert!(hub_row.star_divergence, "four is the threshold here");

    let dir =
        std::env::temp_dir().join(format!("sting-sybil-features-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = write_partition(&dir, &record_batch).unwrap();
    assert!(path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
