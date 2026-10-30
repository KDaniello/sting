//! Real-emulator integration test — probes the exact same real, vendored
//! mainnet jetton-master fixture `sting-emulator`'s own
//! `examples/run_get_jetton_data.rs` already uses
//! (`corpus/fixtures/emulator/lp-jetton-master.*`), no network required.
//! Skips gracefully if `vendor/libemulator/` isn't present locally, same
//! pattern as every other real-emulator test in this workspace.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_abi_dictionary::{MethodDictionary, ProbeTarget, probe};
use sting_emulator_sys::LibEmulator;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn library_or_skip() -> Option<LibEmulator> {
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

/// Real, hand-picked dictionary covering exactly the names this fixture is
/// independently known (via `run_get_jetton_data.rs`'s own ground truth,
/// `corpus/fixtures/emulator/SOURCE.md`) to have or not have — the same
/// "known-real vs. known-absent" empirical design confirmed while building
/// this crate.
fn test_dictionary() -> MethodDictionary {
    let mut dict = MethodDictionary::new();
    // Zero-arg, really present on this exact contract.
    dict.learn_from_schema(
        &minimal_schema_with_niladic_getter("get_jetton_data", 106_029),
        "test",
    );
    // Zero-arg, plausible TON get-method names, but genuinely absent from
    // a jetton *master* (these are NFT/wallet/DNS conventions).
    for name in ["get_nft_data", "seqno", "get_public_key"] {
        dict.learn_from_schema(
            &minimal_schema_with_niladic_getter(name, real_method_id(name)),
            "test",
        );
    }
    // Parameterized — must never be probed (see crate doc comment on why).
    dict.learn_from_schema(
        &minimal_schema_with_one_arg_getter(
            "get_wallet_address",
            real_method_id("get_wallet_address"),
        ),
        "test",
    );
    dict
}

fn real_method_id(name: &str) -> i64 {
    i64::from(sting_emulator::method_id(name))
}

fn minimal_schema_with_niladic_getter(name: &str, id: i64) -> sting_abi_core::ContractSchema {
    use sting_abi_core::{
        ContractMetadata, ContractSchema, ExtractionTier, GetMethod, SourceLanguage,
    };
    ContractSchema {
        ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
        source_language: SourceLanguage::Func,
        extraction_tier: ExtractionTier::Heuristic,
        contract_name: None,
        metadata: ContractMetadata::default(),
        storage: None,
        types: Vec::new(),
        incoming_internal: Vec::new(),
        incoming_external: Vec::new(),
        outgoing: Vec::new(),
        get_methods: vec![GetMethod {
            tvm_method_id: Some(id),
            name: name.to_string(),
            parameters: Vec::new(),
            return_ty: None,
        }],
        errors: Vec::new(),
        confidence: sting_abi_core::ExtractionConfidence::new(0.5).ok(),
        field_confidence: None,
    }
}

fn minimal_schema_with_one_arg_getter(name: &str, id: i64) -> sting_abi_core::ContractSchema {
    use sting_abi_core::{
        ContractMetadata, ContractSchema, ExtractionTier, GetMethod, Param, SourceLanguage, TypeRef,
    };
    ContractSchema {
        ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
        source_language: SourceLanguage::Func,
        extraction_tier: ExtractionTier::Heuristic,
        contract_name: None,
        metadata: ContractMetadata::default(),
        storage: None,
        types: Vec::new(),
        incoming_internal: Vec::new(),
        incoming_external: Vec::new(),
        outgoing: Vec::new(),
        get_methods: vec![GetMethod {
            tvm_method_id: Some(id),
            name: name.to_string(),
            parameters: vec![Param {
                name: "owner_address".to_string(),
                ty: TypeRef(0),
            }],
            return_ty: None,
        }],
        errors: Vec::new(),
        confidence: sting_abi_core::ExtractionConfidence::new(0.5).ok(),
        field_confidence: None,
    }
}

#[test]
fn probing_a_real_mainnet_fixture_recognizes_exactly_the_real_zero_arg_getter() {
    let Some(lib) = library_or_skip() else {
        return;
    };
    let fixtures_dir = repo_root().join("corpus/fixtures/emulator");
    let code = std::fs::read_to_string(fixtures_dir.join("lp-jetton-master.code.boc.base64"))
        .expect("read code fixture");
    let data = std::fs::read_to_string(fixtures_dir.join("lp-jetton-master.data.boc.base64"))
        .expect("read data fixture");

    let dictionary = test_dictionary();
    assert_eq!(
        dictionary.probeable_entries().count(),
        4,
        "get_wallet_address (parameterized) must be excluded from the probeable count"
    );

    let target = ProbeTarget {
        code_boc_base64: code.trim(),
        data_boc_base64: data.trim(),
        address: "0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6",
        unixtime: 1_784_240_154,
        balance: 85_776_672,
    };
    let report = probe(&lib, &target, &dictionary).expect("probe must succeed");

    assert_eq!(
        report.recognized.len(),
        1,
        "exactly one of the four probeable entries is really present: {:?}",
        report.recognized
    );
    assert_eq!(report.recognized[0].name, "get_jetton_data");
    assert_eq!(
        report.recognized[0].stack_depth, 5,
        "ground truth: 5 (SOURCE.md)"
    );
    assert_eq!(report.probed_count, 4);
    assert_eq!(
        report.inconclusive_count, 0,
        "every absent probe must classify cleanly as absent, not inconclusive"
    );
}

#[test]
fn malformed_code_returns_a_probe_error_not_a_panic() {
    let Some(lib) = library_or_skip() else {
        return;
    };
    let target = ProbeTarget {
        code_boc_base64: "not a real base64 BoC",
        data_boc_base64: "also not a real base64 BoC",
        address: "0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6",
        unixtime: 1_784_240_154,
        balance: 0,
    };
    let result = probe(&lib, &target, &test_dictionary());
    assert!(
        result.is_err(),
        "malformed code/data BoC must surface as ProbeError, not succeed or panic"
    );
}
