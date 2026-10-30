//! What ingestion promises, one property at a time.
//!
//! Each test states a property the crate would still compile without: nothing is lost on the way
//! in, the same range written twice leaves one copy, a restart from the checkpoint lands where an
//! uninterrupted run did, what is finalized is never taken back, and a slow consumer holds the
//! producer rather than the queue growing behind it.
//!
//! One test reaches the live chain and is ignored by default. It is kept because a source that
//! parses a captured fixture proves nothing about an endpoint that has since changed its shape.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::sync::Arc;

use sting_sybil_ingestion::{
    BlockBatch, BlockSource, Direction, FakeBlockSource, InMemoryStore, RawMessage, RawTx, Store,
    ingest_range, run_follower,
};
use sting_sybil_types::{Finality, TonAddress};

fn addr(byte: u8) -> TonAddress {
    TonAddress::new(0, [byte; 32])
}

fn tx_hash(byte: u8) -> [u8; 32] {
    [byte; 32]
}

/// A block of `n` transactions that depend on nothing but the sequence number, so a test that
/// counts them is counting the pipeline rather than the fixture.
fn block_with_n_txs(seqno: u32, n: u32) -> BlockBatch {
    let txs = (0..n)
        .map(|i| RawTx {
            account: addr(u8::try_from(seqno % 256).unwrap()),
            lt: u64::from(seqno) * 1000 + u64::from(i),
            tx_hash: tx_hash(u8::try_from((seqno + i) % 256).unwrap()),
            mc_seqno: seqno,
            now: seqno,
            finality: Finality::Finalized,
        })
        .collect();
    BlockBatch {
        seqno,
        finality: Finality::Finalized,
        txs,
        messages: vec![],
    }
}

fn block_with_finality(seqno: u32, finality: Finality, n: u32) -> BlockBatch {
    let mut block = block_with_n_txs(seqno, n);
    block.finality = finality;
    for tx in &mut block.txs {
        tx.finality = finality;
    }
    block
}

fn block_with_messages(seqno: u32, op_codes: Vec<u32>) -> BlockBatch {
    let messages = op_codes
        .into_iter()
        .enumerate()
        .map(|(i, op_code)| {
            let i = u32::try_from(i).unwrap();
            RawMessage {
                tx_hash: tx_hash(u8::try_from((seqno + i) % 256).unwrap()),
                direction: Direction::In,
                src: addr(1),
                dst: addr(2),
                value_ton: 1_000_000,
                op_code,
                body_boc: vec![],
                created_lt: u64::from(seqno) * 1000 + u64::from(i),
                mc_seqno: seqno,
                finality: Finality::Finalized,
            }
        })
        .collect();
    BlockBatch {
        seqno,
        finality: Finality::Finalized,
        txs: vec![],
        messages,
    }
}

/// Whether the tip is reachable and current right now.
///
/// Deliberately a point check rather than a soak: how far the follower stays behind over an hour
/// is an operational measurement, not something a test suite should be holding a connection open
/// for. What this answers is narrower and still worth having — the endpoint answers, its shape
/// still parses, and the newest transaction in the tip block is seconds old.
#[tokio::test]
#[ignore = "reaches the live mainnet indexer: run it deliberately, not in the offline suite"]
async fn the_live_tip_is_seconds_behind() {
    let source = sting_sybil_ingestion::TonCenterV3Source::new();

    // The clock is read before the requests rather than after them, and that is the whole
    // difference between measuring the chain and measuring ourselves: the client paces itself to
    // ten requests a second and retries what it has to, so a run where other live tests are queued
    // ahead of this one spends seconds inside the call. Reading the clock afterwards charged that
    // wait to the chain and failed at 16 s on a chain whose real lag, sampled twenty times the same
    // day, had a median of 1 s and a maximum of 2.
    let asked_at = u32::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();

    let tip = source.masterchain_info().await.unwrap();
    let batch = source
        .fetch_masterchain_block(tip.last_seqno)
        .await
        .unwrap();
    let newest = batch.txs.iter().map(|tx| tx.now).max().unwrap_or(0);
    let lag = asked_at.saturating_sub(newest);

    assert!(lag < 5, "the tip must be under 5 s old, and is {lag} s");
}

#[tokio::test]
async fn ingesting_a_range_loses_no_transaction() {
    let source = FakeBlockSource::new(vec![
        block_with_n_txs(100, 3),
        block_with_n_txs(101, 5),
        block_with_n_txs(102, 0),
    ]);
    let store = InMemoryStore::default();
    ingest_range(&source, &store, 100, 102).await.unwrap();
    assert_eq!(
        store.summary().await.unwrap().tx_count,
        8,
        "three plus five plus none is eight, and an empty block is not a gap"
    );
}

#[tokio::test]
async fn the_same_range_ingested_twice_leaves_one_copy() {
    let source = FakeBlockSource::new(vec![block_with_n_txs(100, 4), block_with_n_txs(101, 2)]);
    let store = InMemoryStore::default();

    ingest_range(&source, &store, 100, 101).await.unwrap();
    let after_first = store.summary().await.unwrap();

    ingest_range(&source, &store, 100, 101).await.unwrap();
    let after_second = store.summary().await.unwrap();

    assert_eq!(
        after_first, after_second,
        "a second pass writes no duplicate"
    );
}

#[tokio::test]
async fn a_restart_from_the_checkpoint_matches_an_uninterrupted_run() {
    let blocks = vec![
        block_with_n_txs(100, 2),
        block_with_n_txs(101, 3),
        block_with_n_txs(102, 1),
    ];
    let source = FakeBlockSource::new(blocks);

    let continuous = InMemoryStore::default();
    ingest_range(&source, &continuous, 100, 102).await.unwrap();

    // A crash after the first block, then a restart from what the checkpoint says was written.
    let restarted = InMemoryStore::default();
    ingest_range(&source, &restarted, 100, 100).await.unwrap();
    let checkpoint = restarted
        .checkpoint()
        .await
        .unwrap()
        .expect("a written block sets the checkpoint");
    ingest_range(&source, &restarted, checkpoint + 1, 102)
        .await
        .unwrap();

    assert_eq!(
        continuous.summary().await.unwrap(),
        restarted.summary().await.unwrap(),
        "a restart is a seam in the run, not in what the run produced"
    );
}

#[tokio::test]
async fn a_replay_matches_its_recorded_snapshot() {
    let source = FakeBlockSource::new(vec![block_with_n_txs(100, 2), block_with_n_txs(101, 3)]);
    let store = InMemoryStore::default();
    ingest_range(&source, &store, 100, 101).await.unwrap();
    insta::assert_json_snapshot!(store.summary().await.unwrap());
}

#[tokio::test]
async fn two_replays_of_one_range_agree_value_for_value() {
    let source = FakeBlockSource::new(vec![block_with_n_txs(100, 2), block_with_n_txs(101, 3)]);

    let first = InMemoryStore::default();
    ingest_range(&source, &first, 100, 101).await.unwrap();

    let second = InMemoryStore::default();
    ingest_range(&source, &second, 100, 101).await.unwrap();

    assert_eq!(
        first.summary().await.unwrap(),
        second.summary().await.unwrap(),
        "a replay that differed between runs would not be a replay"
    );
}

#[tokio::test]
async fn a_confirmed_row_becomes_finalized_without_a_second_row() {
    let store = InMemoryStore::default();

    let confirmed = block_with_finality(100, Finality::Confirmed, 2);
    store.write_batch(confirmed.clone()).await.unwrap();
    assert_eq!(store.summary().await.unwrap().tx_count, 2);

    // The same `(account, lt)` is the same upsert key, which is the whole promotion mechanism.
    let finalized = block_with_finality(100, Finality::Finalized, 2);
    store.write_batch(finalized).await.unwrap();

    assert_eq!(
        store.summary().await.unwrap().tx_count,
        2,
        "a firmer finality replaces a row rather than adding one"
    );
}

#[tokio::test]
async fn invalidation_takes_back_only_what_was_provisional() {
    let store = InMemoryStore::default();

    let pending = block_with_finality(100, Finality::Pending, 2);
    let pending_hashes: Vec<_> = pending.txs.iter().map(|tx| tx.tx_hash).collect();
    store.write_batch(pending).await.unwrap();

    let finalized_elsewhere = block_with_finality(101, Finality::Finalized, 1);
    store.write_batch(finalized_elsewhere).await.unwrap();
    assert_eq!(store.summary().await.unwrap().tx_count, 3);

    store.invalidate(&pending_hashes).await.unwrap();

    assert_eq!(
        store.summary().await.unwrap().tx_count,
        1,
        "what survives is the finalized transaction from the other block"
    );
}

#[tokio::test]
async fn invalidation_never_reaches_a_finalized_row() {
    let store = InMemoryStore::default();

    let finalized = block_with_finality(100, Finality::Finalized, 1);
    let hashes: Vec<_> = finalized.txs.iter().map(|tx| tx.tx_hash).collect();
    store.write_batch(finalized).await.unwrap();

    store.invalidate(&hashes).await.unwrap();

    assert_eq!(
        store.summary().await.unwrap().tx_count,
        1,
        "finalized is the state nothing takes back"
    );
}

#[tokio::test(start_paused = true)]
async fn a_slow_consumer_holds_the_producer_back() {
    let n_blocks = 10u32;
    let capacity = 2usize;
    let consumer_delay = std::time::Duration::from_millis(100);

    let source = Arc::new(FakeBlockSource::new(
        (100..100 + n_blocks)
            .map(|seqno| block_with_n_txs(seqno, 1))
            .collect(),
    ));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<BlockBatch>(capacity);

    let start = tokio::time::Instant::now();
    let follower = tokio::spawn(run_follower(source, tx, 100, Some(100 + n_blocks - 1)));

    let mut received = 0u32;
    while received < n_blocks {
        rx.recv().await.expect("the channel outlives the producer");
        received += 1;
        tokio::time::sleep(consumer_delay).await;
    }
    follower.await.unwrap().unwrap();
    let elapsed = start.elapsed();

    // Over an unbounded channel the producer would finish almost immediately — the fake source
    // has no delay of its own — and the elapsed time would say nothing. Bounded, the producer
    // cannot run more than `capacity` blocks ahead, so the run costs what the consumer costs.
    let expected_min = consumer_delay * (n_blocks - u32::try_from(capacity).unwrap());
    assert!(
        elapsed >= expected_min,
        "elapsed={elapsed:?}, expected at least {expected_min:?}: the producer was not held back"
    );
}

#[tokio::test]
async fn every_message_is_captured_whatever_its_opcode() {
    // The four TEP-74 opcodes, byte-exact, plus a plain transfer that carries none. Gift and NFT
    // opcodes are deliberately absent: they belong to a rule that is verified elsewhere, and a
    // test that asserted them here would be asserting a list rather than the absence of a filter.
    const OP_TRANSFER: u32 = 0x0f8a_7ea5;
    const OP_INTERNAL_TRANSFER: u32 = 0x178d_4519;
    const OP_TRANSFER_NOTIFICATION: u32 = 0x7362_d09c;
    const OP_EXCESSES: u32 = 0xd532_76db;
    const OP_PLAIN_TRANSFER: u32 = 0;

    let block = block_with_messages(
        100,
        vec![
            OP_TRANSFER,
            OP_INTERNAL_TRANSFER,
            OP_TRANSFER_NOTIFICATION,
            OP_EXCESSES,
            OP_PLAIN_TRANSFER,
        ],
    );
    let source = FakeBlockSource::new(vec![block]);
    let store = InMemoryStore::default();
    ingest_range(&source, &store, 100, 100).await.unwrap();

    let summary = store.summary().await.unwrap();
    for op in [
        OP_TRANSFER,
        OP_INTERNAL_TRANSFER,
        OP_TRANSFER_NOTIFICATION,
        OP_EXCESSES,
        OP_PLAIN_TRANSFER,
    ] {
        assert!(
            summary.op_code_counts.contains_key(&op),
            "opcode {op:#x} was dropped"
        );
    }
    assert_eq!(
        summary.message_count, 5,
        "ingestion filters by nothing, which is what makes it re-derivable"
    );
}

/// A stub server answering `responses` in order, one per connection.
///
/// The retry under test is the chain client's, and a client retries a request rather than a
/// source — so what is made flaky here is a server, which is what is flaky in production.
fn serve(responses: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read as _, Write as _};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
    let port = listener.local_addr().expect("has a local addr").port();
    let handle = std::thread::spawn(move || {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0_u8; 4096];
            let mut total = 0;
            loop {
                let Ok(read) = stream.read(&mut buf[total..]) else {
                    return;
                };
                total += read;
                if read == 0 || buf[..total].windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let reason = if status == 200 { "OK" } else { "Server Error" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

/// One masterchain block with no transactions in it — enough to prove the page was read, which is
/// what a retry has to deliver.
fn empty_page() -> (u16, String) {
    (200, r#"{"transactions":[]}"#.to_string())
}

fn server_error() -> (u16, String) {
    (500, r#"{"error":"try again"}"#.to_string())
}

#[tokio::test]
async fn a_transient_failure_costs_a_retry_rather_than_a_block() {
    let (base_url, server) = serve(vec![server_error(), server_error(), empty_page()]);
    let source = sting_sybil_ingestion::TonCenterV3Source::with_base_url(base_url);
    let store = InMemoryStore::default();

    ingest_range(&source, &store, 100, 100)
        .await
        .expect("two refusals in a row must not lose a block");

    assert_eq!(
        store.summary().await.unwrap().tx_count,
        0,
        "the page really was read: an empty block is zero transactions, not a missing one"
    );
    server.join().expect("the stub server does not panic");
}

#[tokio::test]
async fn a_server_that_never_answers_is_an_error_rather_than_a_silent_gap() {
    // More refusals than the client has attempts. What must not happen is a quiet success: a
    // source that is down is not a range with no data in it.
    let (base_url, server) = serve(vec![
        server_error(),
        server_error(),
        server_error(),
        server_error(),
        server_error(),
        server_error(),
    ]);
    let source = sting_sybil_ingestion::TonCenterV3Source::with_base_url(base_url);
    let store = InMemoryStore::default();

    let result = ingest_range(&source, &store, 100, 100).await;

    assert!(
        result.is_err(),
        "exhausting the attempts must return an error rather than continue quietly"
    );
    assert_eq!(store.summary().await.unwrap().tx_count, 0);
    drop(server);
}

// The property tests below are synchronous bodies: `proptest!` expands into `return Err(..)`,
// which would land in the wrong function inside a nested async block. Each run builds its own
// current-thread runtime, and only plain values cross back out of it.

use proptest::prelude::*;

fn tokio_current_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

proptest! {
    /// Idempotence as a property rather than as one example: however many times the same batch is
    /// written, the store must hold what one write leaves.
    #[test]
    fn prop_repeated_write_of_one_batch_is_idempotent(
        seqno in 100u32..10_000,
        n_txs in 1u32..8,
        repeat_count in 1u32..5,
    ) {
        let rt = tokio_current_thread();
        let (repeated, once) = rt.block_on(async {
            let block = block_with_n_txs(seqno, n_txs);

            let repeated_store = InMemoryStore::default();
            for _ in 0..repeat_count {
                repeated_store.write_batch(block.clone()).await.unwrap();
            }

            let once_store = InMemoryStore::default();
            once_store.write_batch(block.clone()).await.unwrap();

            (
                repeated_store.summary().await.unwrap(),
                once_store.summary().await.unwrap(),
            )
        });
        prop_assert_eq!(repeated, once);
    }

    /// Whatever mixture of finalities is in the store, and whichever hashes are handed to
    /// `invalidate` — here the hardest case, all of them at once — the finalized rows must
    /// survive.
    #[test]
    fn prop_invalidation_never_removes_a_finalized_row(
        finalities in prop::collection::vec(
            prop_oneof![
                Just(Finality::Pending),
                Just(Finality::Confirmed),
                Just(Finality::Finalized),
            ],
            1..8,
        ),
    ) {
        let expected_finalized = finalities.iter().filter(|f| **f == Finality::Finalized).count();

        let rt = tokio_current_thread();
        let remaining = rt.block_on(async {
            let store = InMemoryStore::default();
            let mut all_hashes = Vec::new();
            for (i, finality) in finalities.iter().enumerate() {
                let i = u8::try_from(i).unwrap();
                let tx = RawTx {
                    account: TonAddress::new(0, [i; 32]),
                    lt: u64::from(i),
                    tx_hash: tx_hash(i),
                    mc_seqno: 100,
                    now: 100,
                    finality: *finality,
                };
                all_hashes.push(tx.tx_hash);
                store
                    .write_batch(BlockBatch {
                        seqno: 100,
                        finality: *finality,
                        txs: vec![tx],
                        messages: vec![],
                    })
                    .await
                    .unwrap();
            }

            store.invalidate(&all_hashes).await.unwrap();
            store.summary().await.unwrap().tx_count
        });
        prop_assert_eq!(remaining, expected_finalized);
    }
}
