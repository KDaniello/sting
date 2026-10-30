//! `sting_scamcheck::wallet_reputation` against a local mock HTTP server —
//! same convention `tests/holder_concentration.rs` already uses (a
//! hand-rolled one-shot TCP server, not a live call to `tonapi.io` in the
//! automated suite). The scripted response body below is a real, complete
//! `GET tonapi.io/v2/accounts/<addr>/events?limit=2` response for
//! `corpus/fixtures/emulator/SOURCE.md`'s own NFT-collection fixture
//! address, captured live on 2026-07-23 — not an invented shape.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::thread;

use sting_scamcheck::wallet_reputation::fetch_wallet_reputation_from;
use tonlib_core::TonAddress;

/// Same minimal one-shot mock server as `tests/holder_concentration.rs`'s
/// own `serve_one_response` — duplicated rather than shared, since neither
/// test crate exposes a shared test-support module today and this is only
/// ~25 lines (extend to a shared helper if a third test needs it, not
/// speculatively).
fn serve_one_response(
    body: Vec<u8>,
) -> (
    String,
    std::sync::mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
    let port = listener.local_addr().expect("has a local addr").port();
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accepts a connection");
        let mut buf = [0_u8; 4096];
        let mut total = 0;
        loop {
            let n = stream
                .read(&mut buf[total..])
                .expect("reads from the socket");
            total += n;
            if n == 0 || buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let request_line = String::from_utf8_lossy(&buf[..total])
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        tx.send(request_line)
            .expect("send the captured request line");
        stream
            .write_all(&body)
            .expect("writes the scripted response");
        stream.flush().expect("flushes the response");
    });
    (format!("http://127.0.0.1:{port}"), rx, handle)
}

/// Real, complete response body for
/// `EQAAGKP_4XHAZPDhMhcYl20GjGE0qptMp3GRcJ9xvfMMFLSZ` (the NFT-collection
/// fixture address, `corpus/fixtures/emulator/SOURCE.md`), captured live
/// 2026-07-23 — used verbatim, not trimmed or hand-edited beyond the HTTP
/// envelope.
fn real_two_event_response_body() -> Vec<u8> {
    let json = r#"{"events":[{"event_id":"9be6a9dbf05964725e91dd279748cea071d5a19b7545f9776ff1162746402a88","account":{"address":"0:0018a3ffe171c064f0e1321718976d068c6134aa9b4ca77191709f71bdf30c14","is_scam":false,"is_wallet":false},"timestamp":1768130245,"actions":[{"type":"SmartContractExec","status":"ok","SmartContractExec":{"executor":{"address":"0:52992b41f808e13a824ab9a0957dd464f8c32cac95e71aba9978852c30e8bf74","is_scam":false,"is_wallet":true},"contract":{"address":"0:0018a3ffe171c064f0e1321718976d068c6134aa9b4ca77191709f71bdf30c14","is_scam":false,"is_wallet":false},"ton_attached":19000000,"gram_attached":19000000,"operation":"0x00000002"}},{"type":"SmartContractExec","status":"ok","SmartContractExec":{"executor":{"address":"0:0018a3ffe171c064f0e1321718976d068c6134aa9b4ca77191709f71bdf30c14","is_scam":false,"is_wallet":false},"contract":{"address":"0:0f4562ad05d46b4eabcc647707b614b9d7e9e69ae5e4a51d94e9e94f7a4db64c","is_scam":false,"is_wallet":false},"ton_attached":10000000,"gram_attached":10000000,"operation":"0x800a5325"}}],"lt":65677143000001},{"event_id":"78570dbf5e4e8b60e49d39de902957e11afd2db09b0e1238ba95d5e81b641c1c","account":{"address":"0:0018a3ffe171c064f0e1321718976d068c6134aa9b4ca77191709f71bdf30c14","is_scam":false,"is_wallet":false},"timestamp":1768130141,"actions":[{"type":"SmartContractExec","status":"ok","SmartContractExec":{"executor":{"address":"0:52992b41f808e13a824ab9a0957dd464f8c32cac95e71aba9978852c30e8bf74","is_scam":false,"is_wallet":true},"contract":{"address":"0:0018a3ffe171c064f0e1321718976d068c6134aa9b4ca77191709f71bdf30c14","is_scam":false,"is_wallet":false},"ton_attached":19000000,"gram_attached":19000000,"operation":"0x00000002"}},{"type":"SmartContractExec","status":"ok","SmartContractExec":{"executor":{"address":"0:0018a3ffe171c064f0e1321718976d068c6134aa9b4ca77191709f71bdf30c14","is_scam":false,"is_wallet":false},"contract":{"address":"0:3f1ed6ff8ad7a5d9de6bf664650eccf4b57f91ab08e859cde1ba5f9cd1105d95","is_scam":false,"is_wallet":false},"ton_attached":10000000,"gram_attached":10000000,"operation":"0x800a5325"}}],"lt":65677097000001}],"next_from":65677097000001}"#;
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        json.len(),
        json
    )
    .into_bytes()
}

#[test]
fn aggregates_a_real_captured_two_event_response() {
    let (base_url, rx, handle) = serve_one_response(real_two_event_response_body());
    let address = TonAddress::from_hex_str(
        "0:0018A3FFE171C064F0E1321718976D068C6134AA9B4CA77191709F71BDF30C14",
    )
    .expect("parse the fixture's own address");

    let signal = fetch_wallet_reputation_from(&base_url, &address, 2)
        .expect("fetch_wallet_reputation_from failed");

    assert_eq!(signal.events_fetched, 2);
    // Three distinct counterparties across both events, the queried
    // address itself excluded: 52992b41...bf74 (repeated in both events,
    // counted once), 0f4562ad...db64c, 3f1ed6ff...05d95 — hand-verified
    // against the real body above.
    assert_eq!(signal.unique_counterparties, 3);
    assert_eq!(signal.oldest_timestamp_in_page, Some(1_768_130_141));
    assert_eq!(signal.newest_timestamp_in_page, Some(1_768_130_245));
    assert!(!signal.any_event_flagged_scam);

    let request_line = rx.recv().expect("server captured a request line");
    assert!(
        request_line.contains("/events?limit=2"),
        "unexpected request line: {request_line:?}"
    );

    handle.join().expect("mock server thread joins cleanly");
}

#[test]
fn a_status_the_client_cannot_read_is_an_error_rather_than_a_panic() {
    // A client error rather than a server one, for the same reason as the
    // holder-concentration test: a `5xx` is retried.
    let body =
        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec();
    let (base_url, _rx, handle) = serve_one_response(body);
    let address = TonAddress::new(0, [0x99; 32].into());

    let result = fetch_wallet_reputation_from(&base_url, &address, 2);
    assert!(matches!(
        result,
        Err(
            sting_scamcheck::wallet_reputation::WalletReputationError::Chain(
                sting_chain::ChainError::UnexpectedStatus { status: 400, .. }
            )
        )
    ));

    handle.join().expect("mock server thread joins cleanly");
}
