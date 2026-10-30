//! Integration tests against a real, vendored native `tolk` build.
//!
//! `vendor/tolk/` is `.gitignore`d (a large, platform-specific binary plus
//! its stdlib sources, see that path's comment in `.gitignore`) — a fresh
//! checkout without it present is expected and not a failure, mirroring
//! `sting-emulator`'s own `tests/integration.rs` for `vendor/libemulator/`.
//! Every test here calls [`compiler_or_skip`] first and returns early (a
//! trivial pass, Rust's stable test harness has no real "skipped" status)
//! when the binary is absent, so `cargo test --workspace` stays green on a
//! machine that never fetched it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_tolkc::{Declaration, ProjectStore, SourceSpan, TolkCompiler, TypeDescriptor};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn vendored_executable_path() -> PathBuf {
    repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename())
}

/// Builds a compiler handle for the real vendored binary, or prints a
/// message and returns `None` if it is not present locally (see this
/// module's doc comment).
fn compiler_or_skip() -> Option<TolkCompiler> {
    let path = vendored_executable_path();
    if !path.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tolk/ in .gitignore) — \
             fetch it from ton-blockchain/ton's tolk-X.Y.Z release to run this test for real",
            path.display()
        );
        return None;
    }
    Some(TolkCompiler::new(path))
}

fn config_project_dir() -> PathBuf {
    repo_root().join("corpus/clean/acton-contracts/config")
}

fn cross_file_fixture_dir() -> PathBuf {
    repo_root().join("corpus/fixtures/cross-file/ac-unguarded-delegate")
}

#[test]
fn version_matches_the_real_observed_three_line_format() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let version = compiler
        .version()
        .expect("a present, executable `tolk` binary must answer --version");
    assert!(
        !version.version.is_empty(),
        "version string must not be empty"
    );
    assert_eq!(
        version.build_commit.len(),
        40,
        "a git commit hash is 40 hex chars"
    );
}

#[test]
fn compiles_the_real_config_contract_and_resolves_its_import_graph() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect(
            "Config.tolk is a real, previously-compile-verified contract (see its own SOURCE.md)",
        );

    assert!(
        !output.fift_code.is_empty(),
        "a successful compile must produce non-empty Fift code"
    );

    let config_file = output
        .symbol_types
        .files
        .iter()
        .find(|f| f.file_name.ends_with("Config.tolk"))
        .expect("Config.tolk itself must appear in the whole-project file list");
    assert!(
        !config_file.imports.is_empty(),
        "Config.tolk imports several sibling and stdlib files, confirmed by hand"
    );

    let imported_names: Vec<&str> = config_file
        .imports
        .iter()
        .filter_map(|id| {
            output
                .symbol_types
                .files
                .iter()
                .find(|f| f.file_id == *id)
                .map(|f| f.file_name.as_str())
        })
        .collect();
    assert!(
        imported_names.iter().any(|n| n.ends_with("errors.tolk")),
        "Config.tolk's own `import \"errors\"` must resolve to a real file in the graph, got {imported_names:?}"
    );
    assert!(
        imported_names
            .iter()
            .any(|n| n.ends_with("config-helpers.tolk")),
        "Config.tolk's own `import \"config-helpers\"` must resolve to a real file in the graph, got {imported_names:?}"
    );
}

#[test]
fn resolves_oninternalmessage_with_a_real_non_synthetic_location() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk is a real, previously-compile-verified contract");

    let entrypoint = output
        .symbol_types
        .functions
        .iter()
        .find(|f| f.name == "onInternalMessage")
        .expect("Config.tolk defines onInternalMessage directly");
    assert!(
        !entrypoint.ident_loc.is_synthetic(),
        "onInternalMessage is written directly in Config.tolk, its location must be real"
    );
    assert_eq!(entrypoint.num_params, 1);
}

#[test]
fn resolves_a_generic_stdlib_method_with_a_synthetic_location() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk is a real, previously-compile-verified contract");

    let generic_instantiation = output
        .symbol_types
        .functions
        .iter()
        .find(|f| f.name.starts_with("map<") && f.name.ends_with(">.get"))
        .expect("Config.tolk's use of map<K, V> must produce a monomorphized .get instantiation");
    assert!(
        generic_instantiation.ident_loc.is_synthetic(),
        "a compiler-synthesized generic instantiation has no single physical declaration site"
    );
}

#[test]
fn declares_the_storage_struct_with_typed_fields() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk is a real, previously-compile-verified contract");

    let storage = output
        .symbol_types
        .declarations
        .iter()
        .find_map(|d| match d {
            Declaration::Struct { name, fields, .. } if name == "Storage" => Some(fields),
            _ => None,
        })
        .expect("Config.tolk declares a Storage struct");
    assert!(
        storage.iter().any(|f| f.name == "storedSeqno"),
        "Storage's real fields, confirmed by hand, must include storedSeqno"
    );
}

#[test]
fn abi_lists_get_methods_and_named_thrown_errors() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk is a real, previously-compile-verified contract");

    assert!(
        output.abi.get_methods.iter().any(|m| m.name == "seqno"),
        "Config.tolk declares a `seqno` get-method"
    );
    assert!(
        !output.abi.thrown_errors.is_empty(),
        "Config.tolk throws several named Errors.* constants"
    );
}

#[test]
fn compile_failure_reports_the_real_plain_text_stderr_shape() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let err = compiler
        .compile(&project_dir, Path::new("does-not-exist.tolk"))
        .expect_err("a nonexistent entrypoint must fail to compile");
    let message = err.to_string();
    assert!(
        message.contains("does-not-exist"),
        "the real tolk error text names the missing file, got: {message}"
    );
}

/// Guards against silently modeling `TypeDescriptor`'s discriminant wrong —
/// every `unique_types` entry in a real compile must parse into a *known*
/// variant, never fall through to [`TypeDescriptor::Other`]. `Other` exists
/// for forward-compatibility with future compiler versions, not to quietly
/// swallow a real modeling gap in what this crate already claims to
/// support.
#[test]
fn no_type_descriptor_falls_back_to_the_unknown_other_catch_all() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk is a real, previously-compile-verified contract");

    let unmodeled: Vec<usize> = output
        .symbol_types
        .unique_types
        .iter()
        .enumerate()
        .filter(|(_, t)| matches!(t, TypeDescriptor::Other))
        .map(|(i, _)| i)
        .collect();
    assert!(
        unmodeled.is_empty(),
        "unique_types indices {unmodeled:?} fell back to TypeDescriptor::Other — \
         this crate's variant list is missing a `kind` this compile actually produced"
    );
}

/// Same guard as above, for [`SourceSpan`] arrays this crate expects to
/// always be exactly 5 elements (JSON deserialization would already fail
/// loudly if `tolk` ever emitted a different length, but this documents the
/// invariant as an explicit, real-data-backed assertion rather than only an
/// implicit one).
#[test]
fn every_real_declaration_site_location_is_internally_consistent() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk is a real, previously-compile-verified contract");

    for function in &output.symbol_types.functions {
        let span: SourceSpan = function.ident_loc;
        if span.is_synthetic() {
            continue;
        }
        assert!(
            span.start_line >= 1,
            "a real, non-synthetic span must be 1-indexed, got {span:?} for {}",
            function.name
        );
    }
}

#[test]
fn project_store_resolves_a_real_cross_file_call_from_config_tolk() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk is a real, previously-compile-verified contract");
    let store = ProjectStore::new(output.symbol_types);

    let config_file = store
        .file_id_for_path(&project_dir.join("Config.tolk"))
        .expect("Config.tolk itself must resolve to a file_id");
    let storage_file = store
        .file_id_for_path(&project_dir.join("storage.tolk"))
        .expect("storage.tolk must resolve to a file_id");

    assert!(
        store.reachable_files(config_file).contains(&storage_file),
        "Config.tolk's own `import \"storage\"` must make storage.tolk reachable"
    );

    // ConfigDict.getById is a real function defined in storage.tolk, not
    // Config.tolk itself — confirmed by direct inspection of a real compile
    // (not assumed): an earlier version of this test guessed it lived in
    // config-helpers.tolk instead and failed loudly on its very first run,
    // corrected against the file_id the compiler itself actually reported.
    // The exact cross-file shape ac.rs::classify_function cannot resolve
    // today either way.
    let resolved = store.resolve_call(config_file, "ConfigDict.getById");
    assert_eq!(
        resolved.len(),
        1,
        "expected exactly one ConfigDict.getById in the whole project"
    );
    assert_eq!(
        resolved[0].ident_loc.file_id, storage_file,
        "ConfigDict.getById must be attributed to storage.tolk, not Config.tolk"
    );
}

/// A project with no `contract{}` header at all (`corpus/fixtures/cross-file/
/// ac-unguarded-delegate`, built for `sting-pipeline`'s own end-to-end
/// test) still compiles — and its `abi.json`'s `storage` field is
/// a genuinely empty object, not a populated `storage_ty_idx`. Found by that
/// fixture's own real compile failing to parse while `storage_ty_idx` was
/// still not an `Option<u32>`; this test locks the real shape in.
#[test]
fn compiling_a_project_with_no_contract_header_leaves_storage_ty_idx_none() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let project_dir = cross_file_fixture_dir();
    let output = compiler
        .compile(&project_dir, Path::new("Main.tolk"))
        .expect("this fixture is confirmed to compile with no contract{} header");
    assert_eq!(
        output.abi.storage.storage_ty_idx, None,
        "no contract{{}} header means no declared storage type"
    );
}
