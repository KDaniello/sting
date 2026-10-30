//! [`ProjectStore`] — a whole-project name/import index built on top of
//! [`crate::SymbolTypes`], and the foundation cross-file call resolution
//! stands on: today `ac.rs::classify_function`,
//! `uta.rs`'s one-hop call-site walker, and `store_load.rs::classify_function`
//! each independently resolve a callee name against only their *own* file's
//! `sting_ast::AstStore` — a call crossing a file/import boundary resolves to
//! nothing, silently. `SymbolTypes` already contains the answer (a
//! project-wide function table plus a real, compiler-resolved import graph);
//! this module is the thin index over it that turns "is `name` reachable
//! from file X" into a fast lookup instead of a re-walk.
//!
//! **What this still does not give you**: this is a name/reachability index,
//! not a call graph — it does not know which functions actually *call*
//! which (that still requires walking a function body via `sting-ast`/
//! tree-sitter, which this crate deliberately does not attempt — see this
//! crate's own top-level doc comment). What it answers is narrower but
//! sufficient for the immediate use case: "given a call site written in
//! file X naming callee `name`, which function(s) could that possibly
//! resolve to, considering imports" — the same question
//! `ac.rs::classify_function` already answers today, just same-file-only.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::symbol_types::{Declaration, FunctionEntry, SymbolTypes};

/// A whole-project index over one [`SymbolTypes`] snapshot.
#[derive(Debug)]
pub struct ProjectStore {
    symbol_types: SymbolTypes,
    functions_by_name: HashMap<String, Vec<usize>>,
    file_id_by_path: HashMap<String, u32>,
}

impl ProjectStore {
    /// Builds a store, indexing every function by name and every file by
    /// its resolved path once up front so later lookups are `O(1)`/`O(k)`,
    /// not a fresh linear scan per call.
    #[must_use]
    pub fn new(symbol_types: SymbolTypes) -> Self {
        let mut functions_by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, function) in symbol_types.functions.iter().enumerate() {
            functions_by_name
                .entry(function.name.clone())
                .or_default()
                .push(index);
        }
        let file_id_by_path: HashMap<String, u32> = symbol_types
            .files
            .iter()
            .map(|f| (normalize(&f.file_name), f.file_id))
            .collect();
        Self {
            symbol_types,
            functions_by_name,
            file_id_by_path,
        }
    }

    /// The [`SymbolTypes`] snapshot this store was built from, for callers
    /// that need direct access beyond this store's own index (e.g.
    /// `declarations`/`unique_types`, which this module does not index).
    #[must_use]
    pub fn symbol_types(&self) -> &SymbolTypes {
        &self.symbol_types
    }

    /// Every function anywhere in the project with exactly this name —
    /// there can be more than one (an overload family, or the same bare
    /// method name on unrelated receiver types, e.g. `map<int32,
    /// cell>.get` and `map<uint256, slice>.get` both resolve bare `"get"`
    /// only if called unqualified, which `tolk` itself never actually
    /// allows — in practice this is almost always 0 or 1 for a real,
    /// non-generic free-function call site).
    pub fn functions_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a FunctionEntry> {
        self.functions_by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|&i| &self.symbol_types.functions[i])
    }

    /// The [`crate::FileEntry::file_id`] whose resolved path matches `path`,
    /// comparing case-insensitively (Windows path resolution) and after
    /// normalizing separators — `tolk` reports absolute, platform-native
    /// paths (confirmed on real output: `F:\Coding\...\Config.tolk`), a
    /// caller's own `Path` may spell the same file differently (forward
    /// slashes, a different but equivalent absolute form).
    #[must_use]
    pub fn file_id_for_path(&self, path: &Path) -> Option<u32> {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.file_id_by_path
            .get(&normalize(&canonical.display().to_string()))
            .copied()
    }

    /// The real filesystem path [`crate::FileEntry::file_id`] `file_id`
    /// reports — the reverse of [`Self::file_id_for_path`], needed by a
    /// cross-file caller (`sting-detectors::support::CrossFileCache`) that
    /// has already resolved a callee to *which* file it lives in (via
    /// [`Self::resolve_call`]) and now needs to actually read and parse that
    /// file. `None` for a synthetic/compiler-generated `file_id` (see
    /// [`crate::SourceSpan::is_synthetic`]) or one not present in this
    /// snapshot at all.
    #[must_use]
    pub fn path_for_file_id(&self, file_id: u32) -> Option<PathBuf> {
        self.symbol_types
            .files
            .iter()
            .find(|f| f.file_id == file_id)
            .map(|f| PathBuf::from(&f.file_name))
    }

    /// [`crate::FileEntry::file_id`]s `file_id` directly imports (not
    /// transitively closed) — empty if `file_id` is unknown or imports
    /// nothing.
    #[must_use]
    pub fn direct_imports(&self, file_id: u32) -> &[u32] {
        self.symbol_types
            .files
            .iter()
            .find(|f| f.file_id == file_id)
            .map_or(&[], |f| f.imports.as_slice())
    }

    /// Every file reachable from `file_id` via zero or more `import`s,
    /// including `file_id` itself (a file's own top-level functions are
    /// always visible to its own call sites) — the transitive closure
    /// [`Self::direct_imports`] only gives one hop of.
    #[must_use]
    pub fn reachable_files(&self, file_id: u32) -> HashSet<u32> {
        let mut seen = HashSet::from([file_id]);
        let mut queue = VecDeque::from([file_id]);
        while let Some(current) = queue.pop_front() {
            for &next in self.direct_imports(current) {
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        seen
    }

    /// Resolves a call site named `callee_name`, written in `from_file_id`,
    /// to every function it could plausibly refer to — every
    /// [`Self::functions_named`] match whose own defining file is
    /// [`Self::reachable_files`] from `from_file_id`. This is the
    /// cross-file generalization of `ac.rs::classify_function`'s
    /// `env.store.function_by_name(name)` (same-file-only today, see this
    /// module's own doc comment).
    ///
    /// A compiler-synthesized generic instantiation (e.g. `map<int32,
    /// cell>.get`, confirmed via [`crate::SourceSpan::is_synthetic`] on
    /// real output) has no real file of its own — always included,
    /// regardless of `from_file_id`, since stdlib generics are visible from
    /// everywhere by construction.
    #[must_use]
    pub fn resolve_call<'a>(
        &'a self,
        from_file_id: u32,
        callee_name: &str,
    ) -> Vec<&'a FunctionEntry> {
        let reachable = self.reachable_files(from_file_id);
        self.functions_named(callee_name)
            .filter(|f| f.ident_loc.is_synthetic() || reachable.contains(&f.ident_loc.file_id))
            .collect()
    }

    /// Resolves a struct name written in `from_file_id` to every
    /// [`Declaration::Struct`] it could plausibly refer to — the same
    /// reachability rule as [`Self::resolve_call`], applied to
    /// [`SymbolTypes::declarations`] instead of `functions` (every returned
    /// reference is guaranteed to match the `Struct` variant; `Alias`/`Enum`
    /// declarations are filtered out here so callers never need to handle
    /// them). Deliberately unindexed, unlike [`Self::functions_named`]: a
    /// contract's storage type is resolved at most once per function
    /// checked (`gvr.rs`'s only caller today), nowhere near a hot enough
    /// path to justify a second `HashMap` alongside `functions_by_name`.
    #[must_use]
    pub fn resolve_struct<'a>(&'a self, from_file_id: u32, name: &str) -> Vec<&'a Declaration> {
        let reachable = self.reachable_files(from_file_id);
        self.symbol_types
            .declarations
            .iter()
            .filter(|decl| match decl {
                Declaration::Struct {
                    name: struct_name,
                    ident_loc,
                    ..
                } => {
                    struct_name == name
                        && ident_loc.is_some_and(|loc| {
                            loc.is_synthetic() || reachable.contains(&loc.file_id)
                        })
                }
                Declaration::Alias { .. } | Declaration::Enum { .. } => false,
            })
            .collect()
    }
}

/// Case-insensitive, `/`-separated form of a path string, for comparing
/// paths that may differ only in separator style or drive-letter casing —
/// deliberately not full canonicalization (that requires the path to exist
/// on disk, which a comparison target read out of `symbol_types.files`
/// always does, but a caller's own `path` argument might not, e.g. in a
/// test). Also strips a leading `\\?\` (confirmed real, not hypothetical:
/// `std::fs::canonicalize` on Windows returns this verbatim-path-prefixed
/// form — `\\?\F:\...` — which `tolk`'s own reported paths never carry, so
/// comparing the two forms directly always missed; caught by
/// `project_store_resolves_a_real_cross_file_call_from_config_tolk` failing
/// on its very first run, not anticipated in advance).
fn normalize(path: &str) -> String {
    path.strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('\\', "/")
        .to_lowercase()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::ProjectStore;
    use crate::symbol_types::{
        Declaration, FieldEntry, FileEntry, FunctionEntry, SourceSpan, SymbolTypes,
    };

    fn span(file_id: u32, line: u32) -> SourceSpan {
        SourceSpan::from([file_id, line, 1, line, 2])
    }

    fn synthetic_span() -> SourceSpan {
        SourceSpan::from([0, 0, 0, 0, 0])
    }

    /// A small, hand-built 3-file project: `main.tolk` imports `helper.tolk`
    /// (which imports `deep.tolk`), `unrelated.tolk` imports nothing and is
    /// not reachable from `main.tolk` — enough shape to exercise multi-hop
    /// transitivity and confirm isolation, without needing a real `tolk`
    /// compile for a pure-logic unit test (the real-compiler-backed
    /// end-to-end path is already covered by `tests/real_compile.rs`).
    #[allow(
        clippy::too_many_lines,
        reason = "a flat literal fixture covering functions and struct declarations across a 4-file project; splitting it into helper functions would just move the same field literals around, not reduce them"
    )]
    fn sample_store() -> ProjectStore {
        let symbol_types = SymbolTypes {
            files: vec![
                FileEntry {
                    file_id: 0,
                    file_name: "/project/main.tolk".to_string(),
                    size_chars: 0,
                    imports: vec![1],
                },
                FileEntry {
                    file_id: 1,
                    file_name: "/project/helper.tolk".to_string(),
                    size_chars: 0,
                    imports: vec![2],
                },
                FileEntry {
                    file_id: 2,
                    file_name: "/project/deep.tolk".to_string(),
                    size_chars: 0,
                    imports: vec![],
                },
                FileEntry {
                    file_id: 3,
                    file_name: "/project/unrelated.tolk".to_string(),
                    size_chars: 0,
                    imports: vec![],
                },
            ],
            unique_types: Vec::new(),
            struct_instantiations: Vec::new(),
            alias_instantiations: Vec::new(),
            declarations: vec![
                Declaration::Struct {
                    name: "DeepStorage".to_string(),
                    ty_idx: 0,
                    ident_loc: Some(span(2, 1)),
                    type_params: Vec::new(),
                    prefix: None,
                    fields: vec![FieldEntry {
                        name: "balance".to_string(),
                        ty_idx: 0,
                    }],
                    description: None,
                },
                Declaration::Struct {
                    name: "UnrelatedStorage".to_string(),
                    ty_idx: 0,
                    ident_loc: Some(span(3, 1)),
                    type_params: Vec::new(),
                    prefix: None,
                    fields: vec![FieldEntry {
                        name: "owner".to_string(),
                        ty_idx: 0,
                    }],
                    description: None,
                },
                Declaration::Struct {
                    name: "AmbiguousStorage".to_string(),
                    ty_idx: 0,
                    ident_loc: Some(span(1, 1)),
                    type_params: Vec::new(),
                    prefix: None,
                    fields: Vec::new(),
                    description: None,
                },
                Declaration::Struct {
                    name: "AmbiguousStorage".to_string(),
                    ty_idx: 0,
                    ident_loc: Some(span(2, 1)),
                    type_params: Vec::new(),
                    prefix: None,
                    fields: Vec::new(),
                    description: None,
                },
                Declaration::Struct {
                    name: "GenericStorage".to_string(),
                    ty_idx: 0,
                    ident_loc: Some(synthetic_span()),
                    type_params: Vec::new(),
                    prefix: None,
                    fields: Vec::new(),
                    description: None,
                },
            ],
            functions: vec![
                FunctionEntry {
                    f_idx: 0,
                    name: "onInternalMessage".to_string(),
                    return_ty_idx: 0,
                    num_params: 1,
                    ident_loc: span(0, 10),
                    end_loc: span(0, 20),
                    params: Vec::new(),
                },
                FunctionEntry {
                    f_idx: 1,
                    name: "helperFn".to_string(),
                    return_ty_idx: 0,
                    num_params: 0,
                    ident_loc: span(1, 5),
                    end_loc: span(1, 8),
                    params: Vec::new(),
                },
                FunctionEntry {
                    f_idx: 2,
                    name: "deepFn".to_string(),
                    return_ty_idx: 0,
                    num_params: 0,
                    ident_loc: span(2, 3),
                    end_loc: span(2, 6),
                    params: Vec::new(),
                },
                FunctionEntry {
                    f_idx: 3,
                    name: "unrelatedFn".to_string(),
                    return_ty_idx: 0,
                    num_params: 0,
                    ident_loc: span(3, 1),
                    end_loc: span(3, 4),
                    params: Vec::new(),
                },
                FunctionEntry {
                    f_idx: 4,
                    name: "map<int32, cell>.get".to_string(),
                    return_ty_idx: 0,
                    num_params: 2,
                    ident_loc: synthetic_span(),
                    end_loc: synthetic_span(),
                    params: Vec::new(),
                },
            ],
        };
        ProjectStore::new(symbol_types)
    }

    #[test]
    fn reachable_files_includes_self_and_transitive_imports() {
        let store = sample_store();
        let reachable = store.reachable_files(0);
        assert!(reachable.contains(&0), "a file always reaches itself");
        assert!(reachable.contains(&1), "direct import");
        assert!(
            reachable.contains(&2),
            "transitive import (main -> helper -> deep)"
        );
        assert!(
            !reachable.contains(&3),
            "unrelated.tolk is not imported by anything on this path"
        );
    }

    #[test]
    fn resolve_call_finds_a_same_file_function() {
        let store = sample_store();
        let resolved = store.resolve_call(0, "onInternalMessage");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].f_idx, 0);
    }

    #[test]
    fn resolve_call_finds_a_direct_import_function() {
        let store = sample_store();
        let resolved = store.resolve_call(0, "helperFn");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].f_idx, 1);
    }

    #[test]
    fn resolve_call_finds_a_transitively_imported_function() {
        // The exact cross-file gap this module exists to close: `deepFn` is
        // two import hops away from `main.tolk`, invisible to any
        // same-file-only resolver (ac.rs::classify_function today).
        let store = sample_store();
        let resolved = store.resolve_call(0, "deepFn");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].f_idx, 2);
    }

    #[test]
    fn resolve_call_does_not_find_an_unreachable_function() {
        let store = sample_store();
        assert!(store.resolve_call(0, "unrelatedFn").is_empty());
    }

    #[test]
    fn resolve_call_finds_unrelated_fn_from_its_own_file() {
        let store = sample_store();
        let resolved = store.resolve_call(3, "unrelatedFn");
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn resolve_call_always_finds_a_synthetic_generic_instantiation() {
        // Stdlib generics have no real defining file (SourceSpan::is_synthetic),
        // so they must resolve regardless of which file asks.
        let store = sample_store();
        assert_eq!(store.resolve_call(3, "map<int32, cell>.get").len(), 1);
    }

    #[test]
    fn resolve_struct_finds_a_transitively_imported_struct() {
        // The GVR gap this method exists to close: a contract's storage
        // struct commonly lives in a different file than its handlers
        // (`corpus/fixtures/tolk-bench/storage.tolk` vs. `JettonWallet.tolk`).
        let store = sample_store();
        let resolved = store.resolve_struct(0, "DeepStorage");
        assert_eq!(resolved.len(), 1);
        assert!(matches!(resolved[0], Declaration::Struct { fields, .. } if fields.len() == 1));
    }

    #[test]
    fn resolve_struct_does_not_find_an_unreachable_struct() {
        let store = sample_store();
        assert!(store.resolve_struct(0, "UnrelatedStorage").is_empty());
    }

    #[test]
    fn resolve_struct_finds_unrelated_struct_from_its_own_file() {
        let store = sample_store();
        assert_eq!(store.resolve_struct(3, "UnrelatedStorage").len(), 1);
    }

    #[test]
    fn resolve_struct_returns_every_reachable_same_named_match_ambiguously() {
        // Two same-named structs, both reachable from main.tolk (helper.tolk
        // and deep.tolk) — the caller (gvr.rs's storage_field_names) is the
        // one that decides "more than one match means unresolved, not
        // guessed", not this method itself.
        let store = sample_store();
        assert_eq!(store.resolve_struct(0, "AmbiguousStorage").len(), 2);
    }

    #[test]
    fn resolve_struct_always_finds_a_synthetic_generic_instantiation() {
        let store = sample_store();
        assert_eq!(store.resolve_struct(3, "GenericStorage").len(), 1);
    }

    #[test]
    fn resolve_struct_returns_empty_for_an_unknown_name() {
        let store = sample_store();
        assert!(store.resolve_struct(0, "DoesNotExist").is_empty());
    }

    #[test]
    fn functions_named_returns_empty_for_an_unknown_name() {
        let store = sample_store();
        assert_eq!(store.functions_named("doesNotExist").count(), 0);
    }

    #[test]
    fn direct_imports_is_empty_for_an_unknown_file_id() {
        let store = sample_store();
        assert!(store.direct_imports(999).is_empty());
    }

    #[test]
    fn path_for_file_id_finds_a_known_file() {
        let store = sample_store();
        assert_eq!(
            store.path_for_file_id(1),
            Some(std::path::PathBuf::from("/project/helper.tolk"))
        );
    }

    #[test]
    fn path_for_file_id_is_none_for_an_unknown_file_id() {
        let store = sample_store();
        assert_eq!(store.path_for_file_id(999), None);
    }
}
