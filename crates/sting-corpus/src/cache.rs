//! Disk cache for on-chain-resolved contract code/data
//! (`docs/DATA.md`).
//!
//! Written in the exact same base64-`BoC`-text format used everywhere else
//! in the project (§5.3): a `code.boc.base64`/`data.boc.base64` pair per
//! contract, one directory per address.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::source::ContractCode;

const CODE_FILE: &str = "code.boc.base64";
const DATA_FILE: &str = "data.boc.base64";

/// Default cache freshness window — a contract can be redeployed (`set_code`)
/// or have its storage mutated at any time, so caching "forever" would
/// silently serve stale bytecode; `docs/DATA.md`
/// §5.2 calls for a default TTL plus an explicit force-refresh escape hatch
/// ([`crate::CorpusController`]'s `force_refresh` — this crate only stores
/// the TTL, the controller decides whether to honor it).
pub const DEFAULT_TTL: Duration = Duration::from_hours(24);

/// Errors from [`DiskCache`] operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// [`DiskCache::default_root`] could not determine a platform cache
    /// directory (`dirs::cache_dir()` returned `None` — rare, but possible
    /// on an exotic/misconfigured environment).
    #[error("failed to determine the platform cache directory")]
    NoCacheDir,
    /// The cache entry's directory could not be created.
    #[error("failed to create cache directory {path}: {source}")]
    CreateDir {
        /// The directory that failed to be created.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A cache file could not be written.
    #[error("failed to write cache file {path}: {source}")]
    Write {
        /// The file that failed to be written.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// A disk-backed cache of [`ContractCode`], keyed by address, with a TTL.
///
/// Deliberately dumb: it does not fetch anything itself — [`Self::get`]
/// returns `None` on a miss or a stale entry, [`Self::put`] writes a fresh
/// one; the fetch-on-miss/fallback-provider orchestration lives in
/// [`crate::CorpusController`] (`docs/ARCHITECTURE.md`:
/// multi-step orchestration belongs in a controller, not a library type).
pub struct DiskCache {
    root: PathBuf,
    ttl: Duration,
}

impl DiskCache {
    /// A cache rooted at `root` with the given freshness window.
    #[must_use]
    pub fn new(root: PathBuf, ttl: Duration) -> Self {
        Self { root, ttl }
    }

    /// The platform-conventional default root: `<platform cache
    /// dir>/sting/contracts` (via the `dirs` crate — e.g.
    /// `~/.cache/sting/contracts` on Linux, `%LOCALAPPDATA%\sting\contracts`
    /// on Windows), per `docs/DATA.md`
    ///
    /// # Errors
    /// Returns [`CacheError::NoCacheDir`] if the platform cache directory
    /// cannot be determined.
    pub fn default_root() -> Result<PathBuf, CacheError> {
        dirs::cache_dir()
            .map(|dir| dir.join("sting").join("contracts"))
            .ok_or(CacheError::NoCacheDir)
    }

    fn entry_dir(&self, address: &str) -> PathBuf {
        self.root.join(cache_key(address))
    }

    /// Returns the cached [`ContractCode`] for `address`, or `None` if there
    /// is no entry, the entry is unreadable, or the entry is older than this
    /// cache's TTL.
    #[must_use]
    pub fn get(&self, address: &str) -> Option<ContractCode> {
        self.get_at(address, SystemTime::now())
    }

    fn get_at(&self, address: &str, now: SystemTime) -> Option<ContractCode> {
        let dir = self.entry_dir(address);
        let code_path = dir.join(CODE_FILE);
        let data_path = dir.join(DATA_FILE);

        let modified = std::fs::metadata(&code_path).ok()?.modified().ok()?;
        if !is_fresh(modified, self.ttl, now) {
            return None;
        }
        let code = std::fs::read_to_string(&code_path).ok()?;
        let data = std::fs::read_to_string(&data_path).ok()?;
        Some(ContractCode {
            code_boc_base64: code.trim().to_string(),
            data_boc_base64: data.trim().to_string(),
        })
    }

    /// Writes `code` as this address's cache entry, creating its directory
    /// if needed. A subsequent [`Self::get`] for the same address returns
    /// this value until the TTL elapses.
    ///
    /// # Errors
    /// See [`CacheError`].
    pub fn put(&self, address: &str, code: &ContractCode) -> Result<(), CacheError> {
        let dir = self.entry_dir(address);
        std::fs::create_dir_all(&dir).map_err(|source| CacheError::CreateDir {
            path: dir.clone(),
            source,
        })?;
        let code_path = dir.join(CODE_FILE);
        std::fs::write(&code_path, &code.code_boc_base64).map_err(|source| CacheError::Write {
            path: code_path,
            source,
        })?;
        let data_path = dir.join(DATA_FILE);
        std::fs::write(&data_path, &code.data_boc_base64).map_err(|source| CacheError::Write {
            path: data_path,
            source,
        })?;
        Ok(())
    }
}

/// `modified` (the cache entry's own mtime) is fresh relative to `now` if the
/// age is within `ttl`. Clock skew putting `modified` in the future is
/// treated as fresh (not an error case worth invalidating a cache entry
/// over) rather than propagating `SystemTime`'s subtraction error.
fn is_fresh(modified: SystemTime, ttl: Duration, now: SystemTime) -> bool {
    match now.duration_since(modified) {
        Ok(age) => age <= ttl,
        Err(_) => true,
    }
}

/// Normalizes `address` into a string safe to use as a path component or
/// filename prefix: addresses contain `:` (workchain separator), which is
/// not a safe path component on Windows — and mixed-case hex is the same
/// address as any other case, so casing is normalized too, to avoid two
/// cache entries for what toncenter/tonapi would treat as one account.
/// Public because `sting fetch --out` (`sting-cli`) reuses this exact
/// normalization for the filenames it writes — one definition, not two
/// copies that could drift — one format rather than three parsers of it
/// (`docs/DATA.md`), and the same rule applied to naming.
#[must_use]
pub fn cache_key(address: &str) -> String {
    address
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{ContractCode, DiskCache, cache_key, is_fresh};

    #[test]
    fn is_fresh_accepts_an_entry_younger_than_the_ttl() {
        let now = SystemTime::now();
        let modified = now - Duration::from_secs(10);
        assert!(is_fresh(modified, Duration::from_secs(60), now));
    }

    #[test]
    fn is_fresh_rejects_an_entry_older_than_the_ttl() {
        let now = SystemTime::now();
        let modified = now - Duration::from_secs(120);
        assert!(!is_fresh(modified, Duration::from_secs(60), now));
    }

    #[test]
    fn is_fresh_treats_future_mtime_clock_skew_as_fresh() {
        let now = SystemTime::now();
        let modified = now + Duration::from_secs(60);
        assert!(is_fresh(modified, Duration::from_secs(60), now));
    }

    #[test]
    fn cache_key_replaces_the_workchain_colon_and_normalizes_case() {
        assert_eq!(
            cache_key("0:AbCd"),
            cache_key("0:abcd"),
            "case should not create two distinct cache entries for one account"
        );
        assert!(!cache_key("0:AbCd").contains(':'));
    }

    #[test]
    fn put_then_get_round_trips_a_real_entry_on_disk() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let cache = DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
        let code = ContractCode {
            code_boc_base64: "code-bytes".to_string(),
            data_boc_base64: "data-bytes".to_string(),
        };
        cache.put("0:deadBEEF", &code).expect("writes");
        let loaded = cache.get("0:deadBEEF").expect("cache hit");
        assert_eq!(loaded, code);
    }

    #[test]
    fn get_returns_none_for_a_missing_entry() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let cache = DiskCache::new(dir.path().to_path_buf(), Duration::from_secs(60));
        assert!(cache.get("0:never-fetched").is_none());
    }

    #[test]
    fn get_returns_none_once_the_entry_has_expired() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let cache = DiskCache::new(dir.path().to_path_buf(), Duration::from_millis(10));
        let code = ContractCode {
            code_boc_base64: "code-bytes".to_string(),
            data_boc_base64: "data-bytes".to_string(),
        };
        cache.put("0:short-lived", &code).expect("writes");
        std::thread::sleep(Duration::from_millis(50));
        assert!(cache.get("0:short-lived").is_none());
    }
}
