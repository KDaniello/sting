//! The gate: on a **real contract from the corpus** rather than on a fixture,
//! the layer answers all three questions, and the answers are checked against
//! the source by hand.
//!
//! The contract is `corpus/clean/acton-contracts/counter/`, taken from
//! `ton-blockchain/acton-contracts` and compiled by the real `tolk` (see
//! `corpus/clean/acton-contracts/SOURCE.md`). It was chosen not for its size
//! but because it holds every interesting form at once: a path parameter
//! (`in.body`), a union narrowed in a `match`, a method on a struct, and a
//! `Storage.load` the compiler inlined.
//!
//! It needs the real compiler: `vendor/tolk/` is gitignored, so the test skips
//! when it is absent — the same discipline every real-toolchain test in this
//! workspace follows.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_semantic::{ResolvedCall, ResolvedType, SemanticModel, SourceUnit, TypeRef};
use sting_tolkc::{SymbolTypes, TolkCompiler};
use tree_sitter::{Node, Tree};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Compiles the contract with the real `tolk` and returns its symbol table.
fn compile_counter() -> Option<SymbolTypes> {
    let exe = repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !exe.is_file() {
        println!(
            "SKIP: {} is not on disk (vendor/tolk/ is gitignored)",
            exe.display()
        );
        return None;
    }
    let project = repo_root().join("corpus/clean/acton-contracts/counter");
    let entry = project.join("Counter.tolk");
    match TolkCompiler::new(exe).compile(&project, &entry) {
        Ok(output) => Some(output.symbol_types),
        Err(err) => {
            println!("SKIP: the compile failed: {err}");
            None
        }
    }
}

fn parse(path: &Path) -> (String, Tree) {
    let source = std::fs::read_to_string(path).expect("the corpus file reads");
    let tree = sting_ast::parse(&source).expect("the Tolk grammar loads");
    (source, tree)
}

/// The node of kind `kind` with text `text` on line `line`, 1-based.
///
/// The line is mandatory rather than convenient: in `Counter.tolk` the
/// identifier `storage` occurs twice — in the `contract {}` manifest block
/// grammar 0.0.5 does not parse, and in a function body. The gate claims a
/// manual check against the source, so it has to address nodes by their place
/// in the source.
fn at_line<'t>(tree: &'t Tree, source: &str, kind: &str, text: &str, line: usize) -> Node<'t> {
    sting_ast::Preorder::new(tree)
        .map(|v| v.node)
        .find(|n| {
            n.kind() == kind
                && n.start_position().row + 1 == line
                && source.get(n.start_byte()..n.end_byte()) == Some(text)
        })
        .unwrap_or_else(|| panic!("line {line} has no {kind} node with text {text:?}"))
}

/// The shared setup: a real `tolk` compile and both project files parsed.
/// Returns `None` when the compiler is not on disk.
struct Fixture {
    symbols: SymbolTypes,
    counter_path: PathBuf,
    types_path: PathBuf,
    counter_src: String,
    counter_tree: Tree,
    types_src: String,
    types_tree: Tree,
}

impl Fixture {
    fn units(&self) -> Vec<SourceUnit<'_>> {
        vec![
            SourceUnit {
                path: self.counter_path.clone(),
                tree: &self.counter_tree,
                source: &self.counter_src,
            },
            SourceUnit {
                path: self.types_path.clone(),
                tree: &self.types_tree,
                source: &self.types_src,
            },
        ]
    }
}

fn fixture() -> Option<Fixture> {
    let symbols = compile_counter()?;
    let counter_path = repo_root().join("corpus/clean/acton-contracts/counter/Counter.tolk");
    let types_path = repo_root().join("corpus/clean/acton-contracts/counter/types.tolk");
    let (counter_src, counter_tree) = parse(&counter_path);
    let (types_src, types_tree) = parse(&types_path);
    Some(Fixture {
        symbols,
        counter_path,
        types_path,
        counter_src,
        counter_tree,
        types_src,
        types_tree,
    })
}

/// The first question: what type does this variable have at this point.
#[test]
fn question_1_types_of_real_expressions() {
    let Some(f) = fixture() else { return };
    let units = f.units();
    let model = SemanticModel::build(&f.symbols, &units);
    let file = model
        .file(&f.counter_path)
        .expect("Counter.tolk is in the compiler's artefact");
    let (counter_tree, counter_src) = (&f.counter_tree, &f.counter_src);

    //
    // The source (Counter.tolk):
    //     fun onInternalMessage(in: InMessage) {
    //         val msg = lazy AllowedMessage.fromSlice(in.body);
    //
    // `in.body`: the compiler lowers the parameter `in: InMessage` into an
    // access path and reports its type directly. Checked against
    // symbolTypes.json:
    // params = [{"name": "in.body", "ty_idx": 2}], ty_idx 2 = slice.
    let in_body = at_line(counter_tree, counter_src, "dot_access", "in.body", 14);
    assert_eq!(
        file.type_of(in_body).known().map(TypeRef::display_name),
        Some("slice"),
        "the type of a path parameter must come straight from the compiler"
    );

    // The source:
    //         var storage = lazy Storage.load();
    //         storage.counter += msg.increaseBy;
    //
    // `storage`: the type comes from the result of `Storage.load()`, which the
    // compiler inlined and left out of the table, so the answer is read from
    // the return annotation in types.tolk (`fun Storage.load(): Storage`).
    let storage = at_line(counter_tree, counter_src, "identifier", "storage", 19);
    assert_eq!(
        file.type_of(storage).known().map(TypeRef::display_name),
        Some("Storage"),
        "a local's type must be inferred from the call's result"
    );

    // `storage.counter` is a struct field, whose type the compiler knows
    // exactly:
    // declarations[Storage].fields = [id: uint32, counter: uint32].
    let counter_field = at_line(
        counter_tree,
        counter_src,
        "dot_access",
        "storage.counter",
        19,
    );
    assert_eq!(
        file.type_of(counter_field)
            .known()
            .map(TypeRef::display_name),
        Some("uint32"),
        "a struct field's type must come from the compiler"
    );

    // The source matches on `msg` and reads `msg.increaseBy` inside the
    // `IncreaseCounter` arm. Inside that arm `msg` is narrowed to
    // `IncreaseCounter` — a rule the compiler's artefact does not carry at all.
    let increase_by = at_line(
        counter_tree,
        counter_src,
        "dot_access",
        "msg.increaseBy",
        19,
    );
    assert_eq!(
        file.type_of(increase_by).known().map(TypeRef::display_name),
        Some("uint32"),
        "narrowing in a match arm must work: without it the field is unreachable"
    );
}

/// The second question: which function does this call call.
#[test]
fn question_2_call_targets_resolve_by_receiver_type() {
    let Some(f) = fixture() else { return };
    let units = f.units();
    let model = SemanticModel::build(&f.symbols, &units);
    let file = model
        .file(&f.counter_path)
        .expect("Counter.tolk is in the compiler's artefact");
    let (counter_tree, counter_src) = (&f.counter_tree, &f.counter_src);

    // The source: `storage.save();`, a method call on a variable. It resolves
    // by the receiver's type (Storage) rather than by the bare name `save`
    // matching.
    let save_call = at_line(
        counter_tree,
        counter_src,
        "function_call",
        "storage.save()",
        20,
    );
    let resolved = file.resolve_call(save_call);
    assert_eq!(
        resolved.known().map(|f| f.name.as_str()),
        Some("Storage.save"),
        "a method must resolve by the receiver's type: {resolved:?}"
    );
    assert!(
        !resolved.known().expect("checked above").from_ast,
        "Storage.save is in the compiler's table and must not come from the AST"
    );

    // The source: `lazy Storage.load()`, a static call on a type's name. The
    // compiler inlined it, so the answer is honestly marked `from_ast`.
    let load_call = at_line(
        counter_tree,
        counter_src,
        "function_call",
        "Storage.load()",
        18,
    );
    let resolved_load = file.resolve_call(load_call);
    assert_eq!(
        resolved_load.known().map(|f| f.name.as_str()),
        Some("Storage.load"),
        "a static call on a type must resolve: {resolved_load:?}"
    );
    assert!(
        resolved_load.known().expect("checked above").from_ast,
        "Storage.load is absent from the compiler's table, and the answer must admit it"
    );
}

/// The third question: what is reachable from this entry point, transitively.
#[test]
fn question_3_reachability_crosses_files() {
    let Some(f) = fixture() else { return };
    let units = f.units();
    let model = SemanticModel::build(&f.symbols, &units);

    // What is reachable from the entry point, transitively and across files:
    // `onInternalMessage`'s body is in Counter.tolk, while `Storage.save` and
    // `Storage.load` are declared in types.tolk.
    let reachable = model.reachable_from("onInternalMessage");
    assert!(
        reachable.contains("Storage.load"),
        "the call graph must cross files: {reachable:?}"
    );
    assert!(
        reachable.contains("Storage.save"),
        "the call graph must cross files: {reachable:?}"
    );
}

/// "I do not know" is a legitimate answer and has to be distinguishable from
/// "there is a type".
///
/// Uncertainty is covered by tests as thoroughly as success: a layer that
/// quietly returns `Unknown` where it should know is outwardly indistinguishable
/// from one that knows.
#[test]
fn unknown_and_ambiguous_are_expressed_as_types_not_as_silence() {
    let Some(symbols) = compile_counter() else {
        return;
    };
    let counter_path = repo_root().join("corpus/clean/acton-contracts/counter/Counter.tolk");
    let (counter_src, counter_tree) = parse(&counter_path);
    let units = vec![SourceUnit {
        path: counter_path.clone(),
        tree: &counter_tree,
        source: &counter_src,
    }];
    let model = SemanticModel::build(&symbols, &units);
    let file = model
        .file(&counter_path)
        .expect("the file is known to the compiler");

    // Grammar 0.0.5 does not parse `contract Counter { ... }`, so the node
    // lies in an ERROR span outside any function. The layer has to name that
    // reason rather than return a nameless refusal.
    let outside = at_line(&counter_tree, &counter_src, "identifier", "author", 4);
    assert_eq!(
        file.type_of(outside),
        ResolvedType::Unknown(sting_semantic::UnknownReason::OutsideAnyFunction),
        "a node outside a function must get a named reason"
    );

    // This run is deliberately built **without** types.tolk: the compiler
    // inlined `Storage.load`, and the source it could be recovered from was not
    // given to the model. The answer has to be an honest refusal naming the
    // function that was looked for.
    let load_call = at_line(
        &counter_tree,
        &counter_src,
        "function_call",
        "Storage.load()",
        18,
    );
    match file.resolve_call(load_call) {
        ResolvedCall::Unresolved(sting_semantic::UnresolvedCall::NoSuchFunction { looked_for }) => {
            assert_eq!(looked_for, "Storage.load");
        }
        other => panic!("expected an honest refusal naming the function, got {other:?}"),
    }
}

/// `in.senderAddress`: the expression `AC` rests on, and the finding that
/// mattered most.
///
/// The plan claimed the compiler lowers parameters into access paths and
/// therefore reports the type of `in.senderAddress` directly. **That is wrong**:
/// the parameter table holds only `in.body`, the one path that really arrives on
/// the stack, even in contracts that use `in.senderAddress` heavily. The claim
/// was generalised from one example and never checked.
///
/// The type comes from elsewhere: `struct InMessage` is declared in
/// `tolk-stdlib/common.tolk`, which the compiler compiles (`file_id` 0) and does
/// not surface in the artefact's `declarations`. So the fields are read from the
/// standard library's **source**, and for that its files have to be handed to
/// the model alongside the project's own.
#[test]
fn sender_address_resolves_only_when_stdlib_sources_are_given() {
    let exe = repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !exe.is_file() {
        println!("SKIP: no vendor/tolk");
        return;
    }
    let project = repo_root().join("corpus/clean/tolk-bench/01_jetton");
    let entry = project.join("JettonWallet.tolk");
    let Ok(output) = TolkCompiler::new(exe).compile(&project, &entry) else {
        println!("SKIP: the contract does not compile");
        return;
    };
    let symbols = output.symbol_types;

    // The compiler really does know only `in.body` — this is the check the plan
    // was written without.
    let entrypoint = symbols
        .functions
        .iter()
        .find(|f| f.name == "onInternalMessage")
        .expect("the entry point is in the table");
    let param_names: Vec<&str> = entrypoint.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        param_names,
        vec!["in.body"],
        "the compiler lowers body alone; senderAddress is absent from the parameters"
    );

    let (src, tree) = parse(&entry);
    let stdlib: Vec<PathBuf> = std::fs::read_dir(repo_root().join("vendor/tolk/tolk-stdlib"))
        .expect("the standard library is in place")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "tolk"))
        .collect();
    let parsed_stdlib: Vec<(PathBuf, String, Tree)> = stdlib
        .into_iter()
        .filter_map(|path| {
            let source = std::fs::read_to_string(&path).ok()?;
            let tree = sting_ast::parse(&source).ok()?;
            Some((path, source, tree))
        })
        .collect();

    // The `in.senderAddress` node from a real authorization condition.
    let sender = sting_ast::Preorder::new(&tree)
        .map(|v| v.node)
        .find(|n| {
            n.kind() == "dot_access"
                && src.get(n.start_byte()..n.end_byte()) == Some("in.senderAddress")
        })
        .expect("the contract checks the sender");

    // Without the standard library, an honest "I do not know".
    let without = vec![SourceUnit {
        path: entry.clone(),
        tree: &tree,
        source: &src,
    }];
    let model_without = SemanticModel::build(&symbols, &without);
    assert!(
        model_without
            .file(&entry)
            .expect("the file is known")
            .type_of(sender)
            .known()
            .is_none(),
        "without the standard library's sources there is nowhere to get InMessage from, and the layer must admit it"
    );

    // With it, a real type rather than a substring match.
    let mut with = without;
    with.extend(parsed_stdlib.iter().map(|(path, source, tree)| SourceUnit {
        path: path.clone(),
        tree,
        source,
    }));
    let model_with = SemanticModel::build(&symbols, &with);
    assert_eq!(
        model_with
            .file(&entry)
            .expect("the file is known")
            .type_of(sender)
            .known()
            .map(TypeRef::display_name),
        Some("address"),
        "with the standard library's sources the field's type must be read from InMessage's declaration"
    );
}
