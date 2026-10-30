//! Orchestration controllers tying static analysis, emulation, and
//! reporting together (`docs/ARCHITECTURE.md`).
//!
//! [`LintController`] and [`ReportController`] run the static pass and render
//! it; [`CorpusController`] resolves a target to bytecode;
//! [`EmulationPoolController`] runs the machine, Tier 1 only — see that type's
//! own doc comment; [`BridgeController`] and [`ScoringController`] close the
//! static-to-dynamic loop; [`FixController`] applies `Finding::suggestion`s
//! back to disk for `sting lint --fix` (`docs/ARCHITECTURE.md`).
//!
//! [`CorpusController`] no longer *lives* here: it orchestrates only
//! `sting-corpus`'s own pieces and knows nothing about detectors or
//! reporting, so keeping it here forced four abi-layer crates to depend on
//! the whole security contour to load bytecode by address. It is re-exported
//! below so `sting-cli`/`sting-bench` keep importing it from here.

mod controllers;

pub use sting_corpus::{CorpusController, CorpusError, FetchOptions};

pub use controllers::bridge::{
    BridgeController, BuildScenarioError, Hypothesis, ScenarioField, build_scenario_body,
    decode_stack_ints,
};
pub use controllers::doctor::{ComponentVersion, DoctorController, DoctorReport, VersionState};
pub use controllers::fix::{FixController, FixOutcome};
pub use controllers::hypothesis_spec::{HypothesisSpec, HypothesisSpecError};
pub use controllers::lint::{
    DetectorsWithoutARule, LintController, LintFailure, LintFileError, LintOutcome, UnparsedFile,
    language_of, resolve_tolk_files,
};
pub use controllers::pool::{
    EmulationError, EmulationPoolController, EmulationTarget, EmulatorProvider, GetMethodOutcome,
    SendOutcome,
};
pub use controllers::report::{ReportController, ReportFormat};
pub use controllers::scoring::{
    DetectorStatsSnapshot, ScoringController, WEIGHT_DETECTOR_PRECISION, WEIGHT_DYNAMIC_RESULT,
    WEIGHT_SEVERITY, dynamic_result_weight, severity_weight,
};

/// Aggregated error type for pipeline consumers that want one type at the
/// boundary (`sting-cli`, `sting-bench`) instead of matching on individual
/// controllers' own error enums (`docs/ARCHITECTURE.md`
/// principle 3).
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// A report could not be rendered.
    #[error(transparent)]
    Report(#[from] sting_report::ReportError),
    /// A contract source could not be resolved.
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    /// A plain JSON report could not be serialized
    /// (`docs/API.md`'s `ReportFormat::Json`).
    #[error("failed to serialize JSON report: {0}")]
    Json(#[from] serde_json::Error),
}
