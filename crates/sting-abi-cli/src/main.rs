//! Offline `abi-layer` binary — single-contract extraction without HTTP,
//! same `sting-abi-pipeline` controllers as `sting-abi-service`
//! (`docs/domains/abi-ir.md`). `docs/abi-layer/blocks/
//! 02-normalized-ir-cli.md`'s deliverable.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use sting_abi_core::{Extractor, SourceLanguage};
use sting_abi_func::FuncExtractor;
use sting_abi_pipeline::{ExtractController, LanguageHint};
use sting_abi_tact::{TactCompiler, TactExtractor};
use sting_abi_tolk::TolkExtractor;
use sting_tolkc::TolkCompiler;

mod audit;
mod bench;
mod catalogue;
mod coverage;
mod decode;
mod dictionary;
mod share;

#[derive(Parser)]
#[command(
    name = "abi-layer",
    about = "Extracts a normalized ABI/schema from a TON smart contract (Tolk, Tact — compiler-backed; FunC — heuristic, Tier 2)",
    long_about = "abi-layer — extracts a normalized ContractSchema from a single TON smart contract file.\n\n\
        Examples:\n  \
        abi-layer extract corpus/clean/acton-contracts/counter/Counter.tolk\n  \
        abi-layer extract MyContract.tact --lang tact\n  \
        abi-layer extract MyContract.tolk --format json > schema.json"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extract a normalized ABI schema from one contract's entrypoint file.
    #[command(
        long_about = "Extracts a ContractSchema from a single contract entrypoint and prints it to stdout.\n\n\
            Examples:\n  \
            abi-layer extract corpus/clean/acton-contracts/counter/Counter.tolk\n  \
            abi-layer extract MyContract.tact --lang tact\n  \
            abi-layer extract MyContract.tolk --format json > schema.json"
    )]
    Extract {
        /// Path to the contract's entrypoint file (e.g. `MyContract.tolk`).
        /// Sibling files it imports are resolved from the same directory.
        path: PathBuf,
        /// Source language — overrides auto-detection by file extension
        /// when given (docs/API.md).
        #[arg(long, value_enum)]
        lang: Option<LangArg>,
        /// Output format. `tlb` is reserved by design
        /// (docs/domains/abi-ir.md) but not yet implemented.
        #[arg(long, value_enum, default_value_t = FormatArg::Json)]
        format: FormatArg,
    },
    /// Measures schema-extraction accuracy (Tier 1 structural completeness,
    /// Tier 2 precision/recall) plus batch throughput, against this
    /// workspace's real corpora — `docs/domains/precision-sources.md`.
    #[command(
        long_about = "Runs the accuracy/throughput harness and prints a Markdown report.\n\n\
            Examples:\n  \
            abi-layer bench\n  \
            abi-layer bench --tier tier1 --report tier1-report.md\n  \
            abi-layer bench --update-baseline"
    )]
    Bench {
        /// Root of the Tact corpus (Tolk always reuses `corpus/clean/
        /// {acton-contracts,tolk-bench}` directly, not configurable here —
        /// same reasoning as `docs/domains/precision-sources.md`'s
        /// own "abi-layer doesn't fork a separate Tolk corpus").
        #[arg(long, default_value = "corpus-abi/clean/tact")]
        corpus: PathBuf,
        /// Where to write the Markdown report. A sibling `.json` (same stem)
        /// is also written.
        #[arg(long, default_value = "abi-bench-report.md")]
        report: PathBuf,
        /// Which tier(s) to run.
        #[arg(long, value_enum, default_value_t = TierArg::All)]
        tier: TierArg,
        /// Path to the committed baseline snapshot.
        #[arg(long, default_value = "corpus-abi/baseline.json")]
        baseline: PathBuf,
        /// Overwrite `--baseline` with this run's results instead of
        /// comparing against it.
        #[arg(long)]
        update_baseline: bool,
    },
    /// Rebuilds the method-id dictionary from every real get-method this
    /// workspace's own Tolk/Tact corpus knows about
    /// (`docs/abi-layer/critical`'s method-id convention — a direct parallel
    /// to Ethereum's `4byte.directory`).
    #[command(long_about = "Walks the real Tolk+Tact corpus, extracts every real \
            get-method, and writes a MethodDictionary — the knowledge base \
            `dictionary-probe` looks candidate names up in.\n\n\
            Example:\n  \
            abi-layer dictionary-build --output corpus-abi/method-dictionary.json")]
    DictionaryBuild {
        /// Root of the Tact corpus (Tolk always reuses `corpus/clean/
        /// {acton-contracts,tolk-bench}` directly, matching `bench`'s own
        /// convention).
        #[arg(long, default_value = "corpus-abi/clean/tact")]
        corpus: PathBuf,
        /// Root of the real, deployed `FunC` growth corpus — see
        /// `corpus-abi/clean/func/SOURCE.md` for why each file there was
        /// chosen: a real mainnet sweep found each one as an unrecognized
        /// dictionary gap (`docs/domains/method-dictionary.md`).
        #[arg(long, default_value = "corpus-abi/clean/func")]
        func_corpus: PathBuf,
        /// Where to write the dictionary JSON.
        #[arg(long, default_value = "corpus-abi/method-dictionary.json")]
        output: PathBuf,
        /// Where to write the operation dictionary JSON — the same walk's
        /// other half: what a message opcode means rather than what a get
        /// method is called.
        #[arg(long, default_value = "corpus-abi/operation-dictionary.json")]
        operations_output: PathBuf,
    },
    /// Probes a real on-chain address's bytecode against a saved method-id
    /// dictionary — no source is ever read, only real code+data fetched by
    /// address and the real vendored `libemulator`.
    #[command(long_about = "Fetches an address's real on-chain code+data, then \
            calls each dictionary entry's get-method id against it, \
            reporting which ones a real TVM call recognizes as present.\n\n\
            Example:\n  \
            abi-layer dictionary-probe EQCxE6mUtQJKFnGfaROTKOt1lZbDiiX1kCixRv7Nw2Id_sDs")]
    DictionaryProbe {
        /// The on-chain address to probe (raw or user-friendly form).
        address: String,
        /// Path to a dictionary previously written by `dictionary-build`.
        #[arg(long, default_value = "corpus-abi/method-dictionary.json")]
        dictionary: PathBuf,
    },
    /// Measures this dataset against the foundation's own catalogue, in both
    /// directions, and prints both numbers — including the one that does not
    /// flatter this repository.
    ///
    /// The catalogue is read from the recorded lists rather than from a live
    /// clone, because a comparison whose input can change under it is a
    /// comparison nobody can reproduce; `corpus-abi/FOUNDATION-CATALOGUE-SOURCE.md`
    /// records which commit those lists came from.
    CatalogueDiff {
        /// Path to a dictionary previously written by `dictionary-build`.
        #[arg(long, default_value = "corpus-abi/method-dictionary.json")]
        dictionary: PathBuf,
        /// Path to the recorded catalogue lists.
        #[arg(long, default_value = "corpus-abi/foundation-catalogue.json")]
        catalogue: PathBuf,
    },
    /// Checks a sample of the compiler-extracted entries against the
    /// compiler's own `abi.json`, which is the authority for anything
    /// compiled from Tolk.
    ///
    /// The sample is spread by stride rather than drawn at random, so that
    /// two runs over one dictionary check the same entries.
    DictionaryAudit {
        /// The dictionary whose entries are checked.
        #[arg(long, default_value = "corpus-abi/method-dictionary.json")]
        dictionary: PathBuf,
        /// How many entries to check.
        #[arg(long, default_value_t = 100)]
        sample: usize,
    },
    /// Rereads the foundation's catalogue lists from a checkout of their
    /// repository, so that the fixture the comparison runs against can be
    /// rebuilt rather than trusted.
    ///
    /// The date is stated rather than taken from the clock, because the same
    /// checkout has to produce the same file whenever it is read.
    CatalogueFetch {
        /// A checkout of `ton-blockchain/abis`.
        #[arg(long)]
        checkout: PathBuf,
        /// The date the lists were read, as `YYYY-MM-DD`.
        #[arg(long)]
        read_at: String,
        /// Where to write the lists.
        #[arg(long, default_value = "corpus-abi/foundation-catalogue.json")]
        output: PathBuf,
    },
    /// Learns an operation dictionary from the foundation catalogue's own
    /// type files, compiled by this tree's own extractor.
    ///
    /// A dataset of its own rather than part of the learned one: their
    /// sources are MIT and this tree's are not, and a file whose terms
    /// depend on which entry a reader looks at is a file nobody can use.
    CatalogueOperations {
        /// A checkout of `ton-blockchain/abis`.
        #[arg(long)]
        checkout: PathBuf,
        /// Where to write what was learned from it.
        #[arg(long, default_value = "corpus-abi/foundation-operations.json")]
        output: PathBuf,
    },
    /// Reads one message body as the operation its opcode names.
    ///
    /// Both datasets are consulted — the one learned from this tree's
    /// corpora and the one learned from the foundation's catalogue — and
    /// every candidate says which contracts declared it.
    Decode {
        /// A message body, as base64 of its bag of cells.
        #[arg(long)]
        body: Option<String>,
        /// A mainnet transaction, by hash, whose incoming message is read.
        #[arg(long)]
        hash: Option<String>,
        /// The operation dictionaries to read against.
        #[arg(long, default_values = DEFAULT_OPERATION_DICTIONARIES)]
        dictionary: Vec<PathBuf>,
    },
    /// Measures how much of real mainnet traffic the datasets can read.
    ///
    /// Two shares, because they answer different questions: what part of
    /// the messages can be named, and what part of the distinct opcodes.
    DecodeSample {
        /// How many transactions to read, newest first.
        #[arg(long, default_value_t = 10_000)]
        transactions: u32,
        /// The operation dictionaries to read against.
        #[arg(long, default_values = DEFAULT_OPERATION_DICTIONARIES)]
        dictionary: Vec<PathBuf>,
    },
    /// Rebuilds the dictionary into a scratch file and compares it with the
    /// committed one, byte for byte.
    ///
    /// The published artefact has to be reproducible from this repository, and
    /// the only honest way to know that is to reproduce it: a build that
    /// differs is either a corpus that changed or a build that is not
    /// deterministic, and both are things a consumer of the dataset needs told.
    DictionaryVerify {
        /// The committed dictionary the rebuild is compared against.
        #[arg(long, default_value = "corpus-abi/method-dictionary.json")]
        dictionary: PathBuf,
        /// The committed operation dictionary the rebuild is compared against.
        #[arg(long, default_value = "corpus-abi/operation-dictionary.json")]
        operations: PathBuf,
        /// Root of the Tact corpus, as `dictionary-build` takes it.
        #[arg(long, default_value = "corpus-abi/clean/tact")]
        corpus: PathBuf,
        /// Root of the `FunC` corpus, as `dictionary-build` takes it.
        #[arg(long, default_value = "corpus-abi/clean/func")]
        func_corpus: PathBuf,
    },
    /// Runs (or resumes) a deduplicated-by-code-hash mainnet discovery sweep
    /// against real `toncenter.com/api/v3/transactions`, then Tier
    /// 3-probes every newly discovered code. Makes real, sustained traffic
    /// against a public third-party service — read
    /// `docs/domains/method-dictionary.md` §6 before running at more than a
    /// small pilot size.
    #[command(
        long_about = "Discovers real on-chain addresses by paging toncenter's real \
            transaction stream, fetches each newly-seen code exactly once (addresses \
            sharing an already-known code hash skip the fetch entirely), and Tier \
            3-probes every new code against a saved method dictionary.\n\n\
            Example:\n  \
            abi-layer sweep --db coverage.sqlite3 --pilot-size 2000"
    )]
    Sweep {
        /// Path to the `CoverageDb` `SQLite` file (created if absent).
        #[arg(long)]
        db: PathBuf,
        /// Path to a dictionary previously written by `dictionary-build`.
        #[arg(long, default_value = "corpus-abi/method-dictionary.json")]
        dictionary: PathBuf,
        /// Stop once this many *new* addresses have been recorded in this
        /// run (`docs/domains/method-dictionary.md`
        /// recommends starting with 2,000-5,000, not the full stream).
        #[arg(long, default_value_t = 5000)]
        pilot_size: usize,
        /// Resume from this database's saved crawl position instead of
        /// starting over from the newest transaction.
        #[arg(long)]
        resume: bool,
        /// Also re-probes every previously-probed-but-unrecognized code
        /// against `--dictionary` before running, not just brand-new codes
        /// discovered this run — real gap found 2026-07-22: a dictionary
        /// grown *after* a code was first probed (e.g. a separate
        /// `dictionary-build` run) otherwise never gets a chance to
        /// recognize it, since a probed code is never re-checked on its
        /// own. Already-recognized codes are left alone (see
        /// `CoverageDb::reset_unrecognized_probes`'s own doc comment).
        #[arg(long)]
        reprobe: bool,
    },
    /// Counts how many of a `CoverageDb`'s unique on-chain codes Tact
    /// emitted — the measurement behind the language perimeter.
    TactShare {
        /// Path to the `CoverageDb` `SQLite` file.
        #[arg(long)]
        db: PathBuf,
    },
    /// Prints a `CoverageDb`'s aggregate coverage numbers plus its top
    /// dictionary-expansion candidates.
    CoverageReport {
        /// Path to the `CoverageDb` `SQLite` file.
        #[arg(long)]
        db: PathBuf,
        /// Only list an unrecognized code as a gap candidate once at least
        /// this many distinct addresses run it.
        #[arg(long, default_value_t = 2)]
        min_address_count: u64,
        /// How many gap candidates to print.
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TierArg {
    Tier1,
    Tier2,
    All,
}

#[derive(Clone, Copy, ValueEnum)]
enum LangArg {
    Tolk,
    Tact,
    Func,
}

impl From<LangArg> for SourceLanguage {
    fn from(lang: LangArg) -> Self {
        match lang {
            LangArg::Tolk => SourceLanguage::Tolk,
            LangArg::Tact => SourceLanguage::Tact,
            LangArg::Func => SourceLanguage::Func,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FormatArg {
    Json,
    Tlb,
}

/// The two datasets a decode reads against.
///
/// Two files rather than one because they carry different terms: what this tree learns from its own
/// corpora is CC0, and what it learns from the foundation's catalogue is MIT and stays so.
const DEFAULT_OPERATION_DICTIONARIES: [&str; 2] = [
    "corpus-abi/operation-dictionary.json",
    "corpus-abi/foundation-operations.json",
];

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Extract { path, lang, format } => run_extract(&path, lang, format),
        Command::Bench {
            corpus,
            report,
            tier,
            baseline,
            update_baseline,
        } => bench::run_bench(&corpus, &report, tier, &baseline, update_baseline),
        Command::CatalogueDiff {
            dictionary,
            catalogue,
        } => catalogue::run(&dictionary, &catalogue),
        Command::DictionaryAudit { dictionary, sample } => audit::run(&dictionary, sample),
        Command::CatalogueFetch {
            checkout,
            read_at,
            output,
        } => catalogue::fetch(&checkout, &read_at, &output),
        Command::CatalogueOperations { checkout, output } => {
            catalogue::learn_operations(&checkout, &output)
        }
        Command::Decode {
            body,
            hash,
            dictionary,
        } => decode::run(
            body.as_deref(),
            hash.as_deref(),
            &dictionary.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        ),
        Command::DecodeSample {
            transactions,
            dictionary,
        } => decode::run_sample(
            transactions,
            &dictionary.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        ),
        Command::DictionaryVerify {
            dictionary,
            operations,
            corpus,
            func_corpus,
        } => dictionary::verify_rebuild(&corpus, &func_corpus, &dictionary, &operations),
        Command::DictionaryBuild {
            corpus,
            func_corpus,
            output,
            operations_output,
        } => dictionary::build_dictionary(&corpus, &func_corpus, &output, &operations_output),
        Command::DictionaryProbe {
            address,
            dictionary,
        } => dictionary::run_probe(&address, &dictionary),
        Command::Sweep {
            db,
            dictionary,
            pilot_size,
            resume,
            reprobe,
        } => coverage::run_sweep(&db, &dictionary, pilot_size, resume, reprobe),
        Command::TactShare { db } => coverage::run_tact_share(&db),
        Command::CoverageReport {
            db,
            min_address_count,
            top,
        } => coverage::run_coverage_report(&db, min_address_count, top),
    }
}

fn run_extract(path: &Path, lang: Option<LangArg>, format: FormatArg) -> anyhow::Result<()> {
    if format == FormatArg::Tlb {
        anyhow::bail!(
            "--format tlb is not yet implemented — reserved by design \
             (docs/domains/abi-ir.md), not an MVP priority. Use --format json (the default)."
        );
    }

    let project_dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let entrypoint_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("{} has no file name component", path.display()))?;
    let entrypoint = Path::new(entrypoint_name);

    let controller = build_controller();
    let hint = lang.map_or(LanguageHint::AutoDetect, |lang| {
        LanguageHint::Explicit(lang.into())
    });
    let schema = controller.extract(project_dir, entrypoint, hint)?;

    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}

/// Registers every extractor this binary can currently drive.
///
/// Each compiler is resolved the way the whole tree resolves one: an environment override, else
/// next to the running executable, else the vendored directory. Looking only beside the executable
/// is right for a shipped release and wrong from a checkout, where that path is `target/debug/` and
/// holds no compiler at all — which is how a capability that works in release fails on the machine
/// of whoever just cloned it.
///
/// A compiler that is reachable nowhere is not an error here: the extractor is simply not
/// registered, and asking for that language then says which compiler is missing rather than
/// failing every language because one is absent.
fn build_controller() -> ExtractController {
    let mut extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();

    if let Some(tolk_compiler) = TolkCompiler::vendored_or_default() {
        extractors.insert(
            SourceLanguage::Tolk,
            Box::new(TolkExtractor::new(tolk_compiler)),
        );
    }

    if let Some(tact_compiler) = TactCompiler::vendored_or_default() {
        extractors.insert(
            SourceLanguage::Tact,
            Box::new(TactExtractor::new(tact_compiler)),
        );
    }

    extractors.insert(SourceLanguage::Func, Box::new(FuncExtractor));

    ExtractController::new(extractors)
}
