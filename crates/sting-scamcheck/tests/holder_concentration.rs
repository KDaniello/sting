//! `sting_scamcheck::holder_concentration` against a local mock HTTP
//! server — same convention `sting-corpus` already uses for its own
//! `ToncenterClient` tests (`crates/sting-corpus/tests/
//! retry_against_mock_server.rs`), not a live call to `toncenter.com` in
//! the automated suite. The scripted response body's shape (`jetton_wallets:
//! [{address, owner, balance}, ...]`, `balance` as a numeric *string*) and
//! the real, observed `sort=desc`-gives-descending-balance behavior this
//! module relies on were both confirmed against the real, live API
//! (`docs/domains/scamcheck.md`, and the module doc
//! comment's own 2026-07-23 note) before writing this test — this is not
//! a guessed shape.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "test-only recomputation of the same human-facing ratio HolderConcentrationSignal::top_holders_share itself deliberately allows"
)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::thread;

use sting_scamcheck::holder_concentration::fetch_top_holders_concentration_from;
use tonlib_core::TonAddress;

/// Starts a throwaway HTTP/1.1 server on `127.0.0.1` replying once with
/// `body` (a full raw HTTP response, status line included), then stops —
/// same minimal hand-rolled approach as `sting-corpus`'s own mock server
/// test, for the same reason (a tiny scripted exchange, no dependency
/// needed for it). Returns the base URL, a handle to the request's own
/// **request line** (e.g. `"GET /jetton/wallets?jetton_address=...
/// HTTP/1.1"`) so callers can assert on it — an earlier version of this
/// helper discarded the request entirely, which meant a bug dropping
/// `sort=desc`/`limit`/`jetton_address` (or mis-encoding the address)
/// would have passed every test silently (found on review, fixed here,
/// not discovered by the tests themselves) — and a join handle for the
/// server thread.
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

/// Real balances observed 2026-07-23 from `toncenter.com/api/v3/jetton/
/// wallets?jetton_address=<USD₮>&limit=6&sort=desc` (see the module doc
/// comment) — used verbatim as the scripted body, not invented numbers.
fn real_top_holders_response_body() -> Vec<u8> {
    let json = r#"{"jetton_wallets":[
        {"address":"0:1111111111111111111111111111111111111111111111111111111111111111","owner":"0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","balance":"759187920649145"},
        {"address":"0:2222222222222222222222222222222222222222222222222222222222222222","owner":"0:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","balance":"140435052495000"},
        {"address":"0:3333333333333333333333333333333333333333333333333333333333333333","owner":"0:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","balance":"54234267256910"},
        {"address":"0:4444444444444444444444444444444444444444444444444444444444444444","owner":"0:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","balance":"49426484677294"},
        {"address":"0:5555555555555555555555555555555555555555555555555555555555555555","owner":"0:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","balance":"19978001360623"},
        {"address":"0:6666666666666666666666666666666666666666666666666666666666666666","owner":"0:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff","balance":"19737186435225"}
    ]}"#;
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        json.len(),
        json
    )
    .into_bytes()
}

#[test]
fn sums_real_observed_balances_and_computes_the_correct_share() {
    let (base_url, rx, handle) = serve_one_response(real_top_holders_response_body());
    let jetton_master = TonAddress::new(0, [0x99; 32].into());

    // Real USD₮ total_supply ballpark is irrelevant here — a fixed,
    // round total_supply isolates the arithmetic under test.
    let total_supply: u128 = 2_000_000_000_000_000;
    let signal = fetch_top_holders_concentration_from(&base_url, &jetton_master, 6, total_supply)
        .expect("fetch_top_holders_concentration_from failed");

    let expected_sum: u128 = 759_187_920_649_145
        + 140_435_052_495_000
        + 54_234_267_256_910
        + 49_426_484_677_294
        + 19_978_001_360_623
        + 19_737_186_435_225;
    assert_eq!(signal.holders_fetched, 6);
    assert_eq!(signal.top_holders_balance_sum, expected_sum);

    let expected_share = expected_sum as f64 / total_supply as f64;
    let share = signal.top_holders_share().expect("non-zero total_supply");
    assert!(
        (share - expected_share).abs() < 1e-9,
        "expected share {expected_share}, got {share}"
    );

    // The actual outgoing request, not just the canned response — proves
    // `jetton_address`/`limit`/`sort=desc` are really sent, not silently
    // dropped or mis-encoded (see `serve_one_response`'s doc comment).
    let request_line = rx.recv().expect("server captured a request line");
    assert!(
        request_line.starts_with("GET /jetton/wallets?"),
        "unexpected request line: {request_line:?}"
    );
    // `to_hex()`'s `:` is percent-encoded by ureq's query builder
    // (`0:999...` -> `0%3A999...`) — checked against the real encoding
    // above, not guessed.
    assert!(
        request_line.contains(
            "jetton_address=0%3A9999999999999999999999999999999999999999999999999999999999999999"
        ),
        "request line missing jetton_address: {request_line:?}"
    );
    assert!(
        request_line.contains("limit=6"),
        "request line missing limit=6: {request_line:?}"
    );
    assert!(
        request_line.contains("sort=desc"),
        "request line missing sort=desc: {request_line:?}"
    );

    handle.join().expect("mock server thread joins cleanly");
}

#[test]
fn a_status_the_client_cannot_read_is_an_error_rather_than_a_panic() {
    // A client error rather than a server one: a `5xx` is the service saying
    // it could not answer this time and is tried again — `sting-chain`'s own
    // tests cover that — so a stub serving one response would be answering a
    // retry that never comes.
    let body =
        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec();
    let (base_url, _rx, handle) = serve_one_response(body);
    let jetton_master = TonAddress::new(0, [0x99; 32].into());

    let result = fetch_top_holders_concentration_from(&base_url, &jetton_master, 6, 100);
    assert!(matches!(
        result,
        Err(
            sting_scamcheck::holder_concentration::HolderConcentrationError::Chain(
                sting_chain::ChainError::UnexpectedStatus { status: 400, .. }
            )
        )
    ));

    handle.join().expect("mock server thread joins cleanly");
}
