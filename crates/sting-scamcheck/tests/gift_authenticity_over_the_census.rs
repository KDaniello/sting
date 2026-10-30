//! The authenticity rule against the census it was found in.
//!
//! The census is a checked-in reading of the gift market — 167 collections, taken from mainnet
//! over a ninety-day window — and it is where the imposters were noticed in the first place. What
//! is asserted here is that the rule, now living in this crate, still finds them: same catalogue,
//! same verdicts, no chain call involved.
//!
//! Every number below is the measurement rather than a target. The registry is a seed of nine
//! collections, so nine is also the ceiling on how many rows can be confirmed and the reach of
//! the whole check: a market this rule says nothing about is most of the market.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use sting_scamcheck::gift_registry::{ClaimVerdict, KNOWN_GIFT_COLLECTIONS, verdict};
use sting_scamcheck::namesakes::{CollectionRow, namesake_verdict};
use tonlib_core::TonAddress;

const CENSUS: &str = include_str!("../../../corpus/karman/fixtures/ton-census.json");

struct Row {
    address: TonAddress,
    name: String,
    content: String,
    issuer_held: u64,
}

fn census_rows() -> Vec<Row> {
    let census: serde_json::Value = serde_json::from_str(CENSUS).expect("the census is JSON");
    census["collections"]
        .as_array()
        .expect("the census lists collections")
        .iter()
        .map(|row| Row {
            address: TonAddress::from_hex_str(row["collection"].as_str().expect("an address"))
                .expect("an address the census read off the chain"),
            name: row["name"].as_str().unwrap_or_default().to_string(),
            content: row["content"].as_str().unwrap_or_default().to_string(),
            issuer_held: row["issuer_held"].as_u64().unwrap_or_default(),
        })
        .collect()
}

fn count(rows: &[Row], wanted: ClaimVerdict) -> usize {
    rows.iter()
        .filter(|row| verdict(&row.address, &row.name, &row.content) == wanted)
        .count()
}

#[test]
fn the_census_holds_the_hundred_and_sixty_seven_collections_it_claims() {
    assert_eq!(census_rows().len(), 167);
}

#[test]
fn eight_collections_claim_an_identity_that_is_not_theirs() {
    let rows = census_rows();
    let imposters = count(&rows, ClaimVerdict::Imposter);
    let authentic = count(&rows, ClaimVerdict::Authentic);

    assert_eq!(
        authentic, 9,
        "one row per registry entry stands where the registry says it stands"
    );
    assert_eq!(
        imposters, 8,
        "eight rows carry a registry collection's name or its description and a different address"
    );
    assert_eq!(
        authentic + imposters + count(&rows, ClaimVerdict::Unlisted),
        rows.len(),
        "every collection gets exactly one verdict"
    );
}

/// Seven of the eight imposters copy one collection, which is what an imposter population looks
/// like: they cluster on whatever is worth counterfeiting rather than spreading evenly.
#[test]
fn seven_of_the_eight_imposters_copy_the_same_collection() {
    let rows = census_rows();
    let imposter_names: Vec<String> = rows
        .iter()
        .filter(|row| verdict(&row.address, &row.name, &row.content) == ClaimVerdict::Imposter)
        .map(|row| row.name.clone())
        .collect();

    let plush = imposter_names
        .iter()
        .filter(|name| name.as_str() == "Plush Pepes")
        .count();
    assert_eq!(plush, 7);
    assert_eq!(imposter_names.len(), 8);
}

/// What the rule cannot answer, stated as a measurement rather than as a caveat.
///
/// A hundred and fifty rows are outside the seed registry, and none of them is thereby a
/// forgery. A check whose silence read as an accusation would be worse than no check.
#[test]
fn most_of_the_market_is_outside_the_registry_and_is_not_accused() {
    let rows = census_rows();
    assert_eq!(count(&rows, ClaimVerdict::Unlisted), 150);
    assert_eq!(KNOWN_GIFT_COLLECTIONS.len(), 9);
}

fn namesake_rows() -> Vec<CollectionRow> {
    census_rows()
        .into_iter()
        .map(|row| CollectionRow {
            address: row.address.to_hex(),
            name: row.name,
            issuer_held: row.issuer_held,
        })
        .collect()
}

/// What the copies look like across the whole census, which is more than the finding said.
///
/// The census was read for two popular series and reported twenty-seven rows carrying their two
/// names. Over the whole catalogue eight names are claimed by more than one contract, across
/// fifty-three rows: in seven of those groups exactly one contract holds back part of its own
/// supply and the other forty hold none of theirs.
///
/// One group settles nothing and is left alone. Six contracts call themselves the same series and
/// not one of them holds any of its own supply — which is what this rule looks for, so it declines
/// rather than picking a winner. Naming an original there would be inventing the answer, and the
/// count of copies is smaller for it.
///
/// The registry cross-checks two of the seven originals by address, and disagrees with none of
/// them. That it knows only two is the reach of a nine-entry seed list rather than a limit of the
/// rule: the rule found the other five without it, which is the whole reason for having a rule
/// that does not depend on a list.
#[test]
fn forty_copies_stand_behind_seven_contested_names() {
    let verdict = namesake_verdict(&namesake_rows());

    let contested_rows: usize = verdict.contested_names.iter().map(|(_, count)| count).sum();
    assert_eq!(verdict.contested_names.len(), 8);
    assert_eq!(contested_rows, 53);
    assert_eq!(
        verdict.originals.len(),
        7,
        "one group has no contract holding back supply and is left unsettled"
    );
    assert_eq!(verdict.copies.len(), 40);

    let registry_confirmed = verdict
        .originals
        .iter()
        .filter(|address| {
            let parsed = TonAddress::from_hex_str(address).expect("an address from the census");
            sting_scamcheck::gift_registry::lookup_by_address(&parsed).is_some()
        })
        .count();
    assert_eq!(
        registry_confirmed, 2,
        "the seed list knows two of the seven, and contradicts none of them"
    );
}

/// The two series the census reported on, still there and still counted the same way.
#[test]
fn the_two_series_the_census_named_carry_twenty_seven_rows_between_them() {
    let verdict = namesake_verdict(&namesake_rows());
    let counted: usize = verdict
        .contested_names
        .iter()
        .filter(|(name, _)| name == "Heart Lockets" || name == "Plush Pepes")
        .map(|(_, count)| count)
        .sum();
    assert_eq!(
        counted, 27,
        "nineteen rows carry one name and eight the other, as the census read them"
    );
}
