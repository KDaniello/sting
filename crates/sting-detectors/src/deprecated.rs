//! `DEPR` — Deprecated Tolk stdlib usage.
//!
//! **Not a security detector, and deliberately not a [`sting_core::DefectClass`]
//! member at all** — [`sting_core::DefectClass`]'s own doc comment scopes it to
//! "security defect classes", and using a deprecated-but-still-correct stdlib
//! name is not a defect: the code is not wrong, it is just written against an
//! older spelling of the same operation. This module reuses the
//! [`crate::Detector`]/[`sting_core::Finding`] plumbing (so it gets the same
//! human/SARIF/JSON output, the same `Sting.toml` enable/disable filtering)
//! but reports every finding at [`sting_core::Severity::Info`] and is
//! deliberately **not** registered in `sting-bench`'s `DETECTOR_CODES` — there
//! is no "ground truth" to calibrate precision/recall against here the way
//! there is for a real vulnerability pattern: whether `stringCrc32(x)` is
//! deprecated is a fact about the Tolk compiler, not a judgment call.
//!
//! **Origin, found via real external validation** : while investigating why `hacktheton.com`'s CTF levels used
//! bare `setContractData`/`setContractCodePostponed` instead of the current
//! `contract.setData`/`contract.setCodePostponed`, the user asked whether
//! `sting` could flag deprecated-API usage the way `rustc`/`clippy`
//! (`#[deprecated]`) or `ruff` (Python) do. Checking confirmed Tolk has the
//! exact same mechanism: a real `@deprecated("use X instead of Y")` attribute
//! in its own stdlib source, not a guess or a convention sting invented.
//!
//! **Every entry in [`DEPRECATED_FREE_FUNCTIONS`]/[`DEPRECATED_METHODS`] below
//! is transcribed directly from a real `@deprecated(...)` attribute** in
//! `ton-blockchain/ton`'s `crypto/smartcont/tolk-stdlib/{common,gas-payments,
//! lisp-lists}.tolk` (`tvm-dicts.tolk`/`tvm-lowlevel.tolk` have none), checked
//! 2026-07-20 — see `docs/GLOSSARY.md`'s "Tolk stdlib deprecations" section
//! for the exact commits and a fully honest list of what was and was not
//! included (e.g. `ton(...)` is *not* here: its own doc comment says it "will
//! become `@deprecated`" from Tolk v1.5, i.e. it is not deprecated yet at the
//! stdlib revision checked). **This list goes stale as Tolk evolves** — the
//! same primitive checked this round (`setData`) was renamed *twice* in 8
//! months (has the full timeline) — see
//! `docs/GLOSSARY.md`'s maintenance note for how to re-check it.
//!
//! One deprecation is not a plain name, and is handled separately
//! ([`check_old_bounce_mode`]): `type OldBounceMode = bool` is
//! `@deprecated("use `BounceMode.NoBounce` instead of `false`")` — the plain
//! boolean spelling of "this message cannot bounce" in a `createMessage({
//! bounce: ..., ... })` literal. `ubm.rs`'s own `bounce_mode_is_no_bounce`
//! deliberately treats `bounce: false` as equally *safe* as
//! `BounceMode.NoBounce` (both compile, both mean the same thing at the
//! protocol level) — correctly so, that is a soundness question, not a style
//! one. This module's job is the opposite, narrower question: not "is it
//! safe" but "is it the modern spelling" — the two checks coexist
//! deliberately, not a contradiction.
//!
//! `DetectorKind::Ast`: pure name/shape matching, no dataflow, no
//! per-contract attribution — a deprecated stdlib name means the same thing
//! regardless of which contract or function it appears in.
//!
//! **A second, stronger tier: [`REMOVED_FREE_FUNCTIONS`], not merely
//! deprecated.** Running this detector against `hacktheton.com`'s own real
//! level source (the exact motivating example above) found **zero**
//! findings — surprising at first, until checked: that source does not use
//! any currently-`@deprecated`-tagged name at all. It uses `setContractData`/
//! `setContractCodePostponed`, which are not in the *current* stdlib in any
//! form, tagged or not — they were the stdlib's own camelCase names from the
//! 2024-10-31 rework until some point between 2025-04-14 and 2025-06-12, when
//! they were renamed *again* to `contract.setData`/`contract.setCodePostponed`
//! (see `docs/GLOSSARY.md`'s "Tolk stdlib deprecations" section for the
//! exact commits — the same history already cited above). Unlike a
//! `@deprecated`-tagged name, which still compiles, these do not compile with
//! *any* current Tolk toolchain at all — one tier more severe than
//! "discouraged", so [`removed_finding`] reports `Severity::Low`, not
//! `Severity::Info`, with wording that says "no longer exists", not "is
//! deprecated". `ac.rs`'s `LEGACY_DATA_MUTATION_FUNCTIONS`/
//! `LEGACY_CODE_MUTATION_FUNCTIONS` already recognized these same two names
//! for a completely different reason (treating them as the low-level
//! equivalent of `.save()`/`.setCodePostponed()` for access-control analysis)
//! — this module's table is independent, not a re-export, because the two
//! detectors ask different questions about the same two names.

use sting_ast::{AstStore, Language};
use sting_core::{DetectorId, Finding, Severity, Suggestion};
use tree_sitter::Node;

use crate::support::{as_method_call, create_message_argument, identifier_text, location_of};
use crate::{Detector, DetectorContext, DetectorKind};

/// `(deprecated name, exact replacement text)` — a **strict subset** of
/// [`DEPRECATED_FREE_FUNCTIONS`]/[`DEPRECATED_METHODS`]/
/// [`REMOVED_FREE_FUNCTIONS`] whose guidance really is "replace this one
/// name span with this exact text", nothing more. Most `DEPR` guidance describes a
/// *structural* rewrite instead — `stringCrc32("x")` becomes
/// `"x".crc32()` (the argument moves in front of the call, a free function
/// becomes a method call), `loadMessageFlags` needs restructuring around
/// `onBouncedMessage` entirely — this crate does not attempt to compute
/// those automatically: inventing a wrong rewrite would be worse than no
/// suggestion at all (see [`sting_core::Finding::suggestion`]'s own doc
/// comment on why most findings never carry one). `setContractData`/
/// `setContractCodePostponed` qualify too — `contract.` is just prepended
/// text, the call's own arguments never move.
const DETERMINISTIC_RENAMES: &[(&str, &str)] = &[
    ("buildAddress", "calculateAddress"),
    (
        "buildSameAddressInAnotherShard",
        "calculateSameAddressInAnotherShard",
    ),
    ("reserveToncoinsOnBalance", "reserveGramsOnBalance"),
    ("setContractData", "contract.setData"),
    ("setContractCodePostponed", "contract.setCodePostponed"),
    ("getContractData", "contract.getData"),
];

/// `(deprecated free-function name, exact replacement text)` for the three
/// zero-argument-by-definition "empty container" constructors — the whole
/// call span (name **and** its always-empty `()`) is replaced, not just the
/// name, since `[]` is a literal, not a call.
const DETERMINISTIC_WHOLE_CALL_REPLACEMENTS: &[(&str, &str)] = &[
    ("createEmptyTuple", "[]"),
    ("createEmptyMap", "[]"),
    ("createEmptyList", "[]"),
];

/// `(deprecated free-function name, the stdlib's own `@deprecated(...)` message)`.
const DEPRECATED_FREE_FUNCTIONS: &[(&str, &str)] = &[
    (
        "createEmptyTuple",
        "use `[]` instead of `createEmptyTuple()`",
    ),
    ("createEmptyMap", "use `[]` instead of `createEmptyMap()`"),
    ("createEmptyList", "use `[]` instead of `createEmptyList()`"),
    (
        "stringCrc32",
        "use `\"str\".crc32()` instead of `stringCrc32(\"str\")`",
    ),
    (
        "stringCrc16",
        "use `\"str\".crc16()` instead of `stringCrc16(\"str\")`",
    ),
    (
        "stringSha256",
        "use `\"str\".sha256()` instead of `stringSha256(\"str\")`",
    ),
    (
        "stringSha256_32",
        "use `\"str\".sha256_32()` instead of `stringSha256_32(\"str\")`",
    ),
    (
        "stringToBase256",
        "use `\"str\".toBase256()` instead of `stringToBase256(\"str\")`",
    ),
    (
        "stringHexToSlice",
        "use `\"hex\".hexToSlice()` instead of `stringHexToSlice(\"hex\")`",
    ),
    (
        "reserveToncoinsOnBalance",
        "use `reserveGramsOnBalance` instead of `reserveToncoinsOnBalance`",
    ),
    (
        "calculateOriginalForwardFee",
        "use modern `onInternalMessage` and access `in.originalForwardFee` directly",
    ),
];

/// `(deprecated method/field-access name, the stdlib's own `@deprecated(...)` message)`.
/// Matched by bare name only (`as_method_call`'s receiver is not type-checked)
/// — the same textual-heuristic precision level as `.save()`/`.setData()` in
/// `ac.rs`/`ubm.rs`, not full semantic resolution.
const DEPRECATED_METHODS: &[(&str, &str)] = &[
    (
        "bitsEqual",
        "use `senderAddress == ownerAddress`, not `senderAddress.bitsEqual(ownerAddress)`",
    ),
    ("buildAddress", "use `calculateAddress` instead"),
    (
        "addressMatches",
        "use `a == jwDeployed.calculateAddress()` instead of `jwDeployed.addressMatches(a)`",
    ),
    (
        "buildSameAddressInAnotherShard",
        "use `calculateSameAddressInAnotherShard` instead",
    ),
    (
        "fromValidBuilder",
        "prefer using `address` everywhere, avoid using `builder` and manual address construction",
    ),
    (
        "loadMessageFlags",
        "use `onBouncedMessage` handler instead of parsing msgCell flags manually",
    ),
    (
        "storeMessageOp",
        "use `createMessage` instead of composing messages manually",
    ),
    (
        "loadMessageQueryId",
        "use structures and lazy match instead of parsing messages manually",
    ),
    (
        "skipMessageQueryId",
        "use structures and lazy loading instead of parsing messages manually",
    ),
    (
        "storeMessageQueryId",
        "use `createMessage` instead of composing messages manually",
    ),
];

/// `(removed free-function name, replacement guidance)` — see this module's
/// doc comment's "A second, stronger tier" section. Kept to names with real
/// evidence that they are still in live use *and* real evidence that they no
/// longer exist — not extended speculatively to the even-older
/// `set_data`/`set_code` `snake_case` names from before the 2024-10-31
/// rework, which no real code encountered so far has used.
///
/// `getContractData` was added on evidence stronger than the first two had: it is the read-side twin of `setContractData`,
/// removed in the same rework, it is still in live use **in this very
/// repository** (four `corpus/fixtures/detectors/AC/` fixtures), and the
/// vendored compiler proves the removal directly — `tolk 1.4.2` answers with
/// an "undefined symbol" error on it, while `contract.getData()`
/// compiles. Its absence here was a real gap: `sting` measured `AC`'s
/// precision on files that cannot compile, and its own deprecation detector
/// stayed silent about the reason.
const REMOVED_FREE_FUNCTIONS: &[(&str, &str)] = &[
    (
        "setContractData",
        "use `contract.setData(...)` — `setContractData` no longer exists in Tolk's stdlib, code calling it will not compile with any current Tolk toolchain",
    ),
    (
        "setContractCodePostponed",
        "use `contract.setCodePostponed(...)` — `setContractCodePostponed` no longer exists in Tolk's stdlib, code calling it will not compile with any current Tolk toolchain",
    ),
    (
        "getContractData",
        "use `contract.getData()` — `getContractData` no longer exists in Tolk's stdlib, code calling it will not compile with any current Tolk toolchain",
    ),
];

pub(crate) struct DeprecatedDetector;

impl Detector for DeprecatedDetector {
    fn id(&self) -> DetectorId {
        "DEPR"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    fn default_severity(&self) -> Severity {
        Severity::Info
    }

    fn check(&self, _store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let source = ctx.source();
        let mut findings = Vec::new();

        for visited in sting_ast::Preorder::from_node(ctx.tree().root_node()) {
            let node = visited.node;
            if let Some(call) = as_method_call(node, source, Language::Tolk)
                && let Some((name, guidance)) = DEPRECATED_METHODS
                    .iter()
                    .find(|(name, _)| *name == call.method)
            {
                let suggestion = rename_suggestion(call.method_node, call.method.as_str());
                findings.push(self.finding(call.method_node, name, guidance, suggestion, ctx));
                continue;
            }
            if node.kind() == "function_call"
                && let Some(callee) = node.child_by_field_name("callee")
                && callee.kind() == "identifier"
                && let Some(callee_name) = identifier_text(callee, source)
            {
                if let Some((name, guidance)) = REMOVED_FREE_FUNCTIONS
                    .iter()
                    .find(|(name, _)| *name == callee_name)
                {
                    let suggestion = rename_suggestion(callee, &callee_name);
                    findings.push(self.removed_finding(callee, name, guidance, suggestion, ctx));
                    continue;
                }
                if let Some((name, guidance)) = DEPRECATED_FREE_FUNCTIONS
                    .iter()
                    .find(|(name, _)| *name == callee_name)
                {
                    let suggestion = rename_suggestion(callee, &callee_name)
                        .or_else(|| whole_call_suggestion(node, &callee_name));
                    findings.push(self.finding(callee, name, guidance, suggestion, ctx));
                    continue;
                }
            }
            if let Some(bounce_field) = check_old_bounce_mode(node, source) {
                findings.push(self.finding(
                    bounce_field,
                    "false",
                    "use `BounceMode.NoBounce` instead of `false`",
                    Some(Suggestion {
                        start_byte: bounce_field.start_byte(),
                        end_byte: bounce_field.end_byte(),
                        replacement: "BounceMode.NoBounce".to_string(),
                    }),
                    ctx,
                ));
            }
        }

        findings
    }
}

impl DeprecatedDetector {
    fn finding(
        &self,
        node: Node,
        name: &str,
        guidance: &str,
        suggestion: Option<Suggestion>,
        ctx: &DetectorContext,
    ) -> Finding {
        Finding {
            detector_id: self.id(),
            severity: self.default_severity(),
            message: format!("`{name}` is deprecated in Tolk's own standard library"),
            remediation: guidance.to_string(),
            location: location_of(node, ctx),
            confidence: None,
            dynamic_verification: None,
            suggestion,
        }
    }

    /// Same shape as [`Self::finding`], but for a name in
    /// [`REMOVED_FREE_FUNCTIONS`] — `Severity::Low`, not `Info`, and
    /// "no longer exists" wording, not "is deprecated": this is a stronger
    /// claim (does not compile at all with current Tolk), not a style
    /// preference.
    fn removed_finding(
        &self,
        node: Node,
        name: &str,
        guidance: &str,
        suggestion: Option<Suggestion>,
        ctx: &DetectorContext,
    ) -> Finding {
        Finding {
            detector_id: self.id(),
            severity: Severity::Low,
            message: format!(
                "`{name}` no longer exists in Tolk's standard library — this is not just discouraged, it will not compile with any current Tolk toolchain"
            ),
            remediation: guidance.to_string(),
            location: location_of(node, ctx),
            confidence: None,
            dynamic_verification: None,
            suggestion,
        }
    }
}

/// If `name` (the exact text at `name_node`) has an entry in
/// [`DETERMINISTIC_RENAMES`], a [`Suggestion`] replacing just that node's
/// own span with the new name — sound for either a bare identifier
/// (`node.child_by_field_name("callee")`) or a method name node
/// (`MethodCall::method_node`), since both are plain name spans with no
/// arguments attached.
fn rename_suggestion(name_node: Node, name: &str) -> Option<Suggestion> {
    let (_, replacement) = DETERMINISTIC_RENAMES.iter().find(|(old, _)| *old == name)?;
    Some(Suggestion {
        start_byte: name_node.start_byte(),
        end_byte: name_node.end_byte(),
        replacement: (*replacement).to_string(),
    })
}

/// If `callee_name` has an entry in [`DETERMINISTIC_WHOLE_CALL_REPLACEMENTS`],
/// a [`Suggestion`] replacing `call_node`'s **entire** span (name and its
/// always-empty `()`) with the replacement literal.
fn whole_call_suggestion(call_node: Node, callee_name: &str) -> Option<Suggestion> {
    let (_, replacement) = DETERMINISTIC_WHOLE_CALL_REPLACEMENTS
        .iter()
        .find(|(old, _)| *old == callee_name)?;
    Some(Suggestion {
        start_byte: call_node.start_byte(),
        end_byte: call_node.end_byte(),
        replacement: (*replacement).to_string(),
    })
}

/// If `node` is a `createMessage({ ..., bounce: false, ... })` call using the
/// plain boolean literal spelling (`@deprecated("use `BounceMode.NoBounce`
/// instead of `false`")` on Tolk's own `type OldBounceMode = bool`), the
/// `bounce` field's value node — `None` for `bounce: BounceMode.NoBounce`
/// (already the modern spelling) or a bounceable message (nothing to
/// modernize here, that is `ubm.rs`'s concern, not this module's).
fn check_old_bounce_mode<'tree>(node: Node<'tree>, source: &str) -> Option<Node<'tree>> {
    let object_literal = create_message_argument(node, source)?;
    let args = object_literal.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() == "instance_argument")
        .find_map(|arg| {
            let is_bounce_field = arg
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                == Some("bounce");
            if !is_bounce_field {
                return None;
            }
            let value = arg.child_by_field_name("value")?;
            (value.utf8_text(source.as_bytes()).ok() == Some("false")).then_some(value)
        })
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

    use super::{
        DEPRECATED_FREE_FUNCTIONS, DEPRECATED_METHODS, DeprecatedDetector, REMOVED_FREE_FUNCTIONS,
    };
    use crate::{Detector, DetectorContext};

    /// Every name this detector knows about, in any tier.
    fn known_names() -> std::collections::HashSet<&'static str> {
        DEPRECATED_FREE_FUNCTIONS
            .iter()
            .chain(DEPRECATED_METHODS)
            .chain(REMOVED_FREE_FUNCTIONS)
            .map(|(name, _)| *name)
            .collect()
    }

    /// The tables above are **hand-transcribed** from the Tolk stdlib's own
    /// `@deprecated(...)` attributes, and this module's doc comment says so
    /// plainly, including the warning that "this list goes stale as Tolk
    /// evolves". A warning is not a mechanism: the same primitive was renamed
    /// twice in eight months, and nothing would have failed if a rename had
    /// been missed.
    ///
    /// So this reads the real thing. It parses the vendored compiler's own
    /// `tolk-stdlib/*.tolk`, collects every function carrying `@deprecated`,
    /// and asserts this detector knows each one. A stdlib bump that adds a
    /// deprecation now fails here instead of silently widening the gap
    /// between what Tolk deprecates and what `sting` reports.
    ///
    /// Skips when `vendor/tolk/` is absent — it is `.gitignore`d, same
    /// pattern as every other real-toolchain test in this workspace. Checked
    /// against Tolk 1.4.2's stdlib: 22 deprecated functions, all 22 covered.
    #[test]
    fn every_deprecation_in_the_vendored_stdlib_is_known_to_this_detector() {
        let stdlib =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/tolk/tolk-stdlib");
        let Ok(entries) = std::fs::read_dir(&stdlib) else {
            println!(
                "SKIP: {} not present locally (vendor/tolk/ is .gitignore'd)",
                stdlib.display()
            );
            return;
        };

        let known = known_names();
        let mut found = 0_usize;
        let mut missing: Vec<String> = Vec::new();

        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "tolk") {
                let Ok(source) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let lines: Vec<&str> = source.lines().collect();
                for (index, line) in lines.iter().enumerate() {
                    // Must be the attribute itself, not a mention of it. The
                    // stdlib really does discuss `@deprecated` inside doc
                    // comments — `ton()` carries "From Tolk v1.5, it will
                    // become @deprecated(...)", which is a *promise*, not a
                    // deprecation. Matching `contains` alone made this test
                    // report `ton` as an uncovered deprecation the moment the
                    // compiler was bumped to 1.4.2; that was this test's bug,
                    // not the detector's gap.
                    if !line.trim_start().starts_with("@deprecated") {
                        continue;
                    }
                    // The attribute sits directly above its `fun`, possibly
                    // with further attributes in between (`@pure`, `@inline`).
                    let Some(name) = lines[index + 1..]
                        .iter()
                        .take(3)
                        .find_map(|next| next.trim().strip_prefix("fun "))
                        .map(|rest| {
                            rest.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
                                .next()
                                .unwrap_or("")
                                .to_string()
                        })
                    else {
                        continue;
                    };
                    found += 1;
                    // `slice.loadMessageFlags` is matched by its last segment:
                    // the detector's method tier keys on the method name, the
                    // receiver type is not part of what it can see at an AST
                    // call site.
                    let short = name.rsplit('.').next().unwrap_or(&name);
                    if !known.contains(short) {
                        missing.push(name.clone());
                    }
                }
            }
        }

        assert!(
            found >= 20,
            "expected to find the stdlib's deprecations, found {found} — \
             the extraction above stopped matching, which would make this test pass vacuously"
        );
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "the vendored Tolk stdlib deprecates {} function(s) this detector does not know: {missing:?}",
            missing.len()
        );
    }

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        DeprecatedDetector.check(&store, &ctx)
    }

    /// Same byte-splice `sting-cli`'s real `--fix` performs — see
    /// `sua.rs`'s own identical helper.
    fn apply_suggestion(source: &str, finding: &sting_core::Finding) -> String {
        let s = finding
            .suggestion
            .as_ref()
            .expect("this test's finding must carry a suggestion");
        format!(
            "{}{}{}",
            &source[..s.start_byte],
            s.replacement,
            &source[s.end_byte..]
        )
    }

    #[test]
    fn flags_a_deprecated_free_function() {
        let source = "fun f() {\n    val h = stringCrc32(\"abc\");\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "DEPR");
        assert_eq!(findings[0].severity, sting_core::Severity::Info);
        assert!(findings[0].message.contains("stringCrc32"));
        assert!(findings[0].remediation.contains(".crc32()"));
        // A structural rewrite (the argument moves in front of the call) —
        // deliberately no suggestion, see DETERMINISTIC_RENAMES's own doc
        // comment for why this crate does not attempt to compute it.
        assert!(findings[0].suggestion.is_none());
    }

    #[test]
    fn suggests_replacing_a_pure_rename_free_function() {
        let source = "fun f(a: int, b: int) {\n    reserveToncoinsOnBalance(a, b);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            apply_suggestion(source, &findings[0]),
            "fun f(a: int, b: int) {\n    reserveGramsOnBalance(a, b);\n}\n"
        );
    }

    #[test]
    fn suggests_replacing_an_empty_container_constructor_with_a_literal() {
        let source = "fun f() {\n    val t = createEmptyTuple();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            apply_suggestion(source, &findings[0]),
            "fun f() {\n    val t = [];\n}\n"
        );
    }

    #[test]
    fn suggests_replacing_a_removed_free_function_with_its_namespaced_form() {
        let source = "fun f(c: cell) {\n    setContractData(c);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            apply_suggestion(source, &findings[0]),
            "fun f(c: cell) {\n    contract.setData(c);\n}\n"
        );
    }

    /// `getContractData` is the read-side twin of `setContractData` and was
    /// removed in the same rework, but was missing from the tables until
    /// four fixtures in this repository were found still calling it — files
    /// that `tolk 1.4.2` rejects outright with an "undefined symbol" error.
    #[test]
    fn suggests_replacing_the_removed_read_side_contract_data_accessor() {
        let source = "fun f() {\n    var ds = getContractData().beginParse();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            apply_suggestion(source, &findings[0]),
            "fun f() {\n    var ds = contract.getData().beginParse();\n}\n"
        );
        assert_eq!(
            findings[0].severity,
            sting_core::Severity::Low,
            "a name that no longer compiles is not merely discouraged"
        );
    }

    #[test]
    fn suggests_replacing_the_old_bounce_mode_boolean_spelling() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val out = createMessage({ bounce: false, dest: x, value: 0, body: y });\n    out.send(64);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        let fixed = apply_suggestion(source, &findings[0]);
        assert!(fixed.contains("bounce: BounceMode.NoBounce"));
        sting_ast::parse(&fixed).expect("the fixed source must still parse");
    }

    #[test]
    fn does_not_suggest_a_replacement_for_a_deprecated_method_needing_a_structural_rewrite() {
        // bitsEqual's own real fix moves the receiver to the left of `==` —
        // a structural rewrite this crate does not attempt.
        let source =
            "fun f(sender: address, owner: address) {\n    val eq = sender.bitsEqual(owner);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].suggestion.is_none());
    }

    #[test]
    fn applying_every_deterministic_suggestion_still_parses_and_clears_the_finding() {
        for source in [
            "fun f(a: int, b: int) {\n    reserveToncoinsOnBalance(a, b);\n}\n",
            "fun f() {\n    val t = createEmptyTuple();\n}\n",
            "fun f() {\n    val t = createEmptyMap();\n}\n",
            "fun f() {\n    val t = createEmptyList();\n}\n",
            "fun f(c: cell) {\n    setContractData(c);\n}\n",
            "fun f(c: cell) {\n    setContractCodePostponed(c);\n}\n",
        ] {
            let findings = run(source);
            assert_eq!(
                findings.len(),
                1,
                "expected exactly one finding for {source:?}"
            );
            let fixed = apply_suggestion(source, &findings[0]);
            sting_ast::parse(&fixed).expect("the fixed source must still parse");
            assert!(
                run(&fixed).is_empty(),
                "expected no DEPR findings left after fixing {source:?}, got {fixed:?}"
            );
        }
    }

    #[test]
    fn flags_a_deprecated_method_call() {
        let source =
            "fun f(sender: address, owner: address) {\n    val eq = sender.bitsEqual(owner);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("bitsEqual"));
    }

    #[test]
    fn flags_manual_message_flag_parsing() {
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    var cs = inMsgFull.beginParse();\n    val flags = cs.loadMessageFlags();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("loadMessageFlags"));
    }

    #[test]
    fn flags_the_old_bounce_mode_boolean_spelling() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val out = createMessage({ bounce: false, dest: x, value: 0, body: y });\n    out.send(64);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].remediation.contains("BounceMode.NoBounce"));
    }

    #[test]
    fn does_not_flag_the_modern_bounce_mode_spelling() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val out = createMessage({ bounce: BounceMode.NoBounce, dest: x, value: 0, body: y });\n    out.send(64);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_bounceable_message() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(64);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_the_modern_replacement_names() {
        let source = "fun f() {\n    val e = [];\n    val h = \"abc\".crc32();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_an_unrelated_identifier_that_merely_contains_a_deprecated_substring() {
        // `bitsEqual` must match by whole method name, not substring — this
        // call is to a same-named-but-unrelated free function, not a method.
        let source = "fun f() {\n    val x = notBitsEqualAtAll(1, 2);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_a_removed_free_function_as_low_not_info() {
        // Real external finding (direction 5, hacktheton.com's actual level
        // source): `setContractData` is not merely discouraged, it no
        // longer exists in current Tolk at all.
        let source = "fun saveData() {\n    setContractData(beginCell().endCell());\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "DEPR");
        assert_eq!(findings[0].severity, sting_core::Severity::Low);
        assert!(findings[0].message.contains("no longer exists"));
        assert!(findings[0].remediation.contains("contract.setData"));
    }

    #[test]
    fn flags_a_removed_code_upgrade_function() {
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val code: cell = inMsgBody.loadRef();\n    setContractCodePostponed(code);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, sting_core::Severity::Low);
        assert!(findings[0].message.contains("setContractCodePostponed"));
    }

    #[test]
    fn does_not_flag_the_current_contract_namespaced_names() {
        let source = "fun saveData() {\n    contract.setData(beginCell().endCell());\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_info() {
        assert_eq!(DeprecatedDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(
            DeprecatedDetector.default_severity(),
            sting_core::Severity::Info
        );
    }
}
