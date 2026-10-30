//! The asynchronous adapter against the same kind of stub server, because an
//! adapter that compiles is not an adapter that carries a request.
#![cfg(feature = "async")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use sting_chain::{AsyncChainClient, ChainClient, Endpoint, Network, Provider, RetryPolicy};

#[derive(Debug, Deserialize)]
struct Balance {
    balance: String,
}

fn serve(responses: Vec<String>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
    let port = listener.local_addr().expect("has a local addr").port();
    let handle = thread::spawn(move || {
        for body in responses {
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
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("writes the response");
            stream.flush().expect("flushes the response");
        }
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

fn client_at(base_url: String) -> AsyncChainClient {
    AsyncChainClient::over(
        ChainClient::unlimited(Endpoint::at(
            Provider::ToncenterV3,
            Network::Mainnet,
            base_url,
        ))
        .with_retry_policy(RetryPolicy {
            max_attempts: 2,
            initial_backoff: Duration::from_millis(5),
            max_backoff: Duration::from_millis(20),
        }),
    )
}

#[tokio::test]
async fn an_awaited_call_carries_the_same_request_the_core_would() {
    let (base_url, server) = serve(vec![r#"{"balance":"7"}"#.to_string()]);
    let client = client_at(base_url);

    let balance: Balance = client
        .get_json(
            "/account",
            vec![("address".to_string(), "0:abc".to_string())],
        )
        .await
        .expect("the adapter carries the call");

    assert_eq!(balance.balance, "7");
    server.join().expect("the server thread does not panic");
}

#[tokio::test]
async fn two_calls_share_one_client_and_therefore_one_pace() {
    let (base_url, server) = serve(vec![
        r#"{"balance":"1"}"#.to_string(),
        r#"{"balance":"2"}"#.to_string(),
    ]);
    let client = client_at(base_url);
    let second = client.clone();

    // Concurrent from the caller's side, sequential against the service — the
    // pace is the client's and a clone does not get one of its own.
    let (first, other) = tokio::join!(
        client.get_json::<Balance>("/account", Vec::new()),
        second.get_json::<Balance>("/account", Vec::new()),
    );

    let mut answered: Vec<String> = vec![
        first.expect("the first call answers").balance,
        other.expect("the second call answers").balance,
    ];
    answered.sort();
    assert_eq!(answered, vec!["1".to_string(), "2".to_string()]);
    server.join().expect("the server thread does not panic");
}
