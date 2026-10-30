//! `sting-abi-service` binary entrypoint — builds the router
//! (`sting_abi_service::build_router`) and serves it over a real
//! `tokio::net::TcpListener`. All routing/handler logic lives in this
//! crate's library half so tests can drive it via
//! `tower::ServiceExt::oneshot` without a real socket
//! (`docs/TESTING.md`).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use clap::{Parser, Subcommand};

use sting_abi_core::{Extractor, SourceLanguage};
use sting_abi_dictionary::{MethodDictionary, OperationDictionary};
use sting_abi_func::FuncExtractor;
use sting_abi_pipeline::ExtractController;
use sting_abi_registry::{
    BytecodeSource, TactBytecodeSource, TolkBytecodeSource, VerifiedSchemaStore,
};
use sting_abi_service::{
    AppState, REQUEST_TIMEOUT_HEADROOM, ServiceConfig, Tier3Config, build_router,
};
use sting_abi_tact::{TactCompiler, TactExtractor};
use sting_abi_tolk::TolkExtractor;
use sting_corpus::{ContractSource, CorpusController, DiskCache, TonapiClient, ToncenterClient};
use sting_emulator_sys::LibEmulator;
use sting_fift::FiftAssembler;
use sting_tolkc::TolkCompiler;

/// The HTTP surface over schema extraction.
///
/// Everything else is configured through the environment, which is what a deployment supplies and
/// what `docs/domains/service.md` documents; the command line carries only what an operator types
/// by hand.
#[derive(Parser)]
#[command(name = "sting-abi-service", about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serves schema extraction over HTTP.
    Serve {
        /// Which port to listen on. Overrides `PORT`.
        #[arg(long)]
        port: Option<u16>,
    },
}

#[tokio::main]
async fn main() {
    // Parsed before anything is built: a binary that answered `--help` by binding a socket is a
    // binary nobody can ask what it does.
    let Command::Serve { port: port_flag } = Cli::parse().command;

    tracing_subscriber::fmt::init();

    let config = config_from_env();
    let controller = build_controller(config.compile_timeout);
    let bytecode_sources = build_bytecode_sources(config.compile_timeout);
    let corpus = build_corpus_controller();
    let verified_store = build_verified_store();
    let tier3 = Tier3Config {
        dictionary: build_dictionary(),
        emulator_lib: build_emulator_lib(),
        operations: build_operations(),
    };
    let state = AppState::new(
        controller,
        config,
        corpus,
        bytecode_sources,
        verified_store,
        tier3,
    );

    let port: u16 = port_flag.unwrap_or_else(|| {
        std::env::var("PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8080)
    });
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!(%err, %addr, "failed to bind");
            std::process::exit(1);
        }
    };
    tracing::info!(%addr, "sting-abi-service listening");

    let app = build_router(state).into_make_service_with_connect_info::<SocketAddr>();
    if let Err(err) = axum::serve(listener, app).await {
        tracing::error!(%err, "server error");
        std::process::exit(1);
    }
}

/// Reads every [`ServiceConfig`] field from an env var, falling back to
/// [`ServiceConfig::default`] for anything unset or unparseable — no
/// required configuration, a bare `sting-abi-service` binary with no env
/// vars at all is a valid, if permissive, default deployment.
fn config_from_env() -> ServiceConfig {
    let default = ServiceConfig::default();
    let compile_timeout = Duration::from_secs(env_var(
        "COMPILE_TIMEOUT_SECS",
        default.compile_timeout.as_secs(),
    ));
    ServiceConfig {
        max_body_bytes: env_var("MAX_BODY_BYTES", default.max_body_bytes),
        compile_timeout,
        // Defaults to `compile_timeout + REQUEST_TIMEOUT_HEADROOM` (not
        // `default.request_timeout`, which was computed from the *default*
        // compile_timeout and would silently stay too short/long if only
        // `COMPILE_TIMEOUT_SECS` is overridden) unless explicitly
        // overridden itself. Same constant `ServiceConfig::default` uses,
        // so the two can't drift apart the way the request_timeout default
        // itself once drifted from `docs/domains/service.md`'s
        // own finding — see that constant's doc comment.
        request_timeout: Duration::from_secs(env_var(
            "REQUEST_TIMEOUT_SECS",
            (compile_timeout + REQUEST_TIMEOUT_HEADROOM).as_secs(),
        )),
        batch_max_items: env_var("BATCH_MAX_ITEMS", default.batch_max_items),
        rate_limit_requests: env_var("RATE_LIMIT_REQUESTS", default.rate_limit_requests),
        rate_limit_window: Duration::from_secs(env_var(
            "RATE_LIMIT_WINDOW_SECS",
            default.rate_limit_window.as_secs(),
        )),
        // Comma-separated, and absent means open: this service was built permissionless, and a
        // deployment that wants keys says so rather than being handed one by default.
        api_keys: std::env::var("STING_API_KEYS").map_or_else(
            |_| default.api_keys.clone(),
            |value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|key| !key.is_empty())
                    .map(str::to_string)
                    .collect()
            },
        ),
        quota_requests: env_var("QUOTA_REQUESTS", default.quota_requests),
        quota_window: Duration::from_secs(env_var(
            "QUOTA_WINDOW_SECS",
            default.quota_window.as_secs(),
        )),
        max_result_bytes: env_var("MAX_RESULT_BYTES", default.max_result_bytes),
    }
}

fn env_var<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Builds every extractor whose compiler is actually resolvable —
/// deliberately more forgiving than `sting-abi-cli`'s own
/// `build_controller` (`crates/sting-abi-cli/src/main.rs`), which fails to
/// start at all if either compiler is missing. A local CLI invocation for
/// one specific language has no use for the other extractor failing
/// loudly at startup; a public HTTP service should stay up and serve
/// whichever language it *can*, rather than refuse to bind entirely
/// because, say, only the Tact toolchain is available in a given
/// deployment. A request for a language with no registered extractor still
/// fails loudly per-request (`PipelineError::NoExtractorForLanguage` →
/// `unsupported_language`), never silently.
fn build_controller(compile_timeout: Duration) -> ExtractController {
    let mut extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();

    match TolkCompiler::vendored_or_default() {
        Some(resolved) => {
            let compiler = TolkCompiler::with_timeout(
                resolved.executable_path().to_path_buf(),
                compile_timeout,
            );
            extractors.insert(SourceLanguage::Tolk, Box::new(TolkExtractor::new(compiler)));
        }
        None => {
            tracing::warn!("no Tolk compiler is reachable, Tolk extraction unavailable");
        }
    }

    match TactCompiler::vendored_or_default() {
        Some(resolved) => {
            let compiler =
                TactCompiler::with_timeout(resolved.script_path().to_path_buf(), compile_timeout);
            extractors.insert(SourceLanguage::Tact, Box::new(TactExtractor::new(compiler)));
        }
        None => {
            tracing::warn!("no Tact compiler is reachable, Tact extraction unavailable");
        }
    }

    // No external compiler to resolve — `FuncExtractor` is pure
    // `tree-sitter`, always available.
    extractors.insert(SourceLanguage::Func, Box::new(FuncExtractor));

    ExtractController::new(extractors)
}

/// Builds every [`BytecodeSource`] this binary can drive — Tact and Tolk
/// today (see `sting_abi_registry::bytecode`'s own module doc comment for
/// exactly why `FunC` isn't and never will be; not an oversight). A missing
/// compiler/assembler is logged and skipped, same forgiving pattern as
/// [`build_controller`] — `/v1/verify` for that language then fails
/// per-request with a named `NoBytecodeSourceForLanguage`, not a startup
/// crash. Tolk needs *both* `tolk` and `fift` resolvable — either one
/// missing skips Tolk verification, logged with which one was missing.
fn build_bytecode_sources(
    compile_timeout: Duration,
) -> HashMap<SourceLanguage, Box<dyn BytecodeSource>> {
    let mut sources: HashMap<SourceLanguage, Box<dyn BytecodeSource>> = HashMap::new();
    match TactCompiler::vendored_or_default() {
        Some(resolved) => {
            let compiler =
                TactCompiler::with_timeout(resolved.script_path().to_path_buf(), compile_timeout);
            sources.insert(
                SourceLanguage::Tact,
                Box::new(TactBytecodeSource::new(compiler)),
            );
        }
        None => {
            tracing::warn!("no Tact compiler is reachable, Tact verification unavailable");
        }
    }
    match (
        TolkCompiler::vendored_or_default(),
        FiftAssembler::vendored_or_default(),
    ) {
        (Some(tolk_resolved), Some(fift_resolved)) => {
            let tolk = TolkCompiler::with_timeout(
                tolk_resolved.executable_path().to_path_buf(),
                compile_timeout,
            );
            let fift = FiftAssembler::with_timeout(
                fift_resolved.executable_path().to_path_buf(),
                compile_timeout,
            );
            sources.insert(
                SourceLanguage::Tolk,
                Box::new(TolkBytecodeSource::new(tolk, fift)),
            );
        }
        (None, _) => {
            tracing::warn!("no Tolk compiler is reachable, Tolk verification unavailable");
        }
        (_, None) => {
            tracing::warn!("no Fift assembler is reachable, Tolk verification unavailable");
        }
    }
    sources
}

/// Builds the real network-backed [`CorpusController`] — `toncenter`
/// first, `tonapi` as fallback, the same two-source order
/// `sting-cli`'s own `build_corpus_controller` already establishes for the
/// security branch. `None` (not a startup failure) if the platform cache
/// directory can't be determined — `/v1/verify`/`/v1/schema/{address}`
/// then fail per-request with a clear error instead of the whole service
/// refusing to bind over a non-essential cache path.
fn build_corpus_controller() -> Option<CorpusController> {
    let cache_root = match sting_corpus::DiskCache::default_root() {
        Ok(root) => root,
        Err(err) => {
            tracing::warn!(%err, "could not determine the platform cache directory, on-chain verification unavailable");
            return None;
        }
    };
    let sources: Vec<Box<dyn ContractSource>> = vec![
        Box::new(ToncenterClient::new()),
        Box::new(TonapiClient::new()),
    ];
    Some(CorpusController::new(
        sources,
        DiskCache::new(cache_root, sting_corpus::DEFAULT_TTL),
    ))
}

/// Builds the persisted verification-candidate store
/// (`docs/domains/abi-ir.md`) — `None`
/// unreachable state is handled the same as [`build_corpus_controller`]:
/// missing cache directory logs a warning, `/v1/verify` still works, it
/// just can't persist a successful match as a corpus-growth candidate.
fn build_verified_store() -> VerifiedSchemaStore {
    match VerifiedSchemaStore::default_root() {
        Ok(root) => VerifiedSchemaStore::new(root),
        Err(err) => {
            tracing::warn!(%err, "could not determine the platform cache directory, falling back to a process-scoped temp directory for verified schemas");
            VerifiedSchemaStore::new(std::env::temp_dir().join("sting-abi-registry-fallback"))
        }
    }
}

/// Loads the Tier 3 method-id dictionary `POST /v1/probe` probes against —
/// `STING_DICTIONARY_PATH` if set, otherwise `corpus-abi/method-dictionary.json`
/// relative to the current working directory (matches `abi-layer
/// dictionary-probe`'s own CLI default, `crates/sting-abi-cli/src/main.rs`).
/// A real deployment needs this file baked into the image or bind-mounted —
/// the same operational pattern `Dockerfile.abi-service` already documents
/// for `vendor/tolk`/`vendor/tact` — not something this binary fetches on
/// its own. `None` (not a startup failure) if missing/unparseable —
/// `/v1/probe` then fails per-request with `tier3_unavailable`.
fn build_dictionary() -> Option<MethodDictionary> {
    let path = std::env::var("STING_DICTIONARY_PATH").map_or_else(
        |_| std::path::PathBuf::from("corpus-abi/method-dictionary.json"),
        std::path::PathBuf::from,
    );
    match MethodDictionary::load(&path) {
        Ok(dictionary) => Some(dictionary),
        Err(err) => {
            tracing::warn!(%err, path = %path.display(), "could not load the Tier 3 method-id dictionary, Tier 3 probing unavailable");
            None
        }
    }
}

/// Loads every operation dictionary this deployment was given, merged into one lookup.
///
/// Two by default and both optional: what this repository learned from its own corpora, and what it
/// learned from the foundation's catalogue. A deployment naming neither serves everything else and
/// answers `POST /v1/decode` with a named error rather than refusing to start.
fn build_operations() -> Option<OperationDictionary> {
    let configured = std::env::var("STING_OPERATION_DICTIONARIES").unwrap_or_else(|_| {
        "corpus-abi/operation-dictionary.json,corpus-abi/foundation-operations.json".to_string()
    });
    let mut merged = OperationDictionary::new();
    let mut loaded = 0usize;
    for path in configured
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let path = std::path::PathBuf::from(path);
        match OperationDictionary::load(&path) {
            Ok(dictionary) => {
                merged.absorb(&dictionary);
                loaded += 1;
            }
            Err(err) => {
                tracing::warn!(%err, path = %path.display(), "could not load an operation dictionary");
            }
        }
    }
    if loaded == 0 {
        tracing::warn!("no operation dictionary loaded, decoding unavailable");
        return None;
    }
    tracing::info!(
        datasets = loaded,
        opcodes = merged.len(),
        "operation dictionaries loaded"
    );
    Some(merged)
}

/// Loads the vendored `libemulator` `POST /v1/probe` runs real TVM calls
/// against — `sting_emulator::load_default_library`'s own
/// `STING_LIBEMULATOR_PATH`/next-to-executable convention, the same one
/// every other real-emulator caller in this workspace already uses. `None`
/// (not a startup failure) if missing — `/v1/probe` then fails per-request
/// with `tier3_unavailable`, same forgiving pattern as every other optional
/// dependency in this file.
fn build_emulator_lib() -> Option<LibEmulator> {
    match sting_emulator::load_default_library() {
        Ok(lib) => {
            let _ = sting_emulator::set_verbosity_level(&lib, 0);
            Some(lib)
        }
        Err(err) => {
            tracing::warn!(%err, "could not load libemulator, Tier 3 probing unavailable");
            None
        }
    }
}
