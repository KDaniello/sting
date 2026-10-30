//! `abi-layer sweep` / `abi-layer coverage-report` — thin CLI wiring around
//! `sting-abi-coverage`, mirroring `dictionary.rs`'s own "argument parsing
//! plus a call into the library crate" convention.
//! `docs/domains/method-dictionary.md`

use std::path::Path;

use anyhow::Context as _;
use sting_abi_coverage::{CoverageDb, CoverageStats, TransactionAddressWalker};
use sting_abi_dictionary::{MethodDictionary, ProbeTarget, TactVerdict, probe, signature_of};
use sting_corpus::{
    ContractSource, ContractTarget, CorpusController, DiskCache, FetchOptions, TonapiClient,
    ToncenterClient,
};

/// One discovery page's `limit` — moderate rather than maximal, because a
/// sweep runs against somebody else's public service.
const PAGE_LIMIT: u32 = 50;
/// `addresses.source` value for every row this command writes — distinct
/// from `ContractSource::name()` (`"toncenter v3"`/`"tonapi v2"`, which
/// provider resolved a given address's *code*) because discovery and
/// code-fetch are two different steps that can genuinely disagree.
const DISCOVERY_SOURCE: &str = "toncenter-v3-transactions";

/// Runs, or resumes, a deduplicated discovery and Tier 3 probe sweep
/// (`docs/domains/method-dictionary.md` §6).
pub fn run_sweep(
    db_path: &Path,
    dictionary_path: &Path,
    pilot_size: usize,
    resume: bool,
    reprobe: bool,
) -> anyhow::Result<()> {
    let db = CoverageDb::open(db_path).with_context(|| format!("opening {}", db_path.display()))?;
    if reprobe {
        let reset = db
            .reset_unrecognized_probes()
            .context("resetting previously-unrecognized probes")?;
        println!("Reset {reset} previously-unrecognized code(s) for re-probing.");
    }
    let corpus_controller = build_corpus_controller()?;
    let walker = TransactionAddressWalker::new();

    let mut offset: u32 = if resume {
        db.crawl_progress()
            .context("reading saved crawl progress")?
            .map_or(0, |p| u32::try_from(p.last_offset).unwrap_or(0))
    } else {
        0
    };

    let mut newly_recorded = 0usize;
    let mut records_processed = 0usize;
    let mut fetch_failures = 0usize;
    println!(
        "Starting discovery at offset {offset}, target {pilot_size} newly recorded address(es)..."
    );

    while newly_recorded < pilot_size {
        let page = walker
            .fetch_page(PAGE_LIMIT, offset)
            .with_context(|| format!("fetching transactions page at offset {offset}"))?;
        if page.record_count == 0 {
            println!("Reached the end of the transaction stream at offset {offset}.");
            break;
        }

        for discovered in &page.discovered {
            if newly_recorded >= pilot_size {
                break;
            }
            if !db.has_code_hash(&discovered.code_hash)? {
                match corpus_controller.resolve(
                    &ContractTarget::OnChainAddress(discovered.address.clone()),
                    FetchOptions::default(),
                ) {
                    Ok(code) => {
                        db.record_code_sample(
                            &discovered.code_hash,
                            &code.code_boc_base64,
                            &discovered.address,
                        )?;
                    }
                    Err(err) => {
                        fetch_failures += 1;
                        eprintln!("SKIP {}: failed to fetch code: {err}", discovered.address);
                        continue;
                    }
                }
            }
            records_processed += 1;
            // Real mainnet transaction traffic is dominated by a small set
            // of hyperactive senders (routers, highload wallets) — most
            // `discovered` entries re-see an address already recorded by an
            // earlier page. Only a genuinely new address counts toward
            // `pilot_size`, per `CoverageDb::record_address_seen`'s own doc
            // comment; `records_processed` is kept purely to report the real
            // repeat rate, not to gate anything.
            if db.record_address_seen(
                &discovered.address,
                &discovered.code_hash,
                DISCOVERY_SOURCE,
            )? {
                newly_recorded += 1;
            }
        }

        offset += PAGE_LIMIT;
        db.set_crawl_progress(i64::from(offset))?;
        // No pause here: the pace between calls is the chain client's, kept
        // at the tier the key in the environment actually buys. A second
        // pause on top of it would be a second answer to one question.
    }

    println!(
        "Discovery done: {newly_recorded} address(es) newly recorded ({records_processed} transaction-stream entries processed, {fetch_failures} code-fetch failure(s))."
    );

    run_probe_pass(&db, &corpus_controller, dictionary_path)?;

    print_stats(&db.coverage_stats()?);
    Ok(())
}

/// Probes every code sample that has not been Tier 3-probed yet. Needs a
/// real `data` cell alongside each sample's stored `code`, which
/// `code_samples` deliberately does not persist (data is per-address, not
/// per-code — storing it there would defeat the whole point of
/// deduplicating by code hash) — re-resolves `first_seen_address` through
/// the same `corpus_controller` instead, which serves it from
/// `DiskCache` (just written moments earlier during discovery) rather than
/// a second real network round trip in the common case.
fn run_probe_pass(
    db: &CoverageDb,
    corpus_controller: &CorpusController,
    dictionary_path: &Path,
) -> anyhow::Result<()> {
    let dictionary = MethodDictionary::load(dictionary_path)
        .with_context(|| format!("loading {}", dictionary_path.display()))?;
    let lib = quiet_library()?;

    #[allow(
        clippy::cast_possible_truncation,
        reason = "a real unix timestamp comfortably fits u32 until year 2106; matches dictionary.rs's own run_probe"
    )]
    let unixtime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32);

    let mut probed = 0usize;
    let mut skipped = 0usize;
    loop {
        let batch = db.unprobed_code_hashes(200)?;
        if batch.is_empty() {
            break;
        }
        for sample in &batch {
            let code = match corpus_controller.resolve(
                &ContractTarget::OnChainAddress(sample.first_seen_address.clone()),
                FetchOptions::default(),
            ) {
                Ok(code) => code,
                Err(err) => {
                    skipped += 1;
                    eprintln!(
                        "SKIP probe {}: failed to re-fetch a data cell for {}: {err}",
                        sample.code_hash, sample.first_seen_address
                    );
                    // Give up on this code for this run rather than looping
                    // forever — a permanently unresolvable address would
                    // otherwise keep coming back from `unprobed_code_hashes`
                    // every iteration. Recorded as an empty probe result so
                    // it stops showing up as unprobed.
                    db.record_probe_result(
                        &sample.code_hash,
                        &sting_abi_dictionary::ProbeReport {
                            address: sample.first_seen_address.clone(),
                            recognized: Vec::new(),
                            probed_count: 0,
                            inconclusive_count: 0,
                            dictionary_size: dictionary.len(),
                            kind_conflicts: Vec::new(),
                        },
                    )?;
                    continue;
                }
            };
            let target = ProbeTarget {
                code_boc_base64: &code.code_boc_base64,
                data_boc_base64: &code.data_boc_base64,
                address: &sample.first_seen_address,
                unixtime,
                balance: 0,
            };
            match probe(&lib, &target, &dictionary) {
                Ok(report) => {
                    db.record_probe_result(&sample.code_hash, &report)?;
                    probed += 1;
                }
                Err(err) => {
                    skipped += 1;
                    eprintln!("SKIP probe {}: {err}", sample.code_hash);
                }
            }
        }
    }
    println!("Probe pass done: {probed} code(s) probed, {skipped} skipped.");
    Ok(())
}

/// The emulator, with its own logging off.
///
/// Every command here probes hundreds of contracts, and `libemulator` writes a
/// full VM trace per call at its default verbosity — the trace is a debugging
/// tool, and a sweep's output is a measurement.
fn quiet_library() -> anyhow::Result<sting_emulator_sys::LibEmulator> {
    let lib = sting_emulator::load_default_library().context(
        "loading libemulator — see vendor/libemulator/ in .gitignore for how to fetch it",
    )?;
    let _ = sting_emulator::set_verbosity_level(&lib, 0);
    Ok(lib)
}

fn build_corpus_controller() -> anyhow::Result<CorpusController> {
    let cache_root =
        sting_corpus::DiskCache::default_root().context("determining the platform cache dir")?;
    let sources: Vec<Box<dyn ContractSource>> = vec![
        Box::new(ToncenterClient::new()),
        Box::new(TonapiClient::new()),
    ];
    Ok(CorpusController::new(
        sources,
        DiskCache::new(cache_root, sting_corpus::DEFAULT_TTL),
    ))
}

fn print_stats(stats: &CoverageStats) {
    println!(
        "total_addresses={} unique_code_hashes={} probed_code_hashes={} code_hashes_with_recognized_method={}",
        stats.total_addresses,
        stats.unique_code_hashes,
        stats.probed_code_hashes,
        stats.code_hashes_with_recognized_method
    );
}

/// Counts, over every unique on-chain code the database holds, how many
/// declare themselves Tact (`docs/domains/method-dictionary.md` §8).
///
/// The share is a band rather than a point: a code carrying one marker is not
/// resolved by guessing, so it is printed on its own line and both ends of the
/// band are stated. Both ends are ends of what *declares* itself — Tact's
/// markers are optional and off by default since 1.6.0, so this bounds the
/// declared share and not the real one.
pub fn run_tact_share(db_path: &Path) -> anyhow::Result<()> {
    let db = CoverageDb::open(db_path).with_context(|| format!("opening {}", db_path.display()))?;
    let samples = db.all_code_samples()?;
    let corpus_controller = build_corpus_controller()?;
    let lib = quiet_library()?;

    let mut emitted = 0usize;
    let mut one_marker = 0usize;
    let mut undecided = 0usize;
    let mut absent = 0usize;
    let mut without_data = 0usize;
    let mut probe_failed = 0usize;
    for sample in &samples {
        let Ok(code) = corpus_controller.resolve(
            &ContractTarget::OnChainAddress(sample.first_seen_address.clone()),
            FetchOptions::default(),
        ) else {
            // A code whose first-seen address no longer resolves is named
            // and counted apart, never folded into "not Tact" — a probe that
            // did not run is not a negative result.
            without_data += 1;
            eprintln!(
                "NO DATA CELL {}: {} does not resolve",
                sample.code_hash, sample.first_seen_address
            );
            continue;
        };
        let target = ProbeTarget {
            code_boc_base64: &code.code_boc_base64,
            data_boc_base64: &code.data_boc_base64,
            address: &sample.first_seen_address,
            // Fixed rather than the wall clock: none of the three markers
            // reads `NOW`, and a measurement that changes with the hour it ran
            // at is a measurement nobody can repeat.
            unixtime: 0,
            balance: 0,
        };
        match signature_of(&lib, &target).map(|signature| signature.verdict()) {
            Ok(TactVerdict::Emitted) => emitted += 1,
            Ok(TactVerdict::OneMarker) => one_marker += 1,
            Ok(TactVerdict::Undecided) => undecided += 1,
            Ok(TactVerdict::Absent) => absent += 1,
            Err(err) => {
                probe_failed += 1;
                eprintln!("PROBE FAILED {}: {err}", sample.code_hash);
            }
        }
    }

    let probed = emitted + one_marker + undecided + absent;
    println!(
        "unique_code_hashes={} probed={probed} tact_emitted={emitted} one_marker={one_marker} \
         undecided={undecided} not_tact={absent} without_data_cell={without_data} probe_failed={probe_failed}",
        samples.len()
    );
    if probed > 0 {
        let share = |count: usize| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "counts here are in the hundreds; f64 is exact far past that"
            )]
            let ratio = count as f64 / probed as f64;
            ratio * 100.0
        };
        // The upper bound carries everything not ruled out: one marker,
        // and every code a probe stayed silent on.
        println!(
            "tact_declared_at_least={:.1}% tact_declared_at_most={:.1}%",
            share(emitted),
            share(emitted + one_marker + undecided)
        );
    }
    Ok(())
}

/// Prints `CoverageStats` plus the top dictionary-expansion candidates.
pub fn run_coverage_report(
    db_path: &Path,
    min_address_count: u64,
    top: usize,
) -> anyhow::Result<()> {
    let db = CoverageDb::open(db_path).with_context(|| format!("opening {}", db_path.display()))?;
    print_stats(&db.coverage_stats()?);

    let gaps = db.dictionary_gap_candidates(min_address_count)?;
    println!(
        "\nTop {} dictionary gap candidate(s) (>= {min_address_count} address(es) each, {} total):",
        top.min(gaps.len()),
        gaps.len()
    );
    for gap in gaps.iter().take(top) {
        println!(
            "  {} — {} address(es), first seen at {}",
            gap.code_hash, gap.address_count, gap.first_seen_address
        );
    }
    Ok(())
}
