//! `ID` — Inconsistent Data (`docs/domains/detectors.md`).
//!
//! `CertiK`'s swapped-`load_data`/`save_data`-arguments pattern: a "manual"
//! storage write (`beginCell().storeUint(a, ..).storeSlice(b, ..)`) and a
//! "manual" storage read (`val a = s.loadUint(..); val b = s.loadSlice(..)`)
//! that name the same fields in a different order. Both same-typed fields
//! (commonly both `int`), so the type checker cannot catch it.
//!
//! `DetectorKind::Dataflow`, genuinely: unlike `LEP`'s boolean "did we ever
//! see X" question, this needs an **ordered sequence** of events reaching
//! each point, correctly merged across `if`/`match` branches — exactly what
//! `sting_dataflow::{Semilattice, Transfer, WorklistSolver}` (`sting-dataflow`'s
//! own doc comment names a "real Tolk-specific `Transfer` implementation";
//! this is it) exists for. The fact
//! lattice is the textbook flat/constant-propagation shape: `Undef` (no
//! events reached this point yet) below every concrete `Seq`, all `Seq`s
//! mutually incomparable, `Conflict` above everything (two branches
//! disagreed on the sequence so far — the merged sequence is unknowable, not
//! guessed at).
//!
//! The scope is the narrowing the document permits — reliable only for
//! single-function load and save pairs with no intermediate helper:
//! - Only whole-function bodies whose **every exit block** agrees on the
//!   same event `Seq` are classified at all; anything ambiguous is skipped,
//!   not guessed at.
//! - A function is "store-like" only if *every* dataflow event in it is a
//!   `storeXxx` call (≥ 2 of them); "load-like" only if every event is a
//!   `loadXxx` call whose result is bound to a name (≥ 2). A function
//!   mixing both, or with fewer than 2, is not classified — not a false
//!   "no finding" claim, an honest "outside v1's shape".
//! - Struct-based (auto-serialized) storage is **not** special-cased to be
//!   excluded: it does not need to be. `docs.ton.org`'s own Tolk-vs-FunC
//!   stdlib table (checked against the real corpus — none of the 5
//!   `corpus/fixtures/tolk-bench/*.tolk` files call a single `storeXxx`/
//!   `loadXxx` method) says manual `slice.loadXXX`/`builder.storeXXX` calls
//!   exist for manual cell parsing but "their use is not recommended" —
//!   auto-serialization's generated code isn't Tolk *source* a detector
//!   walking `tree-sitter-tolk`'s parse of a `.tolk` file ever sees, so the
//!   `store`/`load` event scan structurally never fires on it. See this
//!   the negative fixture that locks this in.
//!
//! A store-like function is only ever compared against a load-like function
//! attributed to the **same** contract (nearest-preceding `contract{}`
//! header, `support::nearest_contract_for` — same fix, same root cause, as
//! `GvrDetector`). The original implementation compared every store-like
//! function against every load-like function in the analyzed unit
//! regardless of which contract each belonged to: harmless for a real
//! `sting lint <file>` call (never sees more than one contract), but a
//! source of false positives inside `sting-bench`'s multi-file-merged
//! analysis units, where two unrelated contracts' manual (de)serialization
//! helpers can name the same fields in different orders purely by
//! coincidence. Two functions that are both unattributed (no contract header
//! precedes either — e.g. a shared helper file) are still compared, matching
//! this detector's original file-scoped behavior for that common case.

use std::collections::HashSet;

use sting_ast::{AstStore, ContractDef};
use sting_core::{DetectorId, Finding, Location, Severity};

use crate::store_load::{Classification, KeyedEvent, classify_function, same_contract};
use crate::support::{
    CrossFileCache, contracts_in_source_order, function_body, location_of, nearest_contract_for,
};
use crate::{Detector, DetectorContext, DetectorKind};

/// One classified `storeXxx`/`loadXxx` event, past the point where its
/// [`Location`] is needed — [`KeyedEvent::node`]'s only use in this module
/// was ever [`location_of`], computed once, eagerly, right after
/// classification, specifically so this module's own comparison loop below
/// never needs to hold a [`tree_sitter::Node`] borrowed from a *different*
/// file's tree alive (a cross-file candidate's tree lives only as long as
/// the [`CrossFileCache`] entry it came from — see [`gather_events`]).
struct OwnedEvent {
    key: String,
    location: Location,
}

fn owned_events(events: &[KeyedEvent<'_>], ctx: &DetectorContext) -> Vec<OwnedEvent> {
    events
        .iter()
        .map(|e| OwnedEvent {
            key: e.key.clone(),
            location: location_of(e.node, ctx),
        })
        .collect()
}

/// One store-like or load-like function, tagged with its same-file-only
/// contract attribution (`None` unconditionally for a cross-file candidate
/// — see [`gather_events`]'s own doc comment for why that is the correct,
/// conservative choice, not a shortcut).
type Candidate<'a> = (String, Vec<OwnedEvent>, Option<&'a ContractDef>);

/// Gathers every store-like/load-like function this file's own functions
/// should be compared against: `store`'s own file, plus — only when `ctx`
/// carries a compiled `ProjectStore` (`sting-pipeline::LintController::
/// lint_paths` succeeded against a real `tolk` binary; absent otherwise) —
/// every file reachable from it via `import`.
///
/// A cross-file candidate is always attributed `owner: None`.
/// `nearest_contract_for`'s "closest preceding `contract{}` header" concept
/// is inherently file-scoped — a contract's own functions live in the same
/// file as its header, by construction — so a function reached only via
/// cross-file resolution has no coherent same-file contract to attribute to
/// here. This is the conservative direction, not a gap: `same_contract`
/// still permits comparing two unattributed candidates
/// (`same_contract(None, None) == true`), which is the realistic
/// real-corpus shape — manual (de)serialization helpers overwhelmingly live
/// in headerless `storage.tolk`-style files, confirmed throughout this
/// workspace's own corpus — while a cross-file candidate is simply never
/// compared against a same-file candidate that *does* carry a real `Some`
/// attribution, matching this module's existing under-report-not-
/// over-report default for every other unattributed case.
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

pub(crate) struct IdDetector;

impl Detector for IdDetector {
    fn id(&self) -> DetectorId {
        "ID"
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
                    // Attributed to two different contracts (or one
                    // attributed and one not) — not a real load/save pair,
                    // just two unrelated manual (de)serialization helpers
                    // that happen to coexist in the same analysis unit.
                    continue;
                }
                let store_keys: HashSet<&str> = store_seq.iter().map(|e| e.key.as_str()).collect();
                let load_keys: HashSet<&str> = load_seq.iter().map(|e| e.key.as_str()).collect();
                if store_keys != load_keys {
                    // Different fields entirely: either an unrelated pair or
                    // a shape v1 does not attempt to reason about further.
                    continue;
                }
                let store_order: Vec<&str> = store_seq.iter().map(|e| e.key.as_str()).collect();
                let load_order: Vec<&str> = load_seq.iter().map(|e| e.key.as_str()).collect();
                if store_order == load_order {
                    continue;
                }
                let divergence = store_seq
                    .iter()
                    .zip(load_seq.iter())
                    .find(|(s, l)| s.key != l.key)
                    .map_or(&store_seq[0], |(s, _)| s);
                findings.push(Finding {
                    detector_id: self.id(),
                    severity: self.default_severity(),
                    message: format!(
                        "storage write order in `{store_fn}` ({}) does not match read order in `{load_fn}` ({}) — same fields, different positions",
                        store_order.join(", "),
                        load_order.join(", ")
                    ),
                    remediation: format!(
                        "reorder either `{store_fn}`'s `storeXxx` calls or `{load_fn}`'s `loadXxx` calls so both sequences list the same fields in the same order: {}",
                        load_order.join(", ")
                    ),
                    location: divergence.location.clone(),
                    confidence: None,
                    dynamic_verification: None,
                    suggestion: None,
                });
            }
        }
        findings
    }
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

    use super::IdDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        IdDetector.check(&store, &ctx)
    }

    const SAVE: &str = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 64).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";

    #[test]
    fn flags_swapped_field_order_between_manual_save_and_load() {
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val owner = s.loadUint(256);\n    val balance = s.loadUint(64);\n}\n";
        let source = format!("{SAVE}\n{load}");
        let findings = run(&source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "ID");
        assert!(findings[0].message.contains("save"));
        assert!(findings[0].message.contains("load"));
    }

    #[test]
    fn does_not_flag_matching_field_order() {
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        let source = format!("{SAVE}\n{load}");
        assert!(run(&source).is_empty());
    }

    #[test]
    fn does_not_flag_a_struct_based_load_save_pair() {
        // The real, idiomatic shape (100% of corpus/fixtures/tolk-bench):
        // no storeXxx/loadXxx calls at all, so no events are ever collected.
        let source = "fun MinterStorage.load() {\n    return MinterStorage.fromCell(contract.getData());\n}\nfun MinterStorage.save(self) {\n    contract.setData(self.toCell());\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_field_sets_differ() {
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val ownerAddr = s.loadUint(256);\n    val extra = s.loadUint(64);\n}\n";
        let source = format!("{SAVE}\n{load}");
        assert!(
            run(&source).is_empty(),
            "different field names entirely is not the swapped-order pattern"
        );
    }

    #[test]
    fn does_not_flag_a_lone_store_function_with_no_matching_load() {
        assert!(run(SAVE).is_empty());
    }

    #[test]
    fn does_not_classify_a_single_store_call_as_a_store_function() {
        // < 2 events: not enough to call it a "manual save chain" at all.
        let source = "fun save(balance: int) {\n    var b = beginCell().storeUint(balance, 64);\n    contract.setData(b.endCell());\n}\nfun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_across_two_unrelated_contracts_that_swap_order_by_coincidence() {
        // Real-corpus shape: two independent contracts, each with its own
        // manual save/load helper, in the same analysis unit (the shape
        // `sting-bench`'s multi-file merging produces for a directory
        // holding more than one contract — see `gvr.rs`'s module doc for
        // the concrete example this mirrors). `A`'s save order and `B`'s
        // load order happen to name the same two fields in a different
        // order purely by coincidence; they must not be compared at all.
        let source = format!(
            "contract A {{\n    author: \"test\"\n}}\n\n{SAVE}\n\ncontract B {{\n    author: \"test\"\n}}\n\nfun load() {{\n    var s = contract.getData().beginParse();\n    val owner = s.loadUint(256);\n    val balance = s.loadUint(64);\n}}\n"
        );
        assert!(run(&source).is_empty());
    }

    #[test]
    fn flags_a_swapped_pair_that_both_belong_to_the_same_contract_in_a_multi_contract_file() {
        // Same two-contract shape, but this time the swapped save/load pair
        // both belong to `A` — confirms the fix doesn't just suppress
        // everything in a multi-contract file, it still catches a real
        // same-contract swap.
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val owner = s.loadUint(256);\n    val balance = s.loadUint(64);\n}\n";
        let source = format!(
            "contract A {{\n    author: \"test\"\n}}\n\n{SAVE}\n{load}\ncontract B {{\n    author: \"test\"\n}}\n\nfun unrelated() {{\n    return;\n}}\n"
        );
        let findings = run(&source);
        assert_eq!(
            findings.len(),
            1,
            "A's own swapped save/load pair should still be flagged"
        );
    }

    #[test]
    fn kind_is_dataflow_and_severity_is_high() {
        assert_eq!(IdDetector.kind(), crate::DetectorKind::Dataflow);
        assert_eq!(IdDetector.default_severity(), sting_core::Severity::High);
    }

    #[test]
    fn flags_a_cross_file_swapped_pair() {
        // `save` lives in the file being linted, `load`
        // lives in a real, different file on disk, resolved only through a
        // hand-built `ProjectStore` — no `tolk.exe` needed, mirrors `ac.rs`'s
        // own cross-file test style.
        let dir = tempfile::tempdir().expect("tempdir");
        let load_path = dir.path().join("load.tolk");
        std::fs::write(
            &load_path,
            "fun load() {\n    var s = contract.getData().beginParse();\n    val owner = s.loadUint(256);\n    val balance = s.loadUint(64);\n}\n",
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

        let tree = sting_ast::parse(SAVE).expect("grammar loads");
        let store = AstStore::build(&tree, SAVE);
        let ctx = DetectorContext::new("main.tolk".into(), tree, SAVE.to_string())
            .with_project(project, 0);

        let findings = IdDetector.check(&store, &ctx);
        assert_eq!(
            findings.len(),
            1,
            "save (this file) and load (cross-file) name the same fields in swapped order"
        );
        assert!(findings[0].message.contains("save"));
        assert!(findings[0].message.contains("load"));
    }

    #[test]
    fn does_not_flag_a_cross_file_pair_with_matching_order() {
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

        let tree = sting_ast::parse(SAVE).expect("grammar loads");
        let store = AstStore::build(&tree, SAVE);
        let ctx = DetectorContext::new("main.tolk".into(), tree, SAVE.to_string())
            .with_project(project, 0);

        assert!(IdDetector.check(&store, &ctx).is_empty());
    }

    #[test]
    fn does_not_flag_cross_file_when_no_project_is_attached() {
        // Same fixture files as the flagging test above, but through
        // `lint_source`'s exact shape (no `.with_project` call) — proves the
        // cross-file gathering loop is a true no-op, not just skipped by
        // coincidence, when no `ProjectStore` exists at all.
        let tree = sting_ast::parse(SAVE).expect("grammar loads");
        let store = AstStore::build(&tree, SAVE);
        let ctx = DetectorContext::new("main.tolk".into(), tree, SAVE.to_string());
        assert!(IdDetector.check(&store, &ctx).is_empty());
    }
}
