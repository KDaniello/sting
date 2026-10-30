//! What the v3 REST source reads, against a response that was actually captured.
//!
//! The first three tests run against `corpus/sybil/fixtures/mc_block_78805592.json` — a real
//! `transactionsByMasterchainBlock` answer from mainnet — and need no network. They are what
//! proves the mapping handles the shapes the endpoint really uses rather than the ones its
//! documentation implies: stringified `lt` and `value`, base64 hashes and bodies, a hex-string
//! `opcode`.
//!
//! The rest reach the live endpoint and are ignored by default. A fixture cannot notice that an
//! endpoint has changed its shape, which is the only thing those tests are for.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use sting_sybil_ingestion::{BlockSource, TonCenterV3Source};
use sting_sybil_types::Finality;

fn fixture_json() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/sybil/fixtures/mc_block_78805592.json"
    );
    std::fs::read_to_string(path).expect("the captured mainnet block is checked in")
}

#[test]
fn a_captured_block_parses_with_nothing_dropped() {
    let body = fixture_json();
    let (txs, messages, page_len) =
        TonCenterV3Source::parse_transactions_page(&body, 78_805_592).unwrap();

    assert_eq!(page_len, 13, "the capture holds 13 transactions");
    assert_eq!(txs.len(), 13);
    assert_eq!(
        messages.len(),
        18,
        "11 incoming and 7 outgoing: a broad watch filters none of them"
    );

    for tx in &txs {
        assert_eq!(tx.mc_seqno, 78_805_592);
        assert_eq!(
            tx.finality,
            Finality::Finalized,
            "v3 REST answers with committed history and nothing else"
        );
    }
}

#[test]
fn the_jetton_opcodes_are_captured_byte_exact() {
    // The same four opcodes the synthetic suite asserts, now against a real answer: a constant
    // that matches a fixture proves the constant, and a constant that matches only itself does
    // not.
    const OP_JETTON_TRANSFER: u32 = 0x0f8a_7ea5;
    const OP_JETTON_INTERNAL_TRANSFER: u32 = 0x178d_4519;
    const OP_JETTON_NOTIFY: u32 = 0x7362_d09c;
    const OP_EXCESSES: u32 = 0xd532_76db;

    let (_, messages, _) =
        TonCenterV3Source::parse_transactions_page(&fixture_json(), 78_805_592).unwrap();
    let seen: std::collections::HashSet<u32> = messages.iter().map(|msg| msg.op_code).collect();

    for op in [
        OP_JETTON_TRANSFER,
        OP_JETTON_INTERNAL_TRANSFER,
        OP_JETTON_NOTIFY,
        OP_EXCESSES,
    ] {
        assert!(seen.contains(&op), "opcode {op:#x} is in the capture");
    }
}

#[test]
fn addresses_parse_out_of_upper_case_hex() {
    // v3 answers with upper-case hex ("0:06395D8D..."), and both workchains appear in one block,
    // so a parser that handled only one of them would pass a narrower test than this.
    let (txs, _, _) =
        TonCenterV3Source::parse_transactions_page(&fixture_json(), 78_805_592).unwrap();
    assert!(
        txs.iter().any(|tx| tx.account.workchain == 0),
        "the basechain accounts parsed"
    );
    assert!(
        txs.iter().any(|tx| tx.account.workchain == -1),
        "the masterchain accounts parsed"
    );
}

fn live_source() -> TonCenterV3Source {
    TonCenterV3Source::new()
}

#[tokio::test]
#[ignore = "reaches the live mainnet indexer: run it deliberately, not in the offline suite"]
async fn the_live_tip_is_past_the_captured_block() {
    let info = live_source().masterchain_info().await.unwrap();
    assert!(
        info.last_seqno > 78_805_592,
        "the chain has moved on since the capture"
    );
}

#[tokio::test]
#[ignore = "reaches the live mainnet indexer: run it deliberately, not in the offline suite"]
async fn a_block_just_behind_the_tip_fetches() {
    let source = live_source();
    let tip = source.masterchain_info().await.unwrap().last_seqno;
    // Two blocks back rather than the tip itself: the tip block may not be fully indexed at the
    // moment it is announced, and a test that raced that would fail for the wrong reason.
    let batch = source.fetch_masterchain_block(tip - 2).await.unwrap();
    assert_eq!(batch.seqno, tip - 2);
}
