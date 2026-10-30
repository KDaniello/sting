//! `abi-layer decode` / `abi-layer decode-sample` — a message read as an operation, and how much of
//! real traffic can be read that way.
//!
//! Two datasets answer the same question under different terms: the one this repository learns from
//! its own corpora, and the one learned from the foundation's catalogue, whose sources are MIT.
//! Both are loaded and both are consulted, and every candidate says which contracts it was seen in,
//! so a reader can tell whose knowledge answered them.
//!
//! What neither does is choose. When two contracts declare one opcode, both readings are printed:
//! preferring one would invent the fact the datasets say they do not have.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context as _;
use base64::Engine as _;
use sting_abi_coverage::MessageReader;
use sting_abi_decode::{Decoded, decode, standard_operation};
use sting_abi_dictionary::OperationDictionary;

use crate::share::percent;

/// How many transactions one page of the sampling walk asks for.
///
/// The endpoint refuses more, which is a limit of theirs rather than a choice of ours.
const PAGE: u32 = 100;

/// How many of the unknown opcodes a sample names.
///
/// The rest are counted, not listed: what a growth pass needs is where the traffic is, and a list
/// long enough to hide that is a list nobody reads.
const UNKNOWN_SHOWN: usize = 20;

/// Reads one body, given as base64, or one transaction, given by hash.
///
/// # Errors
/// When a dataset cannot be read, when the chain cannot be reached, when the transaction is not
/// found, or when the bytes are not a message body.
pub fn run(body: Option<&str>, hash: Option<&str>, dictionaries: &[&Path]) -> anyhow::Result<()> {
    let dictionary = load_dictionaries(dictionaries)?;
    let bytes = match (body, hash) {
        (Some(text), _) => base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .context("the body is not base64")?,
        (None, Some(hash)) => {
            let observed = MessageReader::new()
                .by_transaction(hash)
                .with_context(|| format!("reading transaction {hash}"))?
                .with_context(|| format!("no transaction {hash} on mainnet"))?;
            observed
                .body
                .with_context(|| format!("transaction {hash} carries no incoming body"))?
        }
        (None, None) => anyhow::bail!("nothing to decode: pass --body or --hash"),
    };

    let decoded = decode(&bytes, &dictionary).context("reading the body")?;
    print_decoded(&decoded);
    println!("{}", serde_json::to_string_pretty(&decoded)?);
    Ok(())
}

/// Says what a body turned out to be, in words before JSON.
fn print_decoded(decoded: &Decoded) {
    println!("opcode {:#010x}", decoded.opcode);
    if let Some(standard) = &decoded.standard {
        println!("  {standard}, by TON's own conventions");
    }
    if let Some(bounced) = &decoded.bounced {
        println!(
            "  what bounced: {:#010x} {}",
            bounced.original_opcode,
            if bounced.original_names.is_empty() {
                "(named by no dataset)".to_string()
            } else {
                bounced.original_names.join(", ")
            }
        );
    }
    if decoded.is_unknown() {
        println!("  no contract in either dataset declares it");
        return;
    }
    for operation in &decoded.operations {
        println!(
            "  {} ({} contract(s))",
            operation.name,
            operation.seen_in.len()
        );
        for field in &operation.fields {
            println!("    {}: {:?}", field.name, field.value);
        }
        if let Some(stopped) = &operation.stopped_at {
            println!("    stopped: {stopped:?}");
        }
    }
}

/// What one sampling pass found.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SampleReport {
    /// Which version of this report's shape it is in.
    pub schema_version: String,
    /// How many transaction records were read.
    pub transactions: usize,
    /// How many of them carried an incoming message with an opcode.
    ///
    /// The rest carry no operation at all — a plain transfer of value, or no incoming message —
    /// and are not part of the share below. A denominator that counted them would be measuring how
    /// much of TON's traffic is a transfer, which nobody asked.
    pub with_an_opcode: usize,
    /// How many of those the datasets name.
    pub named: usize,
    /// The share of the messages that carry an operation that nothing names, in per cent to one
    /// decimal.
    ///
    /// The strictest of the three shares below, and the one a decode product is judged by: of the
    /// messages that are an operation at all, how many can be read.
    pub unknown_share_pct: f64,
    /// The same count over every transaction read, in per cent to one decimal.
    ///
    /// A transaction whose message carries no opcode is not an unknown operation — it is a transfer
    /// of value, and saying so is an answer. This share counts those as answered, which is the
    /// reading of "the unknown share over a sample of transactions" that the denominator names.
    pub unknown_share_of_transactions_pct: f64,
    /// How many distinct opcodes the sample carried.
    pub distinct_opcodes: usize,
    /// How many of those the datasets name.
    pub distinct_named: usize,
    /// The share of distinct opcodes that no dataset names, in per cent to one decimal.
    ///
    /// The other way of counting, and it answers a different question: the weighted share says how
    /// much of the traffic can be read, this one says how much of the vocabulary is known. One busy
    /// application moves the first and not the second.
    pub distinct_unknown_share_pct: f64,
    /// How many of the named messages read to the last field of at least one candidate.
    ///
    /// Naming an operation and reading it are two different successes, and a share that counted
    /// only the first would say a message was decoded when what came back was its name and a note
    /// about where the reading stopped.
    pub read_to_the_end: usize,
    /// The opcodes nothing names, most frequent first — where a growth pass starts.
    pub top_unknown: Vec<UnknownOpcode>,
}

/// One opcode no dataset names, and how often the sample saw it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UnknownOpcode {
    /// The opcode, in hex.
    pub opcode: String,
    /// How many messages carried it.
    pub occurrences: usize,
}

/// This report's own format version.
pub const SAMPLE_SCHEMA_VERSION: &str = "1.0";

/// Walks `transactions` of real mainnet traffic and reports how much of it the datasets can read.
///
/// Newest-first from the head of the chain rather than from a fixed range: what is being measured
/// is what a caller of a decode API would send today, and a range chosen months ago measures what
/// the chain was doing then.
///
/// # Errors
/// When a dataset cannot be read, or when the chain cannot be reached.
pub fn run_sample(transactions: u32, dictionaries: &[&Path]) -> anyhow::Result<()> {
    let dictionary = load_dictionaries(dictionaries)?;
    let reader = MessageReader::new();

    let mut seen = 0usize;
    let mut with_an_opcode = 0usize;
    let mut named = 0usize;
    let mut read_to_the_end = 0usize;
    let mut occurrences: BTreeMap<u32, usize> = BTreeMap::new();
    let mut unknown: BTreeMap<u32, usize> = BTreeMap::new();

    let mut offset = 0u32;
    while offset < transactions {
        let page = reader
            .page(PAGE.min(transactions - offset), offset)
            .with_context(|| format!("reading transactions from offset {offset}"))?;
        if page.record_count == 0 {
            eprintln!("the endpoint stopped answering at offset {offset}");
            break;
        }
        seen += page.record_count;
        for message in &page.messages {
            let Some(opcode) = message.opcode else {
                continue;
            };
            with_an_opcode += 1;
            *occurrences.entry(opcode).or_default() += 1;
            // The chain's own conventions count as named: a bounce is not in any dictionary
            // because no contract declares it, and calling it unknown would say this dataset is
            // missing something that was never a contract's to declare.
            if dictionary.lookup(opcode).is_some() || standard_operation(opcode).is_some() {
                named += 1;
                // The body is already here, so reading it costs nothing beyond the parse — and it
                // is the difference between knowing what a message is called and knowing what it
                // says.
                if let Some(body) = &message.body
                    && decode(body, &dictionary).is_ok_and(|decoded| {
                        decoded
                            .operations
                            .iter()
                            .any(|operation| operation.stopped_at.is_none())
                    })
                {
                    read_to_the_end += 1;
                }
            } else {
                *unknown.entry(opcode).or_default() += 1;
            }
        }
        offset += PAGE;
    }

    anyhow::ensure!(
        with_an_opcode > 0,
        "the sample carried no message with an opcode, so there is nothing to measure"
    );
    let mut top: Vec<UnknownOpcode> = unknown
        .iter()
        .map(|(opcode, count)| UnknownOpcode {
            opcode: format!("{opcode:#010x}"),
            occurrences: *count,
        })
        .collect();
    top.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then(left.opcode.cmp(&right.opcode))
    });
    top.truncate(UNKNOWN_SHOWN);

    let distinct_named = occurrences.len() - unknown.len();
    let report = SampleReport {
        schema_version: SAMPLE_SCHEMA_VERSION.to_string(),
        transactions: seen,
        with_an_opcode,
        named,
        unknown_share_pct: percent(with_an_opcode - named, with_an_opcode),
        unknown_share_of_transactions_pct: percent(with_an_opcode - named, seen.max(1)),
        distinct_opcodes: occurrences.len(),
        distinct_named,
        distinct_unknown_share_pct: percent(unknown.len(), occurrences.len()),
        read_to_the_end,
        top_unknown: top,
    };

    println!(
        "{} transaction(s) read, {} carrying an operation",
        report.transactions, report.with_an_opcode
    );
    println!(
        "  named: {} — unknown {:.1}% of the operations, {:.1}% of the transactions",
        report.named, report.unknown_share_pct, report.unknown_share_of_transactions_pct
    );
    println!(
        "  read to the last field: {} of the {} named",
        report.read_to_the_end, report.named
    );
    println!(
        "  distinct opcodes: {} named of {} — unknown {:.1}% of the vocabulary",
        report.distinct_named, report.distinct_opcodes, report.distinct_unknown_share_pct
    );
    for entry in &report.top_unknown {
        println!("    {} — {} message(s)", entry.opcode, entry.occurrences);
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// Loads every dataset named and merges them into one lookup.
///
/// Merged rather than consulted in turn: an opcode two datasets know is an opcode with two
/// candidates, which is what a collision already is, and preferring whichever file was named first
/// would be a preference nobody stated.
fn load_dictionaries(paths: &[&Path]) -> anyhow::Result<OperationDictionary> {
    anyhow::ensure!(
        !paths.is_empty(),
        "no operation dictionary to decode against"
    );
    let mut merged = OperationDictionary::new();
    for path in paths {
        let dictionary = OperationDictionary::load(path)
            .with_context(|| format!("loading {}", path.display()))?;
        merged.absorb(&dictionary);
    }
    Ok(merged)
}
