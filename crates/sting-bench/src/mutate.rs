//! Mutation engine: injects exactly one known-class defect into a clean
//! [`Project`], per detector (`docs/domains/precision-sources.md`'s
//! algorithm table). This is the main source of vulnerable-corpus volume —
//! the hand-written golden fixtures are the source of
//! *diversity*, not volume (`docs/domains/precision-sources.md`).
//!
//! **Ground truth is derived, not hand-computed.** Each mutator builds a
//! *candidate* mutated source, then immediately runs the real
//! `DetectorRegistry` against it: if the target detector fires exactly one
//! *new* finding — one whose line does not match any finding the same
//! detector already produced on the **unmutated** project — that finding's
//! own `Location` *is* the ground truth kept; otherwise the candidate is
//! discarded, not recorded with a guessed line/column. Comparing against a
//! pre-mutation baseline (not just "exactly one finding total") matters in
//! practice: a merged project can already contain a real, known defect
//! (`corpus/clean/tolk-bench/01_jetton`'s own `JettonMinter.tolk`, see
//! `docs/domains/detectors.md`) — without the
//! baseline comparison, mutating *that* project's `onBouncedMessage`
//! handler would see 2 total `UBM` findings (the pre-existing real one plus
//! the injected one) and get wrongly discarded as "ambiguous", silently
//! losing an otherwise-good mutation instance. This sidesteps an entire
//! class of off-by-one bugs a hand-rolled "count newlines up to the
//! injection point" approach would risk, and doubles as a sanity check that
//! every kept mutation is actually detectable at generation time — `sting
//! bench`'s later, independent re-run of the same detectors is what
//! actually catches a regression (the same source, mutated once,
//! re-analyzed on every run).
//!
//! Every mutator is text-splice based (find a byte range via `tree-sitter`,
//! edit the string) — tree-sitter is a parser, not an editor; there is no
//! AST-to-text pretty-printer to round-trip through, and none is needed
//! for single, localized edits.

use sting_core::Finding;
use sting_detectors::DetectorRegistry;
use sting_pipeline::LintController;
use tree_sitter::Node;

use crate::corpus::{GroundTruthEntry, Project};

/// One generated vulnerable-corpus instance.
#[derive(Debug, Clone)]
pub struct MutatedInstance {
    /// Report-facing name, e.g. `"tolk-bench/01_jetton::gvr-onInternalMessage"`.
    pub name: String,
    /// The mutated source text.
    pub source: String,
    /// The single injected defect's ground truth, derived from the detector's
    /// own `Finding` at generation time (see module doc comment).
    pub ground_truth: GroundTruthEntry,
    /// Exact `Finding::message` text of every finding the project's
    /// **unmutated** baseline already produced (any detector, not just
    /// `ground_truth.defect_class`). A mutation only ever adds one new
    /// defect; the underlying project can still carry pre-existing real
    /// findings unrelated to it (e.g. `corpus/clean/tolk-bench/01_jetton`'s
    /// own known `UBM` finding, `docs/domains/detectors.md`'s
    /// that re-appear verbatim in the mutated source.
    /// Matching by exact message text (not line) sidesteps needing to track
    /// how much a mutation shifted every *other* line in the file — scoring
    /// (`crate::metrics`) treats a finding whose message is in this list as
    /// already accounted for, not a fresh false positive.
    pub pre_existing_messages: Vec<String>,
    /// The defect classes the unmutated project already produced **before**
    /// the injection. Needed for unit-level scoring, where the question is
    /// "did the tool report class C in this file": if C was here beforehand,
    /// the answer proves nothing, and that pair has to be excluded honestly
    /// rather than counted in anyone's favour.
    pub pre_existing_classes: Vec<String>,
    /// The mutated project as files again: the original layout with the one file the injection
    /// landed in replaced.
    ///
    /// The merged text is an internal trick — `lint_source` has no project context, so a
    /// single-file detector would otherwise never see a multi-file case. A participant with a real
    /// resolver reads a directory, and handed the splice instead it sees two definitions of every
    /// symbol and refuses the file. Measuring that would be measuring our own merging.
    pub files: Vec<(String, String)>,
}

/// Runs every mutator against `project`, returning every kept instance.
#[must_use]
pub fn mutate_all(project: &Project) -> Vec<MutatedInstance> {
    let baseline = baseline_findings(project);
    let mut all = mutate_gvr(project, &baseline);
    all.extend(mutate_lep(project, &baseline));
    all.extend(mutate_id(project, &baseline));
    all.extend(mutate_ubm(project, &baseline));
    all.extend(mutate_tlb(project, &baseline));
    all.extend(mutate_pl(project, &baseline));
    all.extend(mutate_br(project, &baseline));
    all.extend(mutate_ur(project, &baseline));
    all.extend(mutate_ac(project, &baseline));
    all.extend(mutate_uta(project, &baseline));
    all.extend(mutate_sua(project, &baseline));
    all.extend(mutate_eps(project, &baseline));
    all.extend(mutate_uda(project, &baseline));
    all
}

/// Every finding the full registry already produces on `project`'s
/// **unmutated** source — see the module doc comment for why each mutator
/// compares against this instead of requiring a bare "exactly one finding
/// total".
fn baseline_findings(project: &Project) -> Vec<Finding> {
    let registry = DetectorRegistry::builtin();
    let name = format!("{}::baseline.tolk", project.name).into();
    LintController::new(&registry)
        .lint_source(name, project.source.clone())
        .unwrap_or_default()
}

/// GVR: inject `var <storage-field> = 0;` as the first statement of a
/// non-`load`/`save` function's body — the storage field's declared type is
/// irrelevant (mutated sources are only ever parsed, never compiled, see
/// `docs/domains/detectors.md`'s own note that `sting` never invokes
/// the real Tolk compiler), so a universal placeholder value keeps this
/// simple.
#[must_use]
pub fn mutate_gvr(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);

    for contract in store.contracts() {
        let Some((_, storage_type)) = contract.fields.iter().find(|(k, _)| k == "storage") else {
            continue;
        };
        let Some(storage_struct) = store.struct_by_name(storage_type) else {
            continue;
        };
        let Some(field) = storage_struct.fields.first() else {
            continue;
        };

        for function in store.functions() {
            if function.receiver_type.as_deref() == Some(storage_type.as_str()) {
                continue;
            }
            let Some(func_node) = function.range.locate(&tree) else {
                continue;
            };
            let Some(body) = func_node.child_by_field_name("body") else {
                continue;
            };
            if body.kind() != "block_statement" {
                continue;
            }
            let insert_at = body.start_byte() + 1;
            let injected = format!("\n    var {} = 0;", field.name);
            let mutated_source = splice(&project.source, insert_at, insert_at, &injected);
            let name = format!("gvr-{}", function.name);
            if let Some(instance) = verify_and_wrap(Candidate {
                project,
                mutated_source,
                target_detector: "GVR",
                description: &name,
                baseline,
                anchor_byte: insert_at,
            }) {
                return vec![instance];
            }
        }
    }
    Vec::new()
}

/// LEP: delete an `.assertEnd()` call's whole statement.
#[must_use]
pub fn mutate_lep(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let mut instances = Vec::new();
    let mut index = 0usize;
    for visited in sting_ast::Preorder::new(&tree) {
        let node = visited.node;
        let Some((method, _, _)) = as_method_call(node, &project.source) else {
            continue;
        };
        if method != "assertEnd" {
            continue;
        }
        let Some(stmt) = enclosing_kind(node, &["expression_statement"]) else {
            continue;
        };
        let mutated_source = splice(&project.source, stmt.start_byte(), stmt.end_byte(), "");
        let name = format!("lep-{index}");
        if let Some(instance) = verify_and_wrap(Candidate {
            project,
            mutated_source,
            target_detector: "LEP",
            description: &name,
            baseline,
            anchor_byte: stmt.start_byte(),
        }) {
            instances.push(instance);
            index += 1;
        }
    }
    instances
}

/// ID: find a manual `beginCell()....storeXxx(a)....storeXxx(b)` chain (≥ 2
/// store calls) and swap the first two *same-method* adjacent calls'
/// argument lists — exactly `docs/domains/precision-sources.md`'s documented
/// algorithm — swap two adjacent fields **of the same type** on the write
/// side and leave the read order untouched: only the *write* side is
/// mutated.
///
/// Requiring the same method name (not just "any two calls", which an
/// earlier version of this mutator did) matters for real precision, not just
/// doc/code parity: swapping a `storeCoins(a)`/`storeAddress(b)` pair's
/// argument lists changes each field's *declared shape*, not just its
/// *position* — a real, additional `TLB`-class defect
/// (`sting_core::DefectClass::Tlb`), not the pure order-only `ID`-class
/// defect this mutator exists to generate. Found via `sting bench` itself,
/// once `TLB` existed to notice: several `corpus/clean/synthetic/*-manual`
/// fixtures' `ID` mutation instances were incidentally also flagged by
/// `TLB`, scored as spurious false positives against ground truth that only
/// asserted the `ID` defect (
/// discovery, not a pre-planned fix).
#[must_use]
pub fn mutate_id(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let mut instances = Vec::new();
    let mut index = 0usize;
    for visited in sting_ast::Preorder::new(&tree) {
        let node = visited.node;
        let Some(calls) = unwind_store_chain(node, &project.source) else {
            continue;
        };
        let Some((a_range, b_range)) = adjacent_same_method_pair(&calls) else {
            continue;
        };
        let mutated_source = swap_ranges(&project.source, a_range, b_range);
        let name = format!("id-{index}");
        if let Some(instance) = verify_and_wrap(Candidate {
            project,
            mutated_source,
            target_detector: "ID",
            description: &name,
            baseline,
            anchor_byte: a_range.0,
        }) {
            instances.push(instance);
            index += 1;
        }
    }
    instances
}

/// The first adjacent pair in `calls` whose method names match exactly —
/// `docs/domains/precision-sources.md`'s two-adjacent-fields-of-one-type
/// requirement, applied to whichever adjacent pair in the chain actually
/// satisfies it first, not just positions 0/1 (a chain whose first two calls
/// happen to differ in method should still yield a mutation if a *later*
/// adjacent pair matches, rather than being skipped entirely).
fn adjacent_same_method_pair(
    calls: &[(String, (usize, usize))],
) -> Option<((usize, usize), (usize, usize))> {
    calls
        .windows(2)
        .find(|pair| pair[0].0 == pair[1].0)
        .map(|pair| (pair[0].1, pair[1].1))
}

/// UBM: delete the whole `onBouncedMessage` handler, if present — the
/// simpler of `docs/domains/precision-sources.md`'s two described UBM
/// mutations (the other, injecting an unreverted state mutation before an
/// already-handler-less send, is exactly what a *positive* corpus file
/// already looks like — nothing to mutate there).
///
/// A merged multi-contract project legitimately declares more than one
/// `onBouncedMessage` — one per contract. Which of them guards the
/// save+risky-send pattern this mutation is meant to expose is not knowable
/// from the handler alone, so this **does not pick one**: it emits one
/// candidate per handler, deleting exactly that handler from the original
/// source, and lets [`verify_and_wrap`] decide. A candidate survives only if
/// deleting that specific handler makes `UBM` fire exactly once more than it
/// did on the clean project — which is precisely the definition of "this was
/// the handler that mattered". Candidates that delete an unrelated
/// contract's handler produce no new finding and are dropped.
///
/// That is the ordinary shape of mutation testing (generate every mutant,
/// keep the ones the analysis actually kills) and it is strictly better than
/// attribution here: no `contract{}`-proximity heuristic to keep in sync with
/// `sting_detectors::ubm::bounce_handler_for`, and a project whose two
/// contracts *both* have an unguarded-bounce shape contributes both instances
/// instead of one.
///
/// **History, because the number moved once already.** This used to delete
/// `AstStore::function_by_name`'s first match blindly. Review found that
/// ambiguous and made the mutator decline whenever a project had more than
/// one handler, on the stated grounds that "today's corpus has zero such
/// projects, so no measured number changes" (commit `e1f3107`). That was
/// wrong on the facts: `corpus/clean/` had **four** — `acton-contracts/
/// jetton-v1.3`, `acton-contracts/jetton-v2.1`, `tolk-bench/03_notcoin`,
/// `tolk-bench/04_sharded_tgbtc` — and `UBM` silently lost four true
/// positives, undetected for two weeks because `bench` is not part of the
/// default gate. Found by measuring the corpus, not by a failing test.
#[must_use]
pub fn mutate_ubm(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);
    let handler_ranges: Vec<(usize, usize)> = store
        .functions()
        .filter(|f| f.name == "onBouncedMessage")
        .map(|f| (f.range.start_byte, f.range.end_byte))
        .collect();

    handler_ranges
        .iter()
        .enumerate()
        .filter_map(|(index, &(start, end))| {
            // Spliced out of the *original* source every time, never
            // cumulatively: each candidate must differ from the clean project
            // by exactly one deleted handler, or `verify_and_wrap`'s
            // "exactly one new finding" check would be measuring two changes.
            let mutated_source = splice(&project.source, start, end, "");
            // The instance name has to distinguish candidates, otherwise two
            // surviving mutants of the same project collide in the ground
            // truth keyed by name.
            let description = if handler_ranges.len() == 1 {
                "ubm-handler-deleted".to_string()
            } else {
                format!("ubm-handler-deleted-{index}")
            };
            verify_and_wrap(Candidate {
                project,
                mutated_source,
                target_detector: "UBM",
                description: &description,
                baseline,
                anchor_byte: start,
            })
        })
        .collect()
}

/// TLB: find a `storeUint(value, WIDTH)`/`storeInt(value, WIDTH)` call whose
/// width argument is a plain integer literal, and change that literal to a
/// different one (`WIDTH + 1`) — the write side now disagrees with whatever
/// the read side still expects, exactly `sting_core::DefectClass::Tlb`'s
/// pattern (a width mismatch between a manual store and its matching load).
/// Only the *write* side is mutated, matching `mutate_id`'s own convention
/// (`docs/domains/precision-sources.md`: mutate the write side, leave the
/// read side as ground truth) — collects every kept instance across the
/// whole file, same as `mutate_lep`/`mutate_id`, not just the first (unlike
/// `mutate_gvr`, which only needs one storage field to make its point).
#[must_use]
pub fn mutate_tlb(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let mut instances = Vec::new();
    let mut index = 0usize;
    for visited in sting_ast::Preorder::new(&tree) {
        let node = visited.node;
        let Some((method, _, _)) = as_method_call(node, &project.source) else {
            continue;
        };
        if method != "storeUint" && method != "storeInt" {
            continue;
        }
        let Some(args_node) = node.child_by_field_name("arguments") else {
            continue;
        };
        let Some((width_start, width_end)) = nth_call_argument_range(args_node, 1) else {
            continue;
        };
        let Ok(width) = project.source[width_start..width_end].trim().parse::<u32>() else {
            continue;
        };
        let mutated_source = splice(
            &project.source,
            width_start,
            width_end,
            &(width + 1).to_string(),
        );
        let name = format!("tlb-{index}");
        if let Some(instance) = verify_and_wrap(Candidate {
            project,
            mutated_source,
            target_detector: "TLB",
            description: &name,
            baseline,
            anchor_byte: width_start,
        }) {
            instances.push(instance);
            index += 1;
        }
    }
    instances
}

/// PL: inject `var __pl_check = 100 / 7 * 3;` as the first statement of a
/// function body — the classic division-before-multiplication shape, built
/// entirely from integer literals (not a real corpus identifier, unlike
/// `GVR`'s injection) since real division-then-multiplication chains are
/// rare in this corpus (confirmed by grep during `PL`'s design — most real
/// arithmetic here is plain multiplication or a single division), so
/// injecting a self-contained synthetic instance is far more reliable than
/// searching for an existing site to transform. `100`/`7`/`3` are mutually
/// distinct on purpose: the divisor (`7`) must not equal the multiplier
/// (`3`), or this would accidentally hit `PlDetector`'s own
/// round-down-to-a-multiple exclusion instead of triggering it. Same "one
/// instance is enough, return after the first success" convention as
/// `mutate_gvr` — this is a synthetic injection independent of the
/// function's own real logic, not a transformation of a real, distinctive
/// existing site the way `mutate_lep`/`mutate_id`/`mutate_tlb` are.
#[must_use]
pub fn mutate_pl(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);

    for function in store.functions() {
        let Some(func_node) = function.range.locate(&tree) else {
            continue;
        };
        let Some(body) = func_node.child_by_field_name("body") else {
            continue;
        };
        if body.kind() != "block_statement" {
            continue;
        }
        let insert_at = body.start_byte() + 1;
        let injected = "\n    var __pl_check = 100 / 7 * 3;";
        let mutated_source = splice(&project.source, insert_at, insert_at, injected);
        let name = format!("pl-{}", function.name);
        if let Some(instance) = verify_and_wrap(Candidate {
            project,
            mutated_source,
            target_detector: "PL",
            description: &name,
            baseline,
            anchor_byte: insert_at,
        }) {
            return vec![instance];
        }
    }
    Vec::new()
}

/// BR: inject `var __br_check = blockchain.now() % 100;` as the first
/// statement of a function body — same self-contained-injection strategy as
/// `mutate_pl` and for the same underlying reason: `blockchain.now()` is
/// common in this corpus, but always for expiry/deadline comparisons, never
/// already modulo'd, so there is no reliable existing site to transform
/// (`mutate_id`/`mutate_tlb`'s strategy needs one; `mutate_gvr`/`mutate_pl`
/// don't, for the same reason this doesn't either).
#[must_use]
pub fn mutate_br(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);

    for function in store.functions() {
        let Some(func_node) = function.range.locate(&tree) else {
            continue;
        };
        let Some(body) = func_node.child_by_field_name("body") else {
            continue;
        };
        if body.kind() != "block_statement" {
            continue;
        }
        let insert_at = body.start_byte() + 1;
        let injected = "\n    var __br_check = blockchain.now() % 100;";
        let mutated_source = splice(&project.source, insert_at, insert_at, injected);
        let name = format!("br-{}", function.name);
        if let Some(instance) = verify_and_wrap(Candidate {
            project,
            mutated_source,
            target_detector: "BR",
            description: &name,
            baseline,
            anchor_byte: insert_at,
        }) {
            return vec![instance];
        }
    }
    Vec::new()
}

/// UR: injects a dictionary lookup and a read of its result **without** the
/// `.isFound` check, as the first statement of a function body.
///
/// The dictionary is declared right there, through the real stdlib API:
/// `map<K, V>.get(key)` returns `MapLookupResult<V>` carrying `.isFound` and
/// `.loadValue()` (`vendor/tolk/tolk-stdlib/common.tolk:852`). The empty map is
/// written `[]` rather than `createEmptyMap()`, which the stdlib marks
/// `@deprecated` and which would drag an extra `DEPR` finding along.
///
/// **The earlier version** wrote a bare `var __ur_value =
/// __urLookup.loadValue();` without declaring `__urLookup` anywhere, resting on
/// the assumption that mutated sources are only ever parsed and never compiled.
/// That holds for our own detectors and fails for any analyser with a type
/// system: all 43 instances failed to compile, and against `acton` they were
/// not code at all.
#[must_use]
pub fn mutate_ur(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);

    for function in store.functions() {
        let Some(func_node) = function.range.locate(&tree) else {
            continue;
        };
        let Some(body) = func_node.child_by_field_name("body") else {
            continue;
        };
        if body.kind() != "block_statement" {
            continue;
        }
        let insert_at = body.start_byte() + 1;
        let injected = "\n    var __urDict: map<int8, int32> = [];\n    val __urLookup = __urDict.get(0);\n    val __ur_value = __urLookup.loadValue();";
        let mutated_source = splice(&project.source, insert_at, insert_at, injected);
        let name = format!("ur-{}", function.name);
        if let Some(instance) = verify_and_wrap(Candidate {
            project,
            mutated_source,
            target_detector: "UR",
            description: &name,
            baseline,
            anchor_byte: insert_at,
        }) {
            return vec![instance];
        }
    }
    Vec::new()
}

/// AC: inject `__acStorage.save();` as the first statement of
/// `onInternalMessage`'s body — same self-contained-injection strategy as
/// `mutate_pl`/`mutate_br`/`mutate_ur`: `AcDetector` only asks whether a
/// recognized check preceded this specific `.save()`-shaped call, which an
/// entirely fresh, never-before-declared receiver trivially never had (same
/// trick `mutate_ur` uses for `.loadValue()` — mutated sources are only
/// ever parsed, never compiled). Unlike `mutate_pl`/`mutate_br`/`mutate_ur`,
/// which inject into the *first* function found, this specifically targets
/// `onInternalMessage` by name — `AcDetector` only ever analyzes that one
/// function (`ac.rs`'s own doc comment's stated v1 scope), so injecting into
/// an arbitrary function would never actually fire.
#[must_use]
pub fn mutate_ac(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);
    let Some(function) = store.function_by_name("onInternalMessage") else {
        return Vec::new();
    };
    let Some(func_node) = function.range.locate(&tree) else {
        return Vec::new();
    };
    let Some(body) = func_node.child_by_field_name("body") else {
        return Vec::new();
    };
    if body.kind() != "block_statement" {
        return Vec::new();
    }
    // The storage object is declared right here, with the contract's real
    // type. The earlier version injected a bare `__acStorage.save();` without
    // declaring `__acStorage` anywhere: to tree-sitter that is a valid method
    // call, to the compiler it is `unknown type name`, and all 42 instances of
    // this mutator failed to compile. The type comes from the `contract {}`
    // header the same way [`mutate_gvr`] already takes it; with no header the
    // mutation is not built at all — fewer instances beat instances that are
    // not code.
    let storage_type = store
        .contracts()
        .find_map(|c| {
            c.fields
                .iter()
                .find(|(k, _)| k == "storage")
                .map(|(_, ty)| ty.clone())
        })
        .filter(|ty| store.struct_by_name(ty).is_some());
    let Some(storage_type) = storage_type else {
        return Vec::new();
    };

    let insert_at = body.start_byte() + 1;
    let injected =
        format!("\n    var __acStorage = lazy {storage_type}.load();\n    __acStorage.save();");
    let mutated_source = splice(&project.source, insert_at, insert_at, &injected);
    verify_and_wrap(Candidate {
        project,
        mutated_source,
        target_detector: "AC",
        description: "ac-onInternalMessage",
        baseline,
        anchor_byte: insert_at,
    })
    .into_iter()
    .collect()
}

/// UTA: inject a self-contained, same-function instance of the asymmetric-
/// check shape (`uta.rs`'s own doc comment) as the first statements of
/// `onInternalMessage`'s body — a signed read, a debit/credit pair sharing
/// that value, and a check on only the debited side. Injection, not
/// transformation, matching `mutate_pl`/`mutate_br`/`mutate_ur`'s own
/// precedent: this multi-statement shape does not naturally occur in the
/// existing corpus (confirmed real only on `hacktheton.com`, an external
/// site —), so there is nothing to
/// transform, only something self-contained to add.
#[must_use]
pub fn mutate_uta(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);
    let Some(function) = store.function_by_name("onInternalMessage") else {
        return Vec::new();
    };
    let Some(func_node) = function.range.locate(&tree) else {
        return Vec::new();
    };
    let Some(body) = func_node.child_by_field_name("body") else {
        return Vec::new();
    };
    if body.kind() != "block_statement" {
        return Vec::new();
    }
    let insert_at = body.start_byte() + 1;
    // The slice is declared right here. The earlier version referred to
    // `__utaBody`, which nobody declared: indistinguishable from valid code to
    // tree-sitter, `unknown type name` to the compiler, and **all** 42
    // instances of this mutator failed to compile.
    let injected = "\n    var __utaBody: slice = beginCell().endCell().beginParse();\n    val __utaAmount: int = __utaBody.loadInt(256);\n    var __utaFromBalance: int = 100;\n    var __utaToBalance: int = 0;\n    __utaFromBalance -= __utaAmount;\n    __utaToBalance += __utaAmount;\n    assert(__utaFromBalance > 0, 900);";
    let mutated_source = splice(&project.source, insert_at, insert_at, injected);
    verify_and_wrap(Candidate {
        project,
        mutated_source,
        target_detector: "UTA",
        description: "uta-onInternalMessage",
        baseline,
        anchor_byte: insert_at,
    })
    .into_iter()
    .collect()
}

/// SUA: inject a self-contained `x =+ y;` typo (Misti prior-art teardown, `sua.rs`) as the first statements
/// of `onInternalMessage`'s body — plain local `int` arithmetic with no
/// `.save()`/slice-loading/store-load pairing/modulo, deliberately inert to
/// every other detector, same reasoning as `mutate_uta`'s own fictional
/// `__uta*`-prefixed names.
pub fn mutate_sua(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);
    let Some(function) = store.function_by_name("onInternalMessage") else {
        return Vec::new();
    };
    let Some(func_node) = function.range.locate(&tree) else {
        return Vec::new();
    };
    let Some(body) = func_node.child_by_field_name("body") else {
        return Vec::new();
    };
    if body.kind() != "block_statement" {
        return Vec::new();
    }
    let insert_at = body.start_byte() + 1;
    let injected = "\n    var __suaBalance: int = 100;\n    var __suaDelta: int = 1;\n    __suaBalance =+ __suaDelta;";
    let mutated_source = splice(&project.source, insert_at, insert_at, injected);
    verify_and_wrap(Candidate {
        project,
        mutated_source,
        target_detector: "SUA",
        description: "sua-onInternalMessage",
        baseline,
        anchor_byte: insert_at,
    })
    .into_iter()
    .collect()
}

/// EPS: inject a self-contained `random.uint256()` call with no preceding
/// seed call (
/// `eps.rs`) as the first statement of `onInternalMessage`'s body — same
/// reasoning as `mutate_uta`/`mutate_sua`'s own injections: a real stdlib
/// call, inert to every other detector (no `.save()`, no slice loading, no
/// store-load pairing, no modulo).
pub fn mutate_eps(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);
    let Some(function) = store.function_by_name("onInternalMessage") else {
        return Vec::new();
    };
    let Some(func_node) = function.range.locate(&tree) else {
        return Vec::new();
    };
    let Some(body) = func_node.child_by_field_name("body") else {
        return Vec::new();
    };
    if body.kind() != "block_statement" {
        return Vec::new();
    }
    let insert_at = body.start_byte() + 1;
    let injected = "\n    val __epsValue: int = random.uint256();";
    let mutated_source = splice(&project.source, insert_at, insert_at, injected);
    verify_and_wrap(Candidate {
        project,
        mutated_source,
        target_detector: "EPS",
        description: "eps-onInternalMessage",
        baseline,
        anchor_byte: insert_at,
    })
    .into_iter()
    .collect()
}

/// UDA: inject a self-contained "unchecked destination address" relay
/// (`uda.rs`) as the
/// first statements of `onInternalMessage`'s body — an address read off
/// `inMsgBody` flowing straight into a `sendRawMessage(...)` sink with no
/// assert in between, on synthetic `__uda*`-prefixed names (same principle
/// as `mutate_uta`/`mutate_sua`'s own injections: parsing does not require
/// `inMsgBody` to be a real declared parameter of the target function, only
/// that the injected text has the right shape — `uda.rs`'s own source
/// recognition is purely textual, not type-checked, same as every other
/// detector's own heuristics in this crate).
pub fn mutate_uda(project: &Project, baseline: &[Finding]) -> Vec<MutatedInstance> {
    let Ok(tree) = sting_ast::parse(&project.source) else {
        return Vec::new();
    };
    let store = sting_ast::AstStore::build(&tree, &project.source);
    let Some(function) = store.function_by_name("onInternalMessage") else {
        return Vec::new();
    };
    let Some(func_node) = function.range.locate(&tree) else {
        return Vec::new();
    };
    let Some(body) = func_node.child_by_field_name("body") else {
        return Vec::new();
    };
    if body.kind() != "block_statement" {
        return Vec::new();
    }
    // The untrusted source is taken from **the function's own signature**
    // rather than substituted blindly. `uda.rs` recognizes exactly two forms:
    // a parameter named `inMsgBody` (the older, FunC-shaped signature) and a
    // `.body` field access (the modern `in: InMessage`). The earlier version
    // always wrote `inMsgBody`; no such name exists in a Tolk 1.x contract, the
    // file did not compile, and all 42 instances were not code.
    //
    // Declaring a slice of our own, the way [`mutate_uta`] does, is **not**
    // allowed here: it fixes compilation and destroys the meaning — a locally
    // built empty slice is not untrusted, the detector stays silent, and zero
    // mutations survive. Checked rather than assumed.
    //
    // The form depends on the signature because the type does: on the old
    // `inMsgBody: slice`, `loadAddress()` yields a `slice`; on the modern
    // `in.body` it yields an `address`. The earlier version declared
    // `val __udaAddr: slice` in both cases, so it was wrong by type even where
    // the name existed.
    let Some((body_expr, addr_ty, store_call)) = function.parameters.iter().find_map(|p| {
        if p.name == "inMsgBody" {
            return Some((p.name.clone(), "slice", "storeSlice"));
        }
        (p.type_text.as_deref() == Some("InMessage"))
            .then(|| (format!("{}.body", p.name), "address", "storeAddress"))
    }) else {
        return Vec::new();
    };

    let insert_at = body.start_byte() + 1;
    // `.loadAddress()` is called **directly** on the message body rather than
    // on a copy: `uda.rs` treats a receiver spelled `inMsgBody` or `<...>.body`
    // as the untrusted source. An intermediate variable compiles and breaks the
    // taint, and the detector goes silent — also checked.
    let injected = format!(
        "\n    val __udaAddr: {addr_ty} = {body_expr}.loadAddress();\n    val __udaMsg: cell = beginCell().storeUint(0x18, 6).{store_call}(__udaAddr).storeCoins(0).endCell();\n    sendRawMessage(__udaMsg, 64);"
    );
    let mutated_source = splice(&project.source, insert_at, insert_at, &injected);
    verify_and_wrap(Candidate {
        project,
        mutated_source,
        target_detector: "UDA",
        description: "uda-onInternalMessage",
        baseline,
        anchor_byte: insert_at,
    })
    .into_iter()
    .collect()
}

/// Builds a candidate's context, runs the full registry, and keeps the
/// instance only if `target_detector` fired exactly one *new* finding
/// relative to `baseline` — see the module doc comment for why this is
/// compared against a baseline rather than requiring "exactly one finding
/// total".
/// Everything needed to check one mutation candidate. A struct because there
/// are seven parameters: binding them positionally at thirteen call sites is a
/// reliable way to eventually swap two `&str` for each other.
struct Candidate<'a> {
    /// The project the injection was made in: its name for the report, its text for the compile
    /// check, and its file layout for what is published.
    project: &'a Project,
    /// The text after the injection.
    mutated_source: String,
    /// The class that must appear exactly once.
    target_detector: &'a str,
    /// The instance name's suffix.
    description: &'a str,
    /// The findings on the unmutated project.
    baseline: &'a [Finding],
    /// The byte offset of the injection, which is how the right occurrence is
    /// picked.
    anchor_byte: usize,
}

fn verify_and_wrap(candidate: Candidate) -> Option<MutatedInstance> {
    let Candidate {
        project,
        mutated_source,
        target_detector,
        description,
        baseline,
        anchor_byte,
    } = candidate;
    let project_name = project.name.as_str();
    let original_source = project.source.as_str();
    // The compile check runs first: it costs less than a full pass of the
    // detector registry and rejects candidates that are not code.
    if !mutation_keeps_compiling(original_source, &mutated_source) {
        return None;
    }
    let registry = DetectorRegistry::builtin();
    let name = format!("{project_name}::{description}.tolk").into();
    let findings = LintController::new(&registry)
        .lint_source(name, mutated_source.clone())
        .ok()?;

    // Counted, not membership-tested. A merged project can hold two or three
    // entrypoints — 11 of the corpus's do — and two same-named entrypoints
    // produce byte-identical `Finding::message` text. Against a plain
    // `HashSet` a genuinely new second occurrence of an already-seen message
    // read as "seen before" (0 new → declined), and two new occurrences of an
    // unseen message both survived (2 new → declined). Comparing counts is
    // what makes "exactly one new finding" mean what it says.
    //
    // Message text stays the comparison key rather than the line: a mutation
    // shifts every subsequent line by an amount that varies per mutator
    // (GVR: +1, LEP: -1 statement, UBM: -1 whole function), and replicating
    // that arithmetic here per mutator would be its own source of off-by-one
    // bugs. `Finding::message` is fully deterministic.
    let mut baseline_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for finding in baseline {
        *baseline_counts.entry(finding.message.as_str()).or_default() += 1;
    }
    let mut mutated_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for finding in findings.iter().filter(|f| f.detector_id == target_detector) {
        *mutated_counts.entry(finding.message.as_str()).or_default() += 1;
    }
    let net_new: usize = mutated_counts
        .iter()
        .map(|(message, count)| {
            count.saturating_sub(baseline_counts.get(message).copied().unwrap_or(0))
        })
        .sum();
    if net_new != 1 {
        return None;
    }

    // Which *occurrence* to record as ground truth cannot come from the
    // message — the ambiguous case is precisely two findings whose messages
    // are equal and whose lines are not. It comes from `anchor_byte`, the
    // offset the mutator spliced at: the injected defect is the one near it.
    //
    // Recording the wrong occurrence is not a cosmetic error. An earlier
    // attempt at this fix took "whichever occurrence gained a count" and
    // produced 13 mutations whose ground-truth line pointed at an unrelated
    // entrypoint, which scoring then dutifully counted as 13 false negatives
    // — recall fell from 100% to 72.3% while the detectors were unchanged.
    let anchor_line =
        u32::try_from(mutated_source[..anchor_byte].matches('\n').count() + 1).unwrap_or(u32::MAX);
    let finding = findings
        .iter()
        .filter(|f| f.detector_id == target_detector)
        .filter(|f| {
            mutated_counts.get(f.message.as_str()).copied().unwrap_or(0)
                > baseline_counts
                    .get(f.message.as_str())
                    .copied()
                    .unwrap_or(0)
        })
        .min_by_key(|f| f.location.start_line.abs_diff(anchor_line))?;
    Some(MutatedInstance {
        files: mutated_files(project, &mutated_source, anchor_byte),
        name: format!("{project_name}::{description}"),
        source: mutated_source,
        ground_truth: GroundTruthEntry {
            defect_class: target_detector.to_string(),
            line: finding.location.start_line,
            column: finding.location.start_column,
        },
        pre_existing_messages: baseline.iter().map(|f| f.message.clone()).collect(),
        pre_existing_classes: baseline.iter().map(|f| f.detector_id.to_string()).collect(),
    })
}

/// The mutated project as files again: the original layout with the one file the injection landed
/// in replaced by its mutated text.
///
/// The merge is a concatenation, so each file owns a contiguous run of lines and the injection falls
/// inside exactly one of them. Everything before that file is untouched, the file itself grows or
/// shrinks by whatever the mutation did, and everything after it keeps its own text — which is why
/// the split is by line rather than by searching for the original content in the new text.
///
/// A project of one file needs no split, and a project whose starts were never recorded gets the
/// merged text under one name rather than nothing: a caller asking for files should not be handed an
/// empty list because the layout is unknown.
fn mutated_files(project: &Project, mutated: &str, anchor_byte: usize) -> Vec<(String, String)> {
    if project.file_starts.len() <= 1 {
        let name = project
            .file_starts
            .first()
            .map_or_else(|| "unit.tolk".to_string(), |(name, _)| name.clone());
        return vec![(name, mutated.to_string())];
    }

    let original_lines: Vec<&str> = project.source.split_inclusive('\n').collect();
    let mutated_lines: Vec<&str> = mutated.split_inclusive('\n').collect();
    let injected_line = project.source[..anchor_byte.min(project.source.len())]
        .matches('\n')
        .count()
        + 1;
    // What the mutation did to the line count, which every file after the injection is shifted by.
    let delta = mutated_lines.len().cast_signed() - original_lines.len().cast_signed();

    let mut files = Vec::with_capacity(project.file_starts.len());
    for (index, (name, start)) in project.file_starts.iter().enumerate() {
        let start = *start as usize;
        let end = project
            .file_starts
            .get(index + 1)
            .map_or(original_lines.len() + 1, |(_, next)| *next as usize);
        let injected_here = injected_line >= start && injected_line < end;
        let shift = |line: usize| -> usize {
            let shifted = line.cast_signed() + delta;
            usize::try_from(shifted.max(1)).unwrap_or(1)
        };
        let (from, to) = if injected_here {
            (start, shift(end))
        } else if start > injected_line {
            (shift(start), shift(end))
        } else {
            (start, end)
        };
        let body: String = mutated_lines
            .get(from.saturating_sub(1)..to.saturating_sub(1).min(mutated_lines.len()))
            .unwrap_or_default()
            .concat();
        files.push((name.clone(), body));
    }
    files
}

/// Whether `source` compiles with the vendored `tolk`.
///
/// `None` means the compiler is unavailable, and the check then skips silently:
/// a missing toolchain must not turn every mutation into a rejected one.
fn compiles(source: &str) -> Option<bool> {
    let compiler = sting_tolkc::TolkCompiler::vendored_or_default()?;
    let dir = tempfile::tempdir().ok()?;
    let entry = dir.path().join("unit.tolk");
    std::fs::write(&entry, crate::strip_sibling_imports(source)).ok()?;
    Some(compiler.compile(dir.path(), &entry).is_ok())
}

/// Whether the mutation stopped the source compiling.
///
/// The claim is deliberately "it compiled **before**, so it must compile
/// **after**" rather than "the mutated text compiles". The merged text does not
/// always compile on its own — it is glued together from a project's files —
/// and demanding more of a mutation than the source itself gives would reject
/// good candidates for somebody else's defect.
///
/// Why this is needed: our detectors stand on tree-sitter and check no types at
/// all, so a mutator referring to an undeclared identifier is indistinguishable
/// from a correct one to us. To an analyser with a type system that file is not
/// code. Measured against `acton 1.1.0`: four mutators (`ur`, `ac`, `uda`,
/// `uta`) produced **100%** uncompilable instances, every one of them with
/// `unknown type name`.
fn mutation_keeps_compiling(original: &str, mutated: &str) -> bool {
    match compiles(original) {
        Some(true) => compiles(mutated).unwrap_or(true),
        // The source did not compile, or the compiler is unavailable: there
        // is nothing to say, and the candidate passes.
        _ => true,
    }
}

fn splice(source: &str, start: usize, end: usize, replacement: &str) -> String {
    format!("{}{}{}", &source[..start], replacement, &source[end..])
}

fn swap_ranges(source: &str, a: (usize, usize), b: (usize, usize)) -> String {
    let (a_start, a_end) = a;
    let (b_start, b_end) = b;
    format!(
        "{}{}{}{}{}",
        &source[..a_start],
        &source[b_start..b_end],
        &source[a_end..b_start],
        &source[a_start..a_end],
        &source[b_end..]
    )
}

/// If `node` is `<obj>.<method>(<args>)`, its method name, the `obj` node,
/// and the `arguments` node's byte range.
fn as_method_call<'tree>(
    node: Node<'tree>,
    source: &str,
) -> Option<(String, Node<'tree>, (usize, usize))> {
    if node.kind() != "function_call" {
        return None;
    }
    let callee = node.child_by_field_name("callee")?;
    if callee.kind() != "dot_access" {
        return None;
    }
    let obj = callee.child_by_field_name("obj")?;
    let field = callee.child_by_field_name("field")?;
    let method = field.utf8_text(source.as_bytes()).ok()?.to_string();
    let args = node.child_by_field_name("arguments")?;
    Some((method, obj, (args.start_byte(), args.end_byte())))
}

/// The `index`-th (0-based) `call_argument`'s byte range inside an
/// `argument_list` node — mirrors `sting-detectors`'s own private
/// `nth_call_argument_text` (a different crate, so not reusable directly),
/// needed here to splice just the width literal, not the whole argument
/// list `as_method_call`'s own range already covers.
fn nth_call_argument_range(args: Node, index: usize) -> Option<(usize, usize)> {
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() == "call_argument")
        .nth(index)
        .and_then(|ca| ca.child_by_field_name("expr"))
        .map(|e| (e.start_byte(), e.end_byte()))
}

fn enclosing_kind<'tree>(node: Node<'tree>, kinds: &[&str]) -> Option<Node<'tree>> {
    let mut current = node.parent()?;
    loop {
        if kinds.contains(&current.kind()) {
            return Some(current);
        }
        current = current.parent()?;
    }
}

/// Whether `node` (a `function_call`) is the outermost call of its chain —
/// i.e. not itself embedded as the `obj` of a further `.method()` call —
/// so a chain is only ever unwound once, from its root, not once per
/// sub-chain `Preorder` also visits.
fn is_chain_root(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return true;
    };
    if parent.kind() != "dot_access" {
        return true;
    }
    let Some(obj) = parent.child_by_field_name("obj") else {
        return true;
    };
    if obj.id() != node.id() {
        return true;
    }
    parent
        .parent()
        .is_none_or(|gp| gp.kind() != "function_call")
}

/// Unwinds a chain rooted at `node` into its calls (execution order), only
/// if every call in it is a `storeXxx` call and there are at least 2.
fn unwind_store_chain(node: Node, source: &str) -> Option<Vec<(String, (usize, usize))>> {
    if !is_chain_root(node) {
        return None;
    }
    let mut calls = Vec::new();
    let mut current = node;
    while let Some((method, obj, args_range)) = as_method_call(current, source) {
        calls.push((method, args_range));
        current = obj;
    }
    calls.reverse();
    if calls.len() >= 2 && calls.iter().all(|(m, _)| m.starts_with("store")) {
        Some(calls)
    } else {
        None
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{
        mutate_ac, mutate_br, mutate_gvr, mutate_id, mutate_lep, mutate_pl, mutate_tlb, mutate_ubm,
        mutate_ur, mutated_files,
    };
    use crate::corpus::Project;
    use sting_ast::Language;

    fn project(source: &str) -> Project {
        Project {
            language: Language::Tolk,
            name: "test-project".to_string(),
            source: source.to_string(),
            ground_truth: Vec::new(),
            file_starts: Vec::new(),
            files: Vec::new(),
        }
    }

    #[test]
    fn gvr_mutator_injects_a_detectable_shadowing_var() {
        let source = r"
contract Wallet {
    author: test
    storage: WalletStorage
}

struct WalletStorage {
    owner: address
    balance: coins
}

fun onInternalMessage(in: InMessage) {
    assert (in.body.isEmpty()) throw 100;
}
";
        let instances = mutate_gvr(&project(source), &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].ground_truth.defect_class, "GVR");
        assert!(instances[0].source.contains("var owner = 0;"));
        // `contract { ... }` is always an ERROR node — a documented gap — so
        // a plain "no error nodes" check would always fail
        // here regardless of the mutation — instead independently
        // re-confirm GVR still fires on the generated source, redundant
        // with `verify_and_wrap`'s own internal check but via a fresh call.
        let tree = sting_ast::parse(&instances[0].source).expect("mutated source parses");
        let ast_store = sting_ast::AstStore::build(&tree, &instances[0].source);
        let ctx = sting_detectors::DetectorContext::new(
            "recheck.tolk".into(),
            tree,
            instances[0].source.clone(),
        );
        let (findings, without_a_rule) =
            sting_detectors::DetectorRegistry::builtin().run_all(&ast_store, &ctx);
        assert!(findings.iter().any(|f| f.detector_id == "GVR"));
        assert!(
            without_a_rule.is_empty(),
            "every detector reads Tolk, so none is held back here"
        );
    }

    #[test]
    fn gvr_mutator_yields_nothing_without_a_colocated_contract_and_storage() {
        let source = "fun f() {\n    return;\n}\n";
        assert!(mutate_gvr(&project(source), &[]).is_empty());
    }

    #[test]
    fn lep_mutator_removes_an_assert_end_call() {
        let source = "fun parse(s: slice) {\n    val x = s.loadUint(32);\n    s.assertEnd();\n}\n";
        let instances = mutate_lep(&project(source), &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].ground_truth.defect_class, "LEP");
        assert!(!instances[0].source.contains("assertEnd"));
    }

    #[test]
    fn lep_mutator_yields_nothing_without_an_assert_end_call() {
        let source = "fun f(s: slice) {\n    return;\n}\n";
        assert!(mutate_lep(&project(source), &[]).is_empty());
    }

    #[test]
    fn id_mutator_swaps_a_manual_store_chain() {
        let save = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 64).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        let source = format!("{save}\n{load}");
        let instances = mutate_id(&project(&source), &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].ground_truth.defect_class, "ID");
    }

    #[test]
    fn id_mutator_yields_nothing_without_a_manual_store_chain() {
        let source = "fun MinterStorage.load() {\n    return MinterStorage.fromCell(contract.getData());\n}\nfun MinterStorage.save(self) {\n    contract.setData(self.toCell());\n}\n";
        assert!(mutate_id(&project(source), &[]).is_empty());
    }

    #[test]
    fn ubm_mutator_deletes_the_bounce_handler() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n\nfun onBouncedMessage(in: InMessageBounced) {\n    var storage = lazy S.load();\n    storage.balance += 1;\n    storage.save();\n}\n";
        let instances = mutate_ubm(&project(source), &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].ground_truth.defect_class, "UBM");
        assert!(!instances[0].source.contains("onBouncedMessage"));
    }

    #[test]
    fn ubm_mutator_yields_nothing_without_a_handler_to_delete() {
        let source = "fun onInternalMessage(in: InMessage) {\n    return;\n}\n";
        assert!(mutate_ubm(&project(source), &[]).is_empty());
    }

    /// A merged project with two contracts' handlers must not be declined
    /// (that cost `UBM` four real true positives, see `mutate_ubm`'s own doc
    /// comment) and must not be mutated blindly either: the mutator emits a
    /// candidate per handler and keeps the one whose deletion actually makes
    /// `UBM` fire. Here only contract `A` has the save+risky-send shape, so
    /// exactly one candidate survives — deleting `B`'s handler changes
    /// nothing and is dropped by verification, not by a heuristic.
    #[test]
    fn ubm_mutator_keeps_only_the_handler_whose_deletion_actually_fires() {
        let contract_a = "contract A {\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\nfun onBouncedMessage(in: InMessageBounced) {\n    var storage = lazy S.load();\n    storage.balance += 1;\n    storage.save();\n}\n";
        let contract_b = "contract B {\n}\nfun onBouncedMessage(in: InMessageBounced) {\n    var storage = lazy T.load();\n    storage.balance += 1;\n    storage.save();\n}\n";
        let source = format!("{contract_a}\n{contract_b}");
        let instances = mutate_ubm(&project(&source), &[]);
        assert_eq!(
            instances.len(),
            1,
            "exactly the handler that guards the flagged pattern must survive verification"
        );
        assert_eq!(instances[0].ground_truth.defect_class, "UBM");
        assert!(
            instances[0].name.contains("ubm-handler-deleted-"),
            "multi-handler candidates must carry an index so two survivors cannot collide: {}",
            instances[0].name
        );
    }

    /// The single-handler case keeps its historical, unindexed name — the
    /// ground truth of every existing corpus project is keyed by it.
    #[test]
    fn ubm_mutator_keeps_the_unindexed_name_when_there_is_only_one_handler() {
        let source = "contract A {\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\nfun onBouncedMessage(in: InMessageBounced) {\n    var storage = lazy S.load();\n    storage.balance += 1;\n    storage.save();\n}\n";
        let instances = mutate_ubm(&project(source), &[]);
        assert_eq!(instances.len(), 1);
        assert!(
            instances[0].name.ends_with("::ubm-handler-deleted"),
            "single-handler name must stay stable: {}",
            instances[0].name
        );
    }

    #[test]
    fn tlb_mutator_widens_every_eligible_storeuint_call() {
        let save = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 64).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let load = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        let source = format!("{save}\n{load}");
        // Both `storeUint` calls are independently eligible (each widening
        // creates its own clean, single-field TLB mismatch against the
        // unmutated read side), so both are kept — not just the first.
        let instances = mutate_tlb(&project(&source), &[]);
        assert_eq!(instances.len(), 2);
        assert!(
            instances
                .iter()
                .all(|i| i.ground_truth.defect_class == "TLB")
        );
        assert!(
            instances
                .iter()
                .any(|i| i.source.contains("storeUint(balance, 65)"))
        );
        assert!(
            instances
                .iter()
                .any(|i| i.source.contains("storeUint(owner, 257)"))
        );
    }

    #[test]
    fn tlb_mutator_yields_nothing_without_a_storeuint_or_storeint_call() {
        let source = "fun MinterStorage.load() {\n    return MinterStorage.fromCell(contract.getData());\n}\nfun MinterStorage.save(self) {\n    contract.setData(self.toCell());\n}\n";
        assert!(mutate_tlb(&project(source), &[]).is_empty());
    }

    #[test]
    fn pl_mutator_injects_a_division_before_multiplication() {
        let source = "fun f() {\n    return;\n}\n";
        let instances = mutate_pl(&project(source), &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].ground_truth.defect_class, "PL");
        assert!(instances[0].source.contains("100 / 7 * 3"));
    }

    #[test]
    fn pl_mutator_yields_nothing_without_any_function() {
        let source = "struct Empty {\n    x: int\n}\n";
        assert!(mutate_pl(&project(source), &[]).is_empty());
    }

    #[test]
    fn br_mutator_injects_a_modulo_seeded_from_now() {
        let source = "fun f() {\n    return;\n}\n";
        let instances = mutate_br(&project(source), &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].ground_truth.defect_class, "BR");
        assert!(instances[0].source.contains("blockchain.now() % 100"));
    }

    #[test]
    fn br_mutator_yields_nothing_without_any_function() {
        let source = "struct Empty {\n    x: int\n}\n";
        assert!(mutate_br(&project(source), &[]).is_empty());
    }

    #[test]
    fn ur_mutator_injects_an_unguarded_load_value_call() {
        let source = "fun f() {\n    return;\n}\n";
        let instances = mutate_ur(&project(source), &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].ground_truth.defect_class, "UR");
        assert!(instances[0].source.contains("__urLookup.loadValue()"));
    }

    #[test]
    fn ur_mutator_yields_nothing_without_any_function() {
        let source = "struct Empty {\n    x: int\n}\n";
        assert!(mutate_ur(&project(source), &[]).is_empty());
    }

    #[test]
    fn ac_mutator_injects_an_unguarded_save_call() {
        // The `contract {}` header and the storage struct are required: the
        // mutator takes the real type from there so the injected code declares
        // everything it uses and compiles. The earlier version made do with a
        // bare `__acStorage.save();` and no declaration.
        let source = "contract Wallet {\n    author: test\n    storage: WalletStorage\n}\nstruct WalletStorage {\n    balance: coins\n}\nfun onInternalMessage(in: InMessage) {\n    return;\n}\n";
        let instances = mutate_ac(&project(source), &[]);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].ground_truth.defect_class, "AC");
        assert!(
            instances[0]
                .source
                .contains("var __acStorage = lazy WalletStorage.load();"),
            "the storage object must be declared with the contract's real type"
        );
        assert!(instances[0].source.contains("__acStorage.save();"));
    }

    #[test]
    fn ac_mutator_yields_nothing_without_a_storage_type_to_declare() {
        // No instance at all beats an instance that is not code.
        let source = "fun onInternalMessage(in: InMessage) {\n    return;\n}\n";
        assert!(mutate_ac(&project(source), &[]).is_empty());
    }

    #[test]
    fn ac_mutator_yields_nothing_without_an_on_internal_message() {
        let source = "fun f() {\n    return;\n}\n";
        assert!(mutate_ac(&project(source), &[]).is_empty());
    }

    /// A two-file project, mutated in the second file: what is written back out has to be the same
    /// text again, or a participant is handed a project that is not the one that was measured.
    #[test]
    fn a_mutated_project_splits_back_into_the_files_it_came_from() {
        let first = "fun helper(): int {
    return 1;
}
";
        let second = "fun onInternalMessage(in: InMessage) {
    val x = helper();
}
";
        let project = Project {
            language: Language::Tolk,
            name: "two-files".to_string(),
            source: format!("{first}{second}"),
            ground_truth: Vec::new(),
            file_starts: vec![("types.tolk".to_string(), 1), ("main.tolk".to_string(), 4)],
            files: vec![
                ("types.tolk".to_string(), first.to_string()),
                ("main.tolk".to_string(), second.to_string()),
            ],
        };
        // An injection in the second file that adds a line.
        let mutated = format!(
            "{first}fun onInternalMessage(in: InMessage) {{
    var y = 0;
    val x = helper();
}}
"
        );
        let anchor = project
            .source
            .find("val x")
            .expect("the anchor is in the second file");

        let files = mutated_files(&project, &mutated, anchor);

        assert_eq!(files.len(), 2);
        assert_eq!(files[0], ("types.tolk".to_string(), first.to_string()));
        assert!(files[1].1.contains("var y = 0;"), "{:?}", files[1].1);
        assert_eq!(
            files
                .iter()
                .map(|(_, body)| body.as_str())
                .collect::<String>(),
            mutated,
            "the files put back together are the mutated text"
        );
    }

    /// A project of one file has no layout to keep, and the whole text is that file.
    #[test]
    fn a_single_file_project_is_written_out_whole() {
        let project = Project {
            language: Language::Tolk,
            name: "one-file".to_string(),
            source: "fun f() {}
"
            .to_string(),
            ground_truth: Vec::new(),
            file_starts: vec![("only.tolk".to_string(), 1)],
            files: vec![(
                "only.tolk".to_string(),
                "fun f() {}
"
                .to_string(),
            )],
        };

        let files = mutated_files(
            &project,
            "fun f() { var x = 1; }
",
            0,
        );

        assert_eq!(
            files,
            vec![(
                "only.tolk".to_string(),
                "fun f() { var x = 1; }
"
                .to_string()
            )]
        );
    }
}
