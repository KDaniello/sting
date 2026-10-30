//! `abi-layer dictionary-build` / `abi-layer dictionary-probe` — thin CLI
//! wiring around `sting-abi-dictionary`, mirroring `bench.rs`'s own
//! "argument parsing plus a call into the library crate" convention.

use std::path::Path;

use anyhow::Context as _;
use sting_abi_bench::corpus;
use sting_abi_core::Extractor;
use sting_abi_dictionary::{
    CorpusSource, MethodDictionary, OperationDictionary, ProbeTarget, probe,
};
use sting_abi_func::FuncExtractor;
use sting_abi_tact::{TactCompiler, TactExtractor};
use sting_abi_tolk::TolkExtractor;
use sting_corpus::{
    ContractSource, ContractTarget, CorpusController, DiskCache, FetchOptions, TonapiClient,
    ToncenterClient,
};
use sting_tolkc::TolkCompiler;

/// Rebuilds the method-id dictionary from every real get-method this
/// workspace's own corpora know about — Tier 1 Tolk/Tact (`abi-layer
/// bench`'s own two Tolk roots, `corpus/clean/{acton-contracts,
/// tolk-bench}`, plus a real, deployed Tolk growth corpus,
/// `corpus-abi/clean/tolk/`, not shared with `bench`; `--corpus` for Tact,
/// plus a real, deployed Tact growth corpus, `corpus-abi/clean/
/// tact-registry/`, likewise not shared with `bench`) plus a real, deployed
/// `FunC` corpus (`--func-corpus`, default `corpus-abi/clean/func/` — real
/// contracts chosen specifically because a mainnet sweep found them as
/// unrecognized dictionary gaps, see that directory's own `SOURCE.md`),
/// reusing `sting_abi_bench::corpus`'s discovery so this never drifts from
/// what `bench` itself considers "the real corpus."
pub fn build_dictionary(
    tact_corpus_root: &Path,
    func_corpus_root: &Path,
    output: &Path,
    operations_output: &Path,
) -> anyhow::Result<()> {
    let tolk_compiler = TolkCompiler::vendored_or_default().context(
        "no Tolk compiler is reachable: see docs/domains/extractors.md for vendoring it",
    )?;
    let tolk_extractor = TolkExtractor::new(tolk_compiler);
    let tact_compiler = TactCompiler::vendored_or_default().context(
        "no Tact compiler is reachable: see docs/domains/extractors.md for vendoring it",
    )?;
    let tact_extractor = TactExtractor::new(tact_compiler);
    let func_extractor = FuncExtractor;

    let tolk_roots = tolk_roots();
    // Real, deployed Tact growth corpus, same purpose and provenance rule as
    // `corpus-abi/clean/tolk` — see `corpus-abi/clean/tact-registry/SOURCE.md`. Deliberately a
    // separate root from `tact_corpus_root` (`bench.rs`'s own `--corpus`, which feeds the accuracy
    // baseline) for the identical reason: bulk-imported, unvetted growth contracts must never
    // silently become part of that baseline.
    let tact_roots = [
        tact_corpus_root,
        Path::new("corpus-abi/clean/tact-registry"),
    ];
    let func_roots = [func_corpus_root];

    let tolk_projects = corpus::discover_tolk_projects(&tolk_roots);
    let mut tact_projects = Vec::new();
    for root in tact_roots {
        tact_projects.extend(corpus::discover_tact_projects(root));
    }
    let func_projects = corpus::discover_func_projects(func_corpus_root);

    let mut dictionary = MethodDictionary::new();
    // One walk, two datasets: extraction is what costs minutes, and the two are two views of the
    // same schemas — a get method's id and a message's opcode.
    let mut operations = OperationDictionary::new();
    let mut learned = 0usize;
    let mut skipped = 0usize;

    for project in &tolk_projects {
        match tolk_extractor.extract(&project.project_dir, &project.entrypoint) {
            Ok(schema) => {
                dictionary.learn_from_schema(&schema, &project.label);
                operations.learn_from_schema(&schema, &project.label);
                learned += 1;
            }
            Err(err) => {
                eprintln!("SKIP {}: {err}", project.label);
                skipped += 1;
            }
        }
    }
    for project in &tact_projects {
        match tact_extractor.extract(&project.project_dir, &project.entrypoint) {
            Ok(schema) => {
                dictionary.learn_from_schema(&schema, &project.label);
                operations.learn_from_schema(&schema, &project.label);
                learned += 1;
            }
            Err(err) => {
                eprintln!("SKIP {}: {err}", project.label);
                skipped += 1;
            }
        }
    }
    // FunC entries carry the same crc16-derived `tvm_method_id` formula as
    // Tolk/Tact (`docs/abi-layer/critical`'s method-id convention) — not a
    // guess, a deterministic hash of the real get-method name — but are
    // heuristically (Tier 2), not compiler-, extracted, so `project.label`
    // (the file name, e.g. `governed-jetton-wallet.fc`) is the only
    // provenance marker distinguishing them from compiler-verified entries
    // in `seen_in`, deliberately not a separate trust-level field (no
    // consumer needs that distinction yet — would be speculative).
    for project in &func_projects {
        match func_extractor.extract(&project.project_dir, &project.entrypoint) {
            Ok(schema) => {
                dictionary.learn_from_schema(&schema, &project.label);
                operations.learn_from_schema(&schema, &project.label);
                learned += 1;
            }
            Err(err) => {
                eprintln!("SKIP {}: {err}", project.label);
                skipped += 1;
            }
        }
    }

    // Where the contracts themselves came from. Per-entry provenance names a file; this names the
    // collection that file belongs to, and points at where that collection's own origin is written
    // down. A dataset that leaves this repository has to answer both, because "seen in
    // `counter/Counter.tolk`" means nothing to somebody who does not have this checkout.
    let sources = corpus_sources(
        tolk_roots
            .iter()
            .copied()
            .chain(tact_roots)
            .chain(func_roots),
    )?;
    dictionary.record_sources(sources.clone());
    operations.record_sources(sources);

    println!(
        "Learned from {learned} contract(s) ({skipped} skipped) — {} distinct method id(s), {} probeable (unambiguous, zero-argument), {} real collision(s)",
        dictionary.len(),
        dictionary.probeable_entries().count(),
        dictionary.collisions().count()
    );
    for (id, group) in dictionary.collisions() {
        let names: Vec<&str> = group.iter().map(|e| e.name.as_str()).collect();
        println!("  collision at id {id}: {names:?}");
    }
    println!(
        "  {} distinct message opcode(s), {} of them claimed by more than one operation",
        operations.len(),
        operations.collisions().count()
    );

    dictionary
        .save(output)
        .with_context(|| format!("writing {}", output.display()))?;
    println!("Wrote {}", output.display());
    operations
        .save(operations_output)
        .with_context(|| format!("writing {}", operations_output.display()))?;
    println!("Wrote {}", operations_output.display());
    Ok(())
}

/// Probes a real on-chain address against a saved dictionary — no source
/// is read anywhere in this call, only real, already-deployed `code`/
/// `data` fetched by address (`sting_corpus::CorpusController`, the same
/// controller `sting-abi-registry`'s own `verify` already uses) and the
/// real vendored `libemulator`.
pub fn run_probe(address: &str, dictionary_path: &Path) -> anyhow::Result<()> {
    let dictionary = MethodDictionary::load(dictionary_path)
        .with_context(|| format!("loading {}", dictionary_path.display()))?;

    let cache_root =
        sting_corpus::DiskCache::default_root().context("determining the platform cache dir")?;
    let sources: Vec<Box<dyn ContractSource>> = vec![
        Box::new(ToncenterClient::new()),
        Box::new(TonapiClient::new()),
    ];
    let corpus_controller = CorpusController::new(
        sources,
        DiskCache::new(cache_root, sting_corpus::DEFAULT_TTL),
    );
    let code = corpus_controller
        .resolve(
            &ContractTarget::OnChainAddress(address.to_string()),
            FetchOptions::default(),
        )
        .with_context(|| format!("fetching {address}'s real on-chain code"))?;

    let lib = sting_emulator::load_default_library().context(
        "loading libemulator — see vendor/libemulator/ in .gitignore for how to fetch it",
    )?;
    // The library defaults to a loud (debug-level) VM log — quiet it down,
    // this command reports its own summary on stdout/stderr.
    let _ = sting_emulator::set_verbosity_level(&lib, 0);

    #[allow(
        clippy::cast_possible_truncation,
        reason = "a real unix timestamp comfortably fits u32 until year 2106; this call site only needs a plausible current time for c7, not a precise one"
    )]
    let unixtime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32);

    let target = ProbeTarget {
        code_boc_base64: &code.code_boc_base64,
        data_boc_base64: &code.data_boc_base64,
        address,
        unixtime,
        // Real balance isn't fetched by `CorpusController::resolve` today
        // (it only returns code+data, not the full account state) — 0 is a
        // documented best-effort placeholder, per `ProbeTarget::balance`'s
        // own doc comment: it can only affect a *recognized* method's
        // returned values, never the presence/absence classification this
        // command reports.
        balance: 0,
    };
    let report = probe(&lib, &target, &dictionary)?;

    println!("{}", serde_json::to_string_pretty(&report)?);
    eprintln!(
        "\n{} of {} probeable dictionary entries recognized ({} inconclusive), out of {} total known method id(s)",
        report.recognized.len(),
        report.probed_count,
        report.inconclusive_count,
        report.dictionary_size
    );
    Ok(())
}

/// The Tolk corpora a dictionary is learned from.
///
/// Shared with the audit rather than written down twice: the audit checks the entries this walk
/// produced, and two lists that must agree and are maintained separately are two lists that will
/// stop agreeing. Deliberately not one of `bench.rs`'s own two roots (a separate hardcoded list) —
/// the growth corpus must never affect the accuracy baseline.
pub fn tolk_roots() -> [&'static Path; 3] {
    [
        Path::new("corpus/clean/acton-contracts"),
        Path::new("corpus/clean/tolk-bench"),
        // Real, deployed Tolk growth corpus, same purpose and provenance rule as the FunC corpus —
        // see `corpus-abi/clean/tolk/SOURCE.md`.
        Path::new("corpus-abi/clean/tolk"),
    ]
}

/// What each corpus in this tree is, in one line.
///
/// Kept beside the walk that reads them rather than in the dictionary crate: which corpora feed a
/// build is a decision of the command that builds, and the crate that holds the data should not
/// have opinions about which directories exist.
const CORPUS_DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "corpus/clean/acton-contracts",
        "the foundation's own reference contracts, in Tolk",
    ),
    ("corpus/clean/tolk-bench", "the Tolk benchmark contracts"),
    (
        "corpus-abi/clean/tolk",
        "deployed Tolk contracts, added as the sweep found gaps",
    ),
    (
        "corpus-abi/clean/tact",
        "Tact contracts the accuracy benchmark is measured over",
    ),
    (
        "corpus-abi/clean/tact-registry",
        "deployed Tact contracts, bulk-imported for coverage",
    ),
    (
        "corpus-abi/clean/func",
        "deployed FunC contracts, chosen where a mainnet sweep found unknown ids",
    ),
];

/// Where each corpus this build read came from, derived from the roots it actually walked.
///
/// Derived rather than listed: a table written beside the walk drifts from it the first time a root
/// moves or a `--corpus` flag points somewhere else, and a dataset whose recorded provenance names
/// directories it was not built from is worse than one that records none.
///
/// # Errors
/// When a corpus has no `SOURCE.md`. A corpus whose own origin is not written down cannot give an
/// entry its origin either, and a dataset that leaves this repository does not carry entries whose
/// provenance is a path nobody can follow.
fn corpus_sources<'roots>(
    roots: impl IntoIterator<Item = &'roots Path>,
) -> anyhow::Result<Vec<CorpusSource>> {
    let mut sources = Vec::new();
    for root in roots {
        // Recorded with forward slashes whatever the host uses, so that a build on Windows and
        // a build on Linux produce the same file.
        let root = root.to_string_lossy().replace('\\', "/");
        let provenance = format!("{root}/SOURCE.md");
        anyhow::ensure!(
            Path::new(&provenance).is_file(),
            "{root} has no SOURCE.md: a corpus without recorded provenance is not published"
        );
        let what = CORPUS_DESCRIPTIONS
            .iter()
            .find(|(known, _)| *known == root)
            .map_or_else(
                || "a corpus given on the command line".to_string(),
                |(_, what)| (*what).to_string(),
            );
        sources.push(CorpusSource {
            root,
            what,
            provenance,
        });
    }
    Ok(sources)
}

/// Rebuilds the dictionary into a scratch file and compares it with the committed one.
///
/// The comparison is on bytes rather than on contents, deliberately: a dataset that leaves this
/// repository is a file, and what a consumer can check is that the file they hold is the file this
/// corpus produces. Two files that parse to the same dictionary and differ in a byte are still two
/// files, and the reason for the difference is worth knowing.
///
/// # Errors
/// When the rebuild fails, when the committed file cannot be read, or when the two differ.
pub fn verify_rebuild(
    tact_corpus_root: &Path,
    func_corpus_root: &Path,
    committed: &Path,
    committed_operations: &Path,
) -> anyhow::Result<()> {
    // Read before the rebuild: an absent or unreadable committed file is a failure of the check
    // itself, and finding that out after a compile of the whole corpus wastes the run.
    let published = read_committed(committed)?;
    let published_operations = read_committed(committed_operations)?;

    let scratch_dir = std::env::temp_dir().join(format!("sting-rebuild-{}", std::process::id()));
    std::fs::create_dir_all(&scratch_dir)
        .with_context(|| format!("making {}", scratch_dir.display()))?;
    let scratch = scratch_dir.join("method-dictionary.json");
    let scratch_operations = scratch_dir.join("operation-dictionary.json");
    build_dictionary(
        tact_corpus_root,
        func_corpus_root,
        &scratch,
        &scratch_operations,
    )?;
    let rebuilt = std::fs::read(&scratch)
        .with_context(|| format!("reading the rebuilt dictionary at {}", scratch.display()))?;
    let rebuilt_operations = std::fs::read(&scratch_operations).with_context(|| {
        format!(
            "reading the rebuilt dictionary at {}",
            scratch_operations.display()
        )
    })?;
    let _ = std::fs::remove_dir_all(&scratch_dir);

    // Both are reported before either fails, so that one run says which of the two datasets moved.
    let methods_match = compare_rebuild(committed, &rebuilt, &published);
    let operations_match = compare_rebuild(
        committed_operations,
        &rebuilt_operations,
        &published_operations,
    );
    anyhow::ensure!(
        methods_match && operations_match,
        "the rebuild does not reproduce what is committed"
    );
    Ok(())
}

/// Reads a committed dataset, saying which one it failed on.
fn read_committed(path: &Path) -> anyhow::Result<Vec<u8>> {
    std::fs::read(path)
        .with_context(|| format!("reading the committed dictionary at {}", path.display()))
}

/// Reports one dataset's comparison and answers whether it reproduced.
///
/// The first differing offset is printed rather than a verdict alone: a rebuild that differs is
/// either a corpus that moved or a build that is not deterministic, and the offset is where whoever
/// investigates starts.
fn compare_rebuild(committed: &Path, rebuilt: &[u8], published: &[u8]) -> bool {
    if rebuilt == published {
        println!(
            "the rebuild reproduces {} byte for byte ({} bytes)",
            committed.display(),
            published.len()
        );
        return true;
    }
    let first_difference = rebuilt
        .iter()
        .zip(published.iter())
        .position(|(left, right)| left != right);
    println!(
        "the rebuild does not reproduce {}: {} bytes against {}, first difference at {}",
        committed.display(),
        rebuilt.len(),
        published.len(),
        first_difference.map_or_else(
            || "the end of the shorter file".to_string(),
            |at| at.to_string()
        ),
    );
    false
}
