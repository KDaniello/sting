//! sting CLI entry point.
//!
//! Eight subcommands (`docs/API.md`). `scan` is Tier 1 dynamic-only analysis
//! of a deployed contract — see [`scan`]'s own doc comment for the honest
//! scope boundary — and `fuzz` is a thin wrapper over `cargo fuzz` rather than
//! a reimplementation of it. `verify` is the only subcommand that chains
//! `lint`'s static step and `scan`'s address resolution into
//! `BridgeController` and `ScoringController`; see its own doc comment for its
//! scope boundary (hypotheses are supplied by the caller, never derived from
//! source — the same constraint `scan`'s doc comment already states for its
//! own, narrower reasons). `-v`/`-vv`/`RUST_LOG`-driven `tracing` logging +
//! a Cargo.toml-style upward `Sting.toml` search apply consistently across
//! every subcommand that reads config. Every real subcommand stays thin: argument parsing
//! plus a call into `sting-pipeline`/`sting-bench`
//! (`docs/ARCHITECTURE.md` — no orchestration logic lives here).
//! `sting-bench` itself uses `sting_pipeline::LintController::lint_source`
//! for the actual detection step (`docs/ARCHITECTURE.md`'s "sting
//! bench uses `LintController`"), just not `lint_paths`/`resolve_tolk_files`
//! — its corpus (merged multi-file projects, in-memory mutation-engine
//! instances) is never a flat list of real files on disk.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use sting_pipeline::EmulatorProvider as _;

/// Which languages the static contour actually understands, and how much of each — `sting
/// --version` has to answer this, because for neither of them is the answer "the latest" or
/// obvious: parsing is `tree-sitter-tolk 0.0.5`, whose language coverage
/// stops at Tolk 1.2, while the language is at 1.4.x. Measured rather than
/// guessed; the exact numbers and the two named gaps are verified by
/// `crates/sting-ast/tests/tolk_version_matrix.rs`, so this string cannot
/// quietly drift away from reality.
const LANGUAGE_COMPATIBILITY: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n\nTolk (parser: vendor/tree-sitter-tolk, forked from 0.0.5)\n",
    "  <= 1.4.x  full\n",
    "  Measured: 297 of 297 corpus files parse with no ERROR node, and the\n",
    "  vendored compiler reads nothing the grammar refuses.\n",
    "  Says so: cargo run -p sting-ast --example grammar_oracle\n",
    "\n",
    "FunC (parser: vendor/tree-sitter-func, forked from f780ca5)\n",
    "  .fc and .func, both spellings\n",
    "  Measured: 199 of 199 files across eight published bug bounty scope\n",
    "  targets parse with no ERROR node.\n",
    "  Says so: cargo run -p sting-ast --example func_oracle -- <directory>\n",
    "\n",
    "One of the fourteen detectors reads both. A run names the ones it held\n",
    "back for want of a rule, because their silence is not a clean result."
);

#[derive(Parser)]
#[command(
    name = "sting",
    version,
    long_version = LANGUAGE_COMPATIBILITY,
    about = "Security static/dynamic analyzer for the Tolk language (TON blockchain)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Increase log verbosity: unset = warnings only, `-v` = debug, `-vv` =
    /// trace. Overridden entirely by `RUST_LOG` when that's set (standard
    /// `tracing-subscriber` precedence) — e.g. `RUST_LOG=sting_pipeline=trace`
    /// to trace just one crate without turning on trace-level noise
    /// everywhere.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    /// Colorize `--format human` output by severity. `auto` (default)
    /// colors only when stdout is an interactive terminal, `NO_COLOR` is
    /// unset, and the report isn't being written to `--output`; `always`/
    /// `never` skip that detection entirely. Never affects `sarif`/`json`.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    color: ColorArg,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect sting's own configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Names the four versions a run's answer depends on, and refuses when any
    /// of them is unknown.
    ///
    /// An emulation is a statement about semantics, and semantics belong to a
    /// version. A run that cannot say which version it emulated against, which
    /// compiler it compared itself to, or which grammar it parsed with is a run
    /// nobody can check.
    #[command(after_help = "\
EXAMPLES:
    sting doctor")]
    Doctor,
    /// Prints a detailed explanation of one detector: what it looks for,
    /// its real calibrated false-positive/limitation profile, and why it
    /// carries the severity it does — `rustc --explain`/`clippy`'s per-lint docs, for
    /// sting's built-in detectors.
    #[command(after_help = "\
EXAMPLES:
    sting explain AC
    sting explain ubm")]
    Explain {
        /// Detector id (e.g. `AC`, `UBM`, `GVR`) — case-insensitive.
        detector_id: String,
    },
    /// Runs the detectors against a Tolk file or directory.
    #[command(after_help = "\
EXAMPLES:
    sting lint contract.tolk
    sting lint src/ --format sarif --output findings.sarif.json
    sting lint src/ --format json --config ./ci/Sting.toml")]
    Lint {
        /// File or directory to analyze. A directory is scanned recursively
        /// for `*.tolk` files.
        path: PathBuf,
        /// Output format. Defaults to `sarif` when `--output` is given,
        /// otherwise `human` on an interactive terminal and `sarif`
        /// otherwise (`docs/ARCHITECTURE.md`).
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,
        /// Write the report to this file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Path to a `Sting.toml` file. Defaults to the nearest `Sting.toml`
        /// found by walking up from the current directory (the same
        /// upward-search convention `Cargo.toml`/`Acton.toml` use), or the
        /// built-in defaults (all compiled-in detectors enabled) if none is
        /// found anywhere above the current directory.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Drop findings below this severity. Overrides `Sting.toml`'s
        /// `min_severity` when given; neither set means no floor at all.
        #[arg(long = "min-severity", value_enum)]
        min_severity: Option<SeverityArg>,
        /// Apply every machine-applicable fix (`DEPR`/`SUA` findings that
        /// carry a `Suggestion`) directly to disk, then re-lint and report the
        /// post-fix state. Without this flag, `lint` only reports how many
        /// fixes are available.
        #[arg(long)]
        fix: bool,
    },
    /// Dynamic-only analysis of an already-deployed contract by address
    /// (`docs/API.md`).
    ///
    /// No source is available for an on-chain address (see `fetch`'s own
    /// note), so this never runs the static detectors and never invokes
    /// `BridgeController` — there is no `Finding` to build a `Hypothesis`
    /// from, and deriving one from bytecode alone is not attempted (see
    /// `docs/domains/emulator.md` §6 on why that is a genuinely open
    /// problem rather than a narrowed-scope shortcut). What this
    /// command actually does: fetches the code+data (same path as `fetch`),
    /// checks it loads as structurally valid TVM state, and optionally
    /// probes specific get-methods you already know the name of.
    #[command(after_help = "\
EXAMPLES:
    sting scan 0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6
    sting scan 0:...ae6 --get-method get_jetton_data --get-method seqno
    sting scan 0:...ae6 --format sarif --output scan.sarif.json
    sting scan 0:...ae6 --testnet --get-method getTotalSupply")]
    Scan {
        /// Contract address, e.g. `"0:000011a5...ae6"`.
        address: String,
        /// A get-method name to call and print the raw result of (repeat for
        /// more than one). Only method names you already know — sting does
        /// not guess or enumerate get-methods on unknown bytecode. A call
        /// that fails outright (as opposed to succeeding with
        /// `success: false`, a legitimate negative result) becomes a
        /// `SCAN-GET-METHOD-FAILED` finding and affects the exit code, the
        /// same as a failed structural check.
        #[arg(long = "get-method")]
        get_method: Vec<String>,
        /// Bypasses the local disk cache and always fetches fresh data.
        #[arg(long)]
        force_refresh: bool,
        /// Resolve `address` against `testnet.toncenter.com`/`testnet.tonapi.io`
        /// instead of the mainnet endpoints — a mainnet-only address will not
        /// resolve with this flag set, and vice versa. The disk cache is
        /// rooted in a separate `mainnet`/`testnet` subdirectory per network
        /// (`build_corpus_controller`), so switching this flag for the same
        /// address never serves the other network's cached bytecode.
        #[arg(long)]
        testnet: bool,
        /// Output format for the structural-validity report. The structural
        /// check and get-method probe status lines are only printed when
        /// this resolves to `human` (default rule same as `lint`: explicit
        /// `--format`, else `human` on a TTY, else `sarif`) — printing them
        /// unconditionally used to interleave plain text ahead of a
        /// `--format json`/`--format sarif` payload on stdout, corrupting it
        /// for any downstream parser (found during a post-Block-09 corpus
        /// review, same stdout-purity concern `init_tracing`'s doc comment
        /// states for logging).
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,
        /// Write the structural-validity report to this file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Measures precision/recall/FP rate against the labeled corpus
    /// (`docs/domains/precision-sources.md`).
    ///
    /// Always runs the full compiled-in detector suite regardless of
    /// `Sting.toml`'s `detectors.enable`/`disable` — deliberately: `bench`
    /// measures each detector's *own* precision/recall as project-wide
    /// calibration data for `ScoringController`
    /// (`docs/domains/emulator.md`), which requires
    /// every detector to actually run; filtering by a user's per-project
    /// config would quietly corrupt that statistic instead of measuring a
    /// deliberate subset (`docs/API.md`).
    #[command(after_help = "\
EXAMPLES:
    sting bench
    sting bench --report target/bench-report.md
    sting bench --update-baseline")]
    Bench {
        /// Root of the corpus (expects `<corpus>/clean/` and
        /// `<corpus>/fixtures/detectors/`). Defaults to `./corpus`.
        #[arg(long)]
        corpus: Option<PathBuf>,
        /// Where to write the Markdown report. Prints to stdout if omitted.
        /// A `.json` sibling (same path, extension replaced) is always
        /// written alongside it (`docs/domains/precision-sources.md`:
        /// JSON for the CI gate, Markdown for humans).
        #[arg(long)]
        report: Option<PathBuf>,
        /// Path to the committed baseline snapshot to compare against.
        /// Defaults to `<corpus>/baseline.json`.
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Overwrite the baseline with the current run's results instead of
        /// comparing against it.
        #[arg(long)]
        update_baseline: bool,
    },
    /// Downloads an already-deployed contract's compiled code+data by
    /// on-chain address (`docs/DATA.md`).
    ///
    /// IMPORTANT — this fetches bytecode, not source: it does **not** make
    /// `address` usable with `sting lint`, which needs a `.tolk` source file.
    /// A deployed contract has no recoverable source through this command —
    /// what `fetch` retrieves is input for the emulator and the fuzzer, never
    /// for the static detectors. If you have both the contract's
    /// source and its own on-chain deployment (e.g. your own testnet
    /// contract), pass the `.tolk` file to `sting lint` and the address to
    /// this command separately; sting never compiles one into the other.
    #[command(after_help = "\
EXAMPLES:
    sting fetch 0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6
    sting fetch 0:...ae6 --out corpus/fixtures/emulator --force-refresh
    sting fetch 0:...ae6 --testnet")]
    Fetch {
        /// Contract address, e.g. `"0:000011a5...ae6"`.
        address: String,
        /// Bypasses the local disk cache and always fetches fresh data,
        /// overwriting any cached entry (the contract may have been
        /// redeployed via `set_code` since it was last cached).
        #[arg(long)]
        force_refresh: bool,
        /// Resolve `address` against the testnet endpoints instead of
        /// mainnet — see `scan --testnet`'s doc comment for how the disk
        /// cache keeps the two networks separate.
        #[arg(long)]
        testnet: bool,
        /// Also writes `<address>.code.boc.base64`/`<address>.data.boc.base64`
        /// into this directory, in addition to the disk cache — the same
        /// base64-`BoC`-text format used by the benchmark corpus
        /// (`docs/domains/precision-sources.md`) and emulator fixtures
        /// (`docs/domains/emulator.md`), ready to drop straight into a
        /// corpus directory without reformatting.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Closes the static→dynamic loop end-to-end (`docs/domains/emulator.md`): lints
    /// `path` statically, then — for every finding whose detector class has
    /// a matching section in `--hypotheses` — drives the already-proven
    /// `BridgeController` strategy against `address`'s live bytecode (the
    /// same chain `bridge_ubm_end_to_end.rs`/`bridge_id_end_to_end.rs` prove
    /// in tests, made reusable instead of a one-off Rust literal per
    /// contract), then scores every finding's confidence against the
    /// committed benchmark baseline.
    ///
    /// Deliberately does **not** attempt to derive a hypothesis from source —
    /// see `sting_pipeline::Hypothesis`'s own doc comment for why that is a
    /// confirmed-out-of-scope, research-grade problem (general
    /// message-dispatch-convention inference), the same reason `scan` never
    /// invokes `BridgeController` at all. `--hypotheses` is a TOML file a
    /// human who already knows the answer writes by hand — one
    /// `[GVR]`/`[LEP]`/`[ID]`/`[UBM]` section per detector class, see
    /// `sting_pipeline::HypothesisSpec`'s doc comment for the exact shape. A
    /// finding whose class has no matching section is still statically
    /// reported and still scored (on static evidence alone, exactly like
    /// `sting lint`) — a hypothesis file only needs to cover what its author
    /// actually wants dynamically verified, not every finding.
    #[command(after_help = "\
EXAMPLES:
    sting verify contract.tolk 0:...ae6 --hypotheses hyp.toml
    sting verify src/ 0:...ae6 --hypotheses hyp.toml --testnet
    sting verify contract.tolk 0:...ae6 --hypotheses hyp.toml --format sarif --output verify.sarif.json")]
    Verify {
        /// File or directory of Tolk source to lint statically. A directory
        /// is scanned recursively for `*.tolk` files, same as `lint`.
        path: PathBuf,
        /// The deployed contract's address whose live bytecode stands in for
        /// `path` at runtime — same address semantics as `scan`/`fetch`.
        address: String,
        /// Path to a hypothesis-spec TOML file (`sting_pipeline::HypothesisSpec`).
        #[arg(long)]
        hypotheses: PathBuf,
        /// Bypasses the local disk cache and always fetches fresh bytecode.
        #[arg(long)]
        force_refresh: bool,
        /// Resolve `address` against the testnet endpoints — see
        /// `scan --testnet`'s doc comment for how the disk cache keeps the
        /// two networks separate.
        #[arg(long)]
        testnet: bool,
        /// Path to the committed benchmark baseline used to calibrate
        /// confidence scoring. Defaults to `corpus/baseline.json`
        /// (`sting bench`'s own default with no `--corpus` override); a
        /// missing file falls back to the neutral per-detector precision
        /// prior (`ScoringController`'s own doc comment), not an error.
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Path to a `Sting.toml` file. Same resolution as `lint --config`.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Output format. Same default rule as `lint`.
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,
        /// Write the report to this file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Drop findings below this severity. Same override rule as
        /// `lint --min-severity`.
        #[arg(long = "min-severity", value_enum)]
        min_severity: Option<SeverityArg>,
    },
    /// Thin wrapper over `cargo fuzz run`, not a reimplementation
    /// (`docs/API.md`) — the fuzz targets
    /// themselves live in `crates/sting-fuzz/fuzz/fuzz_targets/` and use
    /// `sting-emulator` directly, in-process, libFuzzer-style (Tier 1, not
    /// this project's `EmulationPoolController`, see
    /// `docs/ARCHITECTURE.md`/§8). Requires a full sting
    /// checkout (not just the installed release binary — `cargo-fuzz`
    /// itself needs the fuzz targets' source) and a nightly toolchain with
    /// `cargo-fuzz` installed (`docs/domains/emulator.md`).
    #[command(after_help = "\
EXAMPLES:
    sting fuzz fuzz_send_message
    sting fuzz fuzz_tx_sequence --time 300
    sting fuzz fuzz_send_message -- -dict=my.dict")]
    Fuzz {
        /// Fuzz target name, matching a file under
        /// `crates/sting-fuzz/fuzz/fuzz_targets/` (currently
        /// `fuzz_send_message`, `fuzz_tx_sequence`).
        target: String,
        /// Wall-clock seconds to run for, forwarded as libFuzzer's
        /// `-max_total_time`.
        #[arg(long, default_value_t = 60)]
        time: u64,
        /// Extra arguments forwarded verbatim to `cargo fuzz run -- <these>`
        /// (e.g. `-dict=<path>`, `-runs=<n>`) — see `cargo fuzz run --help`
        /// and libFuzzer's own flag reference for what's available.
        #[arg(last = true)]
        extra_args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Parses `Sting.toml` (or falls back to built-in defaults if none is
    /// found) and prints the effective configuration, including which
    /// detectors it activates.
    Dump {
        /// Path to a `Sting.toml` file. Defaults to the nearest `Sting.toml`
        /// found by walking up from the current directory, otherwise the
        /// built-in defaults (all compiled-in detectors enabled).
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

/// `sting lint`/`sting scan --format`. A separate, `clap`-aware mirror of
/// `sting_pipeline::ReportFormat`, not that type reused directly — no
/// controller may depend on `clap` (`docs/ARCHITECTURE.md`), so
/// the CLI-facing enum and the conversion between the two live here.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    /// One line per finding, for a terminal.
    Human,
    /// SARIF 2.1.0 JSON.
    Sarif,
    /// A plain JSON array of findings (not SARIF) — see
    /// `sting_pipeline::ReportFormat::Json`'s doc comment for how this
    /// differs from `sting bench`'s own, unrelated JSON report shape.
    Json,
}

impl From<OutputFormat> for sting_pipeline::ReportFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Human => Self::Human,
            OutputFormat::Sarif => Self::Sarif,
            OutputFormat::Json => Self::Json,
        }
    }
}

/// `sting lint`/`sting verify --min-severity`. A separate, `clap`-aware
/// mirror of `sting_core::Severity`, same reasoning as [`OutputFormat`] —
/// `sting-core` has zero dependencies by design
/// (`docs/ARCHITECTURE.md` §1.1), `clap` is not one of them.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum SeverityArg {
    /// See [`sting_core::Severity::Info`].
    Info,
    /// See [`sting_core::Severity::Low`].
    Low,
    /// See [`sting_core::Severity::Medium`].
    Medium,
    /// See [`sting_core::Severity::High`].
    High,
    /// See [`sting_core::Severity::Critical`].
    Critical,
}

impl From<SeverityArg> for sting_core::Severity {
    fn from(value: SeverityArg) -> Self {
        match value {
            SeverityArg::Info => Self::Info,
            SeverityArg::Low => Self::Low,
            SeverityArg::Medium => Self::Medium,
            SeverityArg::High => Self::High,
            SeverityArg::Critical => Self::Critical,
        }
    }
}

/// `--color`, global across every subcommand — the same "applies uniformly regardless
/// of which subcommand is running" reasoning `--verbose` on [`Cli`] already
/// follows, since color is exactly that kind of cross-cutting rendering
/// concern, not something specific to `lint` alone.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ColorArg {
    /// Color when stdout is an interactive terminal, `NO_COLOR` is unset,
    /// and the report is not being written to `--output`.
    Auto,
    /// Always emit ANSI escape codes for `--format human`, regardless of
    /// terminal/`NO_COLOR`/`--output` — for callers piping through a
    /// color-aware pager (e.g. `less -R`).
    Always,
    /// Never emit ANSI escape codes.
    Never,
}

/// Resolves [`ColorArg`] to the plain bool [`sting_pipeline::ReportController::render`]
/// takes — `output.is_some()` (writing to a file) always wins over `Auto`
/// even on a machine where stdout happens to be a terminal, since color
/// only makes sense for `Auto` when stdout itself *is* the destination
/// (: a `--output
/// report.txt` file must never contain escape codes, matching why SARIF/
/// JSON never get colorized either).
fn resolve_color(color: ColorArg, output: Option<&Path>) -> bool {
    match color {
        ColorArg::Never => false,
        ColorArg::Always => true,
        ColorArg::Auto => {
            output.is_none()
                && std::env::var_os("NO_COLOR").is_none()
                && std::io::stdout().is_terminal()
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:?}");
            ExitCode::FAILURE
        }
    }
}

/// Sets up `tracing-subscriber` (`docs/API.md`).
/// `RUST_LOG` always wins when set (standard `tracing-subscriber`
/// precedence, lets a caller trace a single crate/target without the
/// blanket `-vv`); otherwise `-v`/`-vv` pick a workspace-wide default level.
/// Deliberately written to **stderr**, never stdout: `sting lint --format
/// sarif`/`--format json` prints machine-readable output to stdout, and log
/// lines interleaved into that stream would corrupt it for any downstream
/// parser: output must not be interleaved, and that applies to the two
/// streams as much as to two threads (`docs/API.md`).
fn init_tracing(verbose: u8) {
    use tracing_subscriber::EnvFilter;
    let default_level = match verbose {
        0 => "warn",
        1 => "debug",
        _ => "trace",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    match &cli.command {
        Command::Doctor => doctor(),
        Command::Config {
            command: ConfigCommand::Dump { config },
        } => config_dump(config.as_deref()),
        Command::Explain { detector_id } => explain(detector_id),
        Command::Lint {
            path,
            format,
            output,
            config,
            min_severity,
            fix,
        } => lint(
            path,
            *format,
            output.as_deref(),
            config.as_deref(),
            min_severity.map(Into::into),
            *fix,
            cli.color,
        ),
        Command::Scan {
            address,
            get_method,
            force_refresh,
            testnet,
            format,
            output,
        } => scan(
            address,
            get_method,
            *force_refresh,
            *testnet,
            *format,
            output.as_deref(),
            cli.color,
        ),
        Command::Fuzz {
            target,
            time,
            extra_args,
        } => fuzz(target, *time, extra_args),
        Command::Bench {
            corpus,
            report,
            baseline,
            update_baseline,
        } => bench(
            corpus.as_deref(),
            report.as_deref(),
            baseline.as_deref(),
            *update_baseline,
        ),
        Command::Fetch {
            address,
            force_refresh,
            testnet,
            out,
        } => fetch(address, *force_refresh, *testnet, out.as_deref()),
        Command::Verify {
            path,
            address,
            hypotheses,
            force_refresh,
            testnet,
            baseline,
            config,
            format,
            output,
            min_severity,
        } => verify(
            path,
            address,
            hypotheses,
            *force_refresh,
            *testnet,
            baseline.as_deref(),
            config.as_deref(),
            *format,
            output.as_deref(),
            min_severity.map(Into::into),
            cli.color,
        ),
    }
}

/// Walks up from `start` looking for a file named `filename`, the same
/// convention `Cargo.toml` and `Acton.toml` use: `--config <path>` overrides
/// it, and without one `Sting.toml` is searched for upward from the current
/// directory (`docs/API.md`). Returns the first match's full path, nearest
/// first, or
/// `None` if no ancestor (including `start` itself) has one.
fn find_upward(start: &Path, filename: &str) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Resolves which `Sting.toml` to read: an explicit `--config` path (error if
/// missing), else the nearest `Sting.toml` found by [`find_upward`] from the
/// current directory, else "no file" (built-in defaults).
fn load_config_source(explicit_path: Option<&Path>) -> anyhow::Result<(Option<PathBuf>, String)> {
    if let Some(path) = explicit_path {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        return Ok((Some(path.to_path_buf()), text));
    }
    let cwd = std::env::current_dir().context("determining the current directory")?;
    if let Some(found) = find_upward(&cwd, "Sting.toml") {
        let text = std::fs::read_to_string(&found)
            .with_context(|| format!("reading {}", found.display()))?;
        tracing::debug!(path = %found.display(), "found Sting.toml by upward search");
        return Ok((Some(found), text));
    }
    Ok((None, String::new()))
}

/// Loads `Sting.toml` (or defaults), builds the effective
/// [`sting_detectors::DetectorRegistry`], and returns a human-readable label
/// for where the config came from — the shared setup both `config dump` and
/// `lint` need (the label is `config dump`-only, but cheap enough to always
/// compute).
fn load_registry(
    explicit_config: Option<&Path>,
) -> anyhow::Result<(
    sting_detectors::DetectorRegistry,
    sting_core::StingConfig,
    String,
)> {
    let (path, source) = load_config_source(explicit_config)?;
    let source_label = path.as_ref().map_or_else(
        || "<built-in defaults, no Sting.toml found>".to_string(),
        |p| p.display().to_string(),
    );
    let config = sting_core::StingConfig::parse(&source)
        .with_context(|| format!("parsing {source_label}"))?;
    let registry = sting_detectors::DetectorRegistry::builtin()
        .filtered(&config)
        .with_context(|| format!("applying detectors.enable/disable from {source_label}"))?;
    Ok((registry, config, source_label))
}

/// Prints one warning per `[[suppressions.rule]]` entry that matched zero
/// findings this run — the hygiene check `sting-pipeline::LintController`
/// detects but deliberately leaves the caller to report. To stderr
/// via `tracing::warn!`, same channel as `LintFailure`s just above each call
/// site, not stdout — never mixed into the machine-readable report itself.
fn warn_unused_suppressions(unused: &[sting_core::SuppressionRule]) {
    for rule in unused {
        tracing::warn!(
            detector = %rule.detector,
            file = %rule.file.display(),
            line = rule.line,
            "unused suppression: matched no finding this run — stale, or the code it excused was already fixed"
        );
    }
}

fn config_dump(explicit_path: Option<&Path>) -> anyhow::Result<()> {
    let (registry, config, source_label) = load_registry(explicit_path)?;
    let active_ids: Vec<_> = registry
        .detectors()
        .map(sting_detectors::Detector::id)
        .collect();

    println!("# effective configuration (source: {source_label})");
    println!();
    println!("[detectors]");
    println!("enable = {:?}", config.detectors.enable);
    println!("disable = {:?}", config.detectors.disable);
    println!();
    println!(
        "# active detectors after enable/disable filtering: {}",
        if active_ids.is_empty() {
            "(none)".to_string()
        } else {
            active_ids.join(", ")
        }
    );
    println!();
    println!(
        "min_severity = {}",
        config
            .min_severity
            .map_or_else(|| "(none)".to_string(), |s| format!("{s:?}"))
    );
    println!();
    println!(
        "# {} suppression rule(s) configured",
        config.suppressions.rules.len()
    );
    for rule in &config.suppressions.rules {
        println!(
            "[[suppressions.rule]] detector={:?} file={:?} line={} reason={:?}",
            rule.detector,
            rule.file.display().to_string(),
            rule.line,
            rule.reason
        );
    }

    Ok(())
}

/// `sting explain <ID>`. Case-insensitive on input (`"ac"`/`"AC"` both
/// resolve) — [`sting_detectors::explain::explain`] itself stays a plain
/// exact match against the always-uppercase [`sting_core::DetectorId`]
/// values, matching the rest of this codebase's convention of doing
/// user-friendliness normalization once at the CLI boundary, not inside a
/// library function.
fn explain(detector_id: &str) -> anyhow::Result<()> {
    let normalized = detector_id.to_uppercase();
    let Some(explanation) = sting_detectors::explain::explain(&normalized) else {
        let known = sting_detectors::explain::known_ids().join(", ");
        anyhow::bail!("unknown detector id {detector_id:?} — known detectors: {known}");
    };
    println!("{}\n", explanation.name);
    println!("{}\n", explanation.summary);
    println!(
        "False positives / known limitations:\n{}\n",
        explanation.false_positives
    );
    println!("Severity: {}", explanation.severity_rationale);
    Ok(())
}

#[tracing::instrument(skip(format, output, explicit_config), fields(path = %path.display()))]
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors Command::Lint's own field list one-for-one — a struct here would be pure \
              ceremony for a single non-test call site, same reasoning already applied to verify"
)]
fn lint(
    path: &Path,
    format: Option<OutputFormat>,
    output: Option<&Path>,
    explicit_config: Option<&Path>,
    min_severity: Option<sting_core::Severity>,
    fix: bool,
    color: ColorArg,
) -> anyhow::Result<()> {
    let (registry, config, _source_label) = load_registry(explicit_config)?;

    let paths = sting_pipeline::resolve_tolk_files(path)
        .with_context(|| format!("resolving {}", path.display()))?;
    if paths.is_empty() {
        anyhow::bail!("no .tolk files found at {}", path.display());
    }
    tracing::info!(file_count = paths.len(), "linting");

    let controller = sting_pipeline::LintController::new(&registry)
        .with_suppressions(&config.suppressions.rules)
        .with_min_severity(min_severity.or(config.min_severity));
    let mut outcome = controller.lint_paths(&paths);

    if fix {
        let fix_outcome = sting_pipeline::FixController.apply(&outcome.findings);
        report_fix_outcome(&fix_outcome);
        if !fix_outcome.applied.is_empty() {
            // Re-lint so the report printed below reflects the file as it
            // is now on disk, not the pre-fix findings (which include ones
            // that no longer exist) — the same reason `cargo fix` recompiles
            // after applying fixes instead of reusing the first pass's
            // diagnostics.
            outcome = controller.lint_paths(&paths);
        }
    } else {
        print_fix_hint(&outcome.findings);
    }

    for failure in &outcome.failures {
        tracing::warn!(file = %failure.file.display(), error = %failure.error, "file could not be analyzed");
    }
    warn_unparsed(&outcome.unparsed);
    warn_without_a_rule(&outcome.without_a_rule);
    warn_unused_suppressions(&outcome.unused_suppressions);

    let resolved_format = format.map_or_else(|| default_format(output), Into::into);
    let rendered = sting_pipeline::ReportController
        .render(
            &outcome.findings,
            resolved_format,
            resolve_color(color, output),
        )
        .context("rendering report")?;

    match output {
        Some(out_path) => std::fs::write(out_path, &rendered)
            .with_context(|| format!("writing {}", out_path.display()))?,
        None => println!("{rendered}"),
    }

    if !outcome.failures.is_empty() {
        anyhow::bail!(
            "{} of {} file(s) could not be analyzed (see warnings above)",
            outcome.failures.len(),
            paths.len()
        );
    }
    if !outcome.unparsed.is_empty() {
        anyhow::bail!(
            "{} of {} file(s) were read only in part (see warnings above): \
             a clean report on them would mean the grammar's silence, not the code's",
            outcome.unparsed.len(),
            paths.len()
        );
    }
    Ok(())
}

/// Prints the four versions a run's answer depends on.
///
/// Exits non-zero when any of them is unknown. One is enough: a report that
/// names three and shrugs at the fourth says less than it appears to, and the
/// shrug is exactly the state that lets an answer look checkable when it is
/// not.
///
/// # Errors
/// Returns an error naming every component whose version could not be read.
fn doctor() -> anyhow::Result<()> {
    let report = sting_pipeline::DoctorController.report();

    for component in &report.components {
        let (mark, text) = match &component.state {
            sting_pipeline::VersionState::Known(version) => ("ok", version.as_str()),
            sting_pipeline::VersionState::Unknown(reason) => ("unknown", reason.as_str()),
        };
        println!("{mark:>7}  {}: {text}", component.name);
        println!("         from {}", component.source);
    }

    if report.all_known() {
        return Ok(());
    }
    let unknown: Vec<&str> = report.unknown().map(|component| component.name).collect();
    anyhow::bail!(
        "{} of {} versions could not be read ({}): a run against an unnamed version \
         cannot be checked by anyone",
        unknown.len(),
        report.components.len(),
        unknown.join(", ")
    )
}

/// Names every file the grammar read only part of, with the position it
/// stopped at.
///
/// Silence here is the one failure mode a static analyser cannot afford. A file
/// with an `ERROR` node still yields a tree, every detector still runs over
/// what parsed, and the report still comes back — indistinguishable from a
/// clean file unless the gap is stated. To stderr through `tracing::warn!`,
/// the same channel as a `LintFailure`, never into the machine-readable report.
fn warn_unparsed(unparsed: &[sting_pipeline::UnparsedFile]) {
    for file in unparsed {
        tracing::warn!(
            file = %file.file.display(),
            line = file.region.range.start_line,
            column = file.region.range.start_column,
            kind = file.region.kind,
            regions = file.region.total,
            "the grammar could not read this file whole; findings cover only what parsed"
        );
    }
}

/// Names the detectors that were not run against a file because they have no rule for its
/// language.
///
/// Reported rather than dropped, and reported once per language rather than once per file: a run
/// over a hundred `FunC` files would otherwise say the same thing a hundred times, and a warning
/// nobody finishes reading is a warning nobody reads. **A detector that never looked is not a
/// detector that found nothing**, and a report showing only the second credits this analyser for
/// code no rule of its own examined.
fn warn_without_a_rule(held_back: &[sting_pipeline::DetectorsWithoutARule]) {
    let mut by_language: std::collections::BTreeMap<&str, (usize, Vec<String>)> =
        std::collections::BTreeMap::new();
    for entry in held_back {
        let seen = by_language
            .entry(entry.language.label())
            .or_insert_with(|| (0, entry.detectors.clone()));
        seen.0 += 1;
    }
    for (language, (files, detectors)) in by_language {
        tracing::warn!(
            language,
            files,
            detectors = detectors.join(", "),
            "these detectors have no rule for this language and were not run; their silence is not a clean result"
        );
    }
}

/// Prints the `sting lint --fix` availability hint — a `rustc`-style
/// "N machine-applicable fix(es) available" line, grouped by detector, only
/// when at least one finding actually carries a `suggestion` (silent
/// otherwise, matching `cargo`'s own convention of saying nothing when there
/// is nothing to say).
fn print_fix_hint(findings: &[sting_core::Finding]) {
    let counts = fixable_counts_by_detector(findings);
    if counts.is_empty() {
        return;
    }
    let total: usize = counts.iter().map(|(_, count)| count).sum();
    let breakdown = counts
        .iter()
        .map(|(id, count)| format!("{id}×{count}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!(
        "\n{total} machine-applicable fix(es) available ({breakdown}) — run with `--fix` to apply"
    );
}

/// `(detector_id, count)` for every finding in `findings` carrying a
/// `suggestion`, grouped and sorted by detector id — a plain `Vec` rather
/// than a `HashMap` since the input is at most a few dozen findings and the
/// output must print in a stable order.
fn fixable_counts_by_detector(findings: &[sting_core::Finding]) -> Vec<(&'static str, usize)> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for finding in findings.iter().filter(|f| f.suggestion.is_some()) {
        match counts.iter_mut().find(|(id, _)| *id == finding.detector_id) {
            Some((_, count)) => *count += 1,
            None => counts.push((finding.detector_id, 1)),
        }
    }
    counts.sort_by_key(|(id, _)| *id);
    counts
}

/// Reports what `sting lint --fix` actually did on disk: which fixes were
/// applied (grouped the same way as [`print_fix_hint`]), any suggestion
/// skipped because its byte range overlapped another, and any file that
/// could not be read/written.
fn report_fix_outcome(outcome: &sting_pipeline::FixOutcome) {
    if outcome.applied.is_empty() {
        println!("no fixes applied");
    } else {
        let mut by_file: Vec<(&Path, usize)> = Vec::new();
        for finding in &outcome.applied {
            let file = finding.location.file.as_path();
            match by_file.iter_mut().find(|(f, _)| *f == file) {
                Some((_, count)) => *count += 1,
                None => by_file.push((file, 1)),
            }
        }
        by_file.sort_by_key(|(file, _)| *file);
        println!(
            "fixed {} finding(s) across {} file(s):",
            outcome.applied.len(),
            by_file.len()
        );
        for (file, count) in by_file {
            println!("  {}: {count} fix(es) applied", file.display());
        }
    }
    for finding in &outcome.skipped_overlaps {
        tracing::warn!(
            file = %finding.location.file.display(),
            line = finding.location.start_line,
            detector = finding.detector_id,
            "skipped fix: its suggestion overlaps another finding's suggestion in the same file"
        );
    }
    for (file, error) in &outcome.read_failures {
        tracing::warn!(file = %file.display(), %error, "could not read file to apply fixes");
    }
    for (file, error) in &outcome.write_failures {
        tracing::warn!(file = %file.display(), %error, "could not write fixed file back to disk");
    }
}

#[tracing::instrument(skip(corpus, report, baseline))]
fn bench(
    corpus: Option<&Path>,
    report: Option<&Path>,
    baseline: Option<&Path>,
    update_baseline: bool,
) -> anyhow::Result<()> {
    let corpus_root = corpus.map_or_else(|| PathBuf::from("corpus"), Path::to_path_buf);
    let baseline_path =
        baseline.map_or_else(|| corpus_root.join("baseline.json"), Path::to_path_buf);

    let bench_report = sting_bench::run_benchmark(&corpus_root)
        .with_context(|| format!("running benchmark over {}", corpus_root.display()))?;

    let markdown = sting_bench::to_markdown(&bench_report);
    match report {
        Some(path) => {
            std::fs::write(path, &markdown)
                .with_context(|| format!("writing {}", path.display()))?;
            let json_path = path.with_extension("json");
            std::fs::write(&json_path, sting_bench::to_json_string(&bench_report))
                .with_context(|| format!("writing {}", json_path.display()))?;
        }
        None => println!("{markdown}"),
    }

    if update_baseline {
        let snapshot = sting_bench::Baseline::from(&bench_report);
        std::fs::write(
            &baseline_path,
            sting_bench::baseline_to_json_string(&snapshot),
        )
        .with_context(|| format!("writing {}", baseline_path.display()))?;
        tracing::info!(path = %baseline_path.display(), "baseline updated");
        return Ok(());
    }

    if baseline_path.is_file() {
        let snapshot = sting_bench::load_baseline(&baseline_path)
            .with_context(|| format!("loading {}", baseline_path.display()))?;
        // ±1 finding, per docs/domains/precision-sources.md's own
        // example threshold — a corpus of a few dozen contracts makes a
        // stricter gate mostly noise from single-file statistical wobble,
        // not a real signal; revisit once the corpus is large enough that
        // one file's flip is negligible relative to the whole.
        let regressions = sting_bench::compare_to_baseline(&bench_report, &snapshot, 1);
        if !regressions.is_empty() {
            for r in &regressions {
                tracing::warn!(detector_id = %r.detector_id, "{}", r.description);
            }
            anyhow::bail!("{} regression(s) relative to baseline", regressions.len());
        }
    } else {
        tracing::warn!(
            "no baseline found at {} (first run? use --update-baseline to create one)",
            baseline_path.display()
        );
    }

    Ok(())
}

/// Builds a [`sting_pipeline::CorpusController`] against the platform disk
/// cache and the real `toncenter`-falling-back-to-`tonapi` source chain
/// (`docs/DATA.md`) — the construction `fetch`
/// and `scan` share, since both resolve an on-chain address the same way.
/// `testnet` asks both providers for the other network, which each of them
/// publishes its own host for.
fn build_corpus_controller(testnet: bool) -> anyhow::Result<sting_pipeline::CorpusController> {
    // A separate cache subdirectory per network — found during a full
    // project review: the cache key itself (`sting_corpus::cache_key`) is
    // address-text-only, not network-qualified, so without this,
    // `sting fetch <addr>` followed by `sting fetch <addr> --testnet`
    // within the TTL would silently serve the wrong network's bytecode
    // (a real risk: deterministic TON addresses commonly exist on both
    // networks for the same test contract). Scoping the whole cache *root*
    // by network — not just the key — closes this without touching
    // `sting-corpus`'s `DiskCache`/`cache_key` at all: two entirely
    // separate directory trees can never collide, by construction, not by
    // relying on every current and future cache-key caller to remember to
    // mix in a network label.
    let cache_root = sting_corpus::DiskCache::default_root()
        .context("determining the platform cache directory")?
        .join(if testnet { "testnet" } else { "mainnet" });
    let cache = sting_corpus::DiskCache::new(cache_root, sting_corpus::DEFAULT_TTL);
    // Which host answers for a network is the chain client's table, not this
    // command's: a second copy of it here is how one of them ends up a host
    // behind.
    let network = if testnet {
        sting_corpus::Network::Testnet
    } else {
        sting_corpus::Network::Mainnet
    };
    let sources: Vec<Box<dyn sting_corpus::ContractSource>> = vec![
        Box::new(sting_corpus::ToncenterClient::for_network(network)),
        Box::new(sting_corpus::TonapiClient::for_network(network)),
    ];
    Ok(sting_pipeline::CorpusController::new(sources, cache))
}

/// Resolves `address` through [`sting_pipeline::CorpusController`] (real
/// `toncenter`, falling back to `tonapi`, per
/// `docs/DATA.md`) and writes the result to
/// the disk cache — and, if `out` is given, additionally to `out` in the
/// reusable `<address>.{code,data}.boc.base64` format (§5.3).
///
/// Deliberately does **not** attempt anything static-analysis-shaped with
/// the result — see this subcommand's own `--help` text (`Command::Fetch`'s
/// doc comment) for why an on-chain address can never feed `sting lint`.
#[tracing::instrument(skip(out))]
fn fetch(
    address: &str,
    force_refresh: bool,
    testnet: bool,
    out: Option<&Path>,
) -> anyhow::Result<()> {
    let controller = build_corpus_controller(testnet)?;
    let target = sting_corpus::ContractTarget::OnChainAddress(address.to_string());
    let options = sting_pipeline::FetchOptions { force_refresh };
    let code = controller
        .resolve(&target, options)
        .with_context(|| format!("fetching {address}"))?;

    println!("fetched {address}");
    println!("  code: {} bytes (base64)", code.code_boc_base64.len());
    println!("  data: {} bytes (base64)", code.data_boc_base64.len());

    if let Some(out_dir) = out {
        std::fs::create_dir_all(out_dir)
            .with_context(|| format!("creating {}", out_dir.display()))?;
        let key = sting_corpus::cache_key(address);
        let code_path = out_dir.join(format!("{key}.code.boc.base64"));
        let data_path = out_dir.join(format!("{key}.data.boc.base64"));
        std::fs::write(&code_path, &code.code_boc_base64)
            .with_context(|| format!("writing {}", code_path.display()))?;
        std::fs::write(&data_path, &code.data_boc_base64)
            .with_context(|| format!("writing {}", data_path.display()))?;
        println!("  written to {}", out_dir.display());
    }

    Ok(())
}

/// Builds a [`sting_core::Finding`] pointing at `address` itself (line 1,
/// column 1 — there is no real source location for an on-chain-address-level
/// problem like a failed structural check or a failed get-method call) —
/// factored out of [`scan`] once the same five-field `Location` literal
/// started appearing at every one of its failure sites.
fn address_level_finding(
    detector_id: sting_core::DetectorId,
    severity: sting_core::Severity,
    message: String,
    remediation: String,
    address: &str,
) -> sting_core::Finding {
    sting_core::Finding {
        detector_id,
        severity,
        message,
        remediation,
        location: sting_core::Location {
            file: PathBuf::from(address),
            start_line: 1,
            start_column: 1,
            end_line: 1,
            end_column: 1,
        },
        confidence: None,
        dynamic_verification: None,
        suggestion: None,
    }
}

/// Dynamic-only analysis of a deployed contract by address
/// (`docs/API.md`, `Command::Scan`'s doc comment for
/// the full scope explanation). Uses the Tier 1, in-process
/// [`sting_pipeline::EmulationPoolController`], the same as
/// `BridgeController` — **not** the Tier 2, subprocess-isolated pool
/// `docs/ARCHITECTURE.md` sketches for an arbitrary untrusted address. That
/// deferral is deliberate: full Tier 2 is an escalation to take when real
/// usage shows hangs actually occur, and no such observation exists. The
/// residual risk — a `libemulator` bug hanging independently of `gas_limit` —
/// is
/// accepted here the same way Tier 1 already accepts it elsewhere, logged at
/// `warn` if a single emulator call takes unexpectedly long.
#[tracing::instrument(skip(get_methods, format, output))]
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors Command::Scan's own field list one-for-one — a struct here would be pure \
              ceremony for a single non-test call site, same reasoning already applied to verify"
)]
fn scan(
    address: &str,
    get_methods: &[String],
    force_refresh: bool,
    testnet: bool,
    format: Option<OutputFormat>,
    output: Option<&Path>,
    color: ColorArg,
) -> anyhow::Result<()> {
    let controller = build_corpus_controller(testnet)?;
    let contract_target = sting_corpus::ContractTarget::OnChainAddress(address.to_string());
    let options = sting_pipeline::FetchOptions { force_refresh };
    let code = controller
        .resolve(&contract_target, options)
        .with_context(|| format!("fetching {address}"))?;

    let lib = sting_emulator::load_default_library().context(
        "loading libemulator — set STING_LIBEMULATOR_PATH or place it next to the sting \
         binary, see docs/domains/emulator.md",
    )?;
    let pool = sting_pipeline::EmulationPoolController::new(&lib);
    let target = sting_pipeline::EmulationTarget {
        code_boc_base64: code.code_boc_base64,
        data_boc_base64: code.data_boc_base64,
        address: address.to_string(),
    };

    // Resolved up front (not just before rendering) so the structural-check
    // and get-method status lines below can be gated on it: printing them
    // unconditionally interleaved plain text into stdout ahead of a
    // `--format json`/`--format sarif` payload — or ahead of the *default*
    // format when stdout isn't a TTY (`default_format`) — broke any
    // downstream parser, the same stdout-purity concern `init_tracing`'s doc
    // comment already states for logging. Human format is the only one
    // meant for a human/terminal to read at all, so these lines only make
    // sense there.
    let resolved_format = format.map_or_else(|| default_format(output), Into::into);
    let is_human = resolved_format == sting_pipeline::ReportFormat::Human;

    let mut findings = Vec::new();
    let structural_check_start = std::time::Instant::now();
    match pool.check_structural_validity(&target) {
        Ok(()) => {
            if is_human {
                println!("structural check: OK (code/data load as valid TVM state)");
            }
        }
        Err(err) => {
            tracing::warn!(%err, "structural validity check failed");
            findings.push(address_level_finding(
                "SCAN-INVALID-CODE",
                sting_core::Severity::High,
                format!(
                    "fetched code/data for {address} could not be loaded as valid TVM state: {err}"
                ),
                "re-check the address is correct and actually deployed on the network you targeted (mainnet vs `--testnet`) — a wrong network, an unfinished deploy, or a non-contract account (e.g. a plain wallet with no code) would all fail to load as valid TVM state this way".to_string(),
                address,
            ));
        }
    }
    // Soft wall-clock guard (`docs/ARCHITECTURE.md`'s Tier-1
    // stance: `gas_limit` is the primary defense, a wall-clock check only
    // logs — never kills the thread, which Rust cannot safely do here).
    if structural_check_start.elapsed() > SLOW_CALL_WARNING_THRESHOLD {
        tracing::warn!(
            elapsed_ms = structural_check_start.elapsed().as_millis(),
            "structural validity check took unusually long — possible libemulator hang risk, \
             see docs/ARCHITECTURE.md"
        );
    }

    for method in get_methods {
        let call_start = std::time::Instant::now();
        match pool.run_get_method(&target, method) {
            Ok(outcome) => {
                if is_human {
                    println!(
                        "get-method `{method}`: success={} vm_exit_code={:?}",
                        outcome.success, outcome.vm_exit_code
                    );
                    if let Some(stack) = &outcome.stack_boc_base64 {
                        println!("  stack (base64 BoC): {stack}");
                    }
                }
            }
            Err(err) => {
                // Mirrors the structural-check failure branch above: a
                // pushed Finding, not just a cosmetic println — found
                // during a full project review, a failed --get-method call
                // used to be printed (in human mode only) but never
                // affected `findings`/the exit code, so a script checking
                // `sting scan`'s exit status saw a false "0 = success" even
                // when every probed get-method failed.
                tracing::warn!(method, %err, "get-method call failed");
                findings.push(address_level_finding(
                    "SCAN-GET-METHOD-FAILED",
                    sting_core::Severity::Medium,
                    format!("get-method `{method}` on {address} failed: {err}"),
                    format!(
                        "confirm `{method}` is actually a get-method this contract exposes (a typo'd name fails the same way as a real error) and that it takes no arguments — `sting scan` only calls get-methods with an empty argument stack"
                    ),
                    address,
                ));
            }
        }
        if call_start.elapsed() > SLOW_CALL_WARNING_THRESHOLD {
            tracing::warn!(
                method,
                elapsed_ms = call_start.elapsed().as_millis(),
                "get-method call took unusually long — possible libemulator hang risk, see \
                 docs/ARCHITECTURE.md"
            );
        }
    }

    let rendered = sting_pipeline::ReportController
        .render(&findings, resolved_format, resolve_color(color, output))
        .context("rendering report")?;
    match output {
        Some(out_path) => std::fs::write(out_path, &rendered)
            .with_context(|| format!("writing {}", out_path.display()))?,
        None => println!("{rendered}"),
    }

    if !findings.is_empty() {
        anyhow::bail!("scan reported {} finding(s) for {address}", findings.len());
    }
    Ok(())
}

/// Wall-clock threshold past which a single Tier 1 emulator call in `scan`
/// logs a `warn` (never kills anything — see [`scan`]'s own doc comment).
/// 5 seconds: generous relative to every real call observed in this
/// workspace's own test suite (low milliseconds), but short enough to
/// surface a genuine hang candidate promptly instead of leaving an operator
/// staring at a silently-stuck terminal.
const SLOW_CALL_WARNING_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(5);

/// Builds a [`sting_pipeline::DetectorStatsSnapshot`] from a loaded
/// [`sting_bench::Baseline`] — the one-way conversion
/// `DetectorStatsSnapshot`'s own doc comment says belongs in `sting-cli`:
/// `sting-pipeline` deliberately does not depend on `sting-bench` (that
/// dependency already runs the other way, `docs/ARCHITECTURE.md`
/// §8), so this narrow conversion has nowhere else to live. A detector with
/// zero recorded true-and-false positives (`denom == 0`, e.g. never measured
/// at all) is omitted rather than inserted as `0.0`/`NaN` —
/// `ScoringController`'s own neutral prior already handles an absent entry
/// correctly, and either of those would actively misrepresent "unmeasured"
/// as "measured and terrible".
fn detector_stats_from_baseline(
    baseline: &sting_bench::Baseline,
) -> sting_pipeline::DetectorStatsSnapshot {
    let precision_by_detector = baseline
        .detectors
        .iter()
        .filter_map(|(id, snapshot)| {
            let denom = snapshot.true_positives + snapshot.false_positives;
            if denom == 0 {
                return None;
            }
            // `f64::from(u32)` is lossless; the narrowing back to f32 below
            // is not — but `denom` is a handful of hand-labeled corpus
            // examples in practice (`corpus/baseline.json`), nowhere near
            // f32's 2^24 exact-integer ceiling, so no real precision is
            // lost. `DetectorStatsSnapshot` is pre-existing and fixed at
            // f32 (`docs/domains/emulator.md`, matching
            // `ScoringController`'s own weights) — not a type this new
            // caller gets to change for its own convenience.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "f64 -> f32 narrowing of a corpus-scale precision ratio, see comment above"
            )]
            let precision = (f64::from(snapshot.true_positives) / f64::from(denom)) as f32;
            Some((id.clone(), precision))
        })
        .collect();
    sting_pipeline::DetectorStatsSnapshot::new(precision_by_detector)
}

/// Closes the static→dynamic loop (`Command::Verify`'s doc comment for the
/// full scope explanation). One [`sting_pipeline::EmulationTarget`] for the
/// whole run, dynamically verified once per unique detector class present
/// among the findings — not once per finding: every `BridgeController`
/// strategy's outcome depends only on the `(Hypothesis, EmulationTarget)`
/// pair, never on which specific finding triggered it, so a directory with
/// several findings of the same class would otherwise repeat the identical
/// real emulator call for no new information.
#[tracing::instrument(
    skip(hypotheses_path, baseline, explicit_config, format, output),
    fields(path = %path.display(), address = %address)
)]
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors Command::Verify's own field list one-for-one — the union of scan's and \
              lint's argument surfaces, not sloppy parameterization; a struct here would be pure \
              ceremony for a single non-test call site"
)]
fn verify(
    path: &Path,
    address: &str,
    hypotheses_path: &Path,
    force_refresh: bool,
    testnet: bool,
    baseline: Option<&Path>,
    explicit_config: Option<&Path>,
    format: Option<OutputFormat>,
    output: Option<&Path>,
    min_severity: Option<sting_core::Severity>,
    color: ColorArg,
) -> anyhow::Result<()> {
    let (registry, config, _source_label) = load_registry(explicit_config)?;

    let paths = sting_pipeline::resolve_tolk_files(path)
        .with_context(|| format!("resolving {}", path.display()))?;
    if paths.is_empty() {
        anyhow::bail!("no .tolk files found at {}", path.display());
    }
    let outcome = sting_pipeline::LintController::new(&registry)
        .with_suppressions(&config.suppressions.rules)
        .with_min_severity(min_severity.or(config.min_severity))
        .lint_paths(&paths);
    for failure in &outcome.failures {
        tracing::warn!(file = %failure.file.display(), error = %failure.error, "file could not be analyzed");
    }
    warn_unused_suppressions(&outcome.unused_suppressions);
    let mut findings = outcome.findings;
    tracing::info!(finding_count = findings.len(), "static analysis complete");

    let hypotheses_source = std::fs::read_to_string(hypotheses_path)
        .with_context(|| format!("reading {}", hypotheses_path.display()))?;
    let spec = sting_pipeline::HypothesisSpec::parse(&hypotheses_source)
        .with_context(|| format!("parsing {}", hypotheses_path.display()))?;

    let flagged_classes: std::collections::BTreeSet<&str> =
        findings.iter().map(|f| f.detector_id).collect();
    for id in spec.detector_ids() {
        if !flagged_classes.contains(id) {
            tracing::warn!(
                detector_id = id,
                "hypothesis file has a [{id}] section but no finding of that class was found in \
                 {} — check for a typo in --hypotheses",
                path.display()
            );
        }
    }

    let controller = build_corpus_controller(testnet)?;
    let contract_target = sting_corpus::ContractTarget::OnChainAddress(address.to_string());
    let options = sting_pipeline::FetchOptions { force_refresh };
    let code = controller
        .resolve(&contract_target, options)
        .with_context(|| format!("fetching {address}"))?;
    let lib = sting_emulator::load_default_library().context(
        "loading libemulator — set STING_LIBEMULATOR_PATH or place it next to the sting \
         binary, see docs/domains/emulator.md",
    )?;
    let pool = sting_pipeline::EmulationPoolController::new(&lib);
    let bridge = sting_pipeline::BridgeController::new(&pool);
    let target = sting_pipeline::EmulationTarget {
        code_boc_base64: code.code_boc_base64,
        data_boc_base64: code.data_boc_base64,
        address: address.to_string(),
    };

    let mut verified_classes: std::collections::BTreeMap<&str, sting_core::DynamicVerification> =
        std::collections::BTreeMap::new();
    for &class in &flagged_classes {
        let Some(hypothesis) = spec.hypothesis_for(class) else {
            continue;
        };
        tracing::info!(detector_id = class, "running dynamic verification");
        let verification = bridge.verify(&hypothesis, &target);
        tracing::info!(detector_id = class, result = ?verification, "dynamic verification complete");
        verified_classes.insert(class, verification);
    }
    for finding in &mut findings {
        if let Some(verification) = verified_classes.get(finding.detector_id) {
            finding.dynamic_verification = Some(verification.clone());
        }
    }

    let baseline_path =
        baseline.map_or_else(|| PathBuf::from("corpus/baseline.json"), Path::to_path_buf);
    let stats = if baseline_path.is_file() {
        let loaded = sting_bench::load_baseline(&baseline_path)
            .with_context(|| format!("loading {}", baseline_path.display()))?;
        detector_stats_from_baseline(&loaded)
    } else {
        tracing::warn!(
            "no baseline found at {} — every finding scores with the neutral per-detector \
             precision prior (use --baseline, or run `sting bench --update-baseline` first)",
            baseline_path.display()
        );
        sting_pipeline::DetectorStatsSnapshot::default()
    };
    let scoring = sting_pipeline::ScoringController::new(stats);
    for finding in &mut findings {
        finding.confidence = Some(scoring.score(finding));
    }

    let resolved_format = format.map_or_else(|| default_format(output), Into::into);
    let rendered = sting_pipeline::ReportController
        .render(&findings, resolved_format, resolve_color(color, output))
        .context("rendering report")?;
    match output {
        Some(out_path) => std::fs::write(out_path, &rendered)
            .with_context(|| format!("writing {}", out_path.display()))?,
        None => println!("{rendered}"),
    }

    if !outcome.failures.is_empty() {
        anyhow::bail!(
            "{} file(s) could not be analyzed (see warnings above)",
            outcome.failures.len()
        );
    }
    Ok(())
}

/// Locates the sting workspace root by walking up from `start` looking for a
/// top-level `Cargo.toml` containing a `[workspace]` table — the same
/// upward-search idea as [`find_upward`], specialized to what `fuzz` needs
/// (the `crates/sting-fuzz/fuzz/` directory only exists inside a full
/// checkout, never next to an installed release binary — see
/// `Command::Fuzz`'s doc comment).
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.is_file()
            && std::fs::read_to_string(&candidate).is_ok_and(|text| text.contains("[workspace]"))
        {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Thin wrapper over `cargo fuzz run` (`Command::Fuzz`'s doc comment for the
/// full reasoning) — locates `crates/sting-fuzz/fuzz/`, then execs `cargo
/// +nightly fuzz run <target> -- -max_total_time=<time> <extra_args>`,
/// inheriting stdio so `cargo-fuzz`'s own real-time output (corpus growth,
/// crash reports) reaches the terminal directly instead of being buffered
/// and re-printed.
#[tracing::instrument(skip(extra_args))]
fn fuzz(target: &str, time: u64, extra_args: &[String]) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().context("determining the current directory")?;
    let workspace_root = find_workspace_root(&cwd).context(
        "could not locate the sting workspace root (looked for a top-level Cargo.toml with \
         [workspace]) — `sting fuzz` needs a full sting checkout, not just the installed \
         binary, since the fuzz targets' own source lives in crates/sting-fuzz/fuzz/",
    )?;
    let fuzz_dir = workspace_root.join("crates/sting-fuzz/fuzz");
    if !fuzz_dir.is_dir() {
        anyhow::bail!(
            "fuzz project not found at {} — is this a full sting checkout?",
            fuzz_dir.display()
        );
    }

    tracing::info!(target, time, dir = %fuzz_dir.display(), "invoking cargo fuzz run");
    let status = std::process::Command::new("cargo")
        .current_dir(&fuzz_dir)
        .arg("+nightly")
        .arg("fuzz")
        .arg("run")
        .arg(target)
        .arg("--")
        .arg(format!("-max_total_time={time}"))
        .args(extra_args)
        .status()
        .context(
            "failed to invoke `cargo fuzz` — install a nightly toolchain (`rustup toolchain \
             install nightly`) and cargo-fuzz (`cargo install cargo-fuzz`), see \
             docs/domains/emulator.md",
        )?;
    if !status.success() {
        anyhow::bail!("cargo fuzz run exited with {status}");
    }
    Ok(())
}

/// `--format`'s default when not given explicitly
/// (`docs/ARCHITECTURE.md`): `sarif` for file output, `human` for
/// an interactive terminal, `sarif` otherwise (e.g. piped into another
/// program — machine-readable is the safer default when nothing says a
/// human is reading it directly). `json` is opt-in only via an explicit
/// `--format json` — never guessed as a default, `sarif`'s wider tooling
/// support makes it the safer machine-readable default.
fn default_format(output: Option<&Path>) -> sting_pipeline::ReportFormat {
    if output.is_some() {
        sting_pipeline::ReportFormat::Sarif
    } else if std::io::stdout().is_terminal() {
        sting_pipeline::ReportFormat::Human
    } else {
        sting_pipeline::ReportFormat::Sarif
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        Cli, ColorArg, default_format, detector_stats_from_baseline, find_upward,
        fixable_counts_by_detector, resolve_color,
    };

    #[test]
    fn cli_definition_is_well_formed() {
        // clap's own recommended smoke test: catches malformed arg/subcommand
        // definitions (duplicate flags, conflicting `long`s, etc.) at test
        // time, not on first real invocation.
        use clap::CommandFactory as _;
        Cli::command().debug_assert();
    }

    // `resolve_color`/`default_format`'s `Auto`-with-no-`--output` branches
    // read real process state (`std::io::stdout().is_terminal()`,
    // `NO_COLOR`) that a `cargo test` harness doesn't control deterministically
    // (stdout is normally not a tty under `cargo test`, and mutating
    // process-global env vars in tests that may run in parallel is its own
    // source of flakiness — the same reasoning `sting_tolkc::
    // resolve_executable_path`'s own doc comment already gives for testing
    // the *pure* variant of a similar env-reading function instead). Only
    // the branches whose outcome does not depend on that real environment
    // state are tested directly below.

    #[test]
    fn resolve_color_never_is_always_false() {
        assert!(!resolve_color(ColorArg::Never, None));
        assert!(!resolve_color(ColorArg::Never, Some(Path::new("out.txt"))));
    }

    #[test]
    fn resolve_color_always_is_always_true() {
        assert!(resolve_color(ColorArg::Always, None));
        assert!(resolve_color(ColorArg::Always, Some(Path::new("out.txt"))));
    }

    #[test]
    fn resolve_color_auto_with_an_output_file_is_always_false() {
        // `output.is_some()` short-circuits the `&&` chain before either
        // `NO_COLOR` or `is_terminal()` is even read — deterministic
        // regardless of the real test-process environment.
        assert!(!resolve_color(
            ColorArg::Auto,
            Some(Path::new("report.sarif"))
        ));
    }

    #[test]
    fn default_format_with_an_output_file_is_always_sarif() {
        assert_eq!(
            default_format(Some(Path::new("report.sarif"))),
            sting_pipeline::ReportFormat::Sarif
        );
    }

    #[test]
    fn find_upward_finds_the_file_in_the_starting_directory() {
        let dir = tempfile::tempdir().expect("scratch dir");
        std::fs::write(dir.path().join("Sting.toml"), "").expect("writable");
        assert_eq!(
            find_upward(dir.path(), "Sting.toml"),
            Some(dir.path().join("Sting.toml"))
        );
    }

    #[test]
    fn find_upward_finds_the_file_in_an_ancestor_directory() {
        let dir = tempfile::tempdir().expect("scratch dir");
        std::fs::write(dir.path().join("Sting.toml"), "").expect("writable");
        let nested = dir.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).expect("writable");
        assert_eq!(
            find_upward(&nested, "Sting.toml"),
            Some(dir.path().join("Sting.toml"))
        );
    }

    #[test]
    fn find_upward_returns_none_when_no_ancestor_has_the_file() {
        let dir = tempfile::tempdir().expect("scratch dir");
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).expect("writable");
        // A file that provably doesn't exist anywhere under a fresh scratch
        // tree — no ancestor of `nested` (up through `dir`) has it, and
        // `find_upward` stops climbing once `.pop()` fails, so this can't
        // accidentally find a real `Sting.toml` living above the temp dir.
        assert_eq!(
            find_upward(&nested, "Sting-file-that-does-not-exist.toml"),
            None
        );
    }

    fn fixable_finding(detector_id: &'static str, has_suggestion: bool) -> sting_core::Finding {
        sting_core::Finding {
            detector_id,
            severity: sting_core::Severity::Medium,
            message: "test finding".to_string(),
            remediation: "test remediation".to_string(),
            location: sting_core::Location {
                file: PathBuf::from("contract.tolk"),
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 1,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: has_suggestion.then(|| sting_core::Suggestion {
                start_byte: 0,
                end_byte: 1,
                replacement: "x".to_string(),
            }),
        }
    }

    #[test]
    fn fixable_counts_by_detector_only_counts_findings_with_a_suggestion() {
        let findings = vec![
            fixable_finding("DEPR", true),
            fixable_finding("DEPR", true),
            fixable_finding("SUA", true),
            fixable_finding("AC", false),
        ];
        assert_eq!(
            fixable_counts_by_detector(&findings),
            vec![("DEPR", 2), ("SUA", 1)],
            "AC has no suggestion, so it must not appear at all; DEPR/SUA sorted by id"
        );
    }

    #[test]
    fn fixable_counts_by_detector_is_empty_when_nothing_is_fixable() {
        let findings = vec![fixable_finding("AC", false)];
        assert!(fixable_counts_by_detector(&findings).is_empty());
    }

    #[test]
    fn detector_stats_from_baseline_computes_precision_and_omits_unmeasured_detectors() {
        let mut detectors = std::collections::BTreeMap::new();
        detectors.insert(
            "GVR".to_string(),
            sting_bench::DetectorSnapshot {
                true_positives: 3,
                false_positives: 1,
                false_negatives: 0,
            },
        );
        // Never measured (0/0) — must be omitted, not inserted as 0.0/NaN
        // (the function's own doc comment explains why).
        detectors.insert(
            "UNMEASURED".to_string(),
            sting_bench::DetectorSnapshot::default(),
        );
        let baseline = sting_bench::Baseline {
            schema_version: sting_bench::BASELINE_SCHEMA_VERSION.to_string(),
            detectors,
        };

        let snapshot = detector_stats_from_baseline(&baseline);
        assert_eq!(snapshot.precision("GVR"), Some(0.75));
        assert_eq!(snapshot.precision("UNMEASURED"), None);
        assert_eq!(snapshot.precision("NEVER_SEEN_AT_ALL"), None);
    }
}
