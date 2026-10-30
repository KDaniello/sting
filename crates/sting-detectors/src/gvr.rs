//! `GVR` — Global Var Redefined (`docs/domains/detectors.md`).
//!
//! A local `var` declared inside a function body shares its name with a
//! storage-struct field. Reading it looks exactly like reading storage;
//! assigning to it silently does **not** update storage — `CertiK`'s
//! "namespace pollution" pattern. Purely syntactic (`DetectorKind::Ast`): no
//! control flow, no interprocedural reasoning, just a name collision between
//! two sets.
//!
//! Scope, as specified: only `var` bindings in a function's *body* are
//! checked against the *storage struct's* field names (not parameters, not
//! `val` — `val` cannot be reassigned, so it cannot produce the "looks like
//! a storage write, isn't" failure this pattern is about).
//!
//! **Cross-file storage resolution — no
//! longer out of scope, but conditional.** A contract whose storage struct
//! lives in a different file than its handlers (the common real shape —
//! see `corpus/fixtures/tolk-bench/storage.tolk` vs. `JettonWallet.tolk`)
//! is resolved same-file first ([`sting_ast::AstStore::struct_by_name`] —
//! `AstStore` itself stays single-file,
//! `docs/domains/dataflow-cfg.md`); on a miss, [`storage_field_names`]
//! falls back to [`sting_tolkc::ProjectStore::resolve_struct`], but only
//! when a real, compiled `ProjectStore` is actually attached to this
//! analysis ([`DetectorContext::with_project`] — only `sting lint`'s
//! real-`tolk.exe`-backed path attaches one; every unit test below and
//! `sting bench`'s own `lint_source` never do, see `with_project`'s own
//! doc comment). Ambiguous (more than one same-named struct reachable from
//! this file) or still unresolved even cross-file: skipped, not guessed —
//! the same fail-safe policy `ac.rs`/`uda.rs`'s own cross-file resolvers
//! already use. This can never move `sting bench`'s calibrated numbers
//! either way, for the exact structural reason already documented for
//! `AC`'s own cross-file work (`docs/domains/detectors.md` item 15):
//! `lint_source` never attaches a `ProjectStore`, and
//! `discover_merged_projects` already concatenates every corpus project's
//! files into one blob before analysis, so a once-cross-file storage
//! struct is already same-file-visible by the time `struct_by_name` runs
//! on that merged blob.
//!
//! Each function is matched only against **its own** contract's storage
//! fields, not every contract's — determined by nearest-preceding `contract{}`
//! header in source order (`support::nearest_contract_for`). Found missing
//! during a post-Block-09 manual corpus review, not anticipated in v1's
//! original design: the original implementation checked *every* function
//! found in the analyzed source against *every* contract's storage fields (a
//! full cross product), which is only ever wrong when more than one contract
//! appears in the same analysis unit — `sting lint <file>` never merges
//! files (`docs/ARCHITECTURE.md`), so a real user's
//! single-contract `.tolk` file was never actually affected; the bug only
//! ever fired inside `sting-bench`'s own multi-file-merged project analysis
//! (`docs/domains/precision-sources.md`), where 7 of the corpus's 9 GVR
//! false positives turned out to share this exact root cause (e.g.
//! `corpus/clean/tolk-bench/03_notcoin`'s `JettonMinter.tolk` — a local
//! variable in *its* `onInternalMessage` was being compared against
//! `WalletStorage`, the storage struct of the *unrelated*, separately-defined
//! `JettonWallet` contract two files later in the same merged blob). The
//! "nearest preceding" heuristic is sound specifically because
//! `discover_merged_projects` concatenates whole files in order
//! (`crates/sting-bench/src/corpus.rs`), and every real multi-contract
//! directory in this corpus keeps each contract's own `contract{}` header
//! immediately before that contract's own handler functions, inside that
//! contract's own file — the same real-world layout convention this
//! module's existing single-file boundary above already relies on. A
//! function declared *before* any `contract{}` header at all (unusual style,
//! not observed anywhere in this corpus) has no preceding contract to
//! attribute it to and is skipped — the same "not silently wrong" choice as
//! the cross-file case above, not a guess.
//!
//! Attribution is resolved against **every** contract header in the file,
//! not pre-filtered to ones with a resolvable non-empty storage struct: a
//! function's nearest-preceding contract is always the textually nearest
//! one, full stop — falling back to an *earlier* contract just because the
//! nearest one's storage cannot be resolved would silently re-attribute the
//! function to an unrelated contract, the exact cross-contamination shape
//! this module exists to prevent. If the nearest contract's own storage
//! cannot be resolved, the function is skipped, not reassigned.
//! `support::nearest_contract_for` and `support::contracts_in_source_order`
//! are shared with `IdDetector`/`UbmDetector`, which independently needed
//! the identical per-contract attribution fix.
//!
//! **Write-back exemption, found via real external validation (direction 5,
//! not this project's own corpus)** — a storage field of type `cell` (or a
//! `Map`/dict field) is often decoded into a local variable *named after the
//! field itself*, mutated, and then explicitly re-encoded back into that
//! same field a few statements later — a normal, correct Tolk idiom for a
//! nested sub-struct, not the "silently lost write" bug this detector
//! exists to catch. Confirmed in two independent, unrelated real projects
//! during a batch audit: `ton-prediction-market`'s
//! `lmsr_market_maker.tolk` (`var state = storage.state.load(); ...;
//! storage.state = state.toCell();`) and `subdomains-tolk`'s
//! `SubdomainFactory.tolk` (`var pending = st.pending.mustGet(key).load();
//! ...; st.pending.set(key, pending.toCell());` — the same shape, but for a
//! `Map`-valued field instead of a plain one). [`has_write_back`] looks,
//! anywhere later in the same function body, for an assignment
//! `<obj>.<name> = <rhs>` or a `<obj>.<name>.set(<args>)` call where the
//! field being written is exactly the shadowed name and the local variable
//! `<name>` itself is referenced somewhere in that write — deliberately
//! still "purely syntactic" (no control-flow/branch reasoning about whether
//! the write-back is on every path), matching this whole detector's stated
//! scope above. A `var <name>` with no such later write-back is still
//! flagged exactly as before.

use std::collections::HashSet;
use sting_ast::Language;

use sting_core::{DetectorId, Finding, Severity};
use tree_sitter::Node;

use crate::support::{
    as_method_call, bound_names, contracts_in_source_order, declaration_kind, function_body,
    location_of, nearest_contract_for, subtree_references_identifier,
};
use crate::{Detector, DetectorContext, DetectorKind};

/// `true` if a field access `<obj>.<name>` appears as the direct child
/// `field` of `access` and matches `name` — the shared shape both write-back
/// patterns need to check (`dot_access.field == name`).
fn dot_access_field_is(access: Node, name: &str, source: &str) -> bool {
    access.kind() == "dot_access"
        && access
            .child_by_field_name("field")
            .and_then(|f| f.utf8_text(source.as_bytes()).ok())
            == Some(name)
}

/// See this module's doc comment's "Write-back exemption" section: searches
/// `body` for a later assignment `<obj>.<name> = <rhs>` or `<obj>.<name>.set(<args>)`
/// call — evidence this `var <name>` is a decode/mutate/store-back roundtrip,
/// not a forgotten write.
fn has_write_back(body: Node, after_byte: usize, name: &str, source: &str) -> bool {
    for visited in sting_ast::Preorder::from_node(body) {
        let node = visited.node;
        if node.start_byte() <= after_byte {
            continue;
        }
        if node.kind() == "assignment" {
            let (Some(left), Some(right)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) else {
                continue;
            };
            if dot_access_field_is(left, name, source)
                && subtree_references_identifier(right, name, source)
            {
                return true;
            }
        }
        if let Some(call) = as_method_call(node, source, Language::Tolk)
            && call.method == "set"
            && dot_access_field_is(call.obj, name, source)
            && subtree_references_identifier(call.args, name, source)
        {
            return true;
        }
    }
    false
}

/// Field names of the storage struct named `storage_type`, resolved
/// same-file first and, on a miss, cross-file through `ctx`'s attached
/// `ProjectStore` — see this module's own doc comment's "Cross-file storage
/// resolution" section for the exact policy and why it never affects
/// `sting bench`'s numbers. `None` when the struct cannot be resolved at
/// all (no project attached, no reachable same-named struct, or more than
/// one — ambiguous is treated as unresolved, not guessed).
fn storage_field_names(
    store: &sting_ast::AstStore,
    ctx: &DetectorContext,
    storage_type: &str,
) -> Option<HashSet<String>> {
    if let Some(storage_struct) = store.struct_by_name(storage_type) {
        return Some(
            storage_struct
                .fields
                .iter()
                .map(|f| f.name.clone())
                .collect(),
        );
    }
    let project = ctx.project_store()?;
    let file_id = ctx.file_id()?;
    let matches = project.resolve_struct(file_id, storage_type);
    let [sting_tolkc::Declaration::Struct { fields, .. }] = matches.as_slice() else {
        return None;
    };
    Some(fields.iter().map(|f| f.name.clone()).collect())
}

pub(crate) struct GvrDetector;

impl Detector for GvrDetector {
    fn id(&self) -> DetectorId {
        "GVR"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    fn default_severity(&self) -> Severity {
        Severity::High
    }

    fn check(&self, store: &sting_ast::AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let mut findings = Vec::new();

        let contracts_in_order = contracts_in_source_order(store);

        for function in store.functions() {
            let Some(contract) =
                nearest_contract_for(function.range.start_byte, &contracts_in_order)
            else {
                continue;
            };
            let Some((_, storage_type)) = contract.fields.iter().find(|(k, _)| k == "storage")
            else {
                continue;
            };
            let Some(field_names) = storage_field_names(store, ctx, storage_type) else {
                continue;
            };
            if field_names.is_empty() {
                continue;
            }

            let Some(body) = function_body(function, ctx) else {
                continue;
            };
            for visited in sting_ast::Preorder::from_node(body) {
                let node = visited.node;
                if node.kind() != "var_declaration_lhs" {
                    continue;
                }
                if declaration_kind(node, ctx.source()).as_deref() != Some("var") {
                    continue;
                }
                for (name_node, name) in bound_names(node, ctx.source()) {
                    if !field_names.contains(name.as_str()) {
                        continue;
                    }
                    if has_write_back(body, name_node.start_byte(), &name, ctx.source()) {
                        continue;
                    }
                    findings.push(Finding {
                        detector_id: self.id(),
                        severity: self.default_severity(),
                        message: format!(
                            "local variable `{name}` shadows storage field `{name}` of `{}` (contract `{}`) — assigning to it will not update contract storage",
                            storage_type, contract.name
                        ),
                        remediation: format!(
                            "rename this local variable to something that isn't also a storage field name, or if the intent really is to persist it, assign through the loaded storage value (e.g. `storage.{name} = ...;`) instead of declaring a new local `{name}`"
                        ),
                        location: location_of(name_node, ctx),
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use sting_ast::AstStore;

    use super::GvrDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        GvrDetector.check(&store, &ctx)
    }

    const CONTRACT_AND_STORAGE: &str = r#"
contract Wallet {
    author: "test"
    storage: WalletStorage
}

struct WalletStorage {
    owner: address
    balance: coins
}
"#;

    #[test]
    fn flags_a_local_var_shadowing_a_storage_field() {
        let source =
            format!("{CONTRACT_AND_STORAGE}\nfun f() {{\n    var owner = someAddress;\n}}\n");
        let findings = run(&source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "GVR");
        assert!(findings[0].message.contains("owner"));
        assert!(findings[0].message.contains("WalletStorage"));
    }

    #[test]
    fn does_not_flag_val_since_it_cannot_be_reassigned() {
        let source =
            format!("{CONTRACT_AND_STORAGE}\nfun f() {{\n    val owner = someAddress;\n}}\n");
        assert!(run(&source).is_empty());
    }

    #[test]
    fn does_not_flag_a_var_with_an_unrelated_name() {
        let source = format!("{CONTRACT_AND_STORAGE}\nfun f() {{\n    var totalCount = 0;\n}}\n");
        assert!(run(&source).is_empty());
    }

    #[test]
    fn flags_a_shadowed_name_inside_tuple_destructuring() {
        let source =
            format!("{CONTRACT_AND_STORAGE}\nfun f() {{\n    var (owner, x) = pair();\n}}\n");
        let findings = run(&source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("owner"));
    }

    #[test]
    fn does_not_flag_a_function_against_an_unrelated_second_contracts_storage() {
        // Real corpus finding, not a hypothetical: two independent contracts
        // (own header, own storage struct, own functions) in the same
        // analysis unit — the shape `sting-bench`'s multi-file merging
        // produces for a directory holding more than one contract (e.g.
        // `corpus/clean/tolk-bench/03_notcoin`'s `JettonMinter.tolk` +
        // `JettonWallet.tolk`). `f`'s `var owner` must be checked against
        // `AStorage` (its own, nearest-preceding contract's storage), never
        // against `BStorage` — even though `BStorage` also happens to
        // declare a field named `owner`.
        let source = r#"
contract A {
    author: "test"
    storage: AStorage
}

struct AStorage {
    balance: coins
}

fun f() {
    var owner = someAddress;
}

contract B {
    author: "test"
    storage: BStorage
}

struct BStorage {
    owner: address
}

fun g() {
    return;
}
"#;
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_each_function_against_its_own_nearest_preceding_contract_only() {
        // Same two-contract shape, but this time `A`'s own function *does*
        // shadow `A`'s own storage field — confirms the fix doesn't just
        // suppress everything in a multi-contract file, it correctly
        // attributes each function to its real owner.
        let source = r#"
contract A {
    author: "test"
    storage: AStorage
}

struct AStorage {
    owner: address
}

fun f() {
    var owner = someAddress;
}

contract B {
    author: "test"
    storage: BStorage
}

struct BStorage {
    owner: address
}

fun g() {
    return;
}
"#;
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "only f's own shadowing should be flagged"
        );
        assert!(findings[0].message.contains("contract `A`"));
    }

    #[test]
    fn does_not_flag_when_storage_struct_is_not_in_this_file() {
        // The common real-world shape (`corpus/fixtures/tolk-bench/storage.tolk`
        // vs. `JettonWallet.tolk`): the contract header names a storage type
        // this file never defines, and — unlike the test below — no
        // `ProjectStore` is attached at all (`run` never attaches one, same
        // as `sting bench`'s own `lint_source`), so there is nothing to
        // resolve cross-file against either. Still a documented boundary,
        // not a bug: `sting lint`'s real path (tested below) covers the
        // case this file's own local `run` helper structurally cannot.
        let source = r#"
contract Wallet {
    author: "test"
    storage: WalletStorage
}

fun f() {
    var owner = someAddress;
}
"#;
        assert!(run(source).is_empty());
    }

    #[test]
    fn resolves_a_cross_file_storage_struct_when_a_project_store_is_attached() {
        // Same shape as the test above, but this time a real `ProjectStore`
        // is attached (mirrors `ac.rs`'s own
        // `resolves_a_boolean_auth_helper_across_files`) — `WalletStorage`
        // lives in a second file, reachable via one import hop.
        let dir = tempfile::tempdir().expect("tempdir");
        let storage_path = dir.path().join("storage.tolk");
        std::fs::write(
            &storage_path,
            "struct WalletStorage {\n    owner: address\n    balance: coins\n}\n",
        )
        .expect("write storage.tolk");
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
                        file_name: storage_path.display().to_string(),
                        size_chars: 0,
                        imports: vec![],
                    },
                ],
                unique_types: Vec::new(),
                struct_instantiations: Vec::new(),
                alias_instantiations: Vec::new(),
                declarations: vec![sting_tolkc::Declaration::Struct {
                    name: "WalletStorage".to_string(),
                    ty_idx: 0,
                    ident_loc: Some(sting_tolkc::SourceSpan::from([1, 1, 1, 1, 13])),
                    type_params: Vec::new(),
                    prefix: None,
                    fields: vec![
                        sting_tolkc::FieldEntry {
                            name: "owner".to_string(),
                            ty_idx: 0,
                        },
                        sting_tolkc::FieldEntry {
                            name: "balance".to_string(),
                            ty_idx: 0,
                        },
                    ],
                    description: None,
                }],
                functions: Vec::new(),
            }));

        let source = "contract Wallet {\n    author: \"test\"\n    storage: WalletStorage\n}\n\nfun f() {\n    var owner = someAddress;\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("main.tolk"), tree, source.to_string())
            .with_project(project, 0);
        let findings = GvrDetector.check(&store, &ctx);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("owner"));
        assert!(findings[0].message.contains("WalletStorage"));
    }

    #[test]
    fn does_not_fall_back_to_an_earlier_contract_when_the_nearest_ones_storage_is_unresolvable() {
        // `g`'s nearest-preceding contract is `B`, whose storage type
        // `MissingStorage` is not defined anywhere in this file. The
        // correct behaviour is to skip `g` entirely, not to reattribute it
        // to `A` (the only *other* contract in the file) just because `A`'s
        // storage happens to resolve — that fallback would reintroduce the
        // exact cross-contract contamination this detector exists to avoid.
        let source = r#"
contract A {
    author: "test"
    storage: AStorage
}

struct AStorage {
    owner: address
}

contract B {
    author: "test"
    storage: MissingStorage
}

fun g() {
    var owner = someAddress;
}
"#;
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_there_is_no_contract_header_at_all() {
        let source = r"
struct WalletStorage {
    owner: address
}

fun f() {
    var owner = someAddress;
}
";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_cell_field_decoded_mutated_and_stored_back() {
        // Real external finding (direction 5, not this project's own
        // corpus): `ton-prediction-market`'s `lmsr_market_maker.tolk` does
        // exactly this shape for its `state: cell` field — flagging it was
        // a false positive, not a real "lost write".
        let source = r#"
contract Wallet {
    author: "test"
    storage: WalletStorage
}

struct WalletStorage {
    owner: address
    state: cell
}

fun f() {
    var storage = lazy WalletStorage.load();
    var state = storage.state.load();
    state.fees = 0;
    storage.state = state.toCell();
    storage.save();
}
"#;
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_map_value_decoded_mutated_and_stored_back() {
        // Real external finding: `subdomains-tolk`'s `SubdomainFactory.tolk`
        // does this shape for `pending: dict<int, cell>` — the write-back is
        // a `.set(key, ...)` call, not a plain field assignment.
        let source = r#"
contract Wallet {
    author: "test"
    storage: WalletStorage
}

struct WalletStorage {
    owner: address
    pending: dict<int, cell>
}

fun f(key: int) {
    var storage = lazy WalletStorage.load();
    var pending = storage.pending.mustGet(key).load();
    pending.deploymentId += 1;
    storage.pending.set(key, pending.toCell());
    storage.save();
}
"#;
        assert!(run(source).is_empty());
    }

    #[test]
    fn still_flags_a_shadow_with_no_write_back_even_when_the_field_is_a_cell() {
        // The write-back exemption must not become a blanket exemption for
        // any `cell`-typed field — a `var state` that never gets written
        // back is still exactly the bug this detector exists to catch.
        let source = r#"
contract Wallet {
    author: "test"
    storage: WalletStorage
}

struct WalletStorage {
    owner: address
    state: cell
}

fun f() {
    var storage = lazy WalletStorage.load();
    var state = storage.state.load();
    state.fees = 0;
    storage.save();
}
"#;
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("state"));
    }

    #[test]
    fn kind_is_ast_and_severity_is_high() {
        assert_eq!(GvrDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(GvrDetector.default_severity(), sting_core::Severity::High);
    }
}
