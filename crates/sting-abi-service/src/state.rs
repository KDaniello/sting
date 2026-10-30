//! Shared state every handler receives.

use std::collections::HashMap;
use std::sync::Arc;

use sting_abi_core::SourceLanguage;
use sting_abi_coverage::MessageReader;
use sting_abi_dictionary::{MethodDictionary, OperationDictionary};
use sting_abi_pipeline::ExtractController;
use sting_abi_registry::{BytecodeSource, VerifiedSchemaStore};
use sting_corpus::CorpusController;
use sting_emulator_sys::LibEmulator;

use crate::config::ServiceConfig;
use crate::rate_limit::RateLimiter;

/// The same `ExtractController` `sting-abi-cli` drives
/// (`docs/domains/service.md` principle 5: this
/// crate is the only one that knows about HTTP — `ExtractController` itself
/// stays HTTP-ignorant, no `axum` types leak into `sting-abi-pipeline`).
#[derive(Clone)]
pub struct AppState {
    /// Dispatches extraction requests to the right language's extractor.
    pub controller: Arc<ExtractController>,
    /// Runtime limits (body size, timeouts, batch size, rate limit).
    pub config: Arc<ServiceConfig>,
    /// Per-IP request budget for the extraction routes.
    pub rate_limiter: Arc<RateLimiter>,
    /// Per-key request budget for the period, when this deployment names keys.
    pub quotas: Arc<RateLimiter<String>>,
    /// The one reader `POST /v1/decode` looks a transaction up through.
    ///
    /// Built once and shared, not per request: pacing lives in the client, so a client built per
    /// call is a call with no pace at all — which is how a service under load turns into a flood
    /// against somebody else's endpoint.
    pub messages: Arc<MessageReader>,
    /// What a message opcode means, merged from every dataset this deployment was given — `None`
    /// when it was given none, and `POST /v1/decode` then fails per-request with a named error
    /// rather than the process refusing to start.
    pub operations: Option<Arc<OperationDictionary>>,
    /// Resolves on-chain code by address — `None` if no real network
    /// backend was configured (e.g. a test-only `AppState`); `/v1/verify`
    /// and `/v1/schema/{address}` return a clean, named error rather than
    /// panicking when this is absent.
    pub corpus: Option<Arc<CorpusController>>,
    /// Per-language bytecode compilers `RegistryController` recompiles
    /// submitted source with — see `sting_abi_registry`'s `bytecode`
    /// module doc comment for which languages are actually supported.
    pub bytecode_sources: Arc<HashMap<SourceLanguage, Box<dyn BytecodeSource>>>,
    /// The persisted verification-candidate store `/v1/verify` writes to
    /// on a `Full`/`Partial` match and `/v1/schema/{address}` reads from.
    pub verified_store: Arc<VerifiedSchemaStore>,
    /// Tier 3 (`sting-abi-dictionary`) method-id dictionary `POST /v1/probe`
    /// probes against — `None` if `corpus-abi/method-dictionary.json`
    /// wasn't found at startup, same forgiving pattern as every other
    /// optional dependency on this struct: `/v1/probe` then fails
    /// per-request with a named `tier3_unavailable`, not a startup crash.
    pub dictionary: Option<Arc<MethodDictionary>>,
    /// The loaded `libemulator` handle `POST /v1/probe` runs real TVM calls
    /// against — `None` if the vendored library wasn't found at startup.
    pub emulator_lib: Option<Arc<LibEmulator>>,
}

/// `POST /v1/probe`'s two prerequisites, bundled into one [`AppState::new`]
/// parameter — both are Tier 3-specific, both independently optional (see
/// their own field doc comments on [`AppState`]), and grouping them here is
/// what keeps [`AppState::new`] under `clippy::too_many_arguments` without
/// an `#[allow]`.
#[derive(Default)]
pub struct Tier3Config {
    /// See [`AppState::dictionary`].
    pub dictionary: Option<MethodDictionary>,
    /// See [`AppState::emulator_lib`].
    pub emulator_lib: Option<LibEmulator>,
    /// See [`AppState::operations`].
    pub operations: Option<OperationDictionary>,
}

impl AppState {
    /// Builds the shared state — one [`RateLimiter`] sized from `config`,
    /// shared by every handler and the rate-limit middleware alike.
    #[must_use]
    pub fn new(
        controller: ExtractController,
        config: ServiceConfig,
        corpus: Option<CorpusController>,
        bytecode_sources: HashMap<SourceLanguage, Box<dyn BytecodeSource>>,
        verified_store: VerifiedSchemaStore,
        tier3: Tier3Config,
    ) -> Self {
        let rate_limiter = RateLimiter::new(config.rate_limit_requests, config.rate_limit_window);
        let quotas = RateLimiter::new(config.quota_requests, config.quota_window);
        Self {
            controller: Arc::new(controller),
            config: Arc::new(config),
            rate_limiter: Arc::new(rate_limiter),
            quotas: Arc::new(quotas),
            messages: Arc::new(MessageReader::new()),
            operations: tier3.operations.map(Arc::new),
            corpus: corpus.map(Arc::new),
            bytecode_sources: Arc::new(bytecode_sources),
            verified_store: Arc::new(verified_store),
            dictionary: tier3.dictionary.map(Arc::new),
            emulator_lib: tier3.emulator_lib.map(Arc::new),
        }
    }
}
