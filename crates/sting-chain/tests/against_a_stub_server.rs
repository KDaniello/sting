//! Retries, `Retry-After`, pacing and the key header, against a real socket.
//!
//! A stub server rather than a container: what has to be exercised here is one
//! HTTP exchange, and a hundred lines of `std::net` give it directly, in every
//! run, on every machine. A container would add a dependency the test skips
//! itself on when it is missing, and a skipped test that reads as a pass is the
//! failure mode this repository refuses by name. The container stays where it
//! buys something a socket cannot — a real database with a real schema.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]

use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use sting_chain::{
    ApiKeys, ChainClient, ChainError, Endpoint, Network, Provider, RateLimiter, RetryPolicy,
};

#[derive(Debug, Deserialize)]
struct Balance {
    balance: String,
}

/// Serves `responses` in order, one per connection, and reports every request
/// it read back through the returned receiver.
///
/// Hand-rolled rather than a mock-HTTP crate: the exchange is a status line,
/// two headers and a JSON body, and a dependency would buy nothing that
/// `std::net` does not already give transparently.
fn serve(responses: Vec<Vec<u8>>) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
    let port = listener.local_addr().expect("has a local addr").port();
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().expect("accepts a connection");
            let mut buf = [0_u8; 4096];
            let mut total = 0;
            loop {
                let read = stream
                    .read(&mut buf[total..])
                    .expect("reads from the socket");
                total += read;
                if read == 0 || buf[..total].windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&buf[..total]).into_owned());
            stream
                .write_all(&response)
                .expect("writes the scripted response");
            stream.flush().expect("flushes the response");
        }
    });
    (format!("http://127.0.0.1:{port}"), receiver, handle)
}

fn response(status_line: &str, headers: &[(&str, &str)], body: &str) -> Vec<u8> {
    let mut head = format!(
        "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("Connection: close\r\n\r\n");
    head.into_bytes().into_iter().chain(body.bytes()).collect()
}

/// The body shape toncenter's own `429` returns, confirmed against the live
/// service rather than taken from its documentation.
fn rate_limited() -> Vec<u8> {
    response(
        "429 Too Many Requests",
        &[("Content-Type", "application/json")],
        r#"{"ok":false,"result":"Ratelimit exceed","code":429}"#,
    )
}

fn rate_limited_asking_for(seconds: u32) -> Vec<u8> {
    response(
        "429 Too Many Requests",
        &[
            ("Content-Type", "application/json"),
            ("Retry-After", &seconds.to_string()),
        ],
        r#"{"ok":false,"result":"Ratelimit exceed","code":429}"#,
    )
}

fn ok() -> Vec<u8> {
    response(
        "200 OK",
        &[("Content-Type", "application/json")],
        r#"{"balance":"1"}"#,
    )
}

fn client_at(base_url: String) -> ChainClient {
    ChainClient::unlimited(Endpoint::at(
        Provider::ToncenterV3,
        Network::Mainnet,
        base_url,
    ))
    .with_retry_policy(RetryPolicy {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(5),
        max_backoff: Duration::from_millis(20),
    })
}

#[test]
fn a_refused_call_is_retried_and_the_next_answer_is_the_result() {
    let (base_url, _requests, server) = serve(vec![rate_limited(), ok()]);
    let client = client_at(base_url);

    let balance: Balance = client
        .get_json("/account", &[("address", "0:deadbeef")])
        .expect("succeeds after one retry");

    assert_eq!(balance.balance, "1");
    server.join().expect("the server thread does not panic");
}

#[test]
fn a_server_that_only_refuses_is_given_up_on_by_name() {
    let (base_url, _requests, server) = serve(vec![rate_limited(), rate_limited(), rate_limited()]);
    let client = client_at(base_url);

    let error = client
        .get_json::<Balance>("/account", &[])
        .expect_err("three refusals are three refusals");

    assert!(
        matches!(error, ChainError::RateLimited { attempts: 3, .. }),
        "got {error:?}"
    );
    server.join().expect("the server thread does not panic");
}

#[test]
fn a_server_stating_its_own_delay_is_waited_out_rather_than_guessed_at() {
    let (base_url, _requests, server) = serve(vec![rate_limited_asking_for(1), ok()]);
    // The policy would guess 5 ms; the server asked for a second, and its own
    // number is the one that counts. The ceiling still applies, so the wait is
    // the 200 ms this policy allows rather than the full second.
    let client = ChainClient::unlimited(Endpoint::at(
        Provider::ToncenterV3,
        Network::Mainnet,
        base_url,
    ))
    .with_retry_policy(RetryPolicy {
        max_attempts: 2,
        initial_backoff: Duration::from_millis(5),
        max_backoff: Duration::from_millis(200),
    });

    let started = Instant::now();
    let balance: Balance = client
        .get_json("/account", &[])
        .expect("succeeds on the second attempt");
    let waited = started.elapsed();

    assert_eq!(balance.balance, "1");
    assert!(
        waited >= Duration::from_millis(150),
        "the server's own delay was ignored: waited {waited:?}"
    );
    server.join().expect("the server thread does not panic");
}

#[test]
fn a_status_that_is_neither_success_nor_refusal_is_reported_rather_than_retried() {
    // Three responses are scripted and one is expected to be read: a request
    // answered `400` five times is five wrong requests, not one transient
    // failure. The unread two are what proves it stopped.
    let (base_url, requests, server) =
        serve(vec![response("400 Bad Request", &[], "{}"), ok(), ok()]);
    let client = client_at(base_url);

    let error = client
        .get_json::<Balance>("/account", &[])
        .expect_err("a malformed request is not retried");

    assert!(
        matches!(error, ChainError::UnexpectedStatus { status: 400, .. }),
        "got {error:?}"
    );
    // `try_iter` rather than `iter`: the server thread is still waiting on a
    // connection that will not come, so draining until the channel closes
    // would wait with it.
    assert_eq!(
        requests.try_iter().count(),
        1,
        "the client tried again after a status it cannot read"
    );
    drop(server);
}

#[test]
fn a_server_error_is_tried_again_and_a_client_error_is_not() {
    let (base_url, _requests, server) =
        serve(vec![response("503 Service Unavailable", &[], "{}"), ok()]);
    let client = client_at(base_url);

    let balance: Balance = client
        .get_json("/account", &[])
        .expect("a server that could not answer this time answers the next");

    assert_eq!(balance.balance, "1");
    server.join().expect("the server thread does not panic");
}

#[test]
fn the_query_reaches_the_server_in_the_order_it_was_written() {
    let (base_url, requests, server) = serve(vec![ok()]);
    let client = client_at(base_url);

    let _: Balance = client
        .get_json("/account", &[("address", "0:deadbeef"), ("limit", "1")])
        .expect("succeeds");

    let request = requests.recv().expect("the server read a request");
    assert!(
        request.contains("GET /account?address=0%3Adeadbeef&limit=1 "),
        "{request}"
    );
    server.join().expect("the server thread does not panic");
}

#[test]
fn a_key_travels_in_the_header_its_provider_reads_it_from() {
    let (base_url, requests, server) = serve(vec![ok()]);
    let keys = ApiKeys::from_reader(|name| {
        (name == "TONCENTER_API_KEY_MAINNET").then(|| "the-key".to_string())
    });
    let client = ChainClient::keyed(
        Endpoint::at(Provider::ToncenterV3, Network::Mainnet, base_url),
        &keys,
    )
    .with_rate_limit(RateLimiter::unlimited());

    let _: Balance = client.get_json("/account", &[]).expect("succeeds");

    let request = requests.recv().expect("the server read a request");
    assert!(request.contains("x-api-key: the-key"), "{request}");
    server.join().expect("the server thread does not panic");
}

#[test]
fn the_pace_is_kept_between_calls_rather_than_after_a_refusal() {
    let (base_url, _requests, server) = serve(vec![ok(), ok(), ok()]);
    let client = ChainClient::unlimited(Endpoint::at(
        Provider::ToncenterV3,
        Network::Mainnet,
        base_url,
    ))
    .with_rate_limit(RateLimiter::per_second(20));

    let started = Instant::now();
    for _ in 0..3 {
        let _: Balance = client.get_json("/account", &[]).expect("succeeds");
    }
    let elapsed = started.elapsed();

    // Three calls at twenty a second are two intervals of 50 ms between them.
    assert!(
        elapsed >= Duration::from_millis(90),
        "three calls took {elapsed:?}, which is faster than the pace allows"
    );
    server.join().expect("the server thread does not panic");
}
