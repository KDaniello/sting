//! Turns an indexer's answers about a market of never-sold assets into a census fixture.
//!
//! Reads one JSON object per line — a header, then the pages of the sweep, in the order they were
//! fetched — and writes the folded census to the path given. The reaching is
//! `sweep-ton-census`; the counting is [`sting_karman_data`], because that is the half where a
//! mistake is silent.
//!
//! ```text
//! pull-ton-census <fixture path> [<sweep directory>]
//! ```
//!
//! Given a directory it reads the sweep's own files, in name order, which is why they are
//! numbered. Given none it reads standard input. The directory is the fast path and by a long way:
//! a sweep of this market is several gigabytes across a few hundred files, and pushing that
//! through a pipe spends far more time moving bytes between processes than this program spends
//! reading them.

use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sting_karman_data::{
    AccountId, CensusBuilder, ItemsPage, Listed, PurchasesPage, Sweep, TransfersPage,
};

/// How much of a source to hold at once. Generous, because the lines are pages: one of them can be
/// several megabytes, and a buffer shorter than a line is a system call per fragment of it.
const BUFFER: usize = 1 << 20;

/// One line of the sweep.
#[derive(Deserialize)]
#[serde(tag = "page", rename_all = "snake_case")]
enum Line {
    /// What the sweep is, and where the chain stood when it began. First line, exactly once.
    Header {
        network: String,
        source: String,
        seqno: u32,
        window_start: u64,
        window_end: u64,
        issuers: Vec<AccountId>,
    },
    /// One collection the sweep chose to cover, why it was chosen, and what the chain says about
    /// the collection itself.
    Collection {
        collection: AccountId,
        #[serde(flatten)]
        listed: Listed,
    },
    /// One page of a collection's items.
    Items {
        collection: AccountId,
        answer: ItemsPage,
    },
    /// One page of a collection's transfers inside the window.
    Transfers {
        collection: AccountId,
        answer: TransfersPage,
    },
    /// One page of the chain's purchases inside the window.
    Purchases { answer: PurchasesPage },
    /// Where the chain stood when the sweep finished. Last line, exactly once.
    Footer { seqno: u32 },
}

fn main() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let into: PathBuf = arguments
        .next()
        .ok_or("usage: pull-ton-census <fixture path> [<sweep directory>]")?
        .into();

    let mut census: Option<CensusBuilder> = None;
    match arguments.next() {
        Some(directory) => {
            for file in sweep_files(Path::new(&directory))? {
                let named = file.display().to_string();
                let opened = File::open(&file).map_err(|e| format!("cannot read {named}: {e}"))?;
                fold(&mut census, opened, &named)?;
            }
        }
        None => fold(&mut census, std::io::stdin().lock(), "standard input")?,
    }

    let census = census.ok_or("the sweep has no header, so nothing says what it measured")?;
    let census = census
        .finish()
        .map_err(|e| format!("this sweep does not reconcile: {e}"))?;
    sting_karman_data::write_json_fixture(&into, &census).map_err(|e| format!("{e}"))?;
    // Beside the fixture rather than instead of it: the JSON is what a run reads, and the
    // Markdown is what a person checks the JSON against before quoting a number out of it.
    sting_karman_data::write_text_artifact(&into.with_extension("md"), &census.render())
        .map_err(|e| format!("{e}"))?;
    Ok(())
}

/// The sweep's files, in the order their names put them, which is the order the fold needs: the
/// header frames every page after it.
fn sweep_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let named = directory.display().to_string();
    let mut found: Vec<PathBuf> = fs::read_dir(directory)
        .map_err(|e| format!("cannot read {named}: {e}"))?
        .map(|entry| entry.map(|found| found.path()))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("cannot read {named}: {e}"))?;
    found.retain(|path| path.extension().is_some_and(|of| of == "ndjson"));
    found.sort();
    if found.is_empty() {
        return Err(format!("{named} holds no sweep"));
    }
    Ok(found)
}

/// Folds every line of one source into the census being built.
fn fold(census: &mut Option<CensusBuilder>, from: impl Read, called: &str) -> Result<(), String> {
    for (at, line) in BufReader::with_capacity(BUFFER, from).lines().enumerate() {
        let at = at + 1;
        let line = line.map_err(|e| format!("{called} line {at} could not be read: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let line: Line = serde_json::from_str(&line)
            .map_err(|e| format!("{called} line {at} is not a page of a sweep: {e}"))?;
        apply(census, line).map_err(|e| format!("{called} line {at}: {e}"))?;
    }
    Ok(())
}

/// Folds one line into the census being built, opening it when the header arrives.
fn apply(census: &mut Option<CensusBuilder>, line: Line) -> Result<(), String> {
    if let Line::Header {
        network,
        source,
        seqno,
        window_start,
        window_end,
        issuers,
    } = line
    {
        if census.is_some() {
            return Err("a second header: one sweep, one window, one set of numbers".into());
        }
        let sweep = Sweep {
            network,
            source,
            seqno_before: seqno,
            seqno_after: seqno,
            window_start,
            window_end,
        };
        *census = Some(CensusBuilder::new(sweep, issuers).map_err(|e| e.to_string())?);
        return Ok(());
    }
    let census = census
        .as_mut()
        .ok_or("a page arrived before the header that frames it")?;
    match line {
        Line::Header { .. } => unreachable!("the header is handled above"),
        Line::Collection { collection, listed } => {
            census
                .cover(collection, listed)
                .map_err(|e| e.to_string())?;
        }
        Line::Items { collection, answer } => {
            census
                .count_items(collection, &answer)
                .map_err(|e| e.to_string())?;
        }
        Line::Transfers { collection, answer } => {
            census
                .count_transfers(collection, &answer)
                .map_err(|e| e.to_string())?;
        }
        Line::Purchases { answer } => {
            census.count_purchases(&answer).map_err(|e| e.to_string())?;
        }
        Line::Footer { seqno } => census.ended_at(seqno),
    }
    Ok(())
}
