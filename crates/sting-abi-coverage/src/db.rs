//! `CoverageDb` — the local `SQLite` persistence layer for
//! `docs/domains/method-dictionary.md`: tracks every
//! on-chain address seen during a sweep, deduplicated by `code_hash`, plus
//! the Tier 3 (`sting-abi-dictionary::probe`) result for each unique code.
//!
//! # Deliberate deviation from the written plan — where `code_hash` comes from
//!
//! The plan's §1.2 originally specified `code_hash` as `sha256(decoded code
//! BoC)`, computed locally (a `sha2` dependency). Confirmed against a real,
//! live `toncenter.com/api/v3/transactions` response while building
//! [`crate::walker::TransactionAddressWalker`]: every transaction record
//! already carries `account_state_after.code_hash` — TON's own real cell
//! hash, base64-encoded — for free, with **no extra fetch**. This is
//! strictly better than a hand-rolled `sha256(BoC bytes)`: it is the actual
//! hash TON tooling/explorers use everywhere (identical cell trees serialize
//! to different `BoC` byte strings depending on indexing options, so a flat
//! byte-hash could under-deduplicate two logically identical codes), and it
//! makes the dedup entirely free — a code hash already seen in
//! `code_samples` means the *address* can be recorded without ever fetching
//! its code at all, not just without re-probing it. `code_hash` in this
//! module's schema is therefore always this real TON cell hash (hex-encoded
//! after a base64 decode), supplied by the caller — this module never
//! computes a hash itself, and `sha2` was never added as a dependency.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension as _, params};
use sting_abi_dictionary::ProbeReport;

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS code_samples (
    code_hash TEXT PRIMARY KEY,
    code_boc_base64 TEXT NOT NULL,
    first_seen_address TEXT NOT NULL,
    first_seen_at_unix INTEGER NOT NULL,
    tier3_probed_at_unix INTEGER,
    tier3_recognized_count INTEGER,
    tier3_probed_count INTEGER,
    tier3_kind_conflict_count INTEGER,
    tier3_report_json TEXT
);
CREATE TABLE IF NOT EXISTS addresses (
    address TEXT PRIMARY KEY,
    code_hash TEXT NOT NULL REFERENCES code_samples(code_hash),
    seen_at_unix INTEGER NOT NULL,
    source TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_addresses_code_hash ON addresses(code_hash);
CREATE TABLE IF NOT EXISTS crawl_progress (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_mc_seqno INTEGER,
    last_offset INTEGER,
    updated_at_unix INTEGER NOT NULL
);
";

/// Errors from a [`CoverageDb`] operation.
#[derive(Debug, thiserror::Error)]
pub enum CoverageError {
    /// The underlying `SQLite` call failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A [`ProbeReport`] could not be serialized for storage.
    #[error("failed to serialize probe report: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// One row of `code_samples` — a unique on-chain code, its real `BoC`, and
/// (once probed) its Tier 3 result summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSample {
    /// TON's real cell hash of this code, hex-encoded (see this module's own
    /// doc comment on why this is not a locally-computed hash).
    pub code_hash: String,
    /// The code cell, base64-encoded `BoC` — the exact representation
    /// [`sting_abi_dictionary::ProbeTarget::code_boc_base64`] expects.
    pub code_boc_base64: String,
    /// The first address this code was observed at (provenance only — many
    /// other addresses may share this same code).
    pub first_seen_address: String,
    /// Unix timestamp this code was first recorded.
    pub first_seen_at_unix: i64,
    /// Unix timestamp this code was last Tier 3-probed, or `None` if never probed.
    pub tier3_probed_at_unix: Option<i64>,
    /// How many dictionary entries were recognized on this code, once probed.
    pub tier3_recognized_count: Option<i64>,
    /// How many dictionary entries were probed against this code.
    pub tier3_probed_count: Option<i64>,
    /// How many [`ProbeReport::kind_conflicts`] this code's probe produced.
    pub tier3_kind_conflict_count: Option<i64>,
    /// The full [`ProbeReport`], serialized as JSON, for later ad hoc analysis.
    pub tier3_report_json: Option<String>,
}

/// Aggregate counts across the whole database — the numbers
/// `coverage-report` prints and the decision table is filled from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoverageStats {
    /// Total rows in `addresses` (every address seen, including repeats of
    /// an already-known code).
    pub total_addresses: u64,
    /// Total distinct `code_hash` values known (`addresses.len() >=
    /// unique_code_hashes` always — the whole point of deduplication).
    pub unique_code_hashes: u64,
    /// How many of those unique codes have been Tier 3-probed so far.
    pub probed_code_hashes: u64,
    /// How many probed codes had at least one dictionary entry recognized.
    pub code_hashes_with_recognized_method: u64,
}

/// One candidate for dictionary expansion: a probed-but-unrecognized (or
/// not-yet-probed) code, ranked by how many real addresses run it — a direct
/// measure of how much real mainnet traffic this code, if added to the
/// dictionary, would newly explain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapCandidate {
    /// The unrecognized code's hash.
    pub code_hash: String,
    /// The first address this code was observed at.
    pub first_seen_address: String,
    /// How many distinct addresses in `addresses` run this exact code.
    pub address_count: u64,
}

/// Resumable cursor into the `TransactionAddressWalker`'s discovery sweep.
/// `last_mc_seqno` is reserved for a future masterchain-seqno-windowed
/// strategy — today's walker only ever populates `last_offset` (plain
/// `limit`/`offset` paging over `/api/v3/transactions`, newest first); see
/// [`crate::walker`]'s own doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CrawlProgress {
    /// Reserved and unused — always `None` today.
    pub last_mc_seqno: Option<i64>,
    /// The `offset` to resume `/api/v3/transactions` paging from.
    pub last_offset: i64,
}

/// The local coverage database — one `SQLite` file, opened once per `sweep`/
/// `coverage-report` invocation. See this module's own doc comment for the
/// schema and the `code_hash` provenance deviation from the written plan.
pub struct CoverageDb {
    conn: Connection,
}

impl CoverageDb {
    /// Opens (creating if absent) the database at `path`, ensuring the
    /// schema exists — safe to call repeatedly against the same file.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn open(path: &Path) -> Result<Self, CoverageError> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// An in-memory database — this module's own unit tests only, never a
    /// real sweep (which needs the file to persist across `--resume` runs).
    #[cfg(test)]
    fn open_in_memory() -> Result<Self, CoverageError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self, CoverageError> {
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    /// `true` if `code_hash` already has a `code_samples` row — the check a
    /// caller (the walker) makes *before* deciding whether an address's code
    /// needs fetching at all (see this module's own doc comment: a known
    /// `code_hash` from the transaction stream skips the fetch entirely, not
    /// just the probe).
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn has_code_hash(&self, code_hash: &str) -> Result<bool, CoverageError> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM code_samples WHERE code_hash = ?1",
                [code_hash],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Records a newly-fetched code sample. A no-op if `code_hash` is
    /// already known (first-seen provenance is kept, not overwritten).
    /// Returns `true` if this call actually inserted a new row.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn record_code_sample(
        &self,
        code_hash: &str,
        code_boc_base64: &str,
        first_seen_address: &str,
    ) -> Result<bool, CoverageError> {
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO code_samples \
             (code_hash, code_boc_base64, first_seen_address, first_seen_at_unix) \
             VALUES (?1, ?2, ?3, ?4)",
            params![code_hash, code_boc_base64, first_seen_address, unix_now()],
        )?;
        Ok(inserted > 0)
    }

    /// Records (or refreshes) that `address` was seen running `code_hash`.
    /// `code_hash` must already have a `code_samples` row (the walker always
    /// calls [`Self::record_code_sample`] first for a newly-discovered
    /// code) — this is a documented ordering contract, not enforced via a
    /// `SQLite` foreign-key pragma (this crate does not turn `PRAGMA
    /// foreign_keys` on, matching `SQLite`'s own off-by-default behavior).
    /// Returns `true` if `address` was not already known — a caller counting
    /// "how many genuinely new addresses has this sweep found" (e.g.
    /// `--pilot-size`'s stopping condition) must use this, not just "how
    /// many discovered-stream entries were processed": real mainnet
    /// transaction traffic is dominated by a small set of hyperactive
    /// senders (routers, highload wallets), so most processed entries
    /// re-see an already-known address — confirmed by a real pilot run
    /// (`docs/domains/method-dictionary.md`): 5,000 discovered-stream
    /// entries collapsed to 998 distinct addresses.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn record_address_seen(
        &self,
        address: &str,
        code_hash: &str,
        source: &str,
    ) -> Result<bool, CoverageError> {
        let already_known: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM addresses WHERE address = ?1",
                [address],
                |row| row.get(0),
            )
            .optional()?;
        self.conn.execute(
            "INSERT INTO addresses (address, code_hash, seen_at_unix, source) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(address) DO UPDATE SET \
                code_hash = excluded.code_hash, \
                seen_at_unix = excluded.seen_at_unix, \
                source = excluded.source",
            params![address, code_hash, unix_now(), source],
        )?;
        Ok(already_known.is_none())
    }

    /// Up to `limit` code samples that have never been Tier 3-probed —
    /// what a `sweep` run's probe pass works through.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn unprobed_code_hashes(&self, limit: usize) -> Result<Vec<CodeSample>, CoverageError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT code_hash, code_boc_base64, first_seen_address, first_seen_at_unix, \
                    tier3_probed_at_unix, tier3_recognized_count, tier3_probed_count, \
                    tier3_kind_conflict_count, tier3_report_json \
             FROM code_samples WHERE tier3_probed_at_unix IS NULL LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_code_sample)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Every code sample the database holds, probed or not — what a
    /// measurement over the whole set of unique on-chain codes iterates.
    ///
    /// The whole set is loaded at once rather than paged: a sweep's own
    /// numbers put it in the hundreds, and every caller needs all of it.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn all_code_samples(&self) -> Result<Vec<CodeSample>, CoverageError> {
        let mut stmt = self.conn.prepare(
            "SELECT code_hash, code_boc_base64, first_seen_address, first_seen_at_unix,                     tier3_probed_at_unix, tier3_recognized_count, tier3_probed_count,                     tier3_kind_conflict_count, tier3_report_json              FROM code_samples ORDER BY first_seen_at_unix",
        )?;
        let rows = stmt.query_map([], row_to_code_sample)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Clears the probed-at timestamp (and summary counts) for every code
    /// sample that was probed but recognized nothing (`tier3_recognized_count`
    /// `0` or `NULL`) — makes [`Self::unprobed_code_hashes`] return them
    /// again, so a following probe pass re-checks them against a *newer*
    /// dictionary. Real gap found 2026-07-22: `unprobed_code_hashes` only
    /// ever returns codes with `tier3_probed_at_unix IS NULL`, so growing
    /// the dictionary *after* a code was first probed (`abi-layer
    /// dictionary-build`, run independently of `sweep`) never gave that
    /// code a second chance — confirmed live: a real address recognized
    /// `0` methods on its first probe, then correctly recognized `1` after
    /// its dictionary entry was added and this method was used to force a
    /// re-check. Already-recognized codes are left untouched — dictionary
    /// growth is additive (this codebase's own `dictionary-build` reports
    /// `0` real collisions in practice), so a code that already recognized
    /// something has no realistic need to be re-probed.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn reset_unrecognized_probes(&self) -> Result<usize, CoverageError> {
        let changed = self.conn.execute(
            "UPDATE code_samples SET \
                tier3_probed_at_unix = NULL, \
                tier3_recognized_count = NULL, \
                tier3_probed_count = NULL, \
                tier3_kind_conflict_count = NULL, \
                tier3_report_json = NULL \
             WHERE tier3_probed_at_unix IS NOT NULL \
               AND COALESCE(tier3_recognized_count, 0) = 0",
            [],
        )?;
        Ok(changed)
    }

    /// Records `code_hash`'s Tier 3 [`ProbeReport`] — both the summary
    /// counts (`coverage_stats`/`dictionary_gap_candidates` read these) and
    /// the full report as JSON, for later ad hoc analysis.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn record_probe_result(
        &self,
        code_hash: &str,
        report: &ProbeReport,
    ) -> Result<(), CoverageError> {
        let report_json = serde_json::to_string(report)?;
        let recognized_count = i64::try_from(report.recognized.len()).unwrap_or(i64::MAX);
        let probed_count = i64::try_from(report.probed_count).unwrap_or(i64::MAX);
        let kind_conflict_count = i64::try_from(report.kind_conflicts.len()).unwrap_or(i64::MAX);
        self.conn.execute(
            "UPDATE code_samples SET \
                tier3_probed_at_unix = ?1, \
                tier3_recognized_count = ?2, \
                tier3_probed_count = ?3, \
                tier3_kind_conflict_count = ?4, \
                tier3_report_json = ?5 \
             WHERE code_hash = ?6",
            params![
                unix_now(),
                recognized_count,
                probed_count,
                kind_conflict_count,
                report_json,
                code_hash
            ],
        )?;
        Ok(())
    }

    /// Aggregate coverage numbers — see [`CoverageStats`].
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn coverage_stats(&self) -> Result<CoverageStats, CoverageError> {
        Ok(CoverageStats {
            total_addresses: self.count_rows("SELECT COUNT(*) FROM addresses")?,
            unique_code_hashes: self.count_rows("SELECT COUNT(*) FROM code_samples")?,
            probed_code_hashes: self.count_rows(
                "SELECT COUNT(*) FROM code_samples WHERE tier3_probed_at_unix IS NOT NULL",
            )?,
            code_hashes_with_recognized_method: self
                .count_rows("SELECT COUNT(*) FROM code_samples WHERE tier3_recognized_count > 0")?,
        })
    }

    fn count_rows(&self, sql: &str) -> Result<u64, CoverageError> {
        let n: i64 = self.conn.query_row(sql, [], |row| row.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Unrecognized (zero or never-probed) codes, ranked by how many real
    /// addresses run them — the dictionary-expansion worklist
    /// `docs/domains/method-dictionary.md`'s whole reason for
    /// tracking coverage per code, not per address. Only `min_address_count`
    /// or more distinct addresses qualify — a code seen at exactly one
    /// address is unlikely to be worth manually adding to the dictionary.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn dictionary_gap_candidates(
        &self,
        min_address_count: u64,
    ) -> Result<Vec<GapCandidate>, CoverageError> {
        let min = i64::try_from(min_address_count).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT cs.code_hash, cs.first_seen_address, COUNT(a.address) AS address_count \
             FROM code_samples cs \
             JOIN addresses a ON a.code_hash = cs.code_hash \
             WHERE COALESCE(cs.tier3_recognized_count, 0) = 0 \
             GROUP BY cs.code_hash \
             HAVING address_count >= ?1 \
             ORDER BY address_count DESC",
        )?;
        let rows = stmt.query_map(params![min], |row| {
            let address_count: i64 = row.get(2)?;
            Ok(GapCandidate {
                code_hash: row.get(0)?,
                first_seen_address: row.get(1)?,
                address_count: u64::try_from(address_count).unwrap_or(0),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// The walker's saved resume point, or `None` if `sweep` has never run
    /// against this database.
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn crawl_progress(&self) -> Result<Option<CrawlProgress>, CoverageError> {
        let progress = self
            .conn
            .query_row(
                "SELECT last_mc_seqno, last_offset FROM crawl_progress WHERE id = 1",
                [],
                |row| {
                    Ok(CrawlProgress {
                        last_mc_seqno: row.get(0)?,
                        last_offset: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    })
                },
            )
            .optional()?;
        Ok(progress)
    }

    /// Saves the walker's resume point (`--resume` reads this back via
    /// [`Self::crawl_progress`]).
    ///
    /// # Errors
    /// See [`CoverageError`].
    pub fn set_crawl_progress(&self, last_offset: i64) -> Result<(), CoverageError> {
        self.conn.execute(
            "INSERT INTO crawl_progress (id, last_mc_seqno, last_offset, updated_at_unix) \
             VALUES (1, NULL, ?1, ?2) \
             ON CONFLICT(id) DO UPDATE SET \
                last_offset = excluded.last_offset, \
                updated_at_unix = excluded.updated_at_unix",
            params![last_offset, unix_now()],
        )?;
        Ok(())
    }
}

fn row_to_code_sample(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodeSample> {
    Ok(CodeSample {
        code_hash: row.get(0)?,
        code_boc_base64: row.get(1)?,
        first_seen_address: row.get(2)?,
        first_seen_at_unix: row.get(3)?,
        tier3_probed_at_unix: row.get(4)?,
        tier3_recognized_count: row.get(5)?,
        tier3_probed_count: row.get(6)?,
        tier3_kind_conflict_count: row.get(7)?,
        tier3_report_json: row.get(8)?,
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "a real unix timestamp comfortably fits i64 for the next ~292 billion years; this mirrors the same best-effort 'current time' pattern already used in sting-abi-cli/src/dictionary.rs for c7's NOW"
)]
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_abi_dictionary::dictionary::ContractKind;
    use sting_abi_dictionary::{ProbeReport, RecognizedMethod};

    use super::CoverageDb;

    fn sample_report(recognized_names: &[&str]) -> ProbeReport {
        ProbeReport {
            address: "0:test".to_string(),
            recognized: recognized_names
                .iter()
                .map(|name| RecognizedMethod {
                    name: (*name).to_string(),
                    tvm_method_id: 0,
                    stack_depth: 0,
                    seen_in: Vec::new(),
                    contract_kind: ContractKind::Other,
                })
                .collect(),
            probed_count: 10,
            inconclusive_count: 0,
            dictionary_size: 10,
            kind_conflicts: Vec::new(),
        }
    }

    #[test]
    fn record_code_sample_is_idempotent_and_reports_whether_it_was_new() {
        let db = CoverageDb::open_in_memory().expect("open");
        assert!(
            db.record_code_sample("hash-a", "boc-a", "0:addr-1")
                .expect("insert")
        );
        assert!(
            !db.record_code_sample("hash-a", "boc-a-different", "0:addr-2")
                .expect("no-op on an already-known hash"),
            "a second address sharing the same code_hash must not create a second code_samples row"
        );
    }

    #[test]
    fn has_code_hash_reflects_what_was_recorded() {
        let db = CoverageDb::open_in_memory().expect("open");
        assert!(!db.has_code_hash("hash-a").expect("query"));
        db.record_code_sample("hash-a", "boc-a", "0:addr-1")
            .expect("insert");
        assert!(db.has_code_hash("hash-a").expect("query"));
    }

    #[test]
    fn record_address_seen_updates_an_existing_address_rather_than_erroring() {
        let db = CoverageDb::open_in_memory().expect("open");
        db.record_code_sample("hash-a", "boc-a", "0:addr-1")
            .expect("insert");
        db.record_code_sample("hash-b", "boc-b", "0:addr-1")
            .expect("insert");
        assert!(
            db.record_address_seen("0:addr-1", "hash-a", "toncenter")
                .expect("first seen"),
            "an address never seen before must report true"
        );
        assert!(
            !db.record_address_seen("0:addr-1", "hash-b", "toncenter")
                .expect("re-seen with a different code (redeploy) must not error"),
            "an already-known address must report false, even with a changed code_hash"
        );

        let stats = db.coverage_stats().expect("stats");
        assert_eq!(
            stats.total_addresses, 1,
            "re-seeing the same address must update the row, not duplicate it"
        );
    }

    #[test]
    fn coverage_stats_counts_addresses_by_row_and_code_samples_by_unique_hash() {
        let db = CoverageDb::open_in_memory().expect("open");
        db.record_code_sample("hash-a", "boc-a", "0:addr-1")
            .expect("insert");
        db.record_address_seen("0:addr-1", "hash-a", "toncenter")
            .expect("seen");
        db.record_address_seen("0:addr-2", "hash-a", "toncenter")
            .expect("second address sharing the same code");
        db.record_address_seen("0:addr-3", "hash-a", "toncenter")
            .expect("third address sharing the same code");

        let stats = db.coverage_stats().expect("stats");
        assert_eq!(stats.total_addresses, 3);
        assert_eq!(
            stats.unique_code_hashes, 1,
            "three addresses sharing one code must count as one unique code sample"
        );
        assert_eq!(stats.probed_code_hashes, 0);
    }

    #[test]
    fn unprobed_code_hashes_excludes_already_probed_codes() {
        let db = CoverageDb::open_in_memory().expect("open");
        db.record_code_sample("hash-a", "boc-a", "0:addr-1")
            .expect("insert");
        db.record_code_sample("hash-b", "boc-b", "0:addr-2")
            .expect("insert");
        db.record_probe_result("hash-a", &sample_report(&["get_x"]))
            .expect("record probe");

        let unprobed = db.unprobed_code_hashes(10).expect("query");
        assert_eq!(unprobed.len(), 1);
        assert_eq!(unprobed[0].code_hash, "hash-b");
    }

    #[test]
    fn reset_unrecognized_probes_makes_them_unprobed_again_but_leaves_recognized_ones_alone() {
        let db = CoverageDb::open_in_memory().expect("open");
        db.record_code_sample("hash-recognized", "boc-a", "0:addr-1")
            .expect("insert");
        db.record_code_sample("hash-unrecognized", "boc-b", "0:addr-2")
            .expect("insert");
        db.record_probe_result("hash-recognized", &sample_report(&["get_x"]))
            .expect("probe: recognized something");
        db.record_probe_result("hash-unrecognized", &sample_report(&[]))
            .expect("probe: recognized nothing");

        // Both codes are probed, neither shows up as unprobed yet.
        assert!(db.unprobed_code_hashes(10).expect("query").is_empty());

        let reset_count = db.reset_unrecognized_probes().expect("reset");
        assert_eq!(reset_count, 1, "only the unrecognized code must be reset");

        let unprobed = db.unprobed_code_hashes(10).expect("query");
        assert_eq!(
            unprobed.len(),
            1,
            "the previously-unrecognized code must be probeable again"
        );
        assert_eq!(unprobed[0].code_hash, "hash-unrecognized");

        let stats = db.coverage_stats().expect("stats");
        assert_eq!(
            stats.probed_code_hashes, 1,
            "the recognized code must still count as probed"
        );
    }

    #[test]
    fn record_probe_result_updates_stats_and_round_trips_the_report_json() {
        let db = CoverageDb::open_in_memory().expect("open");
        db.record_code_sample("hash-a", "boc-a", "0:addr-1")
            .expect("insert");
        db.record_probe_result(
            "hash-a",
            &sample_report(&["get_jetton_data", "get_wallet_data"]),
        )
        .expect("record probe");

        let stats = db.coverage_stats().expect("stats");
        assert_eq!(stats.probed_code_hashes, 1);
        assert_eq!(stats.code_hashes_with_recognized_method, 1);

        let unprobed = db.unprobed_code_hashes(10).expect("query");
        assert!(
            unprobed.is_empty(),
            "a probed code must not appear in unprobed_code_hashes anymore"
        );
    }

    #[test]
    fn record_probe_result_with_no_recognized_methods_does_not_count_as_covered() {
        let db = CoverageDb::open_in_memory().expect("open");
        db.record_code_sample("hash-a", "boc-a", "0:addr-1")
            .expect("insert");
        db.record_probe_result("hash-a", &sample_report(&[]))
            .expect("record probe");

        let stats = db.coverage_stats().expect("stats");
        assert_eq!(stats.probed_code_hashes, 1);
        assert_eq!(stats.code_hashes_with_recognized_method, 0);
    }

    #[test]
    fn dictionary_gap_candidates_ranks_unrecognized_codes_by_address_frequency() {
        let db = CoverageDb::open_in_memory().expect("open");
        db.record_code_sample("popular-unknown", "boc-a", "0:addr-1")
            .expect("insert");
        db.record_code_sample("rare-unknown", "boc-b", "0:addr-4")
            .expect("insert");
        db.record_code_sample("recognized", "boc-c", "0:addr-5")
            .expect("insert");

        for addr in ["0:addr-1", "0:addr-2", "0:addr-3"] {
            db.record_address_seen(addr, "popular-unknown", "toncenter")
                .expect("seen");
        }
        db.record_address_seen("0:addr-4", "rare-unknown", "toncenter")
            .expect("seen");
        db.record_address_seen("0:addr-5", "recognized", "toncenter")
            .expect("seen");

        db.record_probe_result("popular-unknown", &sample_report(&[]))
            .expect("probe: nothing recognized");
        db.record_probe_result("rare-unknown", &sample_report(&[]))
            .expect("probe: nothing recognized");
        db.record_probe_result("recognized", &sample_report(&["get_jetton_data"]))
            .expect("probe: recognized");

        let gaps = db.dictionary_gap_candidates(1).expect("query");
        assert_eq!(
            gaps.len(),
            2,
            "the recognized code must not appear as a gap"
        );
        assert_eq!(gaps[0].code_hash, "popular-unknown");
        assert_eq!(gaps[0].address_count, 3);
        assert_eq!(gaps[1].code_hash, "rare-unknown");
        assert_eq!(gaps[1].address_count, 1);
    }

    #[test]
    fn dictionary_gap_candidates_respects_the_min_address_count_threshold() {
        let db = CoverageDb::open_in_memory().expect("open");
        db.record_code_sample("rare-unknown", "boc-a", "0:addr-1")
            .expect("insert");
        db.record_address_seen("0:addr-1", "rare-unknown", "toncenter")
            .expect("seen");
        db.record_probe_result("rare-unknown", &sample_report(&[]))
            .expect("probe");

        assert!(db.dictionary_gap_candidates(2).expect("query").is_empty());
        assert_eq!(db.dictionary_gap_candidates(1).expect("query").len(), 1);
    }

    #[test]
    fn crawl_progress_round_trips_and_starts_absent() {
        let db = CoverageDb::open_in_memory().expect("open");
        assert!(db.crawl_progress().expect("query").is_none());

        db.set_crawl_progress(150).expect("save");
        let progress = db.crawl_progress().expect("query").expect("now present");
        assert_eq!(progress.last_offset, 150);

        db.set_crawl_progress(300).expect("save again");
        let progress = db.crawl_progress().expect("query").expect("still present");
        assert_eq!(
            progress.last_offset, 300,
            "a second save must update the singleton row, not duplicate it"
        );
    }
}
