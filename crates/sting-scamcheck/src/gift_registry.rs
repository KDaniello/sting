//! Curated starter registry of known-good Telegram Gift collection addresses
//! (`docs/domains/scamcheck.md`/§6). **Not exhaustive —
//! explicitly a starting seed, not a claim of full market coverage** (the
//! same honesty discipline §0.2 of that document requires in the product's
//! own responses: "not in this list" must never be presented as "confirmed
//! fake").
//!
//! Every entry below was cross-checked directly against `toncenter.com/api/v3`
//! on 2026-07-23 (not taken on trust from a single search-engine snippet):
//! for each candidate address found via search, this module's data was only
//! added after an independent live `GET /api/v3/nft/collections?
//! collection_address=<addr>` call confirmed `is_indexed: true`,
//! `is_scam: false`, and a `collection_content.uri` resolving to the real
//! `nft.fragment.com` domain (Fragment being Telegram's own official Gift
//! venue, not a third-party marketplace) — three independent corroborating
//! facts per entry, not one.
//!
//! **Real finding, not assumed**: every collection below shares the *exact
//! same* `code_hash` (`iEEMIg+CIYFmgmnOg+ucwNOznCGZnItV3gM2CiDnwoI=`) —
//! Telegram's own Gift-collection contract code, distinct from the generic
//! `getgems-io/nft-contracts` reference collection code used by
//! `corpus/fixtures/emulator/nft-collection.*.boc.base64` (a regular
//! getgems.io collection, "Teddy Brix", used only to ground-truth the
//! TEP-62 call/decode path itself — see that fixture's own `SOURCE.md`
//! entry). [`KNOWN_GIFT_COLLECTION_CODE_HASH`] turns this into a second,
//! independent, and *cheaper* check: it does not by itself prove which
//! specific Gift a collection represents (a name/address match still does
//! that), but a candidate collection whose code hash does not match this at
//! all is not running Telegram's own Gift-collection code, regardless of
//! what its `collection_content` claims.

use tonlib_core::TonAddress;

/// The reference-implementation code hash shared by every confirmed
/// Telegram Gift collection in [`KNOWN_GIFT_COLLECTIONS`] (base64,
/// `get_account_state`'s own `code_hash`, not derived/recomputed here) —
/// see the module doc comment for how this was found and why it is useful
/// as an independent check alongside address matching.
pub const KNOWN_GIFT_COLLECTION_CODE_HASH: &str = "iEEMIg+CIYFmgmnOg+ucwNOznCGZnItV3gM2CiDnwoI=";

/// One curated, cross-checked known-good Gift collection.
#[derive(Debug, Clone, Copy)]
pub struct KnownGiftCollection {
    /// Human name, as reported by `toncenter`'s own indexed metadata
    /// (`token_info[0].name`), not invented.
    pub name: &'static str,
    /// The collection contract's raw address, `workchain:hex` form (same
    /// form [`TonAddress::from_hex_str`] parses).
    pub address: &'static str,
    /// The `nft.fragment.com` slug this collection's `collection_content`
    /// URI resolves to (e.g. `"plushpepe"` for
    /// `https://nft.fragment.com/collection/plushpepe.json`) — kept
    /// alongside the address as a second, human-readable cross-check.
    pub fragment_slug: &'static str,
}

/// Starter registry — 9 entries, cross-checked live on 2026-07-23 (see the
/// module doc comment for the verification method). **Not exhaustive.**
/// Real, well-known Telegram Gift collections beyond these 9 almost
/// certainly exist and are simply not in this seed list yet — extend as
/// more are cross-checked the same way, do not assume this list's current
/// size reflects the real market's size.
pub const KNOWN_GIFT_COLLECTIONS: &[KnownGiftCollection] = &[
    KnownGiftCollection {
        name: "Plush Pepes",
        address: "0:46FA0E9A864014196A5E7D66F1F83FFDB10F2859BBF2EA9BAEABBF14D9CE0D50",
        fragment_slug: "plushpepe",
    },
    KnownGiftCollection {
        name: "Durov's Caps",
        address: "0:FD8A466AEB13E02A3CE67411B41B44BCD11BD42636F0807ACF6570CA73FC2C13",
        fragment_slug: "durovscap",
    },
    KnownGiftCollection {
        name: "Jacks-in-the-Box",
        address: "0:38D3542AA57B419F021A0DB163C1DE725DF3F6998882BAC6C9DD9730F5E5D1F4",
        fragment_slug: "jackinthebox",
    },
    KnownGiftCollection {
        name: "Kissed Frogs",
        address: "0:D3AE8FA88096D2EE8F8E043E9BB76372B0FB7B4A71A6E737EE457181D6713FAD",
        fragment_slug: "kissedfrog",
    },
    KnownGiftCollection {
        name: "Precious Peaches",
        address: "0:388B9F22B92F4351846D519F7BB19A399A791B898501A565D039EDDD11409C3F",
        fragment_slug: "preciouspeach",
    },
    KnownGiftCollection {
        name: "Diamond Rings",
        address: "0:9687594F96DC93C025B10979B8BA62F9B4F6685D1F244C8713FA977426AA1A74",
        fragment_slug: "diamondring",
    },
    KnownGiftCollection {
        name: "Perfume Bottles",
        address: "0:C9B0DF4E24184A199A16656B64129CE409F22ED7A67D5334D66D88DE2E9B8775",
        fragment_slug: "perfumebottle",
    },
    KnownGiftCollection {
        name: "Astral Shards",
        address: "0:C845E95E3A44F1083E20FD7126F318F42D8360EBECCCB13180030080FAF11B90",
        fragment_slug: "astralshard",
    },
    KnownGiftCollection {
        name: "Pet Snakes",
        address: "0:412D116D0BFDCF2F1369597E677DC8706A993B796706DDDCCAA316D5E9C5B3A9",
        fragment_slug: "petsnake",
    },
];

/// Looks up `address` in [`KNOWN_GIFT_COLLECTIONS`] by exact address match.
/// Returns `None` for "not in the starter list" — **never** interpret
/// `None` as "confirmed fake" (§0.2 of the trust-layer TZ requires this
/// distinction survive into the product's own responses, not just this
/// function's contract).
#[must_use]
pub fn lookup_by_address(address: &TonAddress) -> Option<&'static KnownGiftCollection> {
    KNOWN_GIFT_COLLECTIONS
        .iter()
        .find(|entry| match TonAddress::from_hex_str(entry.address) {
            Ok(known) => known == *address,
            Err(_) => false,
        })
}

/// What a collection's own claim is worth, before anything is asked of the chain.
///
/// A collection states a name and serves a description from a URI, and both are free to write.
/// The verdict is therefore about the claim rather than about the collection: a row that claims
/// to be a registry collection and stands at a different address is claiming somebody else's
/// identity, and a row that claims nothing is simply not in the seed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimVerdict {
    /// Claims a collection this registry knows, and stands where that collection stands.
    Authentic,
    /// Claims a collection this registry knows, and stands somewhere else.
    Imposter,
    /// Claims nothing this registry knows. Not a verdict of fake: the registry is a seed, and
    /// most of the market is legitimately outside it.
    Unlisted,
}

/// The registry entry a name or a description URI claims to be, if any.
///
/// Two ways of claiming, because both appear in practice: an imposter usually copies the name,
/// and often serves the real collection's description URI as well.
#[must_use]
pub fn claimed_collection(name: &str, content_uri: &str) -> Option<&'static KnownGiftCollection> {
    KNOWN_GIFT_COLLECTIONS.iter().find(|entry| {
        names_match(entry.name, name) || serves_slug(content_uri, entry.fragment_slug)
    })
}

/// What a collection's claim is worth, given where it actually stands.
///
/// Offline by construction: the address, the name and the URI are facts a caller already has from
/// an index, so a whole catalogue can be checked without a single chain call. What it cannot do
/// is confirm a collection outside the registry, which is why the third verdict says so rather
/// than guessing.
#[must_use]
pub fn verdict(address: &TonAddress, name: &str, content_uri: &str) -> ClaimVerdict {
    let Some(claimed) = claimed_collection(name, content_uri) else {
        return ClaimVerdict::Unlisted;
    };
    match TonAddress::from_hex_str(claimed.address) {
        Ok(known) if known == *address => ClaimVerdict::Authentic,
        _ => ClaimVerdict::Imposter,
    }
}

/// Names compared with the apostrophe normalised: an index reports a typographic apostrophe where
/// a registry written in source carries a plain one, and an imposter differing only there would
/// pass a comparison nobody thought about.
fn names_match(known: &str, claimed: &str) -> bool {
    fn normalise(text: &str) -> String {
        text.trim()
            .to_lowercase()
            .replace(['\u{2018}', '\u{2019}'], "'")
    }
    normalise(known) == normalise(claimed)
}

/// Whether a description URI is the one this collection's slug names.
fn serves_slug(content_uri: &str, slug: &str) -> bool {
    let Some(rest) = content_uri.split("nft.fragment.com/collection/").nth(1) else {
        return false;
    };
    rest.trim_end_matches(".json").eq_ignore_ascii_case(slug)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::{KNOWN_GIFT_COLLECTIONS, lookup_by_address};
    use tonlib_core::TonAddress;

    #[test]
    fn every_entry_address_parses() {
        for entry in KNOWN_GIFT_COLLECTIONS {
            TonAddress::from_hex_str(entry.address)
                .unwrap_or_else(|e| panic!("{} has an unparseable address: {e}", entry.name));
        }
    }

    #[test]
    fn lookup_finds_a_known_entry() {
        let plush_pepe =
            TonAddress::from_hex_str(KNOWN_GIFT_COLLECTIONS[0].address).expect("parse");
        let found = lookup_by_address(&plush_pepe).expect("must find Plush Pepes");
        assert_eq!(found.name, "Plush Pepes");
    }

    #[test]
    fn lookup_returns_none_for_an_unknown_address() {
        let unknown = TonAddress::new(0, [0xAAu8; 32].into());
        assert!(lookup_by_address(&unknown).is_none());
    }
}
