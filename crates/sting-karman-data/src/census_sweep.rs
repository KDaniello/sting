//! Sweeping an indexer for one census of a market that is held rather than traded.
//!
//! What this writes is what the fold reads: one JSON object per line, an indexer's answers kept
//! **verbatim**. The counting is not here — it is [`crate::ton`], where a miscount would be silent
//! and is therefore checked. A page recorded here carries the indexer's own bytes, so that what a
//! census was built from can be compared with what the census says.
//!
//! The reaching was three hundred lines of shell with its own pacing, its own retries and its own
//! key — one of the six ways this platform used to reach a chain. The stages, their names and their
//! format are unchanged, so a sweep interrupted under the old path is resumed by this one.
//!
//! # Why a sweep is written stage by stage
//!
//! A sweep of this market takes hours. Each stage lands in its own file through a `.part`
//! neighbour, and a stage whose file is already there is skipped, so a sweep that failed is
//! continued rather than repeated. The files are numbered because the fold needs the header before
//! the pages it frames, and a directory read in name order is how they reach it.
//!
//! # What this does not do
//!
//! It decides nothing: not what a float is, not which transfer is the second leg of one sale, not
//! what a collection is worth. It fetches and records. It deduplicates nothing across stages,
//! because two indexers answering one question differently is a fact the census reports rather than
//! one this sweep hides.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sting_chain::{ChainClient, Endpoint, Network, Provider};
use thiserror::Error;

/// Rows per call, the documented maximum for both services.
const PAGE: usize = 1000;

/// Why a sweep stopped.
///
/// Every variant names the stage it stopped on: a census missing a page is a collection counted
/// short, and knowing which page went missing is the difference between resuming and starting over.
#[derive(Debug, Error)]
pub enum SweepError {
    /// A call to an indexer failed.
    #[error("{stage}: {source}")]
    Indexer {
        /// The stage being fetched.
        stage: &'static str,
        /// The underlying failure.
        #[source]
        source: sting_chain::ChainError,
    },
    /// A stage file could not be written or read back.
    #[error("{path}: {source}")]
    Stage {
        /// The file involved.
        path: String,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// An answer was not JSON.
    #[error("{stage}: the answer is not readable JSON: {source}")]
    Unreadable {
        /// The stage being read.
        stage: &'static str,
        /// The underlying parse failure.
        #[source]
        source: serde_json::Error,
    },
    /// A field the sweep pages on was not a number.
    ///
    /// Without this the loop comparing it against a page size compares nothing against a number,
    /// which is neither true nor false, and the sweep walks one page forever instead of stopping
    /// where it stopped understanding.
    #[error("{stage}: the indexer answered something this sweep cannot count: {value}")]
    Uncountable {
        /// The stage being read.
        stage: &'static str,
        /// What stood where a number belongs.
        value: String,
    },
    /// The sweep was asked to cover nothing.
    #[error("nothing to cover: name a host to sweep, or name collections outright")]
    NothingToCover,
    /// The rule matched no collection.
    #[error("nothing matched the rule; a census of nothing is not a census")]
    NothingMatched,
}

/// Which service answers which question.
///
/// Neither answers everything this census needs: the first will not page a collection of this
/// market past about ninety thousand items, and the second lists neither transfers nor purchases.
/// Each is asked what it can answer, and the header records both.
#[derive(Debug)]
pub struct Indexers {
    /// Collections, transfers, purchases, and where the chain stands.
    chain: ChainClient,
    /// What a collection holds.
    items: ChainClient,
}

impl Indexers {
    /// The public hosts, keyed and paced by whatever the environment carries.
    #[must_use]
    pub fn public() -> Self {
        Self {
            chain: client(Provider::ToncenterV3, None),
            items: client(Provider::TonapiV2, None),
        }
    }

    /// Both against one host — what a test points at a stub server.
    #[must_use]
    pub fn at(base_url: &str) -> Self {
        Self {
            chain: client(Provider::ToncenterV3, Some(base_url)),
            items: client(Provider::TonapiV2, Some(base_url)),
        }
    }
}

impl Default for Indexers {
    fn default() -> Self {
        Self::public()
    }
}

fn client(provider: Provider, base_url: Option<&str>) -> ChainClient {
    let endpoint = match base_url {
        Some(base_url) => Endpoint::at(provider, Network::Mainnet, base_url),
        None => Endpoint::public(provider, Network::Mainnet),
    };
    ChainClient::keyed(endpoint, &sting_chain::ApiKeys::from_env())
}

/// What a sweep was asked to cover.
#[derive(Debug)]
pub struct SweepPlan {
    /// Where the stages are written.
    pub into: PathBuf,
    /// How far back the window reaches, in days.
    pub days: u32,
    /// A host whose served descriptions decide which collections are covered.
    pub served_by: Option<String>,
    /// Collections named outright, whatever serves them.
    pub also: Vec<String>,
    /// Addresses whose holdings are stock rather than ownership. A class whose issuer went unnamed
    /// reports its undistributed stock as a float that never moves — the finding this census exists
    /// to produce, arrived at by not looking.
    pub issuers: Vec<String>,
}

/// The window a sweep covers, in unix seconds.
#[derive(Clone, Copy)]
struct Window {
    start: i64,
    end: i64,
}

/// Runs the sweep, or continues one that was interrupted.
///
/// # Errors
/// See [`SweepError`].
pub fn run(plan: &SweepPlan) -> Result<(), SweepError> {
    run_against(plan, &Indexers::public())
}

/// The same, against indexers the caller supplies.
///
/// # Errors
/// See [`SweepError`].
pub fn run_against(plan: &SweepPlan, indexers: &Indexers) -> Result<(), SweepError> {
    if plan.served_by.is_none() && plan.also.is_empty() {
        return Err(SweepError::NothingToCover);
    }
    fs::create_dir_all(&plan.into).map_err(|source| SweepError::Stage {
        path: plan.into.display().to_string(),
        source,
    })?;

    let window = header(plan, indexers)?;
    let covered = collections(plan, indexers)?;
    if covered.is_empty() {
        return Err(SweepError::NothingMatched);
    }
    for collection in &covered {
        items(plan, indexers, collection)?;
    }
    for collection in &covered {
        transfers(plan, indexers, collection, window)?;
    }
    purchases(plan, indexers, window)?;
    footer(plan, indexers)
}

/// The version of the line format a sweep writes.
///
/// A sweep is gigabytes of files that are folded weeks later by a different program, and the fold
/// is the half where a mistake is silent. The header says which shape the lines are in, so the
/// fold can refuse a shape it does not know rather than misreading it.
pub const SWEEP_SCHEMA_VERSION: &str = "1.0";

/// Stage 00: when, and where the chain stood.
fn header(plan: &SweepPlan, indexers: &Indexers) -> Result<Window, SweepError> {
    const STAGE: &str = "header";
    let path = plan.into.join("00-header.ndjson");
    if path.is_file() {
        let written = read(&path)?;
        let head = parse(STAGE, written.lines().next().unwrap_or_default())?;
        return Ok(Window {
            start: number(STAGE, &head["window_start"])?,
            end: number(STAGE, &head["window_end"])?,
        });
    }

    let now = parse(
        STAGE,
        &fetch(STAGE, &indexers.chain, "/masterchainInfo", &[])?,
    )?;
    let end = number(STAGE, &now["last"]["gen_utime"])?;
    let window = Window {
        start: end - i64::from(plan.days) * 86_400,
        end,
    };
    let issuers: Vec<String> = plan
        .issuers
        .iter()
        .map(|issuer| issuer.to_lowercase())
        .collect();
    let line = object(&[
        ("page", json("header")),
        ("schema_version", json(SWEEP_SCHEMA_VERSION)),
        ("network", json("mainnet")),
        ("source", json("toncenter v3 and tonapi v2")),
        ("seqno", number(STAGE, &now["last"]["seqno"])?.to_string()),
        ("window_start", window.start.to_string()),
        ("window_end", window.end.to_string()),
        ("issuers", json(&issuers)),
    ]);
    write_stage(plan, "00-header.ndjson", &[line])?;
    Ok(window)
}

/// Stage 10: which collections this census is about.
///
/// Every collection whose own description is served by the named host, plus any named outright. The
/// rule travels with each collection, so a reader can check that it was the rule that applied.
fn collections(plan: &SweepPlan, indexers: &Indexers) -> Result<Vec<String>, SweepError> {
    const STAGE: &str = "collections";
    let path = plan.into.join("10-collections.ndjson");
    if !path.is_file() {
        let mut lines: Vec<String> = Vec::new();
        if let Some(host) = &plan.served_by {
            let mut offset = 0_usize;
            loop {
                let answer = parse(
                    STAGE,
                    &fetch(
                        STAGE,
                        &indexers.chain,
                        "/nft/collections",
                        &[
                            ("limit", &PAGE.to_string()),
                            ("offset", &offset.to_string()),
                        ],
                    )?,
                )?;
                let rows = answer["nft_collections"].as_array().map_or(0, Vec::len);
                lines.extend(kept(&answer, Some(host)));
                offset += rows;
                if rows < PAGE {
                    break;
                }
            }
        }
        for named in &plan.also {
            let answer = parse(
                STAGE,
                &fetch(
                    STAGE,
                    &indexers.chain,
                    "/nft/collections",
                    &[("collection_address", named.as_str())],
                )?,
            )?;
            lines.extend(kept(&answer, None));
        }
        // Named outright and served by the host is one collection, not two.
        let mut seen = BTreeSet::new();
        lines.retain(|line| seen.insert(line.clone()));
        write_stage(plan, "10-collections.ndjson", &lines)?;
    }

    let mut covered = BTreeSet::new();
    for line in read(&path)?.lines() {
        let row = parse(STAGE, line)?;
        if let Some(address) = row["collection"].as_str() {
            covered.insert(address.to_string());
        }
    }
    Ok(covered.into_iter().collect())
}

/// One line per collection the rule kept.
fn kept(answer: &Value, host: Option<&str>) -> Vec<String> {
    let Some(rows) = answer["nft_collections"].as_array() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for row in rows {
        let uri = row["collection_content"]["uri"]
            .as_str()
            .unwrap_or_default();
        if let Some(host) = host
            && !uri.starts_with(&format!("https://{host}/"))
            && !uri.starts_with(&format!("http://{host}/"))
        {
            continue;
        }
        let Some(address) = row["address"].as_str() else {
            continue;
        };
        let owner = row["owner_address"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        lines.push(object(&[
            ("page", json("collection")),
            ("collection", json(address.to_lowercase())),
            (
                "name",
                json(
                    answer["metadata"][address]["token_info"][0]["name"]
                        .as_str()
                        .unwrap_or_default(),
                ),
            ),
            ("content", json(uri)),
            (
                "owner",
                if owner.is_empty() {
                    "null".to_string()
                } else {
                    json(&owner)
                },
            ),
            ("next_index", row["next_item_index"].to_string()),
        ]));
    }
    lines
}

/// Stage 20: what one collection holds.
fn items(plan: &SweepPlan, indexers: &Indexers, collection: &str) -> Result<(), SweepError> {
    const STAGE: &str = "items";
    let stage = format!("20-items-{}.ndjson", short(collection));
    if plan.into.join(&stage).is_file() {
        return Ok(());
    }
    let mut lines = Vec::new();
    let mut offset = 0_usize;
    loop {
        let body = fetch(
            STAGE,
            &indexers.items,
            &format!("/nfts/collections/{collection}/items"),
            &[
                ("limit", &PAGE.to_string()),
                ("offset", &offset.to_string()),
            ],
        )?;
        let answer = parse(STAGE, &body)?;
        let rows = answer["nft_items"].as_array().map_or(0, Vec::len);
        if rows > 0 {
            lines.push(page_line("items", Some(collection), &body));
        }
        offset += rows;
        if rows < PAGE {
            break;
        }
    }
    write_stage(plan, &stage, &lines)
}

/// Stage 30: what changed hands inside the window.
///
/// Walked newest first and paged by the logical time of the last row rather than by an offset: a
/// listing this long grows under the walk, and an offset into a growing listing skips rows.
fn transfers(
    plan: &SweepPlan,
    indexers: &Indexers,
    collection: &str,
    window: Window,
) -> Result<(), SweepError> {
    const STAGE: &str = "transfers";
    let stage = format!("30-transfers-{}.ndjson", short(collection));
    if plan.into.join(&stage).is_file() {
        return Ok(());
    }
    let mut lines = Vec::new();
    let mut cursor: Option<i64> = None;
    loop {
        let mut query: Vec<(&str, String)> = vec![
            ("collection_address", collection.to_string()),
            ("limit", PAGE.to_string()),
            ("sort", "desc".to_string()),
            ("start_utime", window.start.to_string()),
            ("end_utime", window.end.to_string()),
        ];
        if let Some(end_lt) = cursor {
            query.push(("end_lt", end_lt.to_string()));
        }
        let body = fetch(STAGE, &indexers.chain, "/nft/transfers", &borrowed(&query))?;
        let answer = parse(STAGE, &body)?;
        let rows = answer["nft_transfers"].as_array().map_or(0, Vec::len);
        if rows > 0 {
            lines.push(page_line("transfers", Some(collection), &body));
            cursor = Some(number(STAGE, &answer["nft_transfers"][rows - 1]["transaction_lt"])? - 1);
        }
        if rows < PAGE {
            break;
        }
    }
    write_stage(plan, &stage, &lines)
}

/// Stage 40: what was paid.
///
/// The indexer will not filter purchases by collection, so the whole chain's purchases are walked
/// back to the window's start and the ones this census is about are picked out as they are folded.
fn purchases(plan: &SweepPlan, indexers: &Indexers, window: Window) -> Result<(), SweepError> {
    const STAGE: &str = "purchases";
    if plan.into.join("40-purchases.ndjson").is_file() {
        return Ok(());
    }
    let mut lines = Vec::new();
    let mut cursor: Option<i64> = None;
    loop {
        let mut query: Vec<(&str, String)> = vec![
            ("action_type", "nft_purchase".to_string()),
            ("limit", PAGE.to_string()),
            ("sort", "desc".to_string()),
        ];
        if let Some(end_lt) = cursor {
            query.push(("end_lt", end_lt.to_string()));
        }
        let body = fetch(STAGE, &indexers.chain, "/actions", &borrowed(&query))?;
        let answer = parse(STAGE, &body)?;
        let rows = answer["actions"].as_array().map_or(0, Vec::len);
        if rows == 0 {
            break;
        }
        lines.push(page_line("purchases", None, &body));
        cursor = Some(number(STAGE, &answer["actions"][rows - 1]["start_lt"])? - 1);
        let oldest = answer["actions"].as_array().map_or(0, |rows| {
            rows.iter()
                .filter_map(|row| number(STAGE, &row["start_utime"]).ok())
                .min()
                .unwrap_or(0)
        });
        if rows < PAGE || oldest <= window.start {
            break;
        }
    }
    write_stage(plan, "40-purchases.ndjson", &lines)
}

/// Stage 90: where the chain stood when it was over.
fn footer(plan: &SweepPlan, indexers: &Indexers) -> Result<(), SweepError> {
    const STAGE: &str = "footer";
    let now = parse(
        STAGE,
        &fetch(STAGE, &indexers.chain, "/masterchainInfo", &[])?,
    )?;
    let line = format!(
        r#"{{"page":"footer","seqno":{}}}"#,
        number(STAGE, &now["last"]["seqno"])?
    );
    write_stage(plan, "90-footer.ndjson", &[line])
}

/// One page, with the indexer's own bytes inside it.
///
/// Built by hand rather than by serialising a parsed value: re-serialising reorders keys, and a
/// record that cannot be compared byte for byte is not a record of what the indexer said.
fn page_line(page: &str, collection: Option<&str>, body: &str) -> String {
    match collection {
        Some(collection) => format!(
            r#"{{"page":"{page}","collection":"{collection}","answer":{}}}"#,
            body.trim()
        ),
        None => format!(r#"{{"page":"{page}","answer":{}}}"#, body.trim()),
    }
}

/// One JSON object with its keys in the order they are written here.
///
/// Built rather than serialised from a map: a map sorts its keys, and the fold downstream reads
/// these lines as records whose shape a person compares against the sweep that produced them.
/// Alphabetical order would put `page` — the tag that says what a line is — in the middle.
fn object(fields: &[(&str, String)]) -> String {
    let body: Vec<String> = fields
        .iter()
        .map(|(name, value)| format!("\"{name}\":{value}"))
        .collect();
    format!("{{{}}}", body.join(","))
}

/// A value as JSON, escaped the way the standard requires.
fn json<T: serde::Serialize>(value: T) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

fn fetch(
    stage: &'static str,
    client: &ChainClient,
    path: &str,
    query: &[(&str, &str)],
) -> Result<String, SweepError> {
    client
        .get_text(path, query)
        .map_err(|source| SweepError::Indexer { stage, source })
}

fn parse(stage: &'static str, body: &str) -> Result<Value, SweepError> {
    serde_json::from_str(body).map_err(|source| SweepError::Unreadable { stage, source })
}

fn read(path: &Path) -> Result<String, SweepError> {
    fs::read_to_string(path).map_err(|source| SweepError::Stage {
        path: path.display().to_string(),
        source,
    })
}

fn borrowed<'a>(query: &'a [(&'a str, String)]) -> Vec<(&'a str, &'a str)> {
    query
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect()
}

fn number(stage: &'static str, value: &Value) -> Result<i64, SweepError> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .ok_or_else(|| SweepError::Uncountable {
            stage,
            value: value.to_string(),
        })
}

/// A collection's address is too long for a stage name, so a stage is named by the workchain and
/// the leading digits of the hash. The workchain is part of it because the same code and data
/// deployed in two workchains have one hash and are two collections, and this market has such a
/// pair: a name that dropped the workchain would let one overwrite the other.
fn short(collection: &str) -> String {
    collection.replace(':', "-").chars().take(20).collect()
}

/// Writes a stage through a `.part` neighbour, so a sweep that was interrupted is resumed rather
/// than believed.
fn write_stage(plan: &SweepPlan, name: &str, lines: &[String]) -> Result<(), SweepError> {
    let part = plan.into.join(format!("{name}.part"));
    let stage = |source| SweepError::Stage {
        path: part.display().to_string(),
        source,
    };
    let mut file = fs::File::create(&part).map_err(stage)?;
    for line in lines {
        writeln!(file, "{line}").map_err(stage)?;
    }
    file.sync_all().map_err(stage)?;
    drop(file);
    fs::rename(&part, plan.into.join(name)).map_err(stage)
}
