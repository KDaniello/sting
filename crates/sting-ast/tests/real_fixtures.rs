//! Integration tests against real, vendored `tolk-bench` fixtures (see
//! `corpus/fixtures/tolk-bench/SOURCE.md`). `AstStore` is exercised against at
//! least three real files rather than against synthetic inline source alone,
//! which is why this is a separate integration test rather than part of
//! `src/lib.rs`'s unit tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code, see docs/ARCHITECTURE.md §3.2"
)]

use std::path::Path;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fixtures/tolk-bench")
}

fn build_store(file_name: &str) -> (String, sting_ast::AstStore) {
    let source = std::fs::read_to_string(fixtures_dir().join(file_name))
        .unwrap_or_else(|e| panic!("reading fixture {file_name}: {e}"));
    let tree = sting_ast::parse(&source).expect("grammar loads");
    let store = sting_ast::AstStore::build(&tree, &source);
    (source, store)
}

#[test]
fn jetton_minter_yields_its_contract_header_and_oninternalmessage() {
    let (_source, store) = build_store("JettonMinter.tolk");

    let contracts: Vec<_> = store.contracts().collect();
    assert_eq!(
        contracts.len(),
        1,
        "JettonMinter.tolk declares exactly one contract{{}} header"
    );
    assert_eq!(contracts[0].name, "JettonMinter");
    assert!(
        contracts[0]
            .fields
            .iter()
            .any(|(k, v)| k == "storage" && v == "MinterStorage"),
        "expected a storage: MinterStorage field, got {:?}",
        contracts[0].fields
    );

    assert!(
        store.function_by_name("onInternalMessage").is_some(),
        "JettonMinter.tolk defines fun onInternalMessage(...)"
    );
}

#[test]
fn jetton_wallet_yields_its_contract_header() {
    let (_source, store) = build_store("JettonWallet.tolk");
    let contracts: Vec<_> = store.contracts().collect();
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0].name, "JettonWallet");
}

#[test]
fn nft_item_yields_its_contract_header_with_extra_field() {
    let (_source, store) = build_store("NftItem.tolk");
    let contracts: Vec<_> = store.contracts().collect();
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0].name, "NftItem");
    // Confirmed against the real file: its header has a fourth
    // field beyond author/incomingMessages/storage, proving the fallback
    // extractor doesn't hardcode exactly three fields.
    assert!(
        contracts[0]
            .fields
            .iter()
            .any(|(k, _)| k == "storageAtDeployment"),
        "expected the storageAtDeployment field the real header carries, got {:?}",
        contracts[0].fields
    );
}

#[test]
fn wallet_v5_yields_its_contract_header_with_incoming_external_field() {
    let (_source, store) = build_store("WalletV5.tolk");
    let contracts: Vec<_> = store.contracts().collect();
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0].name, "WalletV5");
    assert!(
        contracts[0]
            .fields
            .iter()
            .any(|(k, _)| k == "incomingExternal")
    );
}

#[test]
fn storage_file_yields_structs_and_methods_but_no_contract_header() {
    let (_source, store) = build_store("storage.tolk");

    let struct_names: Vec<_> = store.structs().map(|s| s.name.as_str()).collect();
    assert_eq!(struct_names, vec!["WalletStorage", "MinterStorage"]);

    assert!(
        store.contracts().next().is_none(),
        "storage.tolk has no contract{{}} header"
    );

    let minter_save = store
        .method_by_receiver_and_name("MinterStorage", "save")
        .expect("MinterStorage.save method found");
    assert_eq!(minter_save.parameters.len(), 1);
    assert_eq!(minter_save.parameters[0].name, "self");

    let minter_storage = store
        .struct_by_name("MinterStorage")
        .expect("MinterStorage struct found");
    assert_eq!(minter_storage.fields.len(), 4);
    assert_eq!(minter_storage.fields[0].name, "totalSupply");
    assert_eq!(minter_storage.fields[0].type_text, "coins");
}

#[test]
fn every_definition_range_falls_within_its_own_source_text() {
    // Cross-cutting sanity check across all 5 fixtures: every SourceRange's
    // byte offsets must be internally consistent (start <= end, end within
    // the file), catching an off-by-one in SourceRange::from_node that a
    // single-file test might not happen to exercise.
    for file_name in [
        "JettonMinter.tolk",
        "JettonWallet.tolk",
        "NftItem.tolk",
        "WalletV5.tolk",
        "storage.tolk",
    ] {
        let (source, store) = build_store(file_name);
        let len = source.len();
        for f in store.functions() {
            assert!(
                f.range.start_byte <= f.range.end_byte && f.range.end_byte <= len,
                "{file_name}: {f:?}"
            );
        }
        for s in store.structs() {
            assert!(
                s.range.start_byte <= s.range.end_byte && s.range.end_byte <= len,
                "{file_name}: {s:?}"
            );
        }
        for c in store.contracts() {
            assert!(
                c.range.start_byte <= c.range.end_byte && c.range.end_byte <= len,
                "{file_name}: {c:?}"
            );
        }
    }
}
