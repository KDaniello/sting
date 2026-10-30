//! Collection-authenticity signal
//! (`docs/domains/scamcheck.md`): calls TEP-62's
//! `get_nft_data()`/`get_collection_data()` via [`sting_emulator::TvmEmulator`]
//! — the same generic get-method call + [`crate::stack`] decode pattern
//! already proven three times for jettons (`mint_rights.rs`/`ston_fi.rs`/
//! `holder_concentration.rs`'s sibling signals) — then decodes the
//! collection's TEP-64 content ([`crate::content`]) and checks the resolved
//! collection address against the curated registry
//! ([`crate::gift_registry`]).
//!
//! **Deliberately does not decode `get_nft_data()`'s `index` field**: a real
//! Telegram Gift item's index is not a small sequential counter — a live
//! item fetched during this module's own research (`toncenter.com/api/v3`,
//! Plush Pepes collection, 2026-07-23) had index
//! `71894711297002410861506253698781652110178230441804478605525724062576225121191`,
//! far outside the 64-bit `vm_stk_tinyint` shape [`crate::stack`] decodes —
//! it would need the 257-bit full-int form, which [`crate::stack`]'s own
//! module doc comment already documents as unsupported ("no real get-method
//! this crate calls has ever needed one"). This signal does not need the
//! index value for authenticity purposes, so it is never read, sidestepping
//! that limitation entirely rather than extending the decoder speculatively
//! for a value this module has no use for.

use crate::content::{self, ContentError, NftContent};
use crate::gift_registry::{self, KnownGiftCollection};
use crate::stack::{self, StackError};
use sting_emulator::{EmulatorError, TvmEmulator};
use thiserror::Error;
use tonlib_core::tlb_types::block::msg_address::MsgAddress;
use tonlib_core::{TonAddress, TonAddressParseError};

/// `get_nft_data()`'s `collection_address` (TEP-62: `(init?, index,
/// collection_address, owner_address, individual_content)`).
const NFT_COLLECTION_ADDRESS_STACK_INDEX: u32 = 2;

/// `get_collection_data()`'s fields (TEP-62: `(next_item_index,
/// collection_content, owner_address)`).
const COLLECTION_NEXT_ITEM_INDEX_STACK_INDEX: u32 = 0;
const COLLECTION_CONTENT_STACK_INDEX: u32 = 1;
const COLLECTION_OWNER_ADDRESS_STACK_INDEX: u32 = 2;

/// Errors calling or decoding either get-method.
#[derive(Debug, Error)]
pub enum NftAuthenticityError {
    /// The get-method call itself failed at the emulator layer.
    #[error("{method} call failed: {source}")]
    Emulator {
        /// Which get-method was being called.
        method: &'static str,
        /// The underlying emulator failure.
        #[source]
        source: EmulatorError,
    },
    /// The call completed but did not succeed, or returned no stack — most
    /// likely this contract does not implement the expected TEP-62
    /// get-method at all.
    #[error("{method} did not succeed: exit_code={exit_code:?} error={error:?}")]
    ContractFailure {
        /// Which get-method was being called.
        method: &'static str,
        /// The VM exit code, if the call itself completed.
        exit_code: Option<i32>,
        /// The emulator-reported error, if any.
        error: Option<String>,
    },
    /// The returned stack could not be decoded into the expected shape.
    #[error("failed to decode {method}'s stack: {source}")]
    Stack {
        /// Which get-method was being called.
        method: &'static str,
        /// The underlying decode failure.
        #[source]
        source: StackError,
    },
    /// An address field was present but not a shape [`TonAddress`] can
    /// represent.
    #[error("{field} is not a standard address or addr_none: {source}")]
    UnsupportedAddress {
        /// Which field this was.
        field: &'static str,
        /// The underlying parse failure.
        #[source]
        source: TonAddressParseError,
    },
    /// The content cell could not be decoded (see [`ContentError`]).
    #[error("failed to decode collection_content: {0}")]
    Content(#[source] ContentError),
    /// `get_nft_data()`'s `collection_address` was `addr_none` — an item
    /// with no collection cannot be authenticated against a collection
    /// registry at all (this is a valid TEP-62 state, e.g. a standalone
    /// NFT, not necessarily a malformed contract).
    #[error("item's collection_address is addr_none — no collection to authenticate against")]
    ItemHasNoCollection,
}

fn call_get_method(
    emulator: &TvmEmulator<'_>,
    method: &'static str,
) -> Result<String, NftAuthenticityError> {
    let response = emulator
        .run_get_method_no_args(method)
        .map_err(|source| NftAuthenticityError::Emulator { method, source })?;
    if !response.success || response.vm_exit_code != Some(0) {
        return Err(NftAuthenticityError::ContractFailure {
            method,
            exit_code: response.vm_exit_code,
            error: response.error,
        });
    }
    response.stack.ok_or(NftAuthenticityError::ContractFailure {
        method,
        exit_code: response.vm_exit_code,
        error: Some("success response carried no stack field".to_owned()),
    })
}

fn decode_address(
    stack_boc_base64: &str,
    index: u32,
    method: &'static str,
    field: &'static str,
) -> Result<Option<TonAddress>, NftAuthenticityError> {
    let value = stack::decode_entry(stack_boc_base64, index)
        .map_err(|source| NftAuthenticityError::Stack { method, source })?;
    let msg_addr: MsgAddress = value
        .as_msg_address(index)
        .map_err(|source| NftAuthenticityError::Stack { method, source })?;
    match TonAddress::from_msg_address(msg_addr) {
        Ok(addr) if addr == TonAddress::NULL => Ok(None),
        Ok(addr) => Ok(Some(addr)),
        Err(source) => Err(NftAuthenticityError::UnsupportedAddress { field, source }),
    }
}

/// Calls `get_nft_data()` against `item` and reads only its
/// `collection_address` — see the module doc comment for why `index` is
/// never decoded.
///
/// # Errors
/// See [`NftAuthenticityError`].
pub fn item_collection_address(item: &TvmEmulator<'_>) -> Result<TonAddress, NftAuthenticityError> {
    const METHOD: &str = "get_nft_data";
    let stack = call_get_method(item, METHOD)?;
    decode_address(
        &stack,
        NFT_COLLECTION_ADDRESS_STACK_INDEX,
        METHOD,
        "collection_address",
    )?
    .ok_or(NftAuthenticityError::ItemHasNoCollection)
}

/// `get_collection_data()`'s decoded fields relevant to authenticity.
#[derive(Debug, Clone)]
pub struct CollectionData {
    /// `next_item_index` — informational only, not used for the
    /// authenticity verdict itself.
    pub next_item_index: i64,
    /// The collection's own `owner_address`, or `None` if renounced/absent.
    pub owner_address: Option<TonAddress>,
    /// The decoded TEP-64 `collection_content`.
    pub content: NftContent,
}

/// Calls `get_collection_data()` against `collection` and decodes all three
/// fields.
///
/// # Errors
/// See [`NftAuthenticityError`].
pub fn collection_data(
    collection: &TvmEmulator<'_>,
) -> Result<CollectionData, NftAuthenticityError> {
    const METHOD: &str = "get_collection_data";
    let stack = call_get_method(collection, METHOD)?;

    let next_item_index = stack::decode_entry(&stack, COLLECTION_NEXT_ITEM_INDEX_STACK_INDEX)
        .map_err(|source| NftAuthenticityError::Stack {
            method: METHOD,
            source,
        })?
        .as_int(COLLECTION_NEXT_ITEM_INDEX_STACK_INDEX)
        .map_err(|source| NftAuthenticityError::Stack {
            method: METHOD,
            source,
        })?;

    let content_value =
        stack::decode_entry(&stack, COLLECTION_CONTENT_STACK_INDEX).map_err(|source| {
            NftAuthenticityError::Stack {
                method: METHOD,
                source,
            }
        })?;
    let content_cell = match content_value {
        stack::StackValue::Cell(cell) => cell,
        other => {
            return Err(NftAuthenticityError::Stack {
                method: METHOD,
                source: StackError::UnsupportedTag {
                    tag: match other {
                        stack::StackValue::Null => 0x00,
                        stack::StackValue::Int(_) => 0x01,
                        stack::StackValue::Cell(_) => 0x03,
                        stack::StackValue::Slice { .. } => 0x04,
                    },
                    index: COLLECTION_CONTENT_STACK_INDEX,
                },
            });
        }
    };
    let content = content::decode_content(&content_cell).map_err(NftAuthenticityError::Content)?;

    let owner_address = decode_address(
        &stack,
        COLLECTION_OWNER_ADDRESS_STACK_INDEX,
        METHOD,
        "owner_address",
    )?;

    Ok(CollectionData {
        next_item_index,
        owner_address,
        content,
    })
}

/// The three-tier authenticity verdict for a resolved collection address —
/// deliberately not a bare bool: a signal is clearly present, weak and
/// ambiguous, or a risk — here, known good, present but unverified, and not
/// recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticityVerdict {
    /// The address matched a curated, cross-checked registry entry exactly.
    KnownGood(&'static str),
    /// The address is not in the starter registry — **not** evidence of a
    /// copycat (§6 of the trust-layer TZ: the registry is a small curated
    /// seed, not exhaustive), only "unverified by this instance's data".
    NotInRegistry,
}

/// Checks `collection_address` against [`gift_registry::KNOWN_GIFT_COLLECTIONS`].
#[must_use]
pub fn check_registry(collection_address: &TonAddress) -> AuthenticityVerdict {
    match gift_registry::lookup_by_address(collection_address) {
        Some(KnownGiftCollection { name, .. }) => AuthenticityVerdict::KnownGood(name),
        None => AuthenticityVerdict::NotInRegistry,
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
    use super::{AuthenticityVerdict, check_registry};
    use crate::content::{NftContent, decode_content};
    use crate::gift_registry::KNOWN_GIFT_COLLECTIONS;
    use tonlib_core::TonAddress;
    use tonlib_core::cell::CellBuilder;

    #[test]
    fn unknown_address_is_not_in_registry_not_flagged_fake() {
        let unknown = TonAddress::new(0, [0x77u8; 32].into());
        assert_eq!(check_registry(&unknown), AuthenticityVerdict::NotInRegistry);
    }

    /// `docs/domains/scamcheck.md` point 4 / §7.1's
    /// synthetic-copycat control, built entirely locally (no testnet
    /// deploy, per the same discipline `honeypot.rs`/`SOURCE.md`'s NFT
    /// entry already document): a fabricated "collection" whose decoded
    /// `collection_content` is byte-for-byte identical to a real, known-good
    /// registry entry's, at a different, never-registered address. Proves
    /// the authenticity verdict is decided by **address**, not by content
    /// (content alone is trivially copyable by any real attacker) —
    /// 100% detection by construction, not a statistical claim, because
    /// [`check_registry`] never even looks at content.
    #[test]
    fn identical_content_at_an_unregistered_address_is_not_known_good() {
        let real_plush_pepe = KNOWN_GIFT_COLLECTIONS
            .iter()
            .find(|c| c.name == "Plush Pepes")
            .expect("Plush Pepes must be in the starter registry");
        let real_address =
            TonAddress::from_hex_str(real_plush_pepe.address).expect("parse real address");

        let uri = format!(
            "https://nft.fragment.com/collection/{}.json",
            real_plush_pepe.fragment_slug
        );
        let mut cell = CellBuilder::new();
        let mut bytes = vec![0x01u8];
        bytes.extend_from_slice(uri.as_bytes());
        cell.store_slice(&bytes).expect("build");
        let content_cell = std::sync::Arc::new(cell.build().expect("build"));

        // The copycat's own, never-registered address — e.g. a freshly
        // deployed attacker contract. Content is irrelevant to this check.
        let copycat_address = TonAddress::new(0, [0x99u8; 32].into());

        let decoded = decode_content(&content_cell).expect("decode");
        assert_eq!(decoded, NftContent::OffChainUri(uri));

        assert_eq!(
            check_registry(&real_address),
            AuthenticityVerdict::KnownGood("Plush Pepes"),
            "the real address must resolve to KnownGood"
        );
        assert_eq!(
            check_registry(&copycat_address),
            AuthenticityVerdict::NotInRegistry,
            "identical content at a different address must NOT be KnownGood — \
             the verdict must never be decided by content alone"
        );
    }
}
