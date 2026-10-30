//! Shared core types for sting: the vocabulary every other crate in the
//! workspace builds on (see `docs/ARCHITECTURE.md` §1.1 — this crate has zero
//! internal dependencies by design).

use std::path::PathBuf;

/// TONScanner-derived taxonomy of Tolk security defect classes, plus sting's
/// own additions past that paper. Four of the original eight `TONScanner`
/// classes have detectors (`GVR`, `LEP`, `ID`, `UBM`) — see
/// `docs/domains/detectors.md` for why the other four are deferred rather
/// than abandoned. `#[non_exhaustive]`: external matchers (if this crate is ever
/// consumed outside the workspace) must not break when a 9th class is added —
/// exercised for real by [`Self::Tlb`] :
/// not from the `TONScanner` paper at all, sting's own pattern found by the
/// same "run on real corpus, audit findings by hand" methodology that
/// produced three real detector fixes earlier in that same review. See
/// [`Self::Tlb`]'s own doc comment for why it is a distinct class from
/// [`Self::Id`] rather than a broadening of it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefectClass {
    /// Bad Randomness.
    Br,
    /// Precision Loss.
    Pl,
    /// Unchecked Return.
    Ur,
    /// Global Var Redefined (local shadows storage).
    Gvr,
    /// Improper Function Modifier.
    Ifm,
    /// Unchecked Bounced Message.
    Ubm,
    /// Inconsistent Data (storage read/write order mismatch).
    Id,
    /// Lack End Parse.
    Lep,
    /// TL-B (De)Serialization Mismatch: a manual storage write and its
    /// matching read disagree on a field's TL-B shape (bit-width,
    /// signedness, or primitive — e.g. `storeUint(x, 32)` written back with
    /// `loadUint(64)`, or `storeCoins(x)` read back with `loadUint(64)`) —
    /// sting's own addition, not a `TONScanner` code. Distinct from
    /// [`Self::Id`] (which only ever compares *order*, never the method name
    /// or its arguments — see `id.rs`'s `KeyedEvent`, whose comparison key is
    /// deliberately just the field's value/bound-name text) rather than a
    /// broadening of it, for two reasons: (1) the two failure modes have
    /// different root causes and different fixes (reorder fields vs. fix a
    /// width/type), so a shared rule id would blur an otherwise-actionable
    /// distinction; (2) broadening `ID` in place would silently perturb its
    /// own already-published, cross-referenced precision figures
    /// (`corpus/baseline.json`, `docs/domains/detectors.md`) instead of
    /// starting a fresh, honestly-labeled measurement.
    Tlb,
    /// Access Control: an `onInternalMessage` dispatch path reaches a
    /// storage-mutating `.save()` with no recognized sender/payment/signature
    /// check on that path — sting's own addition, not a `TONScanner` code
    /// (the same corpus-audit
    /// methodology that produced `Tlb`). Explicitly narrower and
    /// lower-confidence than every other detector in this taxonomy:
    /// real-corpus research found no reliable static signal distinguishing
    /// "genuinely missing check" from "intentionally public, spam has no
    /// cost" (`Counter.tolk`'s real shape) — see `ac.rs`'s own doc comment
    /// for the full honest scope.
    Ac,
    /// Unchecked Transfer Amount: a function debits one tracked quantity and
    /// credits a different one by the same amount, but only one side is
    /// validated against going negative — exploitable when that amount is
    /// read via a signed slice load (`.loadInt(...)`), letting a negative
    /// value invert the transfer and mint value from nothing. Sting's own
    /// addition , found on real,
    /// external, intentionally-vulnerable code (`hacktheton.com`'s
    /// `token_level`), not this project's own corpus — see `uta.rs`'s own
    /// doc comment for the full, deliberately narrow v1 scope.
    Uta,
    /// Suspicious Unary Assignment: `x =+ 1`/`x =- 1`, the classic
    /// C-family lexical typo for `x += 1`/`x -= 1` — in a balance/quantity
    /// context this silently replaces the tracked value instead of
    /// adjusting it. Sting's own addition, ported from Misti's
    /// `SuspiciousUnary` (Misti
    /// prior-art teardown) — see `sua.rs`'s own doc comment.
    Sua,
}

impl DefectClass {
    /// The short code used both as the `Detector` id and as the SARIF
    /// `ruleId` suffix (`sting/<CODE>`, see `docs/ARCHITECTURE.md` §6).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Br => "BR",
            Self::Pl => "PL",
            Self::Ur => "UR",
            Self::Gvr => "GVR",
            Self::Ifm => "IFM",
            Self::Ubm => "UBM",
            Self::Id => "ID",
            Self::Lep => "LEP",
            Self::Tlb => "TLB",
            Self::Ac => "AC",
            Self::Uta => "UTA",
            Self::Sua => "SUA",
        }
    }

    /// Parses a code back into a [`DefectClass`], case-sensitively (codes are
    /// always upper-case by convention). Used to validate `Sting.toml`'s
    /// `detectors.enable`/`disable` lists against real detector codes instead
    /// of accepting arbitrary strings silently.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "BR" => Self::Br,
            "PL" => Self::Pl,
            "UR" => Self::Ur,
            "GVR" => Self::Gvr,
            "IFM" => Self::Ifm,
            "UBM" => Self::Ubm,
            "ID" => Self::Id,
            "LEP" => Self::Lep,
            "TLB" => Self::Tlb,
            "AC" => Self::Ac,
            "UTA" => Self::Uta,
            "SUA" => Self::Sua,
            _ => return None,
        })
    }

    /// The original eight `TONScanner` codes in the paper's own order, plus
    /// every class sting has added past that paper afterward, in the order
    /// added. Used to build "unknown code, valid codes are: ..." error
    /// messages.
    pub const ALL: [Self; 12] = [
        Self::Br,
        Self::Pl,
        Self::Ur,
        Self::Gvr,
        Self::Ifm,
        Self::Ubm,
        Self::Id,
        Self::Lep,
        Self::Tlb,
        Self::Ac,
        Self::Uta,
        Self::Sua,
    ];
}

/// How serious a [`Finding`] is. Ordered ascending (`Info < ... < Critical`)
/// so findings can be sorted/filtered by severity directly — the ordering
/// is exactly what `Sting.toml`'s `min_severity` filters findings against.
/// `rename_all = "lowercase"` on [`serde::Deserialize`] matches the CLI
/// flag's own spelling (`-m/--min-severity <info|low|medium|high|critical>`)
/// and Misti's own convention for the same concept — it also changes this
/// type's `Serialize` output from `"High"` to `"high"` (checked: nothing in
/// this workspace asserts the old casing anywhere, the `json` report format
/// is this enum's only `Serialize` consumer).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational; not a defect by itself.
    Info,
    /// Low impact.
    Low,
    /// Medium impact.
    Medium,
    /// High impact.
    High,
    /// Critical impact (e.g. direct fund loss).
    Critical,
}

/// A [`Finding`]'s confidence score, constrained to `[0.0, 1.0]` by
/// construction: a confidence outside that range cannot be built at all
/// (`docs/TESTING.md`). Not populated until `ScoringController` runs
/// (`docs/domains/emulator.md`); `Finding.confidence` stays
/// `Option<Confidence>` until then.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Confidence(f32);

impl Confidence {
    /// # Errors
    /// Returns [`ConfidenceRangeError`] if `value` is not a finite number in
    /// `[0.0, 1.0]`.
    pub fn new(value: f32) -> Result<Self, ConfidenceRangeError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ConfidenceRangeError(value))
        }
    }

    /// Builds a `Confidence` by clamping `value` into `[0.0, 1.0]` (`NaN`
    /// becomes `0.0`) instead of rejecting an out-of-range input — for
    /// callers computing a value that is bounded *by construction*
    /// (`ScoringController`'s weighted sum, `docs/domains/emulator.md`
    /// §7.3, where every term is itself already in range and the weights
    /// sum to `1.0`) but still want defensive robustness against
    /// floating-point edge cases without a `Result` neither caller nor
    /// callee expects to ever actually see used.
    #[must_use]
    pub fn saturating(value: f32) -> Self {
        if value.is_nan() {
            Self(0.0)
        } else {
            Self(value.clamp(0.0, 1.0))
        }
    }

    /// The underlying score.
    #[must_use]
    pub fn value(self) -> f32 {
        self.0
    }
}

/// Error from [`Confidence::new`].
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("confidence {0} is outside the valid range [0.0, 1.0]")]
pub struct ConfidenceRangeError(f32);

/// The result of `BridgeController` trying to dynamically confirm a
/// [`Finding`] (`docs/domains/emulator.md`). Lives in
/// `sting-core` alongside [`Confidence`] rather than in `sting-pipeline`
/// (where `BridgeController` itself lives) because it is attached directly
/// to a `Finding` and consumed by `sting-report` — core vocabulary, not
/// orchestration-internal state, exactly the same reasoning that already
/// placed `Confidence` here before `ScoringController` existed.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DynamicVerification {
    /// The hypothesis was reproduced: the emulator observed exactly the
    /// unsafe behavior the static finding predicted.
    Confirmed(ProofOfConcept),
    /// The static pattern matched, but the emulator observed safe behavior
    /// — a legitimate, useful result, not an error
    /// (`docs/domains/emulator.md`). Does not delete the
    /// finding; `sting-report` lowers its rendered severity instead (§7.3).
    Refuted,
    /// The hypothesis could not be tested either way (e.g. no on-chain
    /// deployment was available, or the specific scenario needed is outside
    /// what `BridgeController`'s strategy for this `DefectClass` can reach
    /// — the confirmed example is that `in.senderAddress`-gated findings are
    /// not reachable through `TvmEmulator::send_internal_message` at all,
    /// `docs/domains/emulator.md` §6).
    Inconclusive(String),
}

/// A reproducible artifact backing a [`DynamicVerification::Confirmed`]
/// result — a proof is kept as a reproducible artefact rather than as a
/// claim (`docs/domains/emulator.md` §6).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProofOfConcept {
    /// What the static finding predicted would happen.
    pub expected: String,
    /// What the emulator actually observed.
    pub actual: String,
    /// Human-readable steps taken to reproduce this (e.g. the message(s)
    /// sent, in order) — enough for a human to redo the same scenario by
    /// hand, not a serialized replay format.
    pub steps: Vec<String>,
}

/// Where in the source a [`Finding`] points to. 1-indexed lines/columns,
/// matching SARIF's `region` convention (`docs/ARCHITECTURE.md` §6) — the
/// conversion from tree-sitter's 0-indexed `Point`s happens once, in
/// `sting-ast`'s `SourceRange::from_node`, not repeated at every call site.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Location {
    /// Path to the source file, as given to the detector run (not
    /// necessarily canonicalized).
    pub file: PathBuf,
    /// First line of the finding, 1-indexed.
    pub start_line: u32,
    /// First column of the finding, 1-indexed.
    pub start_column: u32,
    /// Last line of the finding, 1-indexed.
    pub end_line: u32,
    /// Last column of the finding, 1-indexed.
    pub end_column: u32,
}

/// A `sting-detectors::Detector`'s stable identifier (not an intra-doc link:
/// `sting-core` has no dependency on `sting-detectors`, by design — see
/// `docs/ARCHITECTURE.md` §1.1's dependency graph).
///
/// Deliberately **not** [`DefectClass`]: Misti (`docs/GLOSSARY.md#prior-art`),
/// whose architecture this workspace explicitly follows, does not constrain
/// its detectors to a fixed taxonomy at the type level either — any class
/// extending its `Detector` base is a valid detector, named however its
/// author likes. The "8 `TONScanner` classes" are *this project's chosen
/// initial scope* (`docs/domains/detectors.md`), not a ceiling on what a
/// `Detector` can ever be: the framework-validation demo detector — a
/// "function too long" toy check, explicitly *not* a security defect — has no
/// legitimate `DefectClass` to claim, which is exactly the case this type
/// exists to allow. For the detectors that *do* map onto the taxonomy, the id
/// is exactly
/// [`DefectClass::code`] by convention, recoverable via
/// [`DefectClass::from_code`].
pub type DetectorId = &'static str;

/// One security finding, or a toy one from the demo detector.
///
/// No separate `id` field: unlike Misti's findings, which are looked up by an
/// opaque id, `docs/domains/precision-sources.md`'s benchmark harness matches
/// findings against ground truth by `(file, detector_id, line_range)` — that
/// tuple already uniquely identifies a finding in practice, so a redundant
/// arbitrary id would just be one more thing that could drift out of sync
/// with the fields that actually matter. [`Finding::stable_id`] derives one
/// on demand for callers that want a dedup/tracking key (e.g. SARIF
/// `partialFingerprints`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Finding {
    /// Which detector produced this finding — also the SARIF `ruleId`
    /// suffix (`sting/<detector_id>`, `docs/ARCHITECTURE.md` §6).
    pub detector_id: DetectorId,
    /// Severity, before any dynamic-verification adjustment
    /// (`docs/domains/emulator.md`).
    pub severity: Severity,
    /// Human-readable description of what was found, specific to the actual
    /// code (not a generic "GVR violation" — see
    /// `docs/domains/detectors.md`'s SARIF section).
    pub message: String,
    /// Actionable, specific guidance on how to fix this finding — separate
    /// from `message` ("what's wrong") on purpose: `message` explains the
    /// defect, `remediation` tells a developer what to actually change,
    /// mirroring `rustc`/`clippy`'s `help:` line and mapped to SARIF's own
    /// purpose-built channels for this (`docs/ARCHITECTURE.md` §6) — every
    /// detector must supply one, not an afterthought bolted on for some.
    pub remediation: String,
    /// Where in the source this finding points to.
    pub location: Location,
    /// Set only after `BridgeController`/`ScoringController` run
    /// (`docs/domains/emulator.md`); `None` for a purely
    /// static finding.
    pub confidence: Option<Confidence>,
    /// Set only after `BridgeController` runs; `None` if the bridge was
    /// never attempted for this finding (e.g. `sting lint` with no
    /// on-chain deployment available — the common case, since the bridge
    /// needs a second artifact `sting lint` alone never has, see
    /// `docs/domains/emulator.md`).
    pub dynamic_verification: Option<DynamicVerification>,
    /// A machine-applicable fix, present only when one exists.
    /// **Deliberately populated by only two detectors, `DEPR`/`SUA`** — a
    /// finding whose "fix" requires human judgment about intended behavior
    /// (which check to add, which variable is the right one, whether a
    /// shadow was intentional) must never carry one: a plausible-looking
    /// but wrong inserted check is worse than no finding at all, a false
    /// sense of the problem being solved. `DEPR`/`SUA` are the only two
    /// detectors in this taxonomy whose finding has a single, deterministic,
    /// unambiguous correct replacement — the same "machine-applicable"
    /// boundary `cargo fix`/`clippy --fix` already draw, not a new
    /// invention. See [`Suggestion`]'s own doc comment.
    pub suggestion: Option<Suggestion>,
}

/// A single, deterministic, machine-applicable source edit — see
/// [`Finding::suggestion`]'s own doc comment for which findings may carry
/// one and why most never do.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Suggestion {
    /// Byte offset where the replacement starts (0-indexed, `tree-sitter`
    /// convention — matches `sting_ast::SourceRange::start_byte` in the
    /// crate that actually produces these, `sting-detectors`, which this
    /// crate cannot depend on — see `docs/ARCHITECTURE.md` §1.1's
    /// dependency graph).
    pub start_byte: usize,
    /// Byte offset where the replacement ends (exclusive).
    pub end_byte: usize,
    /// The exact text to put in place of `start_byte..end_byte`.
    pub replacement: String,
}

impl Finding {
    /// This finding's [`DefectClass`], if `detector_id` happens to be one of
    /// the 8 `TONScanner` codes — `None` for framework-validation/non-taxonomy
    /// detectors (see [`DetectorId`]'s doc comment).
    #[must_use]
    pub fn defect_class(&self) -> Option<DefectClass> {
        DefectClass::from_code(self.detector_id)
    }

    /// A dedup/tracking key derived from the fields that identify *this*
    /// finding, not a separately-tracked arbitrary id (see the struct-level
    /// doc comment for why).
    #[must_use]
    pub fn stable_id(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.detector_id,
            self.location.file.display(),
            self.location.start_line,
            self.location.start_column
        )
    }
}

/// Parsed `Sting.toml` (see `docs/ARCHITECTURE.md` §5). `[corpus]`/`[report]`
/// are real fields in the target schema and still have no consumer, so adding
/// them now would be untested surface with no way to verify it is right.
/// `[suppressions]` is implemented — the best value against its cost of
/// everything compared against Misti — and its consumer
/// is `sting-pipeline`'s `LintController`, not this crate, which only
/// parses the structural shape (same division of responsibility as
/// `detectors.enable`/`disable`, see [`StingConfig::parse`]'s doc comment).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct StingConfig {
    /// Which detectors run.
    #[serde(default)]
    pub detectors: DetectorsConfig,
    /// Structural (config-file) finding suppressions.
    #[serde(default)]
    pub suppressions: SuppressionsConfig,
    /// Global severity floor — findings below this are dropped before
    /// output. `None` means no floor (report everything), matching an
    /// absent `min_severity` key entirely, not a default value that would
    /// need documenting separately.
    #[serde(default)]
    pub min_severity: Option<Severity>,
}

/// The `[detectors]` section of `Sting.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct DetectorsConfig {
    /// Detector codes to explicitly enable. Empty means "use the built-in
    /// default set" (defined by whoever constructs `DetectorRegistry`, not by
    /// this config type).
    #[serde(default)]
    pub enable: Vec<String>,
    /// Detector codes to explicitly disable.
    #[serde(default)]
    pub disable: Vec<String>,
}

/// The `[suppressions]` section of `Sting.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct SuppressionsConfig {
    /// `[[suppressions.rule]]` entries — TOML's array-of-tables syntax
    /// under the key `rule` (singular, TOML's own convention for the
    /// per-entry table name in `[[a.b]]`), hence the `rename` here despite
    /// this field itself holding a `Vec`.
    #[serde(default, rename = "rule")]
    pub rules: Vec<SuppressionRule>,
}

/// One `[[suppressions.rule]]` entry: silence a specific detector's finding
/// at one exact location. All four fields are required, deliberately —
/// including `reason`, which Misti's own equivalent config-based mechanism
/// does not require: an unexplained suppression is exactly the kind of stale,
/// unaccountable
/// exception this mechanism should make harder to write, not easier.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct SuppressionRule {
    /// The detector id this rule silences (matches [`Finding::detector_id`]).
    pub detector: String,
    /// The file this rule applies to, exactly as it appears in
    /// [`Location::file`] — matched by equality, not by path
    /// canonicalization (see the consumer in `sting-pipeline` for why).
    pub file: std::path::PathBuf,
    /// The 1-indexed line this rule applies to, matched against
    /// [`Location::start_line`].
    pub line: u32,
    /// Why this finding is suppressed — mandatory, see this struct's own
    /// doc comment.
    pub reason: String,
}

impl StingConfig {
    /// Parses a `Sting.toml` document from its source text.
    ///
    /// This is **structural** parsing only: it does not check that
    /// `detectors.enable`/`disable` name real detector ids. It cannot —
    /// `sting-core` has no dependency on `sting-detectors` (`docs/ARCHITECTURE.md`
    /// §1.1) and so has no way to know the universe of valid ids, which is
    /// not even fixed to [`DefectClass`]'s 8 codes (see [`DetectorId`]'s doc
    /// comment: a framework-validation detector such as `DEMO-LONG-FN` is a
    /// legitimate id that is not one of them — this
    /// crate rejecting it as "unknown" was a real bug, caught by actually
    /// running `sting config dump --config` against it, not by any test
    /// written before that). That validation belongs where the actual set of
    /// registered detectors is known: `sting_detectors::DetectorRegistry::filtered`.
    ///
    /// # Errors
    /// Returns [`ConfigError::Toml`] on malformed TOML.
    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(source)?)
    }
}

/// Errors from [`StingConfig::parse`].
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The document is not valid TOML, or not shaped like a `Sting.toml`.
    #[error("failed to parse Sting.toml: {0}")]
    Toml(#[from] toml::de::Error),
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use super::{Confidence, DefectClass, Finding, Location, Severity, StingConfig};

    #[test]
    fn defect_class_code_roundtrips_through_from_code() {
        for class in DefectClass::ALL {
            assert_eq!(DefectClass::from_code(class.code()), Some(class));
        }
    }

    fn dummy_location() -> Location {
        Location {
            file: PathBuf::from("contract.tolk"),
            start_line: 3,
            start_column: 5,
            end_line: 3,
            end_column: 9,
        }
    }

    #[test]
    fn finding_defect_class_resolves_for_taxonomy_detector_ids() {
        let finding = Finding {
            detector_id: "GVR",
            severity: Severity::High,
            message: "shadowing".to_string(),
            remediation: "rename it".to_string(),
            location: dummy_location(),
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        };
        assert_eq!(finding.defect_class(), Some(DefectClass::Gvr));
    }

    #[test]
    fn finding_defect_class_is_none_for_a_non_taxonomy_detector_id() {
        // Exactly the demo detector's situation: a real Detector::id() that
        // is not one of the eight `TONScanner` codes.
        let finding = Finding {
            detector_id: "DEMO-LONG-FN",
            severity: Severity::Info,
            message: "function is long".to_string(),
            remediation: "split it up".to_string(),
            location: dummy_location(),
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        };
        assert_eq!(finding.defect_class(), None);
    }

    #[test]
    fn stable_id_encodes_detector_file_and_start_position() {
        let finding = Finding {
            detector_id: "LEP",
            severity: Severity::Medium,
            message: "msg".to_string(),
            remediation: "fix".to_string(),
            location: dummy_location(),
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        };
        assert_eq!(
            finding.stable_id(),
            format!("LEP:{}:3:5", PathBuf::from("contract.tolk").display())
        );
    }

    #[test]
    fn from_code_rejects_unknown_strings() {
        assert_eq!(DefectClass::from_code("GRV"), None);
        assert_eq!(DefectClass::from_code(""), None);
    }

    #[test]
    fn confidence_rejects_out_of_range_values() {
        assert!(Confidence::new(-0.1).is_err());
        assert!(Confidence::new(1.1).is_err());
        assert!(Confidence::new(f32::NAN).is_err());
        assert!(Confidence::new(0.0).is_ok());
        assert!(Confidence::new(1.0).is_ok());
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "clamp() produces the exact literal bounds/pass-through values asserted here, not a computed sum"
    )]
    fn confidence_saturating_clamps_instead_of_rejecting() {
        assert_eq!(Confidence::saturating(-0.1).value(), 0.0);
        assert_eq!(Confidence::saturating(1.1).value(), 1.0);
        assert_eq!(Confidence::saturating(f32::NAN).value(), 0.0);
        assert_eq!(Confidence::saturating(0.5).value(), 0.5);
    }

    #[test]
    fn parses_a_minimal_valid_config() {
        let config = StingConfig::parse(
            r#"
            [detectors]
            enable = ["GVR", "LEP"]
            disable = ["ID"]
            "#,
        )
        .expect("valid config");
        assert_eq!(config.detectors.enable, vec!["GVR", "LEP"]);
        assert_eq!(config.detectors.disable, vec!["ID"]);
    }

    #[test]
    fn empty_document_parses_to_defaults() {
        let config = StingConfig::parse("").expect("empty document is valid");
        assert_eq!(config, StingConfig::default());
    }

    #[test]
    fn parses_suppression_rules_from_the_documented_toml_shape() {
        let config = StingConfig::parse(
            r#"
            [[suppressions.rule]]
            detector = "LEP"
            file = "contracts/legacy.tolk"
            line = 42
            reason = "wrapped by an external library, end_parse is called by the caller"
            "#,
        )
        .expect("valid config");
        assert_eq!(config.suppressions.rules.len(), 1);
        let rule = &config.suppressions.rules[0];
        assert_eq!(rule.detector, "LEP");
        assert_eq!(rule.file, PathBuf::from("contracts/legacy.tolk"));
        assert_eq!(rule.line, 42);
        assert_eq!(
            rule.reason,
            "wrapped by an external library, end_parse is called by the caller"
        );
    }

    #[test]
    fn parses_min_severity_from_the_lowercase_toml_spelling() {
        let config = StingConfig::parse("min_severity = \"high\"").expect("valid config");
        assert_eq!(config.min_severity, Some(Severity::High));
    }

    #[test]
    fn absent_min_severity_parses_to_none_not_a_default_floor() {
        let config = StingConfig::parse("").expect("empty document is valid");
        assert_eq!(config.min_severity, None);
    }

    #[test]
    fn suppression_rule_without_a_reason_fails_to_parse() {
        // The mandatory `reason` field is the whole point (see
        // SuppressionRule's own doc comment) — a rule silently accepted
        // without one would defeat it.
        let result = StingConfig::parse(
            r#"
            [[suppressions.rule]]
            detector = "LEP"
            file = "contracts/legacy.tolk"
            line = 42
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn parse_does_not_validate_detector_codes_structurally() {
        // sting-core cannot know the real universe of valid detector ids
        // (see StingConfig::parse's doc comment) — both an unrecognized
        // taxonomy-shaped typo ("GRV") and a real non-taxonomy id
        // ("DEMO-LONG-FN", the demo detector) must parse
        // successfully here. Semantic validation against the *actual*
        // registered detectors is sting-detectors::DetectorRegistry::filtered's
        // job, not this crate's — see that function's tests.
        let config = StingConfig::parse(
            r#"
            [detectors]
            enable = ["GRV", "DEMO-LONG-FN"]
            "#,
        )
        .expect("structurally valid TOML must parse regardless of code semantics");
        assert_eq!(config.detectors.enable, vec!["GRV", "DEMO-LONG-FN"]);
    }

    #[test]
    fn rejects_malformed_toml_with_a_clear_error_not_a_panic() {
        let result = StingConfig::parse("this is not [ valid toml");
        assert!(result.is_err());
    }
}
