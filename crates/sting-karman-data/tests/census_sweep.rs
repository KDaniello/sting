//! The sweep against a stub indexer: what it asks for, what it writes, and what it does when it is
//! run again.
//!
//! A stub on a socket rather than a live indexer: a sweep of the real market takes hours, and a
//! test that reaches a public service to prove a URL is a test that spends somebody's quota to
//! learn what a scripted answer already says.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use sting_karman_data::{Indexers, SweepPlan};

/// Serves the scripted answers in order and reports every request line it read.
fn serve(answers: Vec<&'static str>) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
    let port = listener.local_addr().expect("has a local addr").port();
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let mut answers: VecDeque<&str> = answers.into();
        while let Some(body) = answers.pop_front() {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buffer = [0_u8; 8192];
            let mut total = 0;
            loop {
                let Ok(read) = stream.read(&mut buffer[total..]) else {
                    return;
                };
                total += read;
                if read == 0
                    || buffer[..total]
                        .windows(4)
                        .any(|window| window == b"\r\n\r\n")
                {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&buffer[..total])
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            let _ = sender.send(request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    (format!("http://127.0.0.1:{port}"), receiver, handle)
}

const MASTERCHAIN: &str = r#"{"last":{"seqno":87227287,"gen_utime":1787174683}}"#;
const ONE_COLLECTION: &str = r#"{"nft_collections":[{"address":"0:AAAA","owner_address":"0:BBBB","next_item_index":"3","collection_content":{"uri":"https://nft.example.com/one.json"}}],"metadata":{"0:AAAA":{"token_info":[{"name":"One"}]}}}"#;
const ONE_ITEM_PAGE: &str = r#"{"nft_items":[{"address":"0:1111","owner":{"address":"0:BBBB"}}]}"#;
const ONE_TRANSFER_PAGE: &str =
    r#"{"nft_transfers":[{"nft_address":"0:1111","transaction_lt":"77000000000001"}]}"#;
const ONE_PURCHASE_PAGE: &str =
    r#"{"actions":[{"start_lt":"77000000000002","start_utime":1787000000}]}"#;

fn plan(into: &std::path::Path) -> SweepPlan {
    SweepPlan {
        into: into.to_path_buf(),
        days: 90,
        served_by: Some("nft.example.com".to_string()),
        also: Vec::new(),
        issuers: vec!["0:CCCC".to_string()],
    }
}

/// The order the stages are fetched in, which is the order they are written in.
fn scripted() -> Vec<&'static str> {
    vec![
        MASTERCHAIN,
        ONE_COLLECTION,
        ONE_ITEM_PAGE,
        ONE_TRANSFER_PAGE,
        ONE_PURCHASE_PAGE,
        MASTERCHAIN,
    ]
}

#[test]
fn a_sweep_writes_every_stage_and_keeps_the_indexer_bytes_verbatim() {
    let into = tempfile::tempdir().expect("a directory to sweep into");
    let (base_url, requests, server) = serve(scripted());

    sting_karman_data::sweep_census_against(&plan(into.path()), &Indexers::at(&base_url))
        .expect("the sweep finishes");

    let stage = |name: &str| {
        std::fs::read_to_string(into.path().join(name)).unwrap_or_else(|_| panic!("{name} written"))
    };

    let header: serde_json::Value =
        serde_json::from_str(stage("00-header.ndjson").trim()).expect("the header is one object");
    assert_eq!(header["page"], "header");
    assert_eq!(header["seqno"], 87_227_287);
    assert_eq!(header["window_end"], 1_787_174_683);
    // Ninety days back from where the chain stood, and the issuer folded to lower case: the census
    // compares addresses as text, and two cases of one address are two addresses to it.
    assert_eq!(header["window_start"], 1_787_174_683 - 90 * 86_400);
    assert_eq!(header["issuers"][0], "0:cccc");

    let collection: serde_json::Value = serde_json::from_str(stage("10-collections.ndjson").trim())
        .expect("the collection is one object");
    assert_eq!(collection["collection"], "0:aaaa");
    assert_eq!(collection["name"], "One");
    assert_eq!(collection["owner"], "0:bbbb");

    // The page carries the indexer's own bytes: a record that cannot be compared byte for byte is
    // not a record of what was said.
    let items = stage("20-items-0-aaaa.ndjson");
    assert!(items.contains(ONE_ITEM_PAGE), "{items}");
    let transfers = stage("30-transfers-0-aaaa.ndjson");
    assert!(transfers.contains(ONE_TRANSFER_PAGE), "{transfers}");
    let purchases = stage("40-purchases.ndjson");
    assert!(purchases.contains(ONE_PURCHASE_PAGE), "{purchases}");
    assert_eq!(
        stage("90-footer.ndjson").trim(),
        r#"{"page":"footer","seqno":87227287}"#
    );

    let asked: Vec<String> = requests.try_iter().collect();
    assert_eq!(asked.len(), 6, "{asked:?}");
    assert!(asked[0].starts_with("GET /masterchainInfo "), "{asked:?}");
    assert!(
        asked[1].starts_with("GET /nft/collections?limit=1000&offset=0 "),
        "{asked:?}"
    );
    assert!(
        // Lower case from here on: the collections stage folds an address to one case, because
        // a census comparing addresses as text counts two cases of one address as two.
        asked[2].starts_with("GET /nfts/collections/0:aaaa/items?limit=1000&offset=0 "),
        "{asked:?}"
    );
    // Newest first, inside the window, and no cursor on the first page.
    assert!(
        asked[3].contains("collection_address=0%3Aaaaa")
            && asked[3].contains("sort=desc")
            && asked[3].contains("start_utime=")
            && !asked[3].contains("end_lt="),
        "{asked:?}"
    );
    assert!(
        asked[4].contains("action_type=nft_purchase") && asked[4].contains("sort=desc"),
        "{asked:?}"
    );
    drop(server);
}

#[test]
fn a_stage_already_written_is_not_fetched_again() {
    let into = tempfile::tempdir().expect("a directory to sweep into");
    let (base_url, _requests, server) = serve(scripted());
    sting_karman_data::sweep_census_against(&plan(into.path()), &Indexers::at(&base_url))
        .expect("the first sweep finishes");
    drop(server);

    // The second run is given a server that answers only the footer. Every other stage has to come
    // off the disk, which is what resuming means: a sweep of this market takes hours, and one that
    // re-fetched what it already has would never finish.
    let (base_url, requests, server) = serve(vec![MASTERCHAIN]);
    sting_karman_data::sweep_census_against(&plan(into.path()), &Indexers::at(&base_url))
        .expect("the second sweep finishes off the disk");

    let asked: Vec<String> = requests.try_iter().collect();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0].starts_with("GET /masterchainInfo "), "{asked:?}");
    drop(server);
}

#[test]
fn a_sweep_asked_to_cover_nothing_refuses_rather_than_writing_an_empty_census() {
    let into = tempfile::tempdir().expect("a directory to sweep into");
    let nothing = SweepPlan {
        into: into.path().to_path_buf(),
        days: 90,
        served_by: None,
        also: Vec::new(),
        issuers: Vec::new(),
    };
    let error =
        sting_karman_data::sweep_census_against(&nothing, &Indexers::at("http://127.0.0.1:1"))
            .expect_err("a census of nothing is not a census");
    assert!(error.to_string().contains("nothing to cover"), "{error}");
}

#[test]
fn a_rule_that_matches_no_collection_stops_the_sweep() {
    let into = tempfile::tempdir().expect("a directory to sweep into");
    let (base_url, _requests, server) = serve(vec![MASTERCHAIN, ONE_COLLECTION]);
    let elsewhere = SweepPlan {
        served_by: Some("nft.somewhere-else.com".to_string()),
        ..plan(into.path())
    };

    let error = sting_karman_data::sweep_census_against(&elsewhere, &Indexers::at(&base_url))
        .expect_err("the rule matched nothing");

    assert!(error.to_string().contains("nothing matched"), "{error}");
    drop(server);
}
