//! Telling a collection from a copy of it, when no registry knows either of them.
//!
//! The registry rule answers for the collections somebody has cross-checked by hand, which is
//! nine of them. Most of the market is not in that list and never will be, and the copies are
//! precisely where the money is: a series worth counterfeiting attracts a dozen contracts that
//! carry its name and serve its description.
//!
//! What separates them is not the name and not the description — a copy gets both right — but
//! what the chain says about the supply. An issuer that produced a series and stepped back still
//! holds part of it: items sit at the issuing address because that is where unsold supply lives.
//! A copy has nothing to hold back. It was minted to a handful of addresses and its issuer holds
//! none of it.
//!
//! # What this rule does not do
//!
//! It says nothing about a collection with no namesake: a name nobody has copied is not evidence
//! of anything, in either direction. It does not rank copies by how convincing they are. And it
//! is a claim about a moment — an issuer that has sold its whole stock looks like a copy on this
//! rule, which is why the verdict is only asked for where two contracts claim one name.

use std::collections::HashMap;

/// What an index says about one collection: enough to tell namesakes apart, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRow {
    /// The address the collection stands at.
    pub address: String,
    /// The name it reports.
    pub name: String,
    /// How many of its own items sit at the issuing address.
    pub issuer_held: u64,
}

/// Which of a group of namesakes is the collection and which are copies of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamesakeVerdict {
    /// Every name claimed by more than one contract, and how many claimed it.
    pub contested_names: Vec<(String, usize)>,
    /// The addresses that hold back none of their own supply while a namesake does.
    pub copies: Vec<String>,
    /// The addresses a namesake group settles on: one per contested name, or none where the
    /// group gives no reason to prefer any of them.
    pub originals: Vec<String>,
}

/// Reads a catalogue and separates the namesakes.
///
/// A group where nobody holds back supply produces no verdict at all rather than an arbitrary
/// one: two contracts that both look like copies are two contracts this rule cannot separate, and
/// naming one of them the original would be inventing the answer.
#[must_use]
pub fn namesake_verdict(rows: &[CollectionRow]) -> NamesakeVerdict {
    let mut by_name: HashMap<&str, Vec<&CollectionRow>> = HashMap::new();
    for row in rows {
        by_name.entry(row.name.as_str()).or_default().push(row);
    }

    let mut contested_names = Vec::new();
    let mut copies = Vec::new();
    let mut originals = Vec::new();

    for (name, group) in by_name {
        if group.len() < 2 {
            continue;
        }
        contested_names.push((name.to_string(), group.len()));

        let holders: Vec<&&CollectionRow> = group
            .iter()
            .filter(|row| row.issuer_held > 0)
            .collect::<Vec<_>>();
        if holders.len() != 1 {
            continue;
        }
        originals.push(holders[0].address.clone());
        copies.extend(
            group
                .iter()
                .filter(|row| row.issuer_held == 0)
                .map(|row| row.address.clone()),
        );
    }

    contested_names.sort();
    copies.sort();
    originals.sort();
    NamesakeVerdict {
        contested_names,
        copies,
        originals,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::*;

    fn row(address: &str, name: &str, issuer_held: u64) -> CollectionRow {
        CollectionRow {
            address: address.to_string(),
            name: name.to_string(),
            issuer_held,
        }
    }

    #[test]
    fn a_name_nobody_copied_is_not_contested() {
        let verdict = namesake_verdict(&[row("0:a", "Alone", 5), row("0:b", "Other", 0)]);
        assert!(verdict.contested_names.is_empty());
        assert!(verdict.copies.is_empty());
    }

    #[test]
    fn the_one_holding_back_supply_is_the_original() {
        let verdict = namesake_verdict(&[
            row("0:real", "Series", 100),
            row("0:copy1", "Series", 0),
            row("0:copy2", "Series", 0),
        ]);
        assert_eq!(verdict.contested_names, vec![("Series".to_string(), 3)]);
        assert_eq!(verdict.originals, vec!["0:real".to_string()]);
        assert_eq!(
            verdict.copies,
            vec!["0:copy1".to_string(), "0:copy2".to_string()]
        );
    }

    #[test]
    fn a_group_where_two_hold_back_supply_is_left_alone() {
        let verdict = namesake_verdict(&[
            row("0:one", "Series", 10),
            row("0:two", "Series", 10),
            row("0:three", "Series", 0),
        ]);
        assert_eq!(verdict.contested_names, vec![("Series".to_string(), 3)]);
        assert!(
            verdict.copies.is_empty(),
            "two candidates is a group this rule cannot separate, and it says so"
        );
        assert!(verdict.originals.is_empty());
    }

    #[test]
    fn a_group_where_nobody_holds_back_supply_is_left_alone() {
        let verdict = namesake_verdict(&[row("0:a", "Series", 0), row("0:b", "Series", 0)]);
        assert_eq!(verdict.contested_names, vec![("Series".to_string(), 2)]);
        assert!(verdict.copies.is_empty());
    }
}
