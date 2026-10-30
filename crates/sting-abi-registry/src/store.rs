//! The extensible corpus growth mechanism
//! (`docs/domains/abi-ir.md`,
//! `docs/domains/service.md`): every
//! successful [`crate::RegistryController::verify`] is a **candidate** for
//! `corpus-abi/`, saved here with its provenance — never automatically
//! promoted into golden fixtures or the benchmark corpus, which still
//! requires the same manual review Tier 1 golden fixtures already do
//! (`docs/TESTING.md`). This store is deliberately
//! not wired into [`crate::RegistryController::verify`] itself — `verify`
//! stays a pure, disk-side-effect-free operation callers can test freely
//! (`tests/registry.rs` calls it repeatedly without touching any real
//! cache directory); persisting a candidate is one explicit extra step
//! the HTTP layer takes after a successful verification, not an implicit
//! one.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sting_abi_core::ContractSchema;
use sting_corpus::cache_key;

use crate::MatchStatus;

/// One saved verification candidate, with the metadata that makes it
/// answerable later: where it came from, when it was added, and whether it
/// was picked deliberately or arrived through use.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredVerification {
    /// The address this schema was verified against.
    pub address: String,
    /// The extracted, verified schema.
    pub schema: ContractSchema,
    /// The match status this schema was saved under — always `Full` or
    /// `Partial` in practice (`RegistryController::verify` never returns a
    /// schema for `NoMatch`), kept explicit rather than assumed.
    pub status: MatchStatus,
    /// Unix timestamp (seconds) of when this record was saved.
    pub added_at_unix: u64,
    /// Always `"organic-registry"` for records this store itself writes —
    /// a distinct field (not just a hardcoded assumption at read time) so
    /// a future seed-corpus importer can write `"seed-corpus"` records
    /// into the same store/shape without this type needing to change.
    pub origin: String,
}

/// `"organic-registry"` — see [`StoredVerification::origin`]'s own doc
/// comment for why this is a named constant, not inlined at every call
/// site.
pub const ORIGIN_ORGANIC_REGISTRY: &str = "organic-registry";

/// Bounded retry time budget for [`VerifiedSchemaStore::put`]'s
/// atomic-rename step — see that method's own doc comment for why Windows
/// needs this at all.
const MAX_PERSIST_WAIT: Duration = Duration::from_secs(2);

/// Error from [`VerifiedSchemaStore`].
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Could not determine the platform cache directory.
    #[error("failed to determine the platform cache directory")]
    NoCacheDir,
    /// Reading a stored record failed.
    #[error("failed to read {path}: {source}")]
    Read {
        /// The path that failed to read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A stored record was not valid JSON — would indicate on-disk
    /// corruption or a schema-version mismatch from an older run.
    #[error("failed to parse {path} as a stored verification: {source}")]
    Parse {
        /// The path that failed to parse.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
    },
    /// Writing a record failed.
    #[error("failed to write {path}: {source}")]
    Write {
        /// The path that failed to write.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Serializing a record to JSON failed — would indicate a schema
    /// containing something `serde_json` genuinely cannot represent (e.g.
    /// a non-finite float slipping past [`sting_abi_core::ExtractionConfidence`]'s
    /// own validation), not expected in practice but not silently
    /// swallowed either.
    #[error("failed to serialize a verification record for {address}: {source}")]
    Serialize {
        /// The address whose record failed to serialize.
        address: String,
        /// The underlying error.
        #[source]
        source: serde_json::Error,
    },
}

/// A disk-based store of verification candidates, one JSON file per
/// address — the same `dirs`-based root convention as
/// [`sting_corpus::DiskCache`], a sibling directory under it rather than
/// literally the same one (this store's records are ABI schemas with
/// provenance metadata, not raw `BoC` bytes — a different shape/purpose,
/// so a different directory, not a namespace collision waiting to
/// happen).
pub struct VerifiedSchemaStore {
    root: PathBuf,
}

impl VerifiedSchemaStore {
    /// The platform-conventional default root: `<platform cache
    /// dir>/sting/abi-registry`.
    ///
    /// # Errors
    /// Returns [`StoreError::NoCacheDir`] if the platform cache directory
    /// cannot be determined.
    pub fn default_root() -> Result<PathBuf, StoreError> {
        dirs::cache_dir()
            .map(|dir| dir.join("sting").join("abi-registry"))
            .ok_or(StoreError::NoCacheDir)
    }

    /// A store rooted at `root` — callers pass a scratch directory in
    /// tests, [`Self::default_root`] in production.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path_for(&self, address: &str) -> PathBuf {
        self.root.join(format!("{}.json", cache_key(address)))
    }

    /// The stored verification for `address`, if one exists.
    ///
    /// # Errors
    /// See [`StoreError`]'s variants — a missing file is `Ok(None)`, not
    /// an error.
    pub fn get(&self, address: &str) -> Result<Option<StoredVerification>, StoreError> {
        let path = self.path_for(address);
        if !path.is_file() {
            return Ok(None);
        }
        let text = read_sharing_delete(&path).map_err(|source| StoreError::Read {
            path: path.clone(),
            source,
        })?;
        let record =
            serde_json::from_str(&text).map_err(|source| StoreError::Parse { path, source })?;
        Ok(Some(record))
    }

    /// Saves `schema` as a verification candidate for `address` under
    /// `status`, stamped with the current time and
    /// [`ORIGIN_ORGANIC_REGISTRY`]. Overwrites any existing record for the
    /// same address — a re-verification with corrected/updated source is
    /// expected to replace the previous candidate, not accumulate history
    /// — a contract may be re-verified against different source.
    ///
    /// # Errors
    /// See [`StoreError`]'s variants.
    pub fn put(
        &self,
        address: &str,
        schema: &ContractSchema,
        status: MatchStatus,
    ) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.root).map_err(|source| StoreError::Write {
            path: self.root.clone(),
            source,
        })?;
        let record = StoredVerification {
            address: address.to_string(),
            schema: schema.clone(),
            status,
            added_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            origin: ORIGIN_ORGANIC_REGISTRY.to_string(),
        };
        let text =
            serde_json::to_string_pretty(&record).map_err(|source| StoreError::Serialize {
                address: address.to_string(),
                source,
            })?;
        let path = self.path_for(address);
        // Write-to-temp-then-rename, not a direct `std::fs::write`: two
        // concurrent `put()` calls for the same address (real under
        // `/v1/verify`'s `spawn_blocking` on `tokio`'s multi-threaded
        // runtime) can otherwise interleave their truncate+write into a
        // torn, unparsable file, reproduced empirically. A same-directory
        // `NamedTempFile` plus `persist`
        // is atomic on both POSIX (`rename(2)`) and Windows (same-volume
        // `MoveFileEx`), so a concurrent reader always sees either the old
        // or the new complete file, never a partial one.
        let mut tmp =
            tempfile::NamedTempFile::new_in(&self.root).map_err(|source| StoreError::Write {
                path: self.root.clone(),
                source,
            })?;
        tmp.write_all(text.as_bytes())
            .map_err(|source| StoreError::Write {
                path: path.clone(),
                source,
            })?;
        // On Windows a rename over a file somebody holds open fails with `ERROR_ACCESS_DENIED`
        // unless that handle granted `FILE_SHARE_DELETE`; POSIX `rename(2)` has no such rule. This
        // store's own reader now grants it (`read_sharing_delete`), which is what removed the race
        // rather than waiting it out — before that, `concurrent_puts_for_the_same_address_never_
        // produce_a_torn_file` exhausted even this budget under load.
        //
        // The loop stays because a reader inside this process is not the only thing that can hold a
        // handle: an indexer or a scanner opens files nobody asked it to, and a bounded wait is the
        // difference between a transient collision and a failed verification. A deadline with
        // exponential backoff rather than a fixed attempt count, because the number of tries that
        // suffices depends on load and the time one can afford does not.
        let deadline = std::time::Instant::now() + MAX_PERSIST_WAIT;
        let mut backoff = Duration::from_millis(1);
        let mut outcome = tmp.persist(&path);
        loop {
            match outcome {
                Ok(_) => return Ok(()),
                Err(err)
                    if err.error.kind() == std::io::ErrorKind::PermissionDenied
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(Duration::from_millis(50));
                    outcome = err.file.persist(&path);
                }
                Err(err) => {
                    return Err(StoreError::Write {
                        path,
                        source: err.error,
                    });
                }
            }
        }
    }
}

/// Reads a file without preventing a concurrent rename over it.
///
/// On Windows a handle opened the ordinary way withholds `FILE_SHARE_DELETE`, so a reader makes the
/// atomic replace in [`VerifiedSchemaStore::put`] fail with `ERROR_ACCESS_DENIED` for as long as it
/// holds the file open. That is the whole source of the retry loop there: the writer was waiting out
/// a window the reader created. Granting delete sharing closes the window at its origin rather than
/// outlasting it, and matches what POSIX `rename(2)` does with no ceremony at all.
#[cfg(windows)]
fn read_sharing_delete(path: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    use std::os::windows::fs::OpenOptionsExt as _;

    /// `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`, spelled out rather than pulled in
    /// from a Windows binding crate for three constants.
    const SHARE_READ_WRITE_DELETE: u32 = 0x0000_0001 | 0x0000_0002 | 0x0000_0004;

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_WRITE_DELETE)
        .open(path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

/// Reads a file. Everywhere but Windows a rename over an open file is already unobstructed.
#[cfg(not(windows))]
fn read_sharing_delete(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use sting_abi_core::{ContractMetadata, ContractSchema, ExtractionTier, SourceLanguage};

    use super::{ORIGIN_ORGANIC_REGISTRY, VerifiedSchemaStore};
    use crate::MatchStatus;

    fn minimal_schema() -> ContractSchema {
        ContractSchema {
            ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
            source_language: SourceLanguage::Tact,
            extraction_tier: ExtractionTier::Compiled,
            contract_name: Some("Test".to_string()),
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

    #[test]
    fn a_missing_record_is_a_clean_none_not_an_error() {
        let dir = tempfile::tempdir().expect("scratch dir");
        let store = VerifiedSchemaStore::new(dir.path().to_path_buf());
        assert!(
            store
                .get("EQsomeaddress")
                .expect("missing file is Ok(None)")
                .is_none()
        );
    }

    #[test]
    fn a_saved_record_round_trips_with_the_right_metadata() {
        let dir = tempfile::tempdir().expect("scratch dir");
        let store = VerifiedSchemaStore::new(dir.path().to_path_buf());
        let schema = minimal_schema();

        store
            .put("EQsomeaddress", &schema, MatchStatus::Full)
            .expect("put must succeed");
        let record = store
            .get("EQsomeaddress")
            .expect("get must succeed")
            .expect("record must be present");

        assert_eq!(record.address, "EQsomeaddress");
        assert_eq!(record.schema, schema);
        assert_eq!(record.status, MatchStatus::Full);
        assert_eq!(record.origin, ORIGIN_ORGANIC_REGISTRY);
        assert!(record.added_at_unix > 0);
    }

    #[test]
    fn a_second_put_for_the_same_address_replaces_the_first() {
        let dir = tempfile::tempdir().expect("scratch dir");
        let store = VerifiedSchemaStore::new(dir.path().to_path_buf());
        let mut schema = minimal_schema();

        store
            .put("EQsomeaddress", &schema, MatchStatus::Full)
            .expect("first put must succeed");
        schema.contract_name = Some("Updated".to_string());
        store
            .put("EQsomeaddress", &schema, MatchStatus::Full)
            .expect("second put must succeed");

        let record = store
            .get("EQsomeaddress")
            .expect("get must succeed")
            .expect("record must be present");
        assert_eq!(record.schema.contract_name.as_deref(), Some("Updated"));
    }

    /// Regression test for the torn-write race: many threads `put()`-ing the
    /// same address
    /// concurrently must never leave a truncated/unparsable file behind —
    /// every `get()` after every `put()` must see a fully-formed record,
    /// never a [`super::StoreError::Parse`]. Before the write-to-temp-then-
    /// rename fix, this reliably reproduced a 0-byte-truncated file within
    /// a handful of iterations.
    #[test]
    fn concurrent_puts_for_the_same_address_never_produce_a_torn_file() {
        let dir = tempfile::tempdir().expect("scratch dir");
        let store = std::sync::Arc::new(VerifiedSchemaStore::new(dir.path().to_path_buf()));
        let schema = std::sync::Arc::new(minimal_schema());

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let store = std::sync::Arc::clone(&store);
                let schema = std::sync::Arc::clone(&schema);
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        store
                            .put("EQconcurrent", &schema, MatchStatus::Full)
                            .expect("put must succeed");
                        let record = store.get("EQconcurrent");
                        assert!(
                            record.is_ok(),
                            "get must never see a torn/unparsable file: {record:?}"
                        );
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("writer thread must not panic");
        }
    }
}
