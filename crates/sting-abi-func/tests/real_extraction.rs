//! Real-fixture extraction tests — `docs/TESTING.md`
//! golden/snapshot discipline, applied to Tier 2: these assert against
//! hand-reviewed expectations derived from actually reading the real
//! source (`tests/fixtures/SOURCE.md`), not a compiler's ground truth
//! (`FunC` has none to extract against, the whole reason this is Tier 2).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_abi_core::{ExtractionTier, Extractor, SourceLanguage, TypeShape};
use sting_abi_func::FuncExtractor;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// `jetton-wallet.fc`'s real `load_data()`/`save_data(...)` pair (read
/// directly from the fixture, `tests/fixtures/jetton-wallet.fc:29-36`):
/// `(int, slice, slice, cell) load_data() inline { ... return
/// (ds~load_coins(), ds~load_msg_addr(), ds~load_msg_addr(),
/// ds~load_ref()); }` paired with `save_data(int balance, slice
/// owner_address, slice jetton_master_address, cell jetton_wallet_code)` —
/// the exact paired form Level 2 recognizes.
#[test]
fn jetton_wallet_storage_is_recovered_from_the_load_save_data_pair() {
    let extractor = FuncExtractor;
    let schema = extractor
        .extract(&fixtures_dir(), Path::new("jetton-wallet.fc"))
        .expect("real fixture must extract without error");

    assert_eq!(schema.source_language, SourceLanguage::Func);
    assert_eq!(schema.extraction_tier, ExtractionTier::Heuristic);
    assert!(
        schema.confidence.is_some(),
        "Tier 2 schema must carry a confidence"
    );

    let storage_ty = schema
        .storage
        .expect("load_data/save_data pair must be recognized");
    let storage_type = &schema.types[storage_ty.0 as usize];
    let TypeShape::Struct { fields } = &storage_type.shape else {
        panic!("storage must be a Struct, got {:?}", storage_type.shape);
    };

    let field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        field_names,
        vec![
            "balance",
            "owner_address",
            "jetton_master_address",
            "jetton_wallet_code"
        ],
        "field names must come from save_data's parameter names, in order"
    );

    let field_shape = |name: &str| {
        let field = fields
            .iter()
            .find(|f| f.name == name)
            .expect("field present");
        schema.types[field.ty.0 as usize].shape.clone()
    };
    assert_eq!(
        field_shape("balance"),
        TypeShape::Coins,
        "balance comes from ds~load_coins()"
    );
    assert_eq!(
        field_shape("owner_address"),
        TypeShape::Address,
        "owner_address comes from ds~load_msg_addr()"
    );
    assert_eq!(
        field_shape("jetton_wallet_code"),
        TypeShape::Cell,
        "jetton_wallet_code comes from ds~load_ref()"
    );

    let field_confidence = schema
        .field_confidence
        .expect("a Tier 2 schema with recognized storage must carry field_confidence");
    assert!(field_confidence.storage.is_some());
}

/// `jetton-wallet.fc:246`: `(int, slice, slice, cell) get_wallet_data()
/// method_id { return load_data(); }` — a real `method_id`-specified
/// get-method with a 4-value tensor return type and no explicit numeric
/// override, so its `tvm_method_id` must be the CRC16 default.
#[test]
fn jetton_wallet_get_wallet_data_is_recovered_with_a_tensor_return_type() {
    let extractor = FuncExtractor;
    let schema = extractor
        .extract(&fixtures_dir(), Path::new("jetton-wallet.fc"))
        .expect("real fixture must extract without error");

    let method = schema
        .get_methods
        .iter()
        .find(|m| m.name == "get_wallet_data")
        .expect("get_wallet_data must be recognized as a get-method");

    assert_eq!(
        method.tvm_method_id,
        Some(sting_abi_func::default_method_id("get_wallet_data")),
        "no explicit method_id(N) in source — must fall back to the CRC16 default"
    );

    let return_ty = method
        .return_ty
        .expect("tensor return type must resolve to a TypeRef");
    let TypeShape::Tensor { items } = &schema.types[return_ty.0 as usize].shape else {
        panic!(
            "a 4-value tensor return must produce TypeShape::Tensor, got {:?}",
            schema.types[return_ty.0 as usize].shape
        );
    };
    assert_eq!(items.len(), 4, "(int, slice, slice, cell) has 4 components");
    assert_eq!(
        schema.types[items[0].0 as usize].shape,
        TypeShape::Int {
            bits: None,
            signed: true
        }
    );

    let field_confidence = schema
        .field_confidence
        .expect("must carry field_confidence");
    assert!(field_confidence.get_methods.is_some());
}

/// Corpus-driven audit (`docs/domains/extractors.md`
/// §5), remaining fixtures — one full pass classifying every extraction
/// against the real source: zero extractor bugs found once the two real bugs
/// caught along the way (see `storage.rs`'s
/// own test module doc comments) were fixed. Each assertion here pins one
/// classification from that pass so a regression is caught automatically,
/// not just remembered in a commit message.
#[test]
fn jetton_minter_recovers_two_get_methods_and_the_storage_pair() {
    let extractor = FuncExtractor;
    let schema = extractor
        .extract(&fixtures_dir(), Path::new("jetton-minter.fc"))
        .expect("real fixture must extract without error");

    assert_eq!(
        schema.get_methods.len(),
        2,
        "get_jetton_data + get_wallet_address"
    );
    assert!(
        schema
            .get_methods
            .iter()
            .any(|m| m.name == "get_jetton_data")
    );
    assert!(
        schema
            .get_methods
            .iter()
            .any(|m| m.name == "get_wallet_address")
    );

    let storage_ty = schema
        .storage
        .expect("real load_data/save_data pair, tests/fixtures/jetton-minter.fc:9-19");
    let TypeShape::Struct { fields } = &schema.types[storage_ty.0 as usize].shape else {
        panic!("storage must be a Struct");
    };
    let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "total_supply",
            "admin_address",
            "content",
            "jetton_wallet_code"
        ]
    );
    // `total_supply`/`admin_address` come from `ds~load_coins()`/
    // `ds~load_msg_addr()` in the real source — more precise than the
    // function signature's generic declared `(int, slice, cell, cell)`.
    let shape_of = |name: &str| {
        let field = fields.iter().find(|f| f.name == name).expect("present");
        schema.types[field.ty.0 as usize].shape.clone()
    };
    assert_eq!(shape_of("total_supply"), TypeShape::Coins);
    assert_eq!(shape_of("admin_address"), TypeShape::Address);
}

/// `nft-item.fc` has `load_data()` (tests/fixtures/nft-item.fc:28) but no
/// `save_data` wrapper at all — only inline `set_data(...)` — the exact
/// v1-excluded inline form (`lib.rs`'s own module doc comment). Storage
/// must be `None`, not a wrong guess from `load_data` alone.
#[test]
fn nft_item_recovers_get_method_but_not_storage() {
    let extractor = FuncExtractor;
    let schema = extractor
        .extract(&fixtures_dir(), Path::new("nft-item.fc"))
        .expect("real fixture must extract without error");

    assert_eq!(schema.get_methods.len(), 1);
    assert_eq!(schema.get_methods[0].name, "get_nft_data");
    assert_eq!(
        schema.storage, None,
        "no save_data wrapper in real source — must not guess storage from load_data alone"
    );
}

/// `wallet3-code.fc` uses inline `get_data()`/`set_data()` throughout, no
/// `load_data`/`save_data` wrapper pair at all (old pre-`impure`-era
/// style) — `recv_external` reads `stored_seqno`/`stored_subwallet`/
/// `public_key` via `get_data().begin_parse()` + tuple destructuring, then
/// writes transformed versions of the same three values back via
/// `set_data(begin_cell()...end_cell())`, same function, matching arity —
/// exactly the pattern `storage::extract_inline_storage`
/// (`docs/domains/method-dictionary.md`) now
/// recognizes. Deliberately updated, not left contradicting the new
/// behavior — `seqno()`/`get_public_key()` still each read only *part* of
/// storage independently and are correctly not treated as a second,
/// conflicting source (cross-call-site reconciliation is out of v1 scope).
#[test]
fn wallet3_recovers_two_get_methods_and_inline_storage() {
    let extractor = FuncExtractor;
    let schema = extractor
        .extract(&fixtures_dir(), Path::new("wallet3-code.fc"))
        .expect("real fixture must extract without error");

    assert_eq!(schema.get_methods.len(), 2, "seqno + get_public_key");
    assert!(schema.get_methods.iter().any(|m| m.name == "seqno"));
    assert!(
        schema
            .get_methods
            .iter()
            .any(|m| m.name == "get_public_key")
    );

    let storage_ty = schema
        .storage
        .expect("the inline get_data()/set_data() pair in recv_external must now be recognized");
    let storage_type = &schema.types[storage_ty.0 as usize];
    let TypeShape::Struct { fields } = &storage_type.shape else {
        panic!("storage must be a Struct, got {:?}", storage_type.shape);
    };

    let field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        field_names,
        vec!["stored_seqno", "stored_subwallet", "public_key"],
        "field names must come from the read side's bound tuple names, in order"
    );

    let field_shape = |name: &str| {
        let field = fields
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("{name} must be present"));
        schema.types[field.ty.0 as usize].shape.clone()
    };
    assert_eq!(
        field_shape("stored_seqno"),
        TypeShape::Int {
            bits: Some(32),
            signed: false
        }
    );
    assert_eq!(
        field_shape("stored_subwallet"),
        TypeShape::Int {
            bits: Some(32),
            signed: false
        }
    );
    assert_eq!(
        field_shape("public_key"),
        TypeShape::Int {
            bits: Some(256),
            signed: false
        }
    );
}

/// `stdlib.fc` is `FunC`'s own standard library — dense with `asm`
/// primitive declarations, zero `method_id`-annotated functions, zero
/// storage. The negative case for both categories at once.
#[test]
fn stdlib_recovers_nothing_but_does_not_error() {
    let extractor = FuncExtractor;
    let schema = extractor
        .extract(&fixtures_dir(), Path::new("stdlib.fc"))
        .expect("real fixture must extract without error, even with nothing to recognize");

    assert!(schema.get_methods.is_empty());
    assert_eq!(schema.storage, None);
    assert!(
        schema
            .field_confidence
            .expect("always present for Heuristic tier")
            .get_methods
            .is_none()
    );
}

/// The one hand-authored synthetic fixture (`tests/fixtures/
/// synthetic-storage-pair.fc`, honestly labeled as such — see its own
/// header comment and `SOURCE.md`) — rounds Level 2 storage's positive
/// coverage to 3 real-shaped examples (2 organic + 1 synthetic), and is
/// the one that actually caught the width-bearing-argument bug
/// (`storage.rs`'s own test module doc comment): every organic fixture's
/// `load_data` happens to only use zero-arg calls (`load_coins()`,
/// `load_msg_addr()`, `load_ref()`), so a width-bearing `load_uint(32)`/
/// `load_int(8)` call was never exercised by real corpus alone.
#[test]
fn synthetic_storage_pair_recovers_width_bearing_fields() {
    let extractor = FuncExtractor;
    let schema = extractor
        .extract(&fixtures_dir(), Path::new("synthetic-storage-pair.fc"))
        .expect("synthetic fixture must extract without error");

    let storage_ty = schema
        .storage
        .expect("synthetic fixture's whole point is a recognizable pair");
    let TypeShape::Struct { fields } = &schema.types[storage_ty.0 as usize].shape else {
        panic!("storage must be a Struct");
    };
    let shape_of = |name: &str| {
        let field = fields.iter().find(|f| f.name == name).expect("present");
        schema.types[field.ty.0 as usize].shape.clone()
    };
    assert_eq!(
        shape_of("counter"),
        TypeShape::Int {
            bits: Some(32),
            signed: false
        }
    );
    assert_eq!(
        shape_of("flags"),
        TypeShape::Int {
            bits: Some(8),
            signed: true
        }
    );
}

/// `simple-subscription-plugin.fc`'s real `load_storage()`/
/// `save_storage(...)` pair — structurally the same paired wrapper-function
/// idiom as `load_data`/`save_data`, just under a different name
/// (`docs/domains/method-dictionary.md`). `amount`
/// specifically exercises a real stdlib alias fixed alongside it:
/// `ds~load_grams()`, not `load_coins()`.
#[test]
fn simple_subscription_plugin_recovers_the_named_pair_storage() {
    let extractor = FuncExtractor;
    let schema = extractor
        .extract(&fixtures_dir(), Path::new("simple-subscription-plugin.fc"))
        .expect("real fixture must extract without error");

    let storage_ty = schema
        .storage
        .expect("the load_storage/save_storage pair must be recognized");
    let storage_type = &schema.types[storage_ty.0 as usize];
    let TypeShape::Struct { fields } = &storage_type.shape else {
        panic!("storage must be a Struct, got {:?}", storage_type.shape);
    };

    let field_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        field_names,
        vec![
            "wallet",
            "beneficiary",
            "amount",
            "period",
            "start_time",
            "timeout",
            "last_payment_time",
            "last_request_time",
            "failed_attempts",
            "subscription_id",
        ],
        "field names must come from save_storage's parameter names, in order"
    );

    let shape_of = |name: &str| {
        let field = fields.iter().find(|f| f.name == name).expect("present");
        schema.types[field.ty.0 as usize].shape.clone()
    };
    assert_eq!(
        shape_of("wallet"),
        TypeShape::Address,
        "wallet comes from ds~load_msg_addr()"
    );
    assert_eq!(
        shape_of("amount"),
        TypeShape::Coins,
        "amount comes from ds~load_grams(), a real FunC stdlib alias for load_coins"
    );
    assert_eq!(
        shape_of("failed_attempts"),
        TypeShape::Int {
            bits: Some(8),
            signed: false
        }
    );

    let field_confidence = schema
        .field_confidence
        .expect("a Tier 2 schema with recognized storage must carry field_confidence");
    assert!(field_confidence.storage.is_some());
}
