//! `TLB` — TL-B (De)Serialization Mismatch (`sting_core::DefectClass::Tlb`'s
//! own doc comment for why this is a distinct class from `ID`, not a
//! broadening of it; for where this
//! detector came from).
//!
//! A manual storage write and its matching read name the *same* field (same
//! comparison key `ID` already uses — see `store_load`'s module doc
//! comment) but disagree on its TL-B shape: different declared bit-width
//! (`storeUint(balance, 32)` written back with `loadUint(64)`), different
//! signedness (`storeInt`/`loadUint` on the same field), or a completely
//! different primitive (`storeCoins`/`loadUint`). Any of these silently
//! misreads storage — not just the mismatched field itself, but potentially
//! every field read *after* it in the same slice, since a wrong bit-width
//! shifts the parse position for everything downstream.
//!
//! Reuses `ID`'s exact event-collection/classification/contract-attribution
//! machinery (`store_load::classify_function`) — the same whole-function,
//! branch-merged store-like/load-like classification, the same
//! same-contract requirement, even the same `store_keys == load_keys`
//! precondition before comparing anything further (a field-set mismatch
//! means this is not a real corresponding pair at all, not something to
//! reason about further — identical reasoning to `ID`'s own). The only real
//! difference: where `ID` compares each matched field's *position*, `TLB`
//! compares each matched field's *declared shape*.
//!
//! v1 recognizes exactly 5 TL-B primitives — the ones with an unambiguous,
//! self-contained shape (`docs.ton.org`'s Tolk stdlib reference; cross-
//! checked against every `store`/`load` method actually used anywhere in
//! this workspace's own corpus, `corpus/`):
//! - `storeUint`/`loadUint`, `storeInt`/`loadInt`: the width argument must be
//!   a plain integer literal sting can read statically — a named constant or
//!   computed expression is not evaluated, that pair is left unclassified,
//!   not guessed at (the same "ambiguous → skip" stance `ID` documents for
//!   its own v1 scope).
//! - `storeCoins`/`loadCoins` (`VarUInteger16`, self-describing length
//!   prefix), `storeAddress`/`loadAddress` (`MsgAddress`), `storeBool`/
//!   `loadBool` (1 bit) — fixed shapes, no width to parse.
//!
//! Deliberately **not** attempted: `storeSlice`/`storeRef`/`storeDict`/
//! `loadRef`/`loadDict`/`loadBits`/... — `loadBits` in particular has no
//! same-named `storeBits` counterpart anywhere in this corpus (`storeSlice`
//! is the closest write-side analogue, but the two are not a confident 1:1
//! pairing), so guessing a compatibility rule for them risks exactly the
//! false-positive/false-negative pairs this project's whole precision-first
//! methodology exists to avoid.

use std::collections::{HashMap, HashSet};

use sting_ast::{AstStore, ContractDef};
use sting_core::{DetectorId, Finding, Location, Severity};

use crate::store_load::{Classification, KeyedEvent, classify_function, same_contract};
use crate::support::{
    CrossFileCache, contracts_in_source_order, function_body, location_of, nearest_contract_for,
    nth_call_argument_text,
};
use crate::{Detector, DetectorContext, DetectorKind};

/// One classified `storeXxx`/`loadXxx` event, past the point where its
/// [`Location`]/[`TlbShape`] are needed — both are computed eagerly, right
/// after classification, specifically so this module's own comparison loop
/// below never needs to hold a [`tree_sitter::Node`] (or the `source: &str`
/// it must be read against) borrowed from a *different* file's tree alive —
/// a cross-file candidate's tree/source live only as long as the
/// [`CrossFileCache`] entry they came from (see [`gather_events`]).
struct OwnedEvent {
    key: String,
    method: String,
    location: Location,
    shape: Option<TlbShape>,
}

fn owned_events(events: &[KeyedEvent<'_>], ctx: &DetectorContext) -> Vec<OwnedEvent> {
    let source = ctx.source();
    events
        .iter()
        .map(|e| OwnedEvent {
            key: e.key.clone(),
            method: e.method.clone(),
            location: location_of(e.node, ctx),
            shape: tlb_shape(e, source),
        })
        .collect()
}

/// One store-like or load-like function, tagged with its same-file-only
/// contract attribution — see [`gather_events`]'s own doc comment
/// (identical reasoning and shape to `id.rs`'s own `Candidate`, this
/// module's twin consumer of `store_load.rs`).
type Candidate<'a> = (String, Vec<OwnedEvent>, Option<&'a ContractDef>);

/// Gathers every store-like/load-like function this file's own functions
/// should be compared against — same-file plus, when a compiled
/// `ProjectStore` is attached, every file reachable via `import`
/// Identical
/// shape and reasoning to `id.rs::gather_events` (see that function's own,
/// more detailed doc comment for why a cross-file candidate is always
/// `owner: None`) — kept as a separate, monomorphic copy rather than a
/// shared generic helper: the two differ in exactly what gets eagerly
/// computed per event (`id.rs` only ever needed `Location`; this module
/// also needs `TlbShape`, which requires the event's own file's `source`
/// text still in scope at conversion time), and forcing one generic
/// abstraction over that difference costs more than the ~30 duplicated
/// lines it would save.
fn gather_events<'a>(
    store: &'a AstStore,
    ctx: &'a DetectorContext,
) -> (Vec<Candidate<'a>>, Vec<Candidate<'a>>) {
    let source = ctx.source();
    let contracts_in_order = contracts_in_source_order(store);
    let mut stores = Vec::new();
    let mut loads = Vec::new();

    for function in store.functions() {
        let Some(body) = function_body(function, ctx) else {
            continue;
        };
        let owner = nearest_contract_for(function.range.start_byte, &contracts_in_order);
        match classify_function(body, source) {
            Some(Classification::Store(seq)) => {
                stores.push((function.name.clone(), owned_events(&seq, ctx), owner));
            }
            Some(Classification::Load(seq)) => {
                loads.push((function.name.clone(), owned_events(&seq, ctx), owner));
            }
            None => {}
        }
    }

    let Some(project) = ctx.project_store() else {
        return (stores, loads);
    };
    let Some(file_id) = ctx.file_id() else {
        return (stores, loads);
    };
    let mut cross_file = CrossFileCache::default();
    for reachable_id in project.reachable_files(file_id) {
        if reachable_id == file_id {
            continue;
        }
        let Some(path) = project.path_for_file_id(reachable_id) else {
            continue;
        };
        let Some(loaded) = cross_file.get_or_load(&path, project) else {
            continue;
        };
        let (cross_store, cross_ctx) = (&loaded.0, &loaded.1);
        let cross_source = cross_ctx.source();
        for function in cross_store.functions() {
            let Some(body) = function_body(function, cross_ctx) else {
                continue;
            };
            match classify_function(body, cross_source) {
                Some(Classification::Store(seq)) => {
                    stores.push((function.name.clone(), owned_events(&seq, cross_ctx), None));
                }
                Some(Classification::Load(seq)) => {
                    loads.push((function.name.clone(), owned_events(&seq, cross_ctx), None));
                }
                None => {}
            }
        }
    }

    (stores, loads)
}

pub(crate) struct TlbDetector;

impl Detector for TlbDetector {
    fn id(&self) -> DetectorId {
        "TLB"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Dataflow
    }

    fn default_severity(&self) -> Severity {
        Severity::High
    }

    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let (stores, loads) = gather_events(store, ctx);

        let mut findings = Vec::new();
        for (store_fn, store_seq, store_owner) in &stores {
            for (load_fn, load_seq, load_owner) in &loads {
                if !same_contract(*store_owner, *load_owner) {
                    continue;
                }
                let store_keys: HashSet<&str> = store_seq.iter().map(|e| e.key.as_str()).collect();
                let load_keys: HashSet<&str> = load_seq.iter().map(|e| e.key.as_str()).collect();
                if store_keys != load_keys {
                    // Different fields entirely — not a real corresponding
                    // pair, same precondition ID's own comparison requires.
                    continue;
                }
                let load_by_key: HashMap<&str, &OwnedEvent> =
                    load_seq.iter().map(|e| (e.key.as_str(), e)).collect();
                for store_event in store_seq {
                    let Some(load_event) = load_by_key.get(store_event.key.as_str()) else {
                        continue;
                    };
                    let Some(store_shape) = store_event.shape else {
                        continue;
                    };
                    let Some(load_shape) = load_event.shape else {
                        continue;
                    };
                    if store_shape == load_shape {
                        continue;
                    }
                    findings.push(Finding {
                        detector_id: self.id(),
                        severity: self.default_severity(),
                        message: format!(
                            "`{store_fn}` writes `{}` as `{}` ({}) but `{load_fn}` reads it back as `{}` ({}) — different TL-B shape, storage will be misread",
                            store_event.key,
                            store_event.method,
                            store_shape.label(),
                            load_event.method,
                            load_shape.label()
                        ),
                        remediation: format!(
                            "change either `{store_fn}`'s `{}` call or `{load_fn}`'s `{}` call so both agree on the same width/signedness/primitive — check which side actually matches the field's intended shape before picking which one to fix",
                            store_event.method, load_event.method
                        ),
                        location: store_event.location.clone(),
                        confidence: None,
                        dynamic_verification: None,
                        suggestion: None,
                    });
                }
            }
        }
        findings
    }
}

/// A TL-B primitive shape v1 can read statically off either side of a
/// matched store/load pair — see this module's doc comment for the full
/// list and why it stops there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TlbShape {
    Uint(u16),
    Int(u16),
    Coins,
    Address,
    Bool,
}

impl TlbShape {
    fn label(self) -> String {
        match self {
            Self::Uint(width) => format!("uint{width}"),
            Self::Int(width) => format!("int{width}"),
            Self::Coins => "coins".to_string(),
            Self::Address => "address".to_string(),
            Self::Bool => "bool".to_string(),
        }
    }
}

/// Recognizes `event`'s declared TL-B shape from its method name (works for
/// both a store event, whose width is its *second* argument, and a load
/// event, whose width is its *only* argument — `storeUint`/`loadUint` differ
/// in argument position, not in which argument is "the width", so this
/// branches on the method name itself rather than needing a separate
/// store/load-specific function). `None` for any method outside this
/// module's recognized set, or a `storeUint`/`storeInt`/`loadUint`/`loadInt`
/// whose width argument is not a plain integer literal — not guessed at.
fn tlb_shape(event: &KeyedEvent<'_>, source: &str) -> Option<TlbShape> {
    match event.method.as_str() {
        "storeUint" => Some(TlbShape::Uint(literal_width(event.args, 1, source)?)),
        "loadUint" => Some(TlbShape::Uint(literal_width(event.args, 0, source)?)),
        "storeInt" => Some(TlbShape::Int(literal_width(event.args, 1, source)?)),
        "loadInt" => Some(TlbShape::Int(literal_width(event.args, 0, source)?)),
        "storeCoins" | "loadCoins" => Some(TlbShape::Coins),
        "storeAddress" | "loadAddress" => Some(TlbShape::Address),
        "storeBool" | "loadBool" => Some(TlbShape::Bool),
        _ => None,
    }
}

fn literal_width(args: tree_sitter::Node<'_>, index: usize, source: &str) -> Option<u16> {
    nth_call_argument_text(args, index, source)?.parse().ok()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use sting_ast::AstStore;

    use super::TlbDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        TlbDetector.check(&store, &ctx)
    }

    // Every fixture below carries **two** fields, not one: `store_load`'s
    // shared `classify_function` requires ≥ 2 events before it calls a
    // function store-like/load-like at all (`ID`'s own
    // `does_not_classify_a_single_store_call_as_a_store_function` test locks
    // this in for the shared machinery) — a lone `storeUint` call is not
    // confidently "a manual save helper" on its own. Realistic Storage
    // structs almost always have more than one field anyway, so this is not
    // an artificial constraint. The second field (`owner`) always matches on
    // both sides, keeping each test's single intentional mismatch isolated.

    #[test]
    fn flags_a_width_mismatch_on_the_same_field() {
        let save = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 32).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        let findings = run(&format!("{save}\n{load}"));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "TLB");
        assert!(findings[0].message.contains("uint32"));
        assert!(findings[0].message.contains("uint64"));
    }

    #[test]
    fn flags_a_signedness_mismatch_at_the_same_width() {
        let save = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeInt(balance, 64).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        let findings = run(&format!("{save}\n{load}"));
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("int64"));
        assert!(findings[0].message.contains("uint64"));
    }

    #[test]
    fn flags_a_primitive_family_mismatch() {
        let save = "fun save(amount: int, owner: int) {\n    var b = beginCell().storeCoins(amount).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val amount = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        let findings = run(&format!("{save}\n{load}"));
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("coins"));
        assert!(findings[0].message.contains("uint64"));
    }

    #[test]
    fn does_not_flag_matching_shapes() {
        let save = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 64).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        assert!(run(&format!("{save}\n{load}")).is_empty());
    }

    #[test]
    fn does_not_flag_an_address_field_that_matches_on_both_sides() {
        let save = "fun save(owner: slice, balance: int) {\n    var b = beginCell().storeAddress(owner).storeUint(balance, 64);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val owner = s.loadAddress();\n    val balance = s.loadUint(64);\n}\n";
        assert!(run(&format!("{save}\n{load}")).is_empty());
    }

    #[test]
    fn does_not_flag_a_struct_based_load_save_pair() {
        // Same real, idiomatic shape ID's own equivalent test locks in: no
        // storeXxx/loadXxx calls at all, so no events are ever collected.
        let source = "fun MinterStorage.load() {\n    return MinterStorage.fromCell(contract.getData());\n}\nfun MinterStorage.save(self) {\n    contract.setData(self.toCell());\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_field_sets_differ() {
        let save = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 32).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val other = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        assert!(
            run(&format!("{save}\n{load}")).is_empty(),
            "different field names entirely is not this detector's pattern"
        );
    }

    #[test]
    fn does_not_flag_a_width_expressed_as_a_named_constant_on_either_side() {
        // Cannot statically evaluate `BALANCE_BITS` — left unclassified, not
        // guessed at, same stance `ID` documents for its own v1 scope.
        let save = "const BALANCE_BITS = 64;\nfun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, BALANCE_BITS).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(32);\n    val owner = s.loadUint(256);\n}\n";
        assert!(run(&format!("{save}\n{load}")).is_empty());
    }

    #[test]
    fn does_not_flag_across_two_unrelated_contracts_with_coincidentally_matching_field_names() {
        // Same cross-contract-attribution guard as ID (store_load::same_contract).
        let save = "contract A {\n    author: \"test\"\n}\n\nfun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 32).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "contract B {\n    author: \"test\"\n}\n\nfun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        assert!(run(&format!("{save}\n\n{load}")).is_empty());
    }

    #[test]
    fn flags_a_swapped_and_width_mismatched_pair_that_both_belong_to_the_same_contract_in_a_multi_contract_file()
     {
        // Mirrors ID's own equivalent test: confirms the same-contract guard
        // doesn't suppress everything in a multi-contract file, only
        // cross-contract pairs.
        let save = "contract A {\n    author: \"test\"\n}\n\nfun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 32).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n\ncontract B {\n    author: \"test\"\n}\n";
        let findings = run(&format!("{save}\n{load}"));
        assert_eq!(
            findings.len(),
            1,
            "A's own width-mismatched save/load pair should still be flagged"
        );
    }

    #[test]
    fn kind_is_dataflow_and_severity_is_high() {
        assert_eq!(TlbDetector.kind(), crate::DetectorKind::Dataflow);
        assert_eq!(TlbDetector.default_severity(), sting_core::Severity::High);
    }

    #[test]
    fn flags_a_cross_file_width_mismatch() {
        // `save` in the linted file writes `balance` as
        // 32 bits, `load` — a real, different file, resolved only through a
        // hand-built `ProjectStore` — reads it back as 64.
        let dir = tempfile::tempdir().expect("tempdir");
        let load_path = dir.path().join("load.tolk");
        std::fs::write(
            &load_path,
            "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n",
        )
        .expect("write load.tolk");

        let project =
            std::sync::Arc::new(sting_tolkc::ProjectStore::new(sting_tolkc::SymbolTypes {
                files: vec![
                    sting_tolkc::FileEntry {
                        file_id: 0,
                        file_name: "main.tolk".to_string(),
                        size_chars: 0,
                        imports: vec![1],
                    },
                    sting_tolkc::FileEntry {
                        file_id: 1,
                        file_name: load_path.display().to_string(),
                        size_chars: 0,
                        imports: vec![],
                    },
                ],
                unique_types: Vec::new(),
                struct_instantiations: Vec::new(),
                alias_instantiations: Vec::new(),
                declarations: Vec::new(),
                functions: Vec::new(),
            }));

        let save = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 32).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let tree = sting_ast::parse(save).expect("grammar loads");
        let store = AstStore::build(&tree, save);
        let ctx = DetectorContext::new("main.tolk".into(), tree, save.to_string())
            .with_project(project, 0);

        let findings = TlbDetector.check(&store, &ctx);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("uint32"));
        assert!(findings[0].message.contains("uint64"));
    }
}
