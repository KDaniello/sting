//! Funding edges against a real column store, and the one question that needs both stores at
//! once: whether wallets a campaign actually funded are visible to this pipeline rather than only
//! to the manifest that created them.
//!
//! Every test is ignored by default and needs a running column store:
//! `docker compose up -d clickhouse && cargo test -p sting-sybil-graph --test
//! clickhouse_acceptance -- --ignored --test-threads=1`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use petgraph::visit::EdgeRef;
use sting_sybil_graph::{ExclusionCfg, FundingEdgeStore, GraphKind, load_gas_graph_from_range};
use sting_sybil_ingestion::{BlockBatch, ClickHouseStore, Direction, RawMessage, RawTx, Store};
use sting_sybil_types::{AccountKind, Finality, FundingEdge, OwnerId, TonAddress};

fn clickhouse_url() -> String {
    std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".to_string())
}

async fn fresh_store() -> FundingEdgeStore {
    let store = FundingEdgeStore::new(&clickhouse_url());
    store
        .migrate()
        .await
        .expect("the column store is up: docker compose up -d clickhouse");
    store.truncate().await.unwrap();
    store
}

fn addr(byte: u8) -> TonAddress {
    TonAddress::new(0, [byte; 32])
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn both_graphs_write_into_one_table_and_stay_apart() {
    let store = fresh_store().await;

    let value_edges = vec![FundingEdge {
        src: OwnerId(addr(1)),
        dst: OwnerId(addr(2)),
        amount: 100,
        jetton_master: Some(addr(99)),
        tx_hash: [1; 32],
        first_ts: 10,
    }];
    let gas_edges = vec![FundingEdge {
        src: OwnerId(addr(3)),
        dst: OwnerId(addr(4)),
        amount: 200,
        jetton_master: None,
        tx_hash: [2; 32],
        first_ts: 20,
    }];

    store
        .write_edges(GraphKind::Value, &value_edges)
        .await
        .unwrap();
    store.write_edges(GraphKind::Gas, &gas_edges).await.unwrap();

    assert_eq!(store.count().await.unwrap(), 2);
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn the_same_edge_written_twice_leaves_one_row() {
    let store = fresh_store().await;
    let edges = vec![FundingEdge {
        src: OwnerId(addr(1)),
        dst: OwnerId(addr(2)),
        amount: 100,
        jetton_master: None,
        tx_hash: [1; 32],
        first_ts: 10,
    }];

    store.write_edges(GraphKind::Value, &edges).await.unwrap();
    store.write_edges(GraphKind::Value, &edges).await.unwrap();

    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn a_native_transfer_stores_no_jetton() {
    let store = fresh_store().await;
    let edges = vec![FundingEdge {
        src: OwnerId(addr(1)),
        dst: OwnerId(addr(2)),
        amount: 100,
        jetton_master: None,
        tx_hash: [1; 32],
        first_ts: 10,
    }];

    store.write_edges(GraphKind::Gas, &edges).await.unwrap();
    assert_eq!(store.count().await.unwrap(), 1);
}

async fn fresh_ingestion_store() -> ClickHouseStore {
    let store = ClickHouseStore::new(&clickhouse_url());
    store
        .migrate()
        .await
        .expect("the column store is up: docker compose up -d clickhouse");
    store.truncate_all().await.unwrap();
    store
}

/// Whether wallets a campaign funded are visible to this pipeline, end to end.
///
/// A labelled campaign is worth nothing if the only place its wallets exist is the manifest that
/// created them: what is asked here is whether the same wallets come back out of the raw store and
/// then appear as nodes of the gas graph with the funder on the incoming edge. The scenario
/// mirrors what a real funding run does — one root address sending each wallet its first native
/// transfer.
#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn campaign_wallets_are_visible_in_the_raw_store_and_in_the_graph() {
    let store = fresh_ingestion_store().await;

    let funder = addr(0xf0);
    let campaign_wallets = [addr(1), addr(2), addr(3)];
    let seqno = 500;

    let mut txs = Vec::new();
    let mut messages = Vec::new();
    for (i, wallet) in campaign_wallets.iter().enumerate() {
        let i = u8::try_from(i).unwrap();
        let tx_hash = [10 + i; 32];
        txs.push(RawTx {
            account: *wallet,
            lt: 1000 + u64::from(i),
            tx_hash,
            mc_seqno: seqno,
            now: 1_700_000_000 + u32::from(i),
            finality: Finality::Finalized,
        });
        messages.push(RawMessage {
            tx_hash,
            direction: Direction::In,
            src: funder,
            dst: *wallet,
            value_ton: 5_000_000_000,
            op_code: 0,
            body_boc: vec![],
            created_lt: 1000 + u64::from(i),
            mc_seqno: seqno,
            finality: Finality::Finalized,
        });
    }

    store
        .write_batch(BlockBatch {
            seqno,
            finality: Finality::Finalized,
            txs,
            messages,
        })
        .await
        .unwrap();

    let fetched_txs = store.fetch_txs_in_seqno_range(seqno, seqno).await.unwrap();
    let seen_accounts: std::collections::HashSet<_> =
        fetched_txs.iter().map(|tx| tx.account).collect();
    for wallet in &campaign_wallets {
        assert!(
            seen_accounts.contains(wallet),
            "campaign wallet {wallet:?} is not in the raw store"
        );
    }

    let exclusion_cfg = ExclusionCfg {
        hub_degree_threshold: 100,
        ..Default::default()
    };
    let (graphs, _txs, _messages) =
        load_gas_graph_from_range(&store, seqno, seqno, &exclusion_cfg, |_| {
            AccountKind::Wallet
        })
        .await
        .unwrap();

    for wallet in &campaign_wallets {
        let owner = OwnerId(*wallet);
        let index = graphs
            .gas
            .node_indices()
            .find(|&index| graphs.gas[index] == owner)
            .unwrap_or_else(|| panic!("campaign wallet {wallet:?} is not a node of the gas graph"));

        let funder_edge = graphs
            .gas
            .edges_directed(index, petgraph::Direction::Incoming)
            .next()
            .unwrap_or_else(|| panic!("campaign wallet {wallet:?} has no funder edge"));
        assert_eq!(
            graphs.gas[funder_edge.source()],
            OwnerId(funder),
            "the funder on the edge is the address that really paid"
        );
    }
}
