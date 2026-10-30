//! `ExtractController` — the one entry point both `sting-abi-cli` and
//! `sting-abi-service` call through (`docs/domains/service.md`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sting_abi_core::{ContractSchema, ExtractError, Extractor, SourceLanguage};

/// How to pick which language's [`Extractor`] handles a given entrypoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageHint {
    /// Use exactly this language, no detection.
    Explicit(SourceLanguage),
    /// Detect from `entrypoint`'s file extension — `docs/abi-layer/
    /// blocks/02-normalized-ir-cli.md` §2.2: by extension only
    /// (`.tolk`/`.tact`/`.fc`/`.func`), deliberately never by sniffing
    /// file content — a wrong guess at the very first step of the whole
    /// pipeline is worse than a loud, immediate "I don't know" error.
    AutoDetect,
}

/// Error from [`ExtractController::extract`].
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// [`LanguageHint::AutoDetect`] couldn't map `entrypoint`'s extension
    /// to a known language.
    #[error(
        "could not determine the source language of {0} from its file extension \
         (expected .tolk, .tact, .fc, or .func) — pass the language explicitly instead of relying on auto-detection"
    )]
    UnknownLanguage(PathBuf),
    /// The detected/requested language has no [`Extractor`] registered in
    /// this controller.
    #[error("no extractor registered for {0:?}")]
    NoExtractorForLanguage(SourceLanguage),
    /// The extractor itself failed.
    #[error(transparent)]
    Extract(#[from] ExtractError),
}

/// Dispatches to the right [`Extractor`] by language — explicit or
/// auto-detected — testable without any real compiler
/// (`docs/API.md`: unit tests use a
/// mock `Extractor`, see this module's own `tests`).
pub struct ExtractController {
    extractors: HashMap<SourceLanguage, Box<dyn Extractor>>,
}

impl ExtractController {
    /// Builds a controller dispatching to exactly the given extractors —
    /// a language with no entry here fails with
    /// [`PipelineError::NoExtractorForLanguage`], not a panic.
    #[must_use]
    pub fn new(extractors: HashMap<SourceLanguage, Box<dyn Extractor>>) -> Self {
        Self { extractors }
    }

    /// Extracts a [`ContractSchema`] from `entrypoint` (relative to
    /// `project_dir`, matching every [`Extractor`]'s own convention).
    ///
    /// # Errors
    /// See [`PipelineError`]'s variants.
    pub fn extract(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
        lang: LanguageHint,
    ) -> Result<ContractSchema, PipelineError> {
        let language = match lang {
            LanguageHint::Explicit(language) => language,
            LanguageHint::AutoDetect => detect_language(entrypoint)?,
        };
        let extractor = self
            .extractors
            .get(&language)
            .ok_or(PipelineError::NoExtractorForLanguage(language))?;
        Ok(extractor.extract(project_dir, entrypoint)?)
    }
}

fn detect_language(entrypoint: &Path) -> Result<SourceLanguage, PipelineError> {
    match entrypoint.extension().and_then(|ext| ext.to_str()) {
        Some("tolk") => Ok(SourceLanguage::Tolk),
        Some("tact") => Ok(SourceLanguage::Tact),
        Some("fc" | "func") => Ok(SourceLanguage::Func),
        _ => Err(PipelineError::UnknownLanguage(entrypoint.to_path_buf())),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use sting_abi_core::{ContractMetadata, ExtractionTier};

    use super::{
        ContractSchema, ExtractController, ExtractError, Extractor, LanguageHint, PathBuf,
        PipelineError, SourceLanguage, detect_language,
    };
    use std::collections::HashMap;
    use std::path::Path;

    /// A preset-response `Extractor` — no real compiler involved, per
    /// `docs/API.md` Records the
    /// `(project_dir, entrypoint)` it was actually called with (via
    /// `RefCell` — `Extractor::extract` takes `&self`, not `&mut self`) so
    /// tests can assert the controller passes both through unchanged, not
    /// just that *some* mock got invoked — a bug found by review: the
    /// original version of this mock ignored both parameters, so a
    /// regression swapping them at the call site would have gone
    /// undetected by every test in this file.
    // Arc<Mutex<...>>, not Rc<RefCell<...>>: `Extractor: Send + Sync`
    // (`sting-abi-core`'s trait bound, needed for a future async HTTP
    // service to hold `Box<dyn Extractor>` across await points) — a mock
    // implementing the same trait must satisfy the same bound.
    type CallLog = std::sync::Arc<std::sync::Mutex<Option<(PathBuf, PathBuf)>>>;

    struct MockExtractor {
        language: SourceLanguage,
        result: fn() -> Result<ContractSchema, ExtractError>,
        called_with: CallLog,
    }

    impl MockExtractor {
        /// Returns the mock plus a shared handle to what it was called
        /// with, so a test can inspect it after the mock has been boxed
        /// into the controller (which takes ownership).
        fn new_with_handle(
            language: SourceLanguage,
            result: fn() -> Result<ContractSchema, ExtractError>,
        ) -> (Self, CallLog) {
            let called_with: CallLog = std::sync::Arc::new(std::sync::Mutex::new(None));
            (
                Self {
                    language,
                    result,
                    called_with: called_with.clone(),
                },
                called_with,
            )
        }
    }

    impl Extractor for MockExtractor {
        fn language(&self) -> SourceLanguage {
            self.language
        }

        fn extract(
            &self,
            project_dir: &Path,
            entrypoint: &Path,
        ) -> Result<ContractSchema, ExtractError> {
            *self
                .called_with
                .lock()
                .expect("test-only mutex, never poisoned") =
                Some((project_dir.to_path_buf(), entrypoint.to_path_buf()));
            (self.result)()
        }
    }

    fn minimal_schema(language: SourceLanguage) -> ContractSchema {
        ContractSchema {
            ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
            source_language: language,
            extraction_tier: ExtractionTier::Compiled,
            contract_name: Some("Mock".to_string()),
            metadata: ContractMetadata::default(),
            storage: None,
            types: Vec::new(),
            incoming_internal: Vec::new(),
            incoming_external: Vec::new(),
            outgoing: Vec::new(),
            get_methods: Vec::new(),
            errors: Vec::new(),
            confidence: None,
            field_confidence: None,
        }
    }

    fn controller_with_tolk_mock() -> ExtractController {
        let mut extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
        let (mock, _call_log) = MockExtractor::new_with_handle(SourceLanguage::Tolk, || {
            Ok(minimal_schema(SourceLanguage::Tolk))
        });
        extractors.insert(SourceLanguage::Tolk, Box::new(mock));
        ExtractController::new(extractors)
    }

    #[test]
    fn detects_tolk_tact_and_func_by_extension() {
        assert_eq!(
            detect_language(Path::new("a.tolk")).unwrap(),
            SourceLanguage::Tolk
        );
        assert_eq!(
            detect_language(Path::new("a.tact")).unwrap(),
            SourceLanguage::Tact
        );
        assert_eq!(
            detect_language(Path::new("a.fc")).unwrap(),
            SourceLanguage::Func
        );
        assert_eq!(
            detect_language(Path::new("a.func")).unwrap(),
            SourceLanguage::Func
        );
    }

    #[test]
    fn unknown_extension_is_a_named_error_not_a_silent_default() {
        let result = detect_language(Path::new("a.txt"));
        assert!(matches!(result, Err(PipelineError::UnknownLanguage(_))));
    }

    #[test]
    fn no_extension_is_also_an_unknown_language_error() {
        let result = detect_language(Path::new("a"));
        assert!(matches!(result, Err(PipelineError::UnknownLanguage(_))));
    }

    #[test]
    fn auto_detect_dispatches_to_the_matching_mock_extractor() {
        let controller = controller_with_tolk_mock();
        let schema = controller
            .extract(
                Path::new("."),
                Path::new("a.tolk"),
                LanguageHint::AutoDetect,
            )
            .expect("Tolk extractor is registered");
        assert_eq!(schema.source_language, SourceLanguage::Tolk);
    }

    #[test]
    fn explicit_hint_bypasses_extension_based_detection() {
        // A .tact-named file with an explicit Tolk hint: the flag beats the
        // extension, which is the rule `docs/API.md` states.
        let controller = controller_with_tolk_mock();
        let schema = controller
            .extract(
                Path::new("."),
                Path::new("a.tact"),
                LanguageHint::Explicit(SourceLanguage::Tolk),
            )
            .expect("explicit hint must route to the Tolk mock regardless of extension");
        assert_eq!(schema.source_language, SourceLanguage::Tolk);
    }

    #[test]
    fn missing_extractor_for_a_known_language_is_a_named_error() {
        let controller = controller_with_tolk_mock();
        let result = controller.extract(
            Path::new("."),
            Path::new("a.tact"),
            LanguageHint::AutoDetect,
        );
        assert!(matches!(
            result,
            Err(PipelineError::NoExtractorForLanguage(SourceLanguage::Tact))
        ));
    }

    #[test]
    fn extractor_failure_propagates_as_pipeline_error() {
        let mut extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
        let (mock, _call_log) = MockExtractor::new_with_handle(SourceLanguage::Tolk, || {
            Err(ExtractError::Failed {
                entrypoint: PathBuf::from("a.tolk"),
                message: "synthetic failure".to_string(),
                source: None,
            })
        });
        extractors.insert(SourceLanguage::Tolk, Box::new(mock));
        let controller = ExtractController::new(extractors);
        let result = controller.extract(
            Path::new("."),
            Path::new("a.tolk"),
            LanguageHint::AutoDetect,
        );
        assert!(matches!(result, Err(PipelineError::Extract(_))));
    }

    #[test]
    fn project_dir_and_entrypoint_reach_the_extractor_unchanged() {
        // Real coverage gap found by review: the original MockExtractor
        // ignored both arguments, so nothing here would have caught a
        // regression that swapped project_dir/entrypoint at the call site.
        let mut extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
        let (mock, call_log) = MockExtractor::new_with_handle(SourceLanguage::Tolk, || {
            Ok(minimal_schema(SourceLanguage::Tolk))
        });
        extractors.insert(SourceLanguage::Tolk, Box::new(mock));
        let controller = ExtractController::new(extractors);

        controller
            .extract(
                Path::new("/some/project"),
                Path::new("Contract.tolk"),
                LanguageHint::AutoDetect,
            )
            .expect("Tolk extractor is registered");

        let (project_dir, entrypoint) = call_log
            .lock()
            .expect("test-only mutex, never poisoned")
            .clone()
            .expect("extract must have been called");
        assert_eq!(project_dir, PathBuf::from("/some/project"));
        assert_eq!(entrypoint, PathBuf::from("Contract.tolk"));
    }
}
