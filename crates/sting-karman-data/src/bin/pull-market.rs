//! Turns a node's answers about one lending market into a fixture.
//!
//! Reads on standard input what a node said, and writes the decoded market to the path given.
//! The reaching is the shell's — the calls are listed in [`sting_karman_data`] — and the reading is
//! this crate's, because that is the half where a mistake is silent.

use std::{io::Read, path::PathBuf};

use serde::Deserialize;
use sting_karman_data::{Listing, Origin, PulledMarket, Terms};

/// What a node was asked, and what it said.
#[derive(Deserialize)]
struct Answers {
    origin: Origin,
    /// What the calls that say how the market is listed answered.
    terms: Listing,
    /// What the market getter answered.
    market: String,
    /// What the position getter answered, per holder.
    holders: Vec<Answered>,
}

#[derive(Deserialize)]
struct Answered {
    holder: String,
    answer: String,
}

fn main() -> Result<(), String> {
    let into: PathBuf = std::env::args()
        .nth(1)
        .ok_or("usage: pull-market <fixture path>, with the node's answers on standard input")?
        .into();

    let mut said = String::new();
    std::io::stdin()
        .read_to_string(&mut said)
        .map_err(|e| format!("nothing readable arrived: {e}"))?;
    let answers: Answers =
        serde_json::from_str(&said).map_err(|e| format!("the answers are not readable: {e}"))?;

    let terms = Terms::decode(&answers.terms)
        .map_err(|e| format!("the listing getters answered something else: {e}"))?;
    let mut pulled = PulledMarket::decode(answers.origin, Some(terms), &answers.market)
        .map_err(|e| format!("the market getter answered something else: {e}"))?;
    for answered in &answers.holders {
        pulled = pulled
            .with_holder(&answered.holder, &answered.answer)
            .map_err(|e| {
                format!(
                    "the position getter answered something else for {}: {e}",
                    answered.holder
                )
            })?;
    }
    // Asked for and thrown away: it is derived on the way into a state rather than stored, and
    // a pull that cannot be reconciled should fail here rather than in whatever reads it later.
    pulled
        .unaccounted("unnamed")
        .map_err(|e| format!("this pull does not reconcile: {e}"))?;

    sting_karman_data::write_json_fixture(&into, &pulled).map_err(|e| format!("{e}"))?;
    Ok(())
}
