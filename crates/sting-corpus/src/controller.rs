//! `CorpusController` — resolves a [`ContractTarget`] to a normalized input
//! (`docs/ARCHITECTURE.md`, `docs/DATA.md`).
//!
//! The one controller whose I/O is not CPU-bound:
//! cache-then-fetch-with-fallback orchestration over this crate's own
//! building blocks (HTTP providers, disk cache) — sequential by design, not
//! `rayon`-parallel — only `LintController` and `EmulationPoolController` own
//! concurrency directly.
//!
//! It lives here, not in `sting-pipeline` where it was first written for
//! `sting fetch`, because it orchestrates *this* crate's pieces and nothing
//! from the security contour (`docs/ARCHITECTURE.md` §1). `sting-pipeline`
//! re-exports it, so its own callers are unaffected.

use std::path::PathBuf;

use crate::{
    CacheError, ContractCode, ContractSource, ContractTarget, DiskCache, FetchError,
    LibraryResolver,
};

/// Per-call options for [`CorpusController::resolve`].
#[derive(Debug, Clone, Copy, Default)]
pub struct FetchOptions {
    /// Bypasses the disk cache — always hits the network and overwrites the
    /// cache entry with the fresh result (`sting fetch --force-refresh`,
    /// `docs/DATA.md`).
    pub force_refresh: bool,
}

/// Errors from [`CorpusController::resolve`].
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    /// `target` was a [`ContractTarget::LocalFile`] — Tolk *source*, not
    /// compiled bytecode. This is the type-level enforcement of this
    /// block's "important distinction"
    /// (`docs/DATA.md`'s reminder from
    /// `ROADMAP.md` §5): a `.tolk` file feeds `LintController` for static
    /// analysis directly (never through here); it cannot become a
    /// [`ContractCode`] without a compile step `sting`
    /// deliberately never performs (`docs/GLOSSARY.md`).
    #[error(
        "{path} is a .tolk source file, not compiled bytecode — static analysis \
         (`sting lint`) reads a .tolk file directly; the emulator/fuzzer need compiled \
         code+data (`LocalBoc` or an on-chain address via `sting fetch`) instead. \
         sting never compiles one into the other — see docs/DATA.md."
    )]
    NotBytecode {
        /// The `.tolk` path that was passed where bytecode was expected.
        path: PathBuf,
    },
    /// A [`ContractTarget::LocalBoc`] file could not be read.
    #[error("failed to read local BoC file {path}: {source}")]
    LocalBocRead {
        /// The path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Every configured [`ContractSource`] failed to resolve `address`.
    #[error("every contract source failed to resolve {address}: {last}")]
    AllSourcesFailed {
        /// The address that could not be resolved.
        address: String,
        /// The last source's own error (earlier sources' errors are not
        /// retained individually — `docs/ARCHITECTURE.md`
        /// principle 2 calls for a clear "what went wrong", not a silent
        /// swallow, but does not require preserving every intermediate
        /// attempt's full detail when they are all instances of the same
        /// underlying "this address isn't resolvable right now").
        #[source]
        last: FetchError,
    },
    /// [`CorpusController::resolve`] was called for an
    /// [`ContractTarget::OnChainAddress`] with no [`ContractSource`]s
    /// configured at all — a caller-side misconfiguration, not a network failure.
    #[error("no contract sources configured")]
    NoSources,
    /// The disk cache could not be read or written.
    #[error(transparent)]
    Cache(#[from] CacheError),
}

/// Resolves a [`ContractTarget`] to a normalized [`ContractCode`] —
/// `docs/ARCHITECTURE.md`'s `CorpusController`.
pub struct CorpusController {
    sources: Vec<Box<dyn ContractSource>>,
    cache: DiskCache,
    library_resolver: LibraryResolver,
}

impl CorpusController {
    /// Builds a controller trying `sources` in order (first success wins,
    /// with the cache checked before any of them and updated after) —
    /// production callers pass `[ToncenterClient::new(), TonapiClient::new()]`
    /// per `docs/DATA.md`; tests substitute
    /// fakes with scripted responses (`docs/ARCHITECTURE.md`
    /// principle 1 — this is exactly the `&dyn ContractSource` mocking
    /// boundary that principle calls for). Also resolves TON library
    /// reference cells ([`LibraryResolver`], a real gap found while growing
    /// the dictionary, `docs/domains/method-dictionary.md`) automatically — every
    /// caller of [`Self::resolve`] gets real, probeable code back, not a
    /// caller-visible extra step.
    #[must_use]
    pub fn new(sources: Vec<Box<dyn ContractSource>>, cache: DiskCache) -> Self {
        Self::with_library_resolver(sources, cache, LibraryResolver::new())
    }

    /// Same as [`Self::new`], with an explicit [`LibraryResolver`] instead
    /// of the real `toncenter.com`-pointed default — this crate's own tests
    /// use this to point library resolution at a local mock server, the
    /// same `&dyn ContractSource` mocking-boundary principle applied to
    /// this controller's other real network dependency.
    #[must_use]
    pub fn with_library_resolver(
        sources: Vec<Box<dyn ContractSource>>,
        cache: DiskCache,
        library_resolver: LibraryResolver,
    ) -> Self {
        Self {
            sources,
            cache,
            library_resolver,
        }
    }

    /// Resolves `target` per its variant — see [`CorpusError::NotBytecode`]
    /// for why [`ContractTarget::LocalFile`] is a distinct error, not a
    /// silent no-op.
    ///
    /// # Errors
    /// See [`CorpusError`].
    pub fn resolve(
        &self,
        target: &ContractTarget,
        options: FetchOptions,
    ) -> Result<ContractCode, CorpusError> {
        match target {
            ContractTarget::LocalFile(path) => Err(CorpusError::NotBytecode { path: path.clone() }),
            ContractTarget::LocalBoc { code, data } => Self::read_local_boc(code, data),
            ContractTarget::OnChainAddress(address) => self.resolve_on_chain(address, options),
        }
    }

    fn read_local_boc(
        code: &std::path::Path,
        data: &std::path::Path,
    ) -> Result<ContractCode, CorpusError> {
        let code_text =
            std::fs::read_to_string(code).map_err(|source| CorpusError::LocalBocRead {
                path: code.to_path_buf(),
                source,
            })?;
        let data_text =
            std::fs::read_to_string(data).map_err(|source| CorpusError::LocalBocRead {
                path: data.to_path_buf(),
                source,
            })?;
        Ok(ContractCode {
            code_boc_base64: code_text.trim().to_string(),
            data_boc_base64: data_text.trim().to_string(),
        })
    }

    fn resolve_on_chain(
        &self,
        address: &str,
        options: FetchOptions,
    ) -> Result<ContractCode, CorpusError> {
        if !options.force_refresh
            && let Some(cached) = self.cache.get(address)
        {
            let resolved = self.resolve_library_if_needed(cached);
            self.cache.put(address, &resolved)?;
            return Ok(resolved);
        }

        let mut last_error = None;
        for source in &self.sources {
            match source.fetch(address) {
                Ok(code) => {
                    let code = self.resolve_library_if_needed(code);
                    self.cache.put(address, &code)?;
                    return Ok(code);
                }
                Err(err) => last_error = Some(err),
            }
        }
        match last_error {
            Some(last) => Err(CorpusError::AllSourcesFailed {
                address: address.to_string(),
                last,
            }),
            None => Err(CorpusError::NoSources),
        }
    }

    /// Substitutes a TON library reference cell's real code in place of the
    /// pointer, if `code`'s own `code_boc_base64` is one (the common case —
    /// ordinary code — is a cheap local check, no network call at all, see
    /// `LibraryResolver::resolve_if_library`'s own doc comment).
    /// Deliberately best-effort: any resolution failure (malformed `BoC`,
    /// network error, library not found/deleted) falls back to the
    /// original, unresolved code rather than failing the whole `resolve()`
    /// call — a library-cell contract that can't be resolved right now
    /// behaves exactly as it did before this existed (Tier 3 probing it
    /// stays `inconclusive`, not a new failure mode), not worse.
    fn resolve_library_if_needed(&self, code: ContractCode) -> ContractCode {
        match self
            .library_resolver
            .resolve_if_library(&code.code_boc_base64)
        {
            Ok(Some(real_code_boc_base64)) => ContractCode {
                code_boc_base64: real_code_boc_base64,
                ..code
            },
            Ok(None) | Err(_) => code,
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::{ContractTarget, CorpusController, CorpusError, FetchOptions};
    use crate::{ContractCode, ContractSource, DiskCache, FetchError};

    struct AlwaysFails {
        provider: &'static str,
        calls: AtomicU32,
    }

    impl AlwaysFails {
        fn new(provider: &'static str) -> Self {
            Self {
                provider,
                calls: AtomicU32::new(0),
            }
        }
    }

    impl ContractSource for AlwaysFails {
        fn name(&self) -> &'static str {
            self.provider
        }
        fn fetch(&self, _address: &str) -> Result<ContractCode, FetchError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(FetchError::Chain(
                sting_chain::ChainError::UnexpectedStatus {
                    provider: self.provider,
                    url: "mock://unreachable".to_string(),
                    status: 500,
                },
            ))
        }
    }

    struct AlwaysSucceeds {
        provider: &'static str,
        code: ContractCode,
        calls: AtomicU32,
    }

    impl AlwaysSucceeds {
        fn new(provider: &'static str, code: ContractCode) -> Self {
            Self {
                provider,
                code,
                calls: AtomicU32::new(0),
            }
        }
    }

    impl ContractSource for AlwaysSucceeds {
        fn name(&self) -> &'static str {
            self.provider
        }
        fn fetch(&self, _address: &str) -> Result<ContractCode, FetchError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.code.clone())
        }
    }

    fn sample_code() -> ContractCode {
        ContractCode {
            code_boc_base64: "code-bytes".to_string(),
            data_boc_base64: "data-bytes".to_string(),
        }
    }

    fn controller_with(
        sources: Vec<Box<dyn ContractSource>>,
    ) -> (CorpusController, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let cache = DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
        (CorpusController::new(sources, cache), dir)
    }

    #[test]
    fn resolve_local_file_is_a_distinct_not_bytecode_error_not_a_silent_noop() {
        let (controller, _dir) = controller_with(vec![]);
        let target = ContractTarget::LocalFile("contract.tolk".into());
        let result = controller.resolve(&target, FetchOptions::default());
        assert!(matches!(result, Err(CorpusError::NotBytecode { .. })));
    }

    #[test]
    fn resolve_local_boc_reads_both_files_and_trims_whitespace() {
        let (controller, dir) = controller_with(vec![]);
        let code_path = dir.path().join("code.boc.base64");
        let data_path = dir.path().join("data.boc.base64");
        std::fs::write(&code_path, "  code-bytes  \n").expect("writes");
        std::fs::write(&data_path, "data-bytes\n").expect("writes");

        let target = ContractTarget::LocalBoc {
            code: code_path,
            data: data_path,
        };
        let result = controller
            .resolve(&target, FetchOptions::default())
            .expect("reads local files");
        assert_eq!(result, sample_code());
    }

    #[test]
    fn resolve_on_chain_address_falls_back_to_the_second_source_when_the_first_fails() {
        let first = AlwaysFails::new("first");
        let second = AlwaysSucceeds::new("second", sample_code());
        let (controller, _dir) = controller_with(vec![Box::new(first), Box::new(second)]);

        let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
        let result = controller
            .resolve(&target, FetchOptions::default())
            .expect("second source succeeds");
        assert_eq!(result, sample_code());
    }

    #[test]
    fn resolve_on_chain_address_fails_when_every_source_fails() {
        let (controller, _dir) = controller_with(vec![
            Box::new(AlwaysFails::new("first")),
            Box::new(AlwaysFails::new("second")),
        ]);

        let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
        let result = controller.resolve(&target, FetchOptions::default());
        assert!(matches!(result, Err(CorpusError::AllSourcesFailed { .. })));
    }

    #[test]
    fn resolve_on_chain_address_uses_the_cache_and_skips_the_network_when_fresh() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let cache = DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
        cache
            .put("0:aaaa", &sample_code())
            .expect("seeds the cache");

        let source = AlwaysFails::new("should-not-be-called");
        let controller = CorpusController::new(vec![Box::new(source)], cache);

        let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
        let result = controller
            .resolve(&target, FetchOptions::default())
            .expect("serves from cache without touching the source");
        assert_eq!(result, sample_code());
    }

    #[test]
    fn resolve_on_chain_address_bypasses_a_fresh_cache_when_force_refresh_is_set() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let cache = DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
        let stale_looking = ContractCode {
            code_boc_base64: "old-code".to_string(),
            data_boc_base64: "old-data".to_string(),
        };
        cache
            .put("0:aaaa", &stale_looking)
            .expect("seeds the cache");

        let fresh = sample_code();
        let source = AlwaysSucceeds::new("fresh-source", fresh.clone());
        let controller = CorpusController::new(vec![Box::new(source)], cache);

        let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
        let result = controller
            .resolve(
                &target,
                FetchOptions {
                    force_refresh: true,
                },
            )
            .expect("fetches fresh data");
        assert_eq!(result, fresh);
    }

    #[test]
    fn resolve_on_chain_address_writes_a_successful_fetch_back_to_the_cache() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let cache = DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
        let source = AlwaysSucceeds::new("source", sample_code());
        let controller = CorpusController::new(vec![Box::new(source)], cache);

        let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
        controller
            .resolve(&target, FetchOptions::default())
            .expect("fetches successfully");

        // A second controller sharing the same cache directory, with a
        // source that always fails, still succeeds — proof the first
        // resolve() actually persisted to disk, not just returned a value.
        let dead_source = AlwaysFails::new("dead");
        let second_cache = DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
        let second_controller = CorpusController::new(vec![Box::new(dead_source)], second_cache);
        let cached = second_controller
            .resolve(&target, FetchOptions::default())
            .expect("reads what the first controller cached");
        assert_eq!(cached, sample_code());
    }

    #[test]
    fn resolve_on_chain_address_with_no_sources_configured_is_a_clear_error_not_a_panic() {
        let (controller, _dir) = controller_with(vec![]);
        let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
        let result = controller.resolve(&target, FetchOptions::default());
        assert!(matches!(result, Err(CorpusError::NoSources)));
    }

    mod library_resolution {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        use std::thread;
        use std::time::Duration;

        use crate::LibraryResolver;

        use super::{AlwaysSucceeds, ContractCode, ContractTarget, CorpusController, FetchOptions};

        /// Same hand-rolled scripted-response server as
        /// `sting-corpus/src/library.rs`'s own tests.
        fn serve_scripted_responses(responses: Vec<Vec<u8>>) -> (String, thread::JoinHandle<()>) {
            let listener = TcpListener::bind("127.0.0.1:0").expect("binds an ephemeral port");
            let port = listener.local_addr().expect("has a local addr").port();
            let handle = thread::spawn(move || {
                for response in responses {
                    let (mut stream, _) = listener.accept().expect("accepts a connection");
                    let mut buf = [0_u8; 4096];
                    let mut total = 0;
                    loop {
                        let n = stream
                            .read(&mut buf[total..])
                            .expect("reads from the socket");
                        total += n;
                        if n == 0 || buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    stream
                        .write_all(&response)
                        .expect("writes the scripted response");
                    stream.flush().expect("flushes the response");
                }
            });
            (format!("http://127.0.0.1:{port}"), handle)
        }

        fn json_response(body: &str) -> Vec<u8> {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_bytes()
        }

        /// The same real, unmodified library-reference-cell `BoC` used in
        /// `sting-corpus/src/library.rs`'s own tests — a real, deployed
        /// governed-jetton-wallet address's code as `toncenter.com/api/v3/
        /// account` actually returns it (`corpus-abi/clean/func/SOURCE.md`).
        const REAL_LIBRARY_CELL_BOC: &str =
            "te6ccgEBAQEAIwAIQgKPRS16Tf10BmtoI2UXclntBXNENb52tf1L1divK3w9aA==";

        #[test]
        fn resolve_on_chain_address_substitutes_the_real_code_for_a_library_reference() {
            let real_code_b64 = "te6ccgECDwEAA9EAART/APSkE/S88sgLAQIBYgIDAvjQAdDTAwFxsI5IE18D";
            let response_body = format!(
                r#"{{"ok":true,"result":{{"@type":"smc.libraryResult","result":[{{"@type":"smc.libraryEntry","hash":"j0Utek39dAZraCNlF3JZ7QVzRDW+drX9S9XYryt8PWg=","data":"{real_code_b64}"}}]}},"@extra":"1:1:1"}}"#
            );
            let (base_url, server) = serve_scripted_responses(vec![json_response(&response_body)]);

            let dir = tempfile::tempdir().expect("creates a temp dir");
            let cache = crate::DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
            let source = AlwaysSucceeds::new(
                "source",
                ContractCode {
                    code_boc_base64: REAL_LIBRARY_CELL_BOC.to_string(),
                    data_boc_base64: "data-bytes".to_string(),
                },
            );
            let controller = CorpusController::with_library_resolver(
                vec![Box::new(source)],
                cache,
                LibraryResolver::with_base_url(base_url),
            );

            let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
            let resolved = controller
                .resolve(&target, FetchOptions::default())
                .expect("resolves successfully");
            assert_eq!(
                resolved.code_boc_base64, real_code_b64,
                "a library-reference cell's code must be substituted with the real library code"
            );

            server.join().expect("server thread does not panic");
        }

        #[test]
        fn resolve_on_chain_address_resolves_a_library_reference_found_stale_in_the_cache() {
            // Real bug found while re-probing mainnet coverage data: an
            // address's code was cached *before* `LibraryResolver` existed
            // (or before the network could resolve its library), so the
            // cache holds the raw, unresolved pointer cell. A later
            // `resolve()` call within the cache's TTL must still substitute
            // the real code — not silently keep returning the stale pointer
            // until the TTL happens to expire.
            let real_code_b64 = "te6ccgECDwEAA9EAART/APSkE/S88sgLAQIBYgIDAvjQAdDTAwFxsI5IE18D";
            let response_body = format!(
                r#"{{"ok":true,"result":{{"@type":"smc.libraryResult","result":[{{"@type":"smc.libraryEntry","hash":"j0Utek39dAZraCNlF3JZ7QVzRDW+drX9S9XYryt8PWg=","data":"{real_code_b64}"}}]}},"@extra":"1:1:1"}}"#
            );
            let (base_url, server) = serve_scripted_responses(vec![json_response(&response_body)]);

            let dir = tempfile::tempdir().expect("creates a temp dir");
            let cache = crate::DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
            // Seed the cache directly with the stale, unresolved pointer —
            // simulates an entry written before this bug was fixed.
            cache
                .put(
                    "0:aaaa",
                    &ContractCode {
                        code_boc_base64: REAL_LIBRARY_CELL_BOC.to_string(),
                        data_boc_base64: "data-bytes".to_string(),
                    },
                )
                .expect("seeds the cache");
            // A source that would fail if hit — this exercises the
            // cache-hit path, not the fetch-miss path already covered above.
            let source = AlwaysSucceeds::new(
                "source",
                ContractCode {
                    code_boc_base64: "should-not-be-used".to_string(),
                    data_boc_base64: "should-not-be-used".to_string(),
                },
            );
            let controller = CorpusController::with_library_resolver(
                vec![Box::new(source)],
                cache,
                LibraryResolver::with_base_url(base_url),
            );

            let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
            let resolved = controller
                .resolve(&target, FetchOptions::default())
                .expect("resolves successfully");
            assert_eq!(
                resolved.code_boc_base64, real_code_b64,
                "a stale cached library-reference cell must still be resolved to the real code"
            );

            server.join().expect("server thread does not panic");
        }

        #[test]
        fn resolve_on_chain_address_leaves_ordinary_code_unchanged_with_no_extra_network_call() {
            // Points at an address nothing listens on — if `resolve()`
            // attempted a library-resolution network call for ordinary
            // code, this would hang/fail instead of returning immediately.
            let library_resolver = LibraryResolver::with_base_url("http://127.0.0.1:1".to_string());

            let dir = tempfile::tempdir().expect("creates a temp dir");
            let cache = crate::DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
            let ordinary_code = ContractCode {
                code_boc_base64: "code-bytes".to_string(),
                data_boc_base64: "data-bytes".to_string(),
            };
            let source = AlwaysSucceeds::new("source", ordinary_code.clone());
            let controller = CorpusController::with_library_resolver(
                vec![Box::new(source)],
                cache,
                library_resolver,
            );

            let target = ContractTarget::OnChainAddress("0:aaaa".to_string());
            let resolved = controller
                .resolve(&target, FetchOptions::default())
                .expect("resolves successfully, malformed-BoC library check fails soft");
            assert_eq!(resolved, ordinary_code);
        }
    }
}
