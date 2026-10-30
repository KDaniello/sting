//! The structural half of clustering over a real column store, in the order the terminal surface
//! runs it: rows written, read back, a gas graph built, components found, a partition written.
//!
//! Ignored by default and needs a running column store:
//! `docker compose up -d clickhouse && cargo test -p sting-sybil-cluster --test
//! clickhouse_acceptance -- --ignored --test-threads=1`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use sting_sybil_cluster::materialize_hard_clusters;
use sting_sybil_cluster::schema::{
    ClusterMembershipRow, build_record_batch, cluster_membership_schema,
};
use sting_sybil_graph::{ExclusionCfg, load_gas_graph_from_range};
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

const SEQNO: u32 = 888;
const HUB: u8 = 2;
const CHILDREN: [u8; 3] = [20, 21, 22];

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn the_whole_clustering_runs_against_a_real_store() {
    let store = fresh_store().await;

    let mut seed_txs = Vec::new();
    let mut seed_messages = Vec::new();
    for (i, &child) in CHILDREN.iter().enumerate() {
        let ts = 5_000 + u32::try_from(i).unwrap_or(0) * 10;
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

    let exclusion_cfg = ExclusionCfg {
        hub_degree_threshold: 10,
        ..ExclusionCfg::default()
    };
    let account_kind = |_owner: OwnerId| AccountKind::Wallet;
    let (graphs, txs, _messages) =
        load_gas_graph_from_range(&store, SEQNO, SEQNO, &exclusion_cfg, account_kind)
            .await
            .unwrap();
    assert_eq!(
        graphs.gas_node_count(),
        4,
        "the hub and the three it funded"
    );

    let snapshot_ts = txs.iter().map(|tx| tx.now).max().unwrap_or(0);
    let hard = materialize_hard_clusters(&graphs.gas, 10_000, 2);
    assert_eq!(hard.len(), 4);

    // The hub connects all three, so the four of them are one component.
    let hub_wcc = hard[&OwnerId(addr(HUB))].wcc_id;
    for &child in &CHILDREN {
        let info = hard[&OwnerId(addr(child))];
        assert_eq!(info.wcc_id, hub_wcc);
        assert_eq!(info.wcc_size, 4);
    }

    let rows: Vec<ClusterMembershipRow> = hard
        .into_iter()
        .map(|(owner, info)| ClusterMembershipRow {
            owner,
            wcc_id: info.wcc_id,
            wcc_size: info.wcc_size,
            snapshot_ts,
        })
        .collect();

    let record_batch = build_record_batch(&rows).unwrap();
    assert_eq!(
        record_batch.schema().as_ref(),
        cluster_membership_schema().as_ref(),
        "the canonical schema holds on the live path too, not only over synthetic rows"
    );

    let dir = std::env::temp_dir().join(format!("sting-sybil-cluster-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = sting_sybil_cluster::persist::write_partition(&dir, &record_batch).unwrap();
    assert!(path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
