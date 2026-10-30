//! The same store contract, against a real column store.
//!
//! The invariants themselves — idempotent upsert, a confirmed row promoted to finalized,
//! invalidation reaching only what was provisional, a monotonic checkpoint — are proven against
//! the in-memory twin, where no database is needed. What is answered here is whether they survive
//! the things the twin does not have: a replacing merge tree, `FINAL`, and mutations that are
//! applied in the background after the statement returns.
//!
//! Every test is ignored by default and needs a running column store:
//! `docker compose up -d clickhouse && cargo test -p sting-sybil-ingestion --test
//! clickhouse_acceptance -- --ignored --test-threads=1`.
//!
//! One thread is not a convenience. Every test shares the same three tables and starts by
//! emptying them, so a parallel run would race one test's truncate against another's assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use sting_sybil_ingestion::{BlockBatch, ClickHouseStore, Direction, RawMessage, RawTx, Store};
use sting_sybil_types::{Finality, TonAddress};

fn clickhouse_url() -> String {
    std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".to_string())
}

/// A connection to a migrated, emptied schema, so a test starts from nothing however long the
/// volume behind it has been alive.
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

fn mk_tx(seqno: u32, i: u64, finality: Finality) -> RawTx {
    RawTx {
        account: addr(u8::try_from(seqno % 256).unwrap()),
        lt: u64::from(seqno) * 1000 + i,
        tx_hash: [u8::try_from((u64::from(seqno) + i) % 256).unwrap(); 32],
        mc_seqno: seqno,
        now: seqno,
        finality,
    }
}

fn mk_block(seqno: u32, n: u64, finality: Finality) -> BlockBatch {
    BlockBatch {
        seqno,
        finality,
        txs: (0..n).map(|i| mk_tx(seqno, i, finality)).collect(),
        messages: vec![],
    }
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn migrating_twice_changes_nothing() {
    let store = fresh_store().await;
    store
        .migrate()
        .await
        .expect("a second migration is a no-op, not a failure");
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn what_is_written_is_what_the_summary_reports() {
    let store = fresh_store().await;
    store
        .write_batch(mk_block(100, 3, Finality::Finalized))
        .await
        .unwrap();

    let summary = store.summary().await.unwrap();
    assert_eq!(summary.tx_count, 3);
    assert_eq!(summary.checkpoint, Some(100));
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn the_same_batch_written_twice_leaves_one_copy() {
    let store = fresh_store().await;
    let block = mk_block(100, 4, Finality::Finalized);

    store.write_batch(block.clone()).await.unwrap();
    store.write_batch(block).await.unwrap();

    assert_eq!(
        store.summary().await.unwrap().tx_count,
        4,
        "the replacing merge tree deduplicates under FINAL, as the twin does under its key"
    );
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn a_confirmed_row_becomes_finalized_without_a_second_row() {
    let store = fresh_store().await;

    store
        .write_batch(mk_block(100, 2, Finality::Confirmed))
        .await
        .unwrap();
    assert_eq!(store.summary().await.unwrap().tx_count, 2);

    // The same `(account, lt)` is the same sort key, so the promotion replaces rather than adds.
    store
        .write_batch(mk_block(100, 2, Finality::Finalized))
        .await
        .unwrap();

    assert_eq!(store.summary().await.unwrap().tx_count, 2);
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn the_checkpoint_is_the_last_block_written() {
    let store = fresh_store().await;

    for seqno in 100..=102 {
        store
            .write_batch(mk_block(seqno, 1, Finality::Finalized))
            .await
            .unwrap();
    }

    assert_eq!(store.checkpoint().await.unwrap(), Some(102));
}

#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn invalidation_takes_back_only_what_was_provisional() {
    let store = fresh_store().await;

    let confirmed_tx = mk_tx(200, 0, Finality::Confirmed);
    let finalized_tx = mk_tx(200, 1, Finality::Finalized);
    store
        .write_batch(BlockBatch {
            seqno: 200,
            finality: Finality::Confirmed,
            txs: vec![confirmed_tx.clone(), finalized_tx.clone()],
            messages: vec![],
        })
        .await
        .unwrap();
    assert_eq!(store.summary().await.unwrap().tx_count, 2);

    store
        .invalidate(&[confirmed_tx.tx_hash, finalized_tx.tx_hash])
        .await
        .unwrap();
    // The delete is a background mutation: without this wait the assertion below would be
    // measuring how fast the server happens to be today.
    store
        .wait_for_pending_mutations("transactions")
        .await
        .unwrap();

    assert_eq!(
        store.summary().await.unwrap().tx_count,
        1,
        "the finalized row survives an invalidation aimed at it"
    );
}

fn mk_message(seqno: u32, i: u64, direction: Direction) -> RawMessage {
    RawMessage {
        tx_hash: [u8::try_from((u64::from(seqno) + i) % 256).unwrap(); 32],
        direction,
        src: addr(u8::try_from(seqno % 256).unwrap()),
        dst: addr(u8::try_from((seqno + 1) % 256).unwrap()),
        value_ton: 1_000_000 + i,
        op_code: if i.is_multiple_of(2) { 0 } else { 0x0f8a_7ea5 },
        body_boc: vec![1, 2, 3, u8::try_from(i % 256).unwrap()],
        created_lt: u64::from(seqno) * 1000 + i,
        mc_seqno: seqno,
        finality: Finality::Finalized,
    }
}

/// The round trip that feature extraction depends on: a realistic batch — both directions, a
/// non-empty body, a real opcode — written and then read back over the same range.
///
/// Compared as sets, because a query with no `ORDER BY` promises no order and asserting one would
/// be asserting today's storage layout.
#[tokio::test]
#[ignore = "needs a running column store: docker compose up -d clickhouse"]
async fn rows_read_back_are_the_rows_written() {
    let store = fresh_store().await;

    let mut block = mk_block(150, 3, Finality::Finalized);
    block.messages = vec![
        mk_message(150, 0, Direction::Out),
        mk_message(150, 1, Direction::In),
    ];
    store.write_batch(block.clone()).await.unwrap();
    // Outside the range asked for below, so a fetch that ignored its bounds would be caught.
    store
        .write_batch(mk_block(151, 1, Finality::Finalized))
        .await
        .unwrap();

    let mut fetched_txs = store.fetch_txs_in_seqno_range(150, 150).await.unwrap();
    let mut fetched_messages = store.fetch_messages_in_seqno_range(150, 150).await.unwrap();

    let mut expected_txs = block.txs.clone();
    fetched_txs.sort_by_key(|tx| tx.lt);
    expected_txs.sort_by_key(|tx| tx.lt);
    assert_eq!(fetched_txs, expected_txs);

    let mut expected_messages = block.messages.clone();
    fetched_messages.sort_by_key(|msg| msg.created_lt);
    expected_messages.sort_by_key(|msg| msg.created_lt);
    assert_eq!(fetched_messages, expected_messages);
}

/// What the network path costs, reported rather than gated.
///
/// No threshold is claimed for synthetic writes: the only pace worth gating is the live one, and
/// a number measured against a local container would be a number about this machine. What the
/// figure is for is the comparison with the same replay over the in-memory store, which is orders
/// of magnitude faster — the bottleneck is the round trip per block, not the logic.
#[tokio::test]
#[ignore = "needs a running column store; reports a figure and gates nothing"]
async fn write_throughput_is_reported() {
    let store = fresh_store().await;
    let n_blocks: u32 = 2000;
    let txs_per_block: u64 = 5;

    let start = std::time::Instant::now();
    for seqno in 0..n_blocks {
        store
            .write_batch(mk_block(seqno, txs_per_block, Finality::Finalized))
            .await
            .unwrap();
    }
    let elapsed = start.elapsed();
    let total_txs = u64::from(n_blocks) * txs_per_block;

    #[allow(
        clippy::cast_precision_loss,
        reason = "a count of blocks and transactions at this size is exact in a double, and both figures are rates"
    )]
    {
        eprintln!(
            "write_batch: {n_blocks} blocks / {total_txs} transactions in {elapsed:?} \
             ({:.0} tx/s, {:.0} blocks/s)",
            total_txs as f64 / elapsed.as_secs_f64(),
            f64::from(n_blocks) / elapsed.as_secs_f64(),
        );
    }
}
