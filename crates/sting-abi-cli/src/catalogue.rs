//! What this dataset knows that the foundation's catalogue does not, and the other way round.
//!
//! Two artefacts answer two different questions about the same chain. Theirs answers "these are the
//! contracts worth describing", written by hand, one project at a time. Ours answers "this is what
//! is actually deployed", walked by machine and recorded with where each name was seen. Neither
//! contains the other, and a claim that one supersedes the other is a claim nobody has measured.
//!
//! So it is measured, in both directions, and both numbers are printed — including the one that
//! does not flatter this repository. A comparison that only reported what we have and they do not
//! would be an advertisement.
//!
//! What this does not do: it does not compare opcodes against our own knowledge of them. Their
//! catalogue declares message opcodes per contract; this dataset is about get-method ids, and the
//! opcodes this tree knows are the standard ones, declared in `sting-tep` rather than learned into
//! a dataset. The opcode count is reported as theirs alone, which is what it is.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::Context as _;

use crate::share::percent;
use sting_abi_core::Extractor as _;
use sting_abi_dictionary::{CorpusSource, MethodDictionary, OperationDictionary};
use sting_abi_tolk::TolkExtractor;
use sting_tolkc::TolkCompiler;

/// The shape of the recorded catalogue lists.
///
/// Required rather than defaulted: the only files of this shape are the ones this command wrote,
/// and a file that does not say which shape it is in is a file this command did not write.
pub const CATALOGUE_SCHEMA_VERSION: &str = "1.0";

/// The shape of the comparison this command prints.
///
/// Separate from the catalogue's version because they change for different reasons: one moves when
/// their lists are recorded differently, the other when the comparison counts something new.
pub const DIFF_SCHEMA_VERSION: &str = "1.0";

/// The catalogue as this repository reads it: two lists and the commit they were read at.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FoundationCatalogue {
    /// Which version of this file's shape it is in.
    pub schema_version: String,
    /// Which catalogue this is.
    pub catalogue: String,
    /// The commit the lists were read at.
    pub commit: String,
    /// When that commit was made.
    pub committed_at: String,
    /// When it was read.
    pub read_at: String,
    /// How many files were read to produce the lists.
    pub files_read: usize,
    /// Every get-method name it declares.
    pub get_methods: Vec<String>,
    /// Every message opcode it declares.
    pub opcodes: Vec<String>,
}

/// What the comparison found, in both directions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CatalogueDiff {
    /// Which version of this comparison's shape it is in.
    pub schema_version: String,
    /// The commit the catalogue was read at.
    pub catalogue_commit: String,
    /// Distinct get-method names in this dataset.
    pub ours: usize,
    /// Distinct get-method names in the catalogue.
    pub theirs: usize,
    /// Names both carry.
    pub in_both: usize,
    /// Names only this dataset carries.
    pub ours_only: usize,
    /// Names only the catalogue carries — the unflattering number.
    pub theirs_only: usize,
    /// What share of the catalogue's names this dataset carries, in per cent to one decimal.
    pub catalogue_covered_pct: f64,
    /// What share of this dataset's names the catalogue carries, in per cent to one decimal.
    pub ours_covered_pct: f64,
    /// Message opcodes the catalogue declares. Reported, not compared: see the module note.
    pub catalogue_opcodes: usize,
}

/// Reads the recorded catalogue.
///
/// # Errors
/// When the file is absent or is not the shape `catalogue-fetch` writes.
pub fn read_catalogue(path: &Path) -> anyhow::Result<FoundationCatalogue> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the catalogue at {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not a catalogue this command wrote", path.display()))
}

/// Compares the dataset against the catalogue, both ways.
#[must_use]
pub fn compare(dictionary: &MethodDictionary, catalogue: &FoundationCatalogue) -> CatalogueDiff {
    let ours: BTreeSet<&str> = dictionary
        .iter()
        .flat_map(|(_, group)| group.iter().map(|entry| entry.name.as_str()))
        .collect();
    let theirs: BTreeSet<&str> = catalogue.get_methods.iter().map(String::as_str).collect();

    let in_both = ours.intersection(&theirs).count();
    CatalogueDiff {
        schema_version: DIFF_SCHEMA_VERSION.to_string(),
        catalogue_commit: catalogue.commit.clone(),
        ours: ours.len(),
        theirs: theirs.len(),
        in_both,
        ours_only: ours.difference(&theirs).count(),
        theirs_only: theirs.difference(&ours).count(),
        catalogue_covered_pct: percent(in_both, theirs.len()),
        ours_covered_pct: percent(in_both, ours.len()),
        catalogue_opcodes: catalogue.opcodes.len(),
    }
}

/// Prints the comparison, both directions, as text and as JSON.
///
/// # Errors
/// When either artefact cannot be read.
pub fn run(dictionary_path: &Path, catalogue_path: &Path) -> anyhow::Result<()> {
    let dictionary = MethodDictionary::load(dictionary_path)
        .with_context(|| format!("loading {}", dictionary_path.display()))?;
    let catalogue = read_catalogue(catalogue_path)?;
    let diff = compare(&dictionary, &catalogue);

    println!(
        "against {} at {}:",
        catalogue.catalogue, diff.catalogue_commit
    );
    println!(
        "  this dataset: {} get-method names; the catalogue: {}",
        diff.ours, diff.theirs
    );
    println!(
        "  in both: {} — {:.1}% of the catalogue, {:.1}% of this dataset",
        diff.in_both, diff.catalogue_covered_pct, diff.ours_covered_pct
    );
    println!("  only here: {}", diff.ours_only);
    println!("  only there: {}", diff.theirs_only);
    println!(
        "  the catalogue also declares {} message opcodes, which this dataset is not about",
        diff.catalogue_opcodes
    );
    println!("{}", serde_json::to_string_pretty(&diff)?);
    Ok(())
}

/// What a declaration in their sources looks like, on the line it is declared on.
///
/// Their catalogue is written in Tolk type files, and both facts this comparison needs are stated
/// syntactically: a get-method by `get fun name(`, a message opcode by the prefix in front of the
/// structure it belongs to. Reading the two out of the text rather than building their repository
/// keeps this command independent of their toolchain, which is theirs to change and not ours.
const GET_METHOD_PREFIX: &str = "get fun ";
/// The opening of an opcode-carrying structure declaration.
const OPCODE_PREFIX: &str = "struct (";
/// Which of their files declare contracts rather than exercise them.
///
/// The repository carries 1 034 Tolk files, and 457 of them are tests whose every case is written
/// as a get-method. Counting those would credit their catalogue with names no contract on chain
/// answers to, and inflating the side this repository is compared against is as dishonest as
/// deflating it — so only the type files are read.
const TYPE_FILE_SUFFIX: &str = ".types.tolk";

/// Rebuilds the recorded catalogue lists from a checkout of their repository.
///
/// The commit is read from the checkout itself rather than taken on trust, because a comparison is
/// against a state of their repository and a state nobody can name is a state nobody can check.
/// The date the lists were read is stated by the caller and not by the clock: this command has to
/// reproduce the same file from the same checkout months later, and a wall-clock field would make
/// every rebuild differ from the file it was meant to reproduce.
///
/// # Errors
/// When the checkout is absent, when its commit cannot be read, or when the output cannot be
/// written.
pub fn fetch(checkout: &Path, read_at: &str, output: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        checkout.is_dir(),
        "{} is not a checkout of ton-blockchain/abis",
        checkout.display()
    );
    let commit = git(checkout, &["rev-parse", "HEAD"])?;
    // In UTC rather than in whatever zone the machine sits in: the same checkout read in two
    // places has to produce the same file, and a local offset would make the two differ.
    let committed_at = git(
        checkout,
        &[
            "show",
            "-s",
            "--format=%cd",
            "--date=format-local:%Y-%m-%dT%H:%M:%SZ",
            "HEAD",
        ],
    )?;

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect_tolk(checkout, &mut files)?;
    files.sort();

    let mut get_methods = BTreeSet::new();
    let mut opcodes = BTreeSet::new();
    for file in &files {
        let text =
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        declarations_in(&text, &mut get_methods, &mut opcodes);
    }

    let catalogue = FoundationCatalogue {
        schema_version: CATALOGUE_SCHEMA_VERSION.to_string(),
        catalogue: "ton-blockchain/abis".to_string(),
        commit,
        committed_at,
        read_at: read_at.to_string(),
        files_read: files.len(),
        get_methods: get_methods.into_iter().collect(),
        opcodes: opcodes.into_iter().collect(),
    };
    let text = serde_json::to_string_pretty(&catalogue)?;
    std::fs::write(output, text)
        .with_context(|| format!("writing the catalogue to {}", output.display()))?;
    println!(
        "read {} type file(s) at {}: {} get-method name(s), {} opcode(s) -> {}",
        catalogue.files_read,
        catalogue.commit,
        catalogue.get_methods.len(),
        catalogue.opcodes.len(),
        output.display()
    );
    Ok(())
}

/// Reads one type file's declarations into the two lists.
///
/// Separated from the walk so that the rule can be stated on a string and checked: what counts as a
/// declaration is the only judgement this command makes about somebody else's sources, and a
/// judgement buried in a directory walk is a judgement nobody tests.
fn declarations_in(text: &str, get_methods: &mut BTreeSet<String>, opcodes: &mut BTreeSet<String>) {
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix(GET_METHOD_PREFIX)
            && let Some(name) = rest.split('(').next()
        {
            // Tolk writes a name that is not an identifier — namespaced, or with spaces in it —
            // inside backticks. The quotes belong to the language rather than to the method, and a
            // name that kept them would never match the same name read off a chain.
            let name = name.trim().trim_matches('`');
            if !name.is_empty() {
                get_methods.insert(name.to_string());
            }
        }
        if let Some(rest) = line.strip_prefix(OPCODE_PREFIX)
            && let Some(prefix) = rest.split(')').next()
            && prefix.starts_with("0x")
        {
            opcodes.insert(prefix.trim().to_string());
        }
    }
}

/// Every Tolk type file under the checkout, in whatever order the walk returns them.
fn collect_tolk(root: &Path, found: &mut Vec<std::path::PathBuf>) -> anyhow::Result<()> {
    let entries = std::fs::read_dir(root).with_context(|| format!("walking {}", root.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("walking {}", root.display()))?
            .path();
        if path.is_dir() {
            collect_tolk(&path, found)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(TYPE_FILE_SUFFIX))
        {
            found.push(path);
        }
    }
    Ok(())
}

/// Asks the checkout what state it is in, and treats a refusal as an answer rather than a default.
fn git(checkout: &Path, arguments: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .env("TZ", "UTC")
        .arg("-C")
        .arg(checkout)
        .args(arguments)
        .output()
        .with_context(|| {
            format!(
                "running git {} in {}",
                arguments.join(" "),
                checkout.display()
            )
        })?;
    anyhow::ensure!(
        output.status.success(),
        "git {} in {} failed: {}",
        arguments.join(" "),
        checkout.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)
        .context("git answered with something that is not text")?
        .trim()
        .to_string())
}

/// What their type files are missing to be a contract the compiler will describe.
///
/// Their files declare types and a `contract` header and stop there, because describing is all they
/// are for. `tolk` refuses to emit an ABI for a contract with no entrypoint, so one that does
/// nothing is appended before compiling — it declares nothing, changes no type, and leaves what the
/// compiler reports about their messages exactly as they wrote it.
const SYNTHETIC_ENTRYPOINT: &str = "

fun onInternalMessage(in: InMessage) {
}
";

/// The licence their sources carry, and therefore the licence what is learned from them carries.
///
/// `ton-blockchain/abis` is MIT. A dataset learned from it is not this repository's to waive rights
/// in, so it says MIT and points at their notice rather than claiming the CC0 the datasets learned
/// from this tree's own corpora carry.
const CATALOGUE_LICENCE: &str =
    "MIT — see corpus-abi/FOUNDATION-OPERATIONS-NOTICE.md (ton-blockchain/abis, TON Core)";

/// Learns an operation dictionary from the catalogue's own type files.
///
/// Their catalogue is where a large part of what a mainnet opcode means is written down, and it is
/// written in the language this tree already compiles. So it is read the same way every other
/// corpus is — through the real extractor, not by transcribing their tables — and what comes out
/// says in its own header whose sources it came from and under what terms.
///
/// Kept as a dataset of its own rather than merged into the learned one: the two carry different
/// terms, and a file whose licence depends on which entry a reader looks at is a file nobody can
/// use.
///
/// # Errors
/// When the checkout is absent, when its commit cannot be read, or when the output cannot be
/// written. A file that does not compile is reported and skipped: their repository is theirs, and a
/// type file this tree's compiler refuses is a fact about the pair rather than a failure of this
/// command.
pub fn learn_operations(checkout: &Path, output: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        checkout.is_dir(),
        "{} is not a checkout of ton-blockchain/abis",
        checkout.display()
    );
    let commit = git(checkout, &["rev-parse", "HEAD"])?;
    let compiler = TolkCompiler::vendored_or_default().context(
        "no Tolk compiler is reachable: see docs/domains/extractors.md for vendoring it",
    )?;
    let extractor = TolkExtractor::new(compiler);

    let mut files = Vec::new();
    collect_tolk(checkout, &mut files)?;
    files.sort();

    let scratch = tempfile::tempdir().context("making a scratch directory to compile in")?;
    let mut dictionary = OperationDictionary::new()
        .under_licence(CATALOGUE_LICENCE)
        .built_by("abi-layer catalogue-operations");
    let (mut learned, mut skipped) = (0usize, 0usize);
    for file in &files {
        let label = label_of(checkout, file);
        // Compiled in a copy of its own directory rather than alone: their files import their
        // siblings by relative path, and a file lifted out of its neighbours is a file that does
        // not compile for a reason that has nothing to do with what it declares.
        let with_entrypoint = stage_beside_siblings(file, scratch.path())?;
        match extractor.extract(scratch.path(), &with_entrypoint) {
            Ok(schema) => {
                dictionary.learn_from_schema(&schema, &label);
                learned += 1;
            }
            Err(error) => {
                eprintln!("SKIP {label}: {error}");
                skipped += 1;
            }
        }
    }

    dictionary.record_sources(vec![CorpusSource {
        root: format!("ton-blockchain/abis@{commit}"),
        what: "the foundation's own catalogue of described contracts, read through this tree's Tolk extractor".to_string(),
        provenance: "corpus-abi/FOUNDATION-OPERATIONS-NOTICE.md".to_string(),
    }]);
    println!(
        "learned from {learned} of {} type file(s) ({skipped} skipped) — {} distinct opcode(s), {} claimed by more than one operation",
        files.len(),
        dictionary.len(),
        dictionary.collisions().count()
    );
    dictionary
        .save(output)
        .with_context(|| format!("writing {}", output.display()))?;
    println!("Wrote {}", output.display());
    Ok(())
}

/// Copies one type file and its neighbours into `scratch`, appending the entrypoint to the copy.
///
/// Returns the path of the copy to compile. The neighbours come along because their files import
/// each other by relative path; the entrypoint goes on the copy so that nothing is written into
/// somebody else's checkout.
fn stage_beside_siblings(file: &Path, scratch: &Path) -> anyhow::Result<std::path::PathBuf> {
    for existing in std::fs::read_dir(scratch)
        .with_context(|| format!("reading {}", scratch.display()))?
        .flatten()
    {
        if existing.path().is_file() {
            let _ = std::fs::remove_file(existing.path());
        }
    }
    let directory = file
        .parent()
        .with_context(|| format!("{} has no directory", file.display()))?;
    for sibling in std::fs::read_dir(directory)
        .with_context(|| format!("reading {}", directory.display()))?
        .flatten()
    {
        let path = sibling.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        std::fs::copy(&path, scratch.join(name))
            .with_context(|| format!("copying {}", path.display()))?;
    }

    let name = file
        .file_name()
        .with_context(|| format!("{} has no name", file.display()))?;
    let staged = scratch.join(name);
    let source = std::fs::read_to_string(&staged)
        .with_context(|| format!("reading {}", staged.display()))?;
    std::fs::write(&staged, format!("{source}{SYNTHETIC_ENTRYPOINT}"))
        .with_context(|| format!("writing {}", staged.display()))?;
    Ok(staged)
}

/// What to call a file of theirs in provenance: its path inside their repository.
fn label_of(checkout: &Path, file: &Path) -> String {
    file.strip_prefix(checkout)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_abi_core::{
        ContractMetadata, ContractSchema, ExtractionTier, GetMethod, SourceLanguage, TypeRef,
    };
    use sting_abi_dictionary::MethodDictionary;

    use super::{FoundationCatalogue, compare, declarations_in};

    fn dictionary_with(names: &[&str]) -> MethodDictionary {
        let mut dictionary = MethodDictionary::new();
        for (offset, name) in names.iter().enumerate() {
            let schema = ContractSchema {
                ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
                source_language: SourceLanguage::Tolk,
                extraction_tier: ExtractionTier::Compiled,
                contract_name: None,
                metadata: ContractMetadata::default(),
                storage: None,
                types: Vec::new(),
                incoming_internal: Vec::new(),
                incoming_external: Vec::new(),
                outgoing: Vec::new(),
                get_methods: vec![GetMethod {
                    tvm_method_id: Some(100_000 + i64::try_from(offset).unwrap()),
                    name: (*name).to_string(),
                    parameters: Vec::new(),
                    return_ty: Some(TypeRef(0)),
                }],
                errors: Vec::new(),
                confidence: None,
                field_confidence: None,
            };
            dictionary.learn_from_schema(&schema, "contract.tolk");
        }
        dictionary
    }

    fn catalogue_with(names: &[&str]) -> FoundationCatalogue {
        FoundationCatalogue {
            schema_version: super::CATALOGUE_SCHEMA_VERSION.to_string(),
            catalogue: "ton-blockchain/abis".to_string(),
            commit: "0".repeat(40),
            committed_at: "2026-08-10T19:55:12Z".to_string(),
            read_at: "2026-08-23".to_string(),
            files_read: 1,
            get_methods: names.iter().map(|name| (*name).to_string()).collect(),
            opcodes: vec!["0x0f8a7ea5".to_string()],
        }
    }

    #[test]
    fn a_difference_is_counted_in_both_directions() {
        let diff = compare(
            &dictionary_with(&["get_jetton_data", "only_here"]),
            &catalogue_with(&["get_jetton_data", "only_there"]),
        );

        assert_eq!(diff.ours, 2);
        assert_eq!(diff.theirs, 2);
        assert_eq!(diff.in_both, 1);
        assert_eq!(diff.ours_only, 1);
        assert_eq!(diff.theirs_only, 1);
        assert!((diff.catalogue_covered_pct - 50.0).abs() < f64::EPSILON);
        assert!((diff.ours_covered_pct - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn an_empty_catalogue_reports_no_coverage_rather_than_dividing_by_zero() {
        let diff = compare(&dictionary_with(&["get_jetton_data"]), &catalogue_with(&[]));

        assert_eq!(diff.theirs, 0);
        assert_eq!(diff.in_both, 0);
        assert!((diff.catalogue_covered_pct - 0.0).abs() < f64::EPSILON);
        assert_eq!(diff.catalogue_opcodes, 1);
    }

    #[test]
    fn a_declaration_is_read_off_the_line_that_declares_it() {
        let mut names = std::collections::BTreeSet::new();
        let mut opcodes = std::collections::BTreeSet::new();
        declarations_in(
            concat!(
                "struct (0x0f8a7ea5) JettonTransfer {
",
                "/// get fun in a comment is not a declaration
",
                "get fun get_jetton_data(): JettonData {
",
                "get fun `utils::check`(): int {
",
                "struct JettonData {
",
            ),
            &mut names,
            &mut opcodes,
        );

        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            vec!["get_jetton_data", "utils::check"]
        );
        assert_eq!(opcodes.into_iter().collect::<Vec<_>>(), vec!["0x0f8a7ea5"]);
    }
}
