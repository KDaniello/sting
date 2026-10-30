//! The Tolk version-compatibility matrix.
//!
//! `README` states which versions of Tolk the binary understands. A statement
//! without a check has already cost this project five disagreements between
//! documentation and code, so the matrix is verified here rather than
//! declared (`docs/ARCHITECTURE.md` §7).
//!
//! The test is **symmetric**: it fails when support breaks and equally when
//! support suddenly appears. The second is not a celebration but a signal that
//! `tree-sitter-tolk` has moved and the matrix needs changing; what it cannot
//! do is drift away from reality quietly.
//!
//! Every fragment below is a real construct from the corpus
//! (`corpus/clean/acton-contracts/`) cut to its minimum, never an invented
//! one, because that is the corpus the measurement was taken on.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::path::{Path, PathBuf};

/// Parses, and returns how many `ERROR` and `MISSING` nodes came back.
fn error_count(source: &str) -> usize {
    let tree = sting_ast::parse(source).expect("the Tolk grammar loads");
    sting_ast::find_error_nodes(&tree).len()
}

// Tolk 1.2 and earlier.

#[test]
fn tolk_1_2_and_earlier_constructs_parse_without_error() {
    // Structs with an opcode, union types, enums, get methods, methods on a
    // type, `match`, `lazy`, lambdas — everything grammar 0.0.5 claims.
    let source = r#"
tolk 1.0

import "types"

global ctxOwner: address;

const MIN_VALUE = 100;

struct (0x12345678) Increase {
    queryId: uint64
    by: int32
}

struct Storage {
    owner: address
    counter: int32
}

type AllowedMessage = Increase | Reset

enum Color {
    Red,
    Green,
}

fun Storage.load(): Storage {
    return Storage.fromCell(contract.getData());
}

fun onInternalMessage(in: InMessage) {
    val msg = lazy AllowedMessage.fromSlice(in.body);
    match (msg) {
        Increase => {
            var storage = lazy Storage.load();
            storage.counter += msg.by;
            storage.save();
        }
        else => {
            throw 0xFFFF;
        }
    }
}

get fun currentCounter(): int32 {
    return Storage.load().counter;
}
"#;
    assert_eq!(
        error_count(source),
        0,
        "Tolk 1.2 constructs must parse without an ERROR — that is what grammar 0.0.5 claims"
    );
}

// Tolk 1.3.

#[test]
fn tolk_1_3_contract_block_parses_and_binds_its_storage() {
    // The `contract {}` manifest block is Tolk 1.3 and later. Grammar 0.0.5 had
    // no rule for it and the whole block arrived as one opaque `ERROR` node;
    // the vendored grammar has the rule, so it arrives as a node a detector can
    // read a field from.
    let source = r#"
contract Counter {
    author: "The Tolk Team"
    version: "1.0.0"
    storage: Storage
    incomingMessages: AllowedMessage
}

fun onInternalMessage(in: InMessage) {
    return;
}
"#;
    assert_eq!(
        error_count(source),
        0,
        "the manifest block must parse; the README matrix says it does"
    );

    let tree = sting_ast::parse(source).expect("the Tolk grammar loads");
    let store = sting_ast::AstStore::build(&tree, source);
    let contracts: Vec<_> = store.contracts().collect();
    assert_eq!(contracts.len(), 1, "one block, one contract: {contracts:?}");
    assert_eq!(contracts[0].name, "Counter");
    assert!(
        contracts[0]
            .fields
            .contains(&("storage".to_string(), "Storage".to_string())),
        "the storage binding is what a detector asks this block for, got {:?}",
        contracts[0].fields
    );
    assert!(
        store.function_by_name("onInternalMessage").is_some(),
        "declarations after the block are still there"
    );
}

#[test]
fn two_adjacent_contract_blocks_stay_two_contracts() {
    // The old text scan folded two blocks with nothing between them into one
    // `ERROR` node and recovered a single header from it, losing the second
    // contract entirely — and with it the per-contract attribution three
    // detectors rest on. A real node cannot fold.
    let source = r"
contract First {
    storage: FirstStorage
}

contract Second {
    storage: SecondStorage
}
";
    assert_eq!(error_count(source), 0, "both blocks must parse");
    let tree = sting_ast::parse(source).expect("the Tolk grammar loads");
    let store = sting_ast::AstStore::build(&tree, source);
    let names: Vec<&str> = store
        .contracts()
        .map(|contract| contract.name.as_str())
        .collect();
    assert_eq!(names, vec!["First", "Second"]);
}

// Tolk 1.4.

#[test]
fn tolk_1_4_dotted_abi_annotation_parses_on_a_struct_field() {
    // `@abi.clientType(...)` is Tolk 1.4, and it needed two things grammar
    // 0.0.5 lacked: a dotted annotation name, and an annotation on a struct
    // field at all rather than only on a declaration.
    let dotted = r"
struct (0x52674370) NewComplaint {
    queryId: uint64
    @abi.clientType(Complaint)
    rawComplaint: RemainingBitsAndRefs
}
";
    assert_eq!(
        error_count(dotted),
        0,
        "a dotted annotation on a struct field must parse"
    );

    // Its argument may be a type rather than an expression, which no
    // expression rule reaches.
    let typed_argument = r"
struct (0x7369676E) ExternalSignedRequest {
    @abi.clientType(Cell<OutActionWithSendMessageOnly>?)
    outActions: OutActionsCell?
}
";
    assert_eq!(
        error_count(typed_argument),
        0,
        "an annotation argument may be a type"
    );

    // And a plain annotation still works, or something was traded for this.
    let plain = r"
@inline
fun helper(x: int): int {
    return x + 1;
}
";
    assert_eq!(error_count(plain), 0, "a plain annotation still parses");
}

#[test]
fn an_enum_member_may_carry_a_value_and_a_semicolon() {
    // Real production code separates enum members with `;` and gives the first
    // one an explicit value; grammar 0.0.5 allowed only `,` and no value.
    let source = r"
enum Ownable2Step_Error {
    OnlyCallableByOwner = 100;
    CannotTransferToSelf,
    MustBeProposedOwner,
}
";
    assert_eq!(
        error_count(source),
        0,
        "an enum with `;` separators must parse"
    );
}

// Syntax the language has removed.

#[test]
fn removed_c_string_suffix_is_rejected_exactly_like_the_compiler_rejects_it() {
    // `"..."c` left Tolk in 0.10, replaced by `stringCrc32("...")`. Compiler
    // 1.4.1 answers `error: expected top-level declaration, got \`c\``. The
    // grammar gives an ERROR too, and that is **not** a gap but agreement with
    // the compiler: the measurement first counted it as a third gap, wrongly.
    let removed = r#"const OP_UNLOCK = "op::unlock"c;"#;
    assert!(
        error_count(removed) > 0,
        "syntax removed from the language must not parse — the grammar agrees with the compiler here"
    );

    let modern = r#"const OP_UNLOCK = stringCrc32("op::unlock");"#;
    assert_eq!(error_count(modern), 0, "the modern replacement must parse");
}

// The aggregate of the measurement.

/// The gate's threshold. The value measured on 2026-08-05 was 98.2%.
const MIN_TOP_LEVEL_COVERAGE_PERCENT: f64 = 95.0;

/// The gate's other threshold: the share of corpus files carrying no `ERROR` or
/// `MISSING` node at all. Every one of them, now that the grammar carries what
/// the live language has.
///
/// Two numbers because they answer different questions. The construct share
/// says how much of the language the grammar reads; the file share says how
/// many files a reader can trust a clean report on. One `ERROR` in a rare place
/// and an `ERROR` in every file are indistinguishable by the first and obvious
/// by the second.
const MIN_CLEAN_FILE_PERCENT: f64 = 100.0;

/// Every corpus file, and how many parse whole.
fn corpus_parse_counts() -> (usize, usize) {
    let corpus = repo_root().join("corpus");
    let mut files = Vec::new();
    collect_tolk(&corpus, &mut files);
    assert!(
        files.len() > 100,
        "the corpus must be in place; files found: {}",
        files.len()
    );

    let clean = files
        .iter()
        .filter(|path| {
            let Ok(source) = std::fs::read_to_string(path) else {
                return false;
            };
            let tree = sting_ast::parse(&source).expect("the Tolk grammar loads");
            sting_ast::first_unparsed(&tree).is_none()
        })
        .count();
    (clean, files.len())
}

/// The share of corpus files the grammar reads whole, checked against a floor
/// that only ever rises.
///
/// This is the number the blindness is measured by, and it is a test rather
/// than a report so that a decline fails a run instead of appearing in one.
#[test]
fn real_corpus_clean_file_share_stays_above_the_documented_floor() {
    let (clean, total) = corpus_parse_counts();

    #[allow(
        clippy::cast_precision_loss,
        reason = "the corpus counters are far below 2^53"
    )]
    let share = clean as f64 * 100.0 / total as f64;
    println!("corpus files parsing whole: {clean} of {total} ({share:.1}%)");
    assert!(
        share >= MIN_CLEAN_FILE_PERCENT,
        "the share of corpus files parsing whole fell to {share:.1}% \
         (floor {MIN_CLEAN_FILE_PERCENT}%): {clean} of {total} files"
    );
}

#[test]
fn real_corpus_top_level_construct_coverage_stays_above_the_documented_floor() {
    let corpus = repo_root().join("corpus");
    let mut files = Vec::new();
    collect_tolk(&corpus, &mut files);
    assert!(
        files.len() > 100,
        "the corpus must be in place; files found: {}",
        files.len()
    );

    let (mut ok, mut bad) = (0_usize, 0_usize);
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let tree = sting_ast::parse(&source).expect("the Tolk grammar loads");
        let mut cursor = tree.root_node().walk();
        for child in tree.root_node().named_children(&mut cursor) {
            if child.is_error() || child.is_missing() {
                bad += 1;
            } else if child.kind() != "comment" {
                ok += 1;
            }
        }
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "the corpus counters are far below 2^53"
    )]
    let coverage = ok as f64 * 100.0 / (ok + bad) as f64;
    assert!(
        coverage >= MIN_TOP_LEVEL_COVERAGE_PERCENT,
        "the share of parsed top-level constructs fell to {coverage:.1}% \
         (threshold {MIN_TOP_LEVEL_COVERAGE_PERCENT}%, measured at 98.2%): {ok} parsed, {bad} not"
    );
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn collect_tolk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_tolk(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "tolk") {
            out.push(path);
        }
    }
}
