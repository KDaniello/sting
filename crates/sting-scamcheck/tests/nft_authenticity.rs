//! Real-`libemulator` integration test for [`sting_scamcheck::nft_authenticity`]
//! (`docs/domains/scamcheck.md`), against a real,
//! currently active TEP-62 collection+item pair
//! (`corpus/fixtures/emulator/nft-collection.{code,data}.boc.base64` /
//! `nft-item.{code,data}.boc.base64`, see `corpus/fixtures/emulator/SOURCE.md`).
//!
//! Ground truth cross-checked against `toncenter.com/api/v3`'s own already-
//! resolved `collection_content`/`next_item_index`/`owner_address` at fetch
//! time (2026-07-23) — a wrong stack decode or a wrong content-cell parse
//! fails loudly here against real data, not a hand-picked edge case.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_emulator::TvmEmulator;
use sting_emulator_sys::LibEmulator;
use sting_scamcheck::content::NftContent;
use sting_scamcheck::nft_authenticity::{collection_data, item_collection_address};
use tonlib_core::TonAddress;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_lib_or_skip() -> Option<LibEmulator> {
    match sting_emulator::load_default_library() {
        Ok(lib) => Some(lib),
        // A test that cannot load the library says so and passes; a silent
        // skip and a real run must never look alike.
        Err(error) => {
            println!("SKIP: {error} — see docs/domains/emulator.md for how to fetch it");
            None
        }
    }
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(repo_root().join("corpus/fixtures/emulator").join(name))
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

/// The real collection's own address (`corpus/fixtures/emulator/SOURCE.md`),
/// used both to build the item's `set_c7` (irrelevant to it, but harmless)
/// and to build the collection's own `set_c7`.
const COLLECTION_ADDRESS: &str = "EQAAGKP_4XHAZPDhMhcYl20GjGE0qptMp3GRcJ9xvfMMFLSZ";
const ITEM_ADDRESS: &str = "EQAJXfPmAjwx3W5emt21a_-Fe0u4uj3u3IXYLknzxugmCd7m";

fn collection_emulator(lib: &LibEmulator) -> TvmEmulator<'_> {
    let code_boc = read_fixture("nft-collection.code.boc.base64");
    let data_boc = read_fixture("nft-collection.data.boc.base64");
    let emulator = TvmEmulator::new(lib, code_boc.trim(), data_boc.trim())
        .expect("tvm_emulator_create failed");
    let address: TonAddress = COLLECTION_ADDRESS
        .parse()
        .expect("parse collection address");
    emulator
        .set_c7(&address.to_hex(), 1_768_130_245, 24_903_101, None)
        .expect("tvm_emulator_set_c7 failed");
    emulator
}

fn item_emulator(lib: &LibEmulator) -> TvmEmulator<'_> {
    let code_boc = read_fixture("nft-item.code.boc.base64");
    let data_boc = read_fixture("nft-item.data.boc.base64");
    let emulator = TvmEmulator::new(lib, code_boc.trim(), data_boc.trim())
        .expect("tvm_emulator_create failed");
    let address: TonAddress = ITEM_ADDRESS.parse().expect("parse item address");
    emulator
        .set_c7(&address.to_hex(), 1_768_130_245, 47_367_337, None)
        .expect("tvm_emulator_set_c7 failed");
    emulator
}

#[test]
fn collection_data_matches_toncenter_ground_truth() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let collection = collection_emulator(&lib);
    let data = collection_data(&collection).expect("get_collection_data failed");

    assert_eq!(data.next_item_index, 5, "ground truth: SOURCE.md");
    assert_eq!(
        data.owner_address,
        Some(
            TonAddress::from_hex_str(
                "0:52992B41F808E13A824AB9A0957DD464F8C32CAC95E71ABA9978852C30E8BF74"
            )
            .expect("parse ground-truth owner")
        )
    );
    assert_eq!(
        data.content,
        NftContent::OffChainUri(
            "https://s.getgems.io/nft/c/69637b02b57084ca3c10f72b/meta.json".to_owned()
        )
    );
}

#[test]
fn item_resolves_the_real_collection_address() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let item = item_emulator(&lib);
    let resolved = item_collection_address(&item).expect("get_nft_data failed");
    let expected: TonAddress = COLLECTION_ADDRESS.parse().expect("parse expected");
    assert_eq!(
        resolved, expected,
        "the item's own get_nft_data() must resolve to its real collection's address"
    );
}
