//! Detector trait, registry, and built-in detectors.
//!
//! The framework shipped first with one toy detector, to validate
//! `Detector` and `DetectorRegistry` end to end (`DEMO-LONG-FN`, since removed —
//! see `docs/domains/dataflow-cfg.md` for why it existed
//! and `docs/domains/detectors.md` for the real detectors that replaced
//! it as the framework's actual coverage). The first four real security
//! detectors came next, chosen from the `TONScanner` taxonomy
//! (`sting_core::DefectClass`): `gvr`, `lep`, `id`, `ubm`.

mod ac;
mod br;
mod cfg;
mod deprecated;
mod eps;
pub mod explain;
mod gvr;
mod id;
mod lep;
mod pl;
mod store_load;
mod sua;
mod support;
mod tlb;
mod ubm;
mod uda;
mod ur;
mod uta;

use std::path::PathBuf;
use std::sync::Arc;

use sting_ast::{AstStore, Language, SourceRange};
use sting_core::{DetectorId, Finding, Location, Severity, StingConfig};
use sting_tolkc::{FunctionEntry, ProjectStore};
use tree_sitter::Tree;

/// Which analysis machinery a [`Detector`] needs. Misti has a third kind,
/// Soufflé/Datalog-backed, for complex interprocedural queries
/// (`docs/GLOSSARY.md#prior-art`) — not ported here: it is only worth its
/// complexity past ~40 detectors with heavy cross-procedure needs, nowhere
/// near this project's MVP scope (`docs/domains/dataflow-cfg.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorKind {
    /// Pattern-matches over the AST directly, no cross-block state.
    Ast,
    /// Needs `sting-dataflow`'s `Semilattice`/`Transfer`/`WorklistSolver`.
    Dataflow,
}

/// What a [`Detector::check`] needs beyond the [`AstStore`] itself.
///
/// The first version of this type carried only the file path: `AstStore`'s
/// signatures — name and type text, already-converted 1-indexed ranges — were
/// enough for a "function too long" toy check. The real detectors
/// need more — `GVR` walks `var` declarations inside a function body, `LEP`
/// walks a function's statement/branch structure, `ID`/`UBM` build a local
/// CFG — none of which `AstStore` captures (deliberately; see its own doc
/// comment). This is exactly the "add it if a real future detector does"
/// escape hatch the first version of this doc comment named in advance:
/// the live `tree` (to re-locate an `AstStore` range's `Node` via
/// [`sting_ast::SourceRange::locate`] and walk into it) and `source` (to
/// read identifier text tree-sitter nodes don't carry themselves).
#[derive(Debug, Clone)]
pub struct DetectorContext {
    file: PathBuf,
    tree: Tree,
    source: String,
    language: Language,
    project: Option<ProjectContext>,
}

/// A compiled whole-project index this file's own analysis can resolve
/// cross-file calls against — attached via [`DetectorContext::with_project`]
/// only by `sting-pipeline::LintController::lint_paths`, and only when a
/// real, vendored `tolk` binary was present and successfully compiled this
/// file's own project.
/// [`LintController::lint_source`] never attaches one — see that method's
/// own doc comment for why (the same guarantee already established for
/// suppressions/`min_severity`): `sting-bench` scores findings against
/// ground truth via `lint_source` over textually merged corpus projects,
/// where every function is already same-file-visible by construction, so
/// this field existing or not can never move a calibrated precision/recall
/// number either way.
#[derive(Debug, Clone)]
struct ProjectContext {
    store: Arc<ProjectStore>,
    file_id: u32,
}

impl DetectorContext {
    /// Builds a context for analyzing `file`, whose already-parsed `tree`
    /// and `source` text back it, with no project context attached (see
    /// [`Self::with_project`]).
    #[must_use]
    pub fn new(file: PathBuf, tree: Tree, source: String) -> Self {
        Self {
            file,
            tree,
            source,
            language: Language::Tolk,
            project: None,
        }
    }

    /// Says which grammar `tree` came from — builder-style, so the many callers holding a Tolk tree
    /// are unaffected and the one holding another has to say so.
    #[must_use]
    pub fn in_language(mut self, language: Language) -> Self {
        self.language = language;
        self
    }

    /// Which grammar this context's tree came from.
    ///
    /// A detector reads this before it reads a node kind. The two grammars name some of their nodes
    /// alike and shape them differently, so a detector that walks one tree with the other one's
    /// assumptions finds nothing and reports that as a clean file.
    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    /// Attaches a compiled [`ProjectStore`] plus this context's own file's
    /// resolved `file_id` within it — builder-style, so a caller that never
    /// has one (`lint_source`, every existing unit test) is unaffected.
    #[must_use]
    pub fn with_project(mut self, store: Arc<ProjectStore>, file_id: u32) -> Self {
        self.project = Some(ProjectContext { store, file_id });
        self
    }

    /// The file being analyzed.
    #[must_use]
    pub fn file(&self) -> &std::path::Path {
        &self.file
    }

    /// The parsed tree backing this context's `AstStore` — lets a detector
    /// re-locate a signature's [`SourceRange`] back to a live `Node` and
    /// walk into statement-level structure `AstStore` does not capture.
    #[must_use]
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The raw source text `tree` was parsed from.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Builds a [`Location`] by attaching this context's file to `range`.
    #[must_use]
    pub fn location(&self, range: SourceRange) -> Location {
        range.with_file(self.file.clone())
    }

    /// The compiled [`ProjectStore`] backing [`Self::resolve_call`], if
    /// [`Self::with_project`] attached one.
    #[must_use]
    pub fn project_store(&self) -> Option<&Arc<ProjectStore>> {
        self.project.as_ref().map(|p| &p.store)
    }

    /// This context's own file's `file_id` within [`Self::project_store`],
    /// if one is attached — the counterpart callers need alongside
    /// [`Self::project_store`] to call [`ProjectStore::reachable_files`]
    /// themselves (`id.rs`/`tlb.rs`'s cross-file store/load pairing,
    ///): [`Self::resolve_call`]
    /// already threads this internally for a single-name lookup, but a
    /// caller that wants the whole reachable-file set for itself needs the
    /// raw id, not just what `resolve_call` does with it.
    #[must_use]
    pub fn file_id(&self) -> Option<u32> {
        self.project.as_ref().map(|p| p.file_id)
    }

    /// Resolves `callee_name`, called from this context's own file, to
    /// every function it could refer to anywhere in the whole compiled
    /// project — `None` if no [`ProjectStore`] is attached at all (see this
    /// struct's own doc comment for when that is/isn't the case), `Some(_)`
    /// (possibly empty) once one is.
    #[must_use]
    pub fn resolve_call(&self, callee_name: &str) -> Option<Vec<&FunctionEntry>> {
        let project = self.project.as_ref()?;
        Some(project.store.resolve_call(project.file_id, callee_name))
    }
}

/// One registrable check. Implementors are `Send + Sync` so
/// `DetectorRegistry::run_all` can, in a later block, be parallelized over
/// files with `rayon` without redesigning this trait
/// (`docs/domains/dataflow-cfg.md` makes the same argument for
/// `AstStore` itself).
pub trait Detector: Send + Sync {
    /// Stable identifier — see [`sting_core::DetectorId`]'s doc comment for
    /// why this is a plain string, not a [`sting_core::DefectClass`].
    fn id(&self) -> DetectorId;

    /// Which analysis machinery this detector needs.
    fn kind(&self) -> DetectorKind;

    /// The languages this detector has a rule for.
    ///
    /// A detector runs only against a file it can read, and the run says which detectors it held
    /// back rather than letting their silence read as a clean result. **"This detector has no rule
    /// for this language" and "this detector found nothing" are different claims**, and a report
    /// that renders them alike credits an analyser for code it never examined.
    ///
    /// The default is Tolk alone, which is where every detector started.
    fn languages(&self) -> &'static [Language] {
        &[Language::Tolk]
    }

    /// Severity to report before any dynamic-verification adjustment
    /// (`docs/domains/emulator.md`).
    fn default_severity(&self) -> Severity;

    /// Whether [`DetectorRegistry::filtered`] keeps this detector when
    /// `Sting.toml` does not name it. `false` marks an **experimental**
    /// detector: compiled in, measured by the benchmark, documented — but
    /// off unless the user asks for it by id in `detectors.enable`.
    ///
    /// Exists because of a measurement rather than a preference. The labelled set
    /// labeled 409 storage-mutation sites across 89 real contracts by hand
    /// and found that roughly half of `AC`'s firings land on code that is
    /// deliberately open to anyone, with **no** static signal separating
    /// those from a forgotten check: the best of the four hypotheses named
    /// in advance reached 33% precision on the held-out half against a
    /// threshold of 80%. A detector honestly marked high-noise and left off
    /// is more useful than one pretending to be precise — the first gets
    /// reached for when it is wanted, the second stops being read.
    fn is_default_enabled(&self) -> bool {
        true
    }

    /// Runs this detector against one file's already-built [`AstStore`].
    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding>;
}

/// A set of [`Detector`]s to run, built from the compiled-in defaults and
/// optionally narrowed by a [`StingConfig`].
///
/// Custom/plugin detectors (loaded at runtime rather than compiled in) are
/// explicitly out of scope (`docs/domains/dataflow-cfg.md`) — `Box<dyn Detector>` already
/// abstracts over where a detector came from, so adding that later does not
/// require reshaping this type.
pub struct DetectorRegistry {
    detectors: Vec<Box<dyn Detector>>,
}

impl std::fmt::Debug for DetectorRegistry {
    // Manual, not derived: `Detector` itself is not `Debug` (a trait object
    // bound just to enable printing detector internals is not worth the
    // restriction on implementors), so this lists ids only — still enough to
    // make `#[derive(Debug)]`-requiring test helpers like `expect_err` work
    // without genuinely wanting to inspect a `DetectorRegistry`'s contents.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DetectorRegistry")
            .field(
                "detector_ids",
                &self.detectors().map(Detector::id).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl DetectorRegistry {
    /// All compiled-in detectors, unfiltered: the four original ones
    /// (`docs/domains/detectors.md`) plus `TLB`, `PL`, `BR`, `UR` and `AC` —
    /// `TLB`/`AC` are sting's own additions past the `TONScanner` taxonomy;
    /// `PL`/`BR`/`UR` are the remaining three of that taxonomy's original 8
    /// codes, deferred rather than abandoned and implemented once a detector
    /// for each was actually built. `IFM` is the only original
    /// code still not implemented — structurally moot for Tolk, not merely
    /// deferred, see `docs/domains/detectors.md`'s own reasoning. `AC`
    /// is explicitly narrower/lower-confidence than the other 8 — see
    /// `ac.rs`'s own doc comment for the real-corpus research this scope was
    /// derived from. `UTA` (direction 5, `hacktheton.com`) is a real,
    /// narrow-v1 vulnerability class, same taxonomy as the rest. `SUA`
    /// (direction 5, Misti prior-art teardown — `sua.rs`'s own doc comment)
    /// is sting's own port of Misti's `SuspiciousUnary`, same taxonomy too.
    /// `DEPR` is the odd one out — not a [`sting_core::DefectClass`] member at all, not
    /// a vulnerability, see `deprecated.rs`'s own doc comment; included here
    /// anyway (not a separate registry constructor) so it gets the same
    /// `Sting.toml` enable/disable filtering and output plumbing as every
    /// other detector for free, and a user who only wants security findings
    /// can disable it by id exactly like any other.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            detectors: vec![
                Box::new(gvr::GvrDetector),
                Box::new(lep::LepDetector),
                Box::new(id::IdDetector),
                Box::new(ubm::UbmDetector),
                Box::new(tlb::TlbDetector),
                Box::new(pl::PlDetector),
                Box::new(br::BrDetector),
                Box::new(ur::UrDetector),
                Box::new(ac::AcDetector),
                Box::new(uta::UtaDetector),
                Box::new(sua::SuaDetector),
                Box::new(eps::EpsDetector),
                Box::new(uda::UdaDetector),
                Box::new(deprecated::DeprecatedDetector),
            ],
        }
    }

    /// An empty registry, useful for tests that want to register exactly one
    /// detector without pulling in the compiled-in defaults.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            detectors: Vec::new(),
        }
    }

    /// Adds `detector`, builder-style.
    #[must_use]
    pub fn with(mut self, detector: Box<dyn Detector>) -> Self {
        self.detectors.push(detector);
        self
    }

    /// Narrows the registry per `cfg.detectors`: `disable` always removes a
    /// detector; `enable`, if non-empty, keeps *only* the named detectors
    /// (a non-empty `enable` acts as a whitelist, an empty one is "use
    /// whatever this registry already had" — see `docs/ARCHITECTURE.md` §5's
    /// `Sting.toml` shape).
    ///
    /// Validates `enable`/`disable` against *this registry's actual
    /// detector ids* before filtering — not against
    /// [`sting_core::DefectClass::ALL`], which `sting-core` itself cannot do
    /// (see [`sting_core::StingConfig::parse`]'s doc comment) since the real
    /// universe of valid ids includes non-taxonomy detectors such as
    /// `DEMO-LONG-FN`, which only this registry — the thing that
    /// actually holds the registered detectors — knows about.
    ///
    /// # Errors
    /// Returns [`UnknownDetectorIdError`] naming the first `enable`/`disable`
    /// entry that matches no detector currently in this registry (a typo
    /// like `"GRV"` for `"GVR"` would otherwise silently filter nothing).
    pub fn filtered(mut self, cfg: &StingConfig) -> Result<Self, UnknownDetectorIdError> {
        let enable = &cfg.detectors.enable;
        let disable = &cfg.detectors.disable;
        let known_ids: Vec<DetectorId> = self.detectors.iter().map(|d| d.id()).collect();
        for requested in enable.iter().chain(disable) {
            if !known_ids.contains(&requested.as_str()) {
                return Err(UnknownDetectorIdError {
                    requested: requested.clone(),
                    known_ids,
                });
            }
        }

        self.detectors.retain(|d| {
            let id = d.id();
            if disable.iter().any(|c| c == id) {
                return false;
            }
            if enable.iter().any(|c| c == id) {
                return true;
            }
            // An experimental detector needs its id named explicitly; a
            // non-empty `enable` is still a whitelist for everyone else.
            d.is_default_enabled() && enable.is_empty()
        });
        Ok(self)
    }

    /// The registered detectors, in registration order.
    pub fn detectors(&self) -> impl Iterator<Item = &dyn Detector> {
        self.detectors.iter().map(AsRef::as_ref)
    }

    /// Runs every detector that reads this file's language against `store`, concatenates their
    /// findings in registration order, and names the ones it held back.
    ///
    /// A detector without a rule for a language is not run against it, and the caller is handed its
    /// id rather than its silence. **"No rule here" and "nothing found here" are different claims**,
    /// and a run that renders them alike credits the analyser for code no rule ever looked at.
    #[must_use]
    pub fn run_all(
        &self,
        store: &AstStore,
        ctx: &DetectorContext,
    ) -> (Vec<Finding>, Vec<DetectorId>) {
        // The tree and the store are built from one grammar and the context names one; a caller
        // that mismatched them would have every detector walk a tree with the other language's
        // assumptions, find nothing, and report a clean file. Nothing else notices, so this does.
        debug_assert_eq!(
            store.language(),
            ctx.language(),
            "the store and the context disagree about which grammar produced this tree"
        );
        let mut findings = Vec::new();
        let mut without_a_rule = Vec::new();
        for detector in &self.detectors {
            if detector.languages().contains(&ctx.language()) {
                findings.extend(detector.check(store, ctx));
            } else {
                without_a_rule.push(detector.id());
            }
        }
        (findings, without_a_rule)
    }
}

/// Error from [`DetectorRegistry::filtered`]: a `Sting.toml`
/// `detectors.enable`/`disable` entry that matches no detector actually
/// registered.
#[derive(Debug, Clone, thiserror::Error)]
#[error("unknown detector id {requested:?} in Sting.toml (registered detectors: {})", known_ids.join(", "))]
pub struct UnknownDetectorIdError {
    /// The `enable`/`disable` entry that did not match any registered detector.
    pub requested: String,
    /// The ids that *were* registered, for the error message.
    pub known_ids: Vec<DetectorId>,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_core::{DetectorsConfig, StingConfig};

    use super::{Detector, DetectorContext, DetectorRegistry};

    #[test]
    fn an_experimental_detector_is_dropped_unless_named_explicitly() {
        // `AC` is compiled in and benchmarked, but a user
        // who configures nothing must not see it.
        let default_ids: Vec<_> = DetectorRegistry::builtin()
            .filtered(&StingConfig::default())
            .unwrap()
            .detectors()
            .map(Detector::id)
            .collect();
        assert!(
            !default_ids.contains(&"AC"),
            "AC must be absent from the default set, got: {default_ids:?}"
        );
        assert!(
            default_ids.contains(&"UR"),
            "a non-experimental detector must be unaffected, got: {default_ids:?}"
        );

        let enabled = StingConfig {
            detectors: DetectorsConfig {
                enable: vec!["AC".to_string()],
                disable: Vec::new(),
            },
            ..StingConfig::default()
        };
        let ids: Vec<_> = DetectorRegistry::builtin()
            .filtered(&enabled)
            .unwrap()
            .detectors()
            .map(Detector::id)
            .collect();
        assert_eq!(
            ids,
            vec!["AC"],
            "naming AC explicitly must enable it, and a non-empty `enable` stays a whitelist"
        );
    }

    #[test]
    fn builtin_registry_contains_all_fourteen_detectors() {
        let registry = DetectorRegistry::builtin();
        let ids: Vec<_> = registry.detectors().map(Detector::id).collect();
        assert_eq!(
            ids,
            vec![
                "GVR", "LEP", "ID", "UBM", "TLB", "PL", "BR", "UR", "AC", "UTA", "SUA", "EPS",
                "UDA", "DEPR"
            ]
        );
    }

    #[test]
    fn filtered_disable_removes_a_detector() {
        let cfg = StingConfig {
            detectors: DetectorsConfig {
                enable: vec![],
                disable: vec!["GVR".to_string()],
            },
            ..Default::default()
        };
        let registry = DetectorRegistry::builtin()
            .filtered(&cfg)
            .expect("GVR is a registered id");
        // 14 registered, minus GVR by `disable`, minus the experimental `AC`
        // which an empty `enable` does not pick up.
        assert_eq!(registry.detectors().count(), 12);
    }

    #[test]
    fn filtered_empty_enable_keeps_the_default_set() {
        let cfg = StingConfig::default();
        let registry = DetectorRegistry::builtin()
            .filtered(&cfg)
            .expect("empty config is always valid");
        // 14 registered, 13 default-enabled: `AC` is experimental.
        assert_eq!(registry.detectors().count(), 13);
    }

    #[test]
    fn filtered_non_empty_enable_acts_as_a_whitelist() {
        let cfg = StingConfig {
            detectors: DetectorsConfig {
                enable: vec!["GVR".to_string()],
                disable: vec![],
            },
            ..Default::default()
        };
        let registry = DetectorRegistry::builtin()
            .filtered(&cfg)
            .expect("GVR is a registered id");
        assert_eq!(
            registry.detectors().count(),
            1,
            "GVR is the only entry in its own whitelist"
        );
    }

    #[test]
    fn filtered_rejects_a_reference_to_an_unregistered_detector_id() {
        // The bug this guards against was caught by hand, not by a test
        // written in advance (`docs/domains/dataflow-cfg.md`
        // section): validation used to live in sting-core against the fixed
        // 8-code DefectClass taxonomy instead of here, against the
        // registry's actual detectors. "IFM" is a real TONScanner code
        // (`docs/domains/detectors.md` explicitly defers it past v1 —
        // structurally moot for Tolk, not merely deferred like `BR`/`PL`/`UR`
        // turned out to be, see) but is
        // not a registered detector — exactly the shape of typo/reference
        // this validation exists to catch.
        let cfg = StingConfig {
            detectors: DetectorsConfig {
                enable: vec![],
                disable: vec!["IFM".to_string()],
            },
            ..Default::default()
        };
        let err = DetectorRegistry::builtin()
            .filtered(&cfg)
            .expect_err("IFM is not a registered detector");
        assert_eq!(err.requested, "IFM");
        assert_eq!(
            err.known_ids,
            vec![
                "GVR", "LEP", "ID", "UBM", "TLB", "PL", "BR", "UR", "AC", "UTA", "SUA", "EPS",
                "UDA", "DEPR"
            ]
        );
    }

    #[test]
    fn detector_context_location_attaches_the_file_path() {
        let source = "fun f() { return; }";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let ctx = DetectorContext::new("contract.tolk".into(), tree, source.to_string());
        let range = sting_ast::SourceRange {
            start_byte: 0,
            end_byte: 10,
            start_line: 1,
            start_column: 1,
            end_line: 1,
            end_column: 11,
        };
        let location = ctx.location(range);
        assert_eq!(location.file, std::path::PathBuf::from("contract.tolk"));
        assert_eq!(location.start_line, 1);
    }

    #[test]
    fn detector_context_exposes_its_tree_and_source() {
        let source = "fun f() { return; }";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let ctx = DetectorContext::new("contract.tolk".into(), tree, source.to_string());
        assert_eq!(ctx.source(), source);
        assert_eq!(ctx.tree().root_node().kind(), "source_file");
    }

    #[test]
    fn detector_context_with_no_project_resolves_nothing() {
        let source = "fun f() { return; }";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let ctx = DetectorContext::new("contract.tolk".into(), tree, source.to_string());
        assert!(ctx.project_store().is_none());
        assert!(ctx.resolve_call("helperFn").is_none());
    }

    #[test]
    fn detector_context_with_project_resolves_a_cross_file_function() {
        use std::sync::Arc;

        use sting_tolkc::{FileEntry, FunctionEntry, ProjectStore, SourceSpan, SymbolTypes};

        let symbol_types = SymbolTypes {
            files: vec![
                FileEntry {
                    file_id: 0,
                    file_name: "/project/main.tolk".to_string(),
                    size_chars: 0,
                    imports: vec![1],
                },
                FileEntry {
                    file_id: 1,
                    file_name: "/project/helper.tolk".to_string(),
                    size_chars: 0,
                    imports: vec![],
                },
            ],
            unique_types: Vec::new(),
            struct_instantiations: Vec::new(),
            alias_instantiations: Vec::new(),
            declarations: Vec::new(),
            functions: vec![FunctionEntry {
                f_idx: 0,
                name: "helperFn".to_string(),
                return_ty_idx: 0,
                num_params: 0,
                ident_loc: SourceSpan::from([1, 5, 1, 5, 8]),
                end_loc: SourceSpan::from([1, 5, 1, 5, 8]),
                params: Vec::new(),
            }],
        };
        let store = Arc::new(ProjectStore::new(symbol_types));

        let source = "fun f() { return; }";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let ctx = DetectorContext::new("main.tolk".into(), tree, source.to_string())
            .with_project(Arc::clone(&store), 0);

        assert!(ctx.project_store().is_some());
        assert_eq!(ctx.file_id(), Some(0));
        let resolved = ctx.resolve_call("helperFn").expect("a project is attached");
        assert_eq!(resolved.len(), 1);
        assert!(
            ctx.resolve_call("doesNotExist")
                .expect("attached")
                .is_empty()
        );
    }

    #[test]
    fn detector_context_with_no_project_has_no_file_id() {
        let source = "fun f() { return; }";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let ctx = DetectorContext::new("contract.tolk".into(), tree, source.to_string());
        assert_eq!(ctx.file_id(), None);
    }
}
