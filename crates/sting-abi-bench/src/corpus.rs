//! Corpus discovery — walks the real, already-vendored corpora this
//! workspace has (`corpus/clean/{acton-contracts,tolk-bench}` for Tolk,
//! `corpus-abi/clean/tact/` for Tact) into a flat list of `(project_dir,
//! entrypoint)` pairs the harness can compile, mirroring
//! `sting-bench::corpus`'s own discovery role for the security branch.

use std::path::{Path, PathBuf};

/// One Tier 1 contract to score: `entrypoint` (relative, e.g.
/// `Config.tolk`) resolved against `project_dir`.
#[derive(Debug, Clone)]
pub struct Tier1Project {
    /// A human-readable label for reporting (e.g. `"config/Config.tolk"`).
    pub label: String,
    /// The directory to compile from (siblings resolve relative to this).
    pub project_dir: PathBuf,
    /// The entrypoint file name, relative to `project_dir`.
    pub entrypoint: PathBuf,
}

fn subdirectories(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Every real Tolk contract in `roots` (typically
/// `corpus/clean/acton-contracts` and `corpus/clean/tolk-bench`) —
/// one [`Tier1Project`] per PascalCase-named `.tolk` file directly inside
/// each immediate subdirectory (Tolk's own entrypoint-naming convention,
/// confirmed against every real project in both corpora; a subdirectory
/// can hold more than one real entrypoint, e.g. `acton-contracts/dns/` has
/// three — `DnsCollection.tolk`/`DnsItem.tolk`/`RootDns.tolk` — so this
/// scans every matching file per directory, not just the first).
#[must_use]
pub fn discover_tolk_projects(roots: &[&Path]) -> Vec<Tier1Project> {
    let mut projects = Vec::new();
    for root in roots {
        for project_dir in subdirectories(root) {
            let Ok(files) = std::fs::read_dir(&project_dir) else {
                continue;
            };
            let mut entrypoints: Vec<PathBuf> = files
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "tolk"))
                .filter(|p| {
                    p.file_stem()
                        .and_then(|s| s.to_str())
                        .and_then(|s| s.chars().next())
                        .is_some_and(char::is_uppercase)
                })
                .collect();
            entrypoints.sort();
            let dir_name = project_dir
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().to_string());
            for entrypoint in entrypoints {
                let file_name = entrypoint
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().to_string());
                projects.push(Tier1Project {
                    label: format!("{dir_name}/{file_name}"),
                    project_dir: project_dir.clone(),
                    entrypoint: PathBuf::from(file_name),
                });
            }
        }
    }
    projects
}

/// Every real Tact contract in `root` (`corpus-abi/clean/tact/`) — one
/// [`Tier1Project`] per immediate subdirectory, entrypoint
/// `<subdirectory-name>.tact` (the layout `corpus-abi/clean/tact/SOURCE.md`
/// documents: one contract per subdirectory, entrypoint named after it).
#[must_use]
pub fn discover_tact_projects(root: &Path) -> Vec<Tier1Project> {
    let mut projects = Vec::new();
    for project_dir in subdirectories(root) {
        let Some(dir_name) = project_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
        else {
            continue;
        };
        let entrypoint = PathBuf::from(format!("{dir_name}.tact"));
        if project_dir.join(&entrypoint).is_file() {
            projects.push(Tier1Project {
                label: format!("{dir_name}/{}", entrypoint.display()),
                project_dir,
                entrypoint,
            });
        }
    }
    projects
}

/// Every real `FunC` contract anywhere under `root`
/// (`corpus-abi/clean/func/`) — one [`Tier1Project`] per `.fc`/`.func` file,
/// walked **recursively** (unlike [`discover_tolk_projects`]/
/// [`discover_tact_projects`]'s per-subdirectory-as-one-project layout —
/// `FunC` contracts here are each a single self-contained file, so nesting
/// is purely organizational, e.g. `registry/<code_hash>/<file>.fc` for a
/// bulk-imported batch, see `corpus-abi/clean/func/SOURCE.md`). Every entry
/// shares `root` as `project_dir`, with `entrypoint` set to the file's path
/// relative to `root` — a flat file directly in `root` therefore still gets
/// exactly the same `entrypoint`/`label` (its own file name) as before this
/// became recursive, preserving every existing label. Both extensions are
/// real, live `FunC` conventions — `.func` is
/// `ton-blockchain/multisig-contract-v2`'s own (`contracts/multisig.func`),
/// vendored under its real upstream extension, not renamed to fit this
/// corpus's more common `.fc` files. Used by `abi-layer dictionary-build` to
/// grow the method-id dictionary from real, deployed `FunC` contracts, not
/// just Tier 1's compiled Tolk/Tact corpora.
#[must_use]
pub fn discover_func_projects(root: &Path) -> Vec<Tier1Project> {
    let mut files = Vec::new();
    collect_func_files(root, &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|full_path| {
            let entrypoint = full_path.strip_prefix(root).ok()?.to_path_buf();
            let label = entrypoint.to_string_lossy().replace('\\', "/");
            Some(Tier1Project {
                label,
                project_dir: root.to_path_buf(),
                entrypoint,
            })
        })
        .collect()
}

/// Recursively collects every `.fc`/`.func` file under `dir` into `out`.
/// Missing/unreadable directories are silently skipped (matches
/// [`discover_tolk_projects`]/[`discover_tact_projects`]'s own tolerance for
/// a not-yet-created corpus root), not surfaced as an error — this is a
/// best-effort discovery helper, not a validating one.
fn collect_func_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_func_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext == "fc" || ext == "func")
        {
            out.push(path);
        }
    }
}

/// One Tier 2 ground-truth pair: a real `FunC` fixture and the
/// `func2tolk`-converted Tolk equivalent of the same contract — see
/// `tier2`'s own module doc comment for the full methodology and why this
/// list is honestly small (4), not padded out.
#[derive(Debug, Clone, Copy)]
pub struct Tier2Pair {
    /// The `FunC` fixture file name, relative to
    /// `crates/sting-abi-func/tests/fixtures/`.
    pub func_file: &'static str,
    /// The converted Tolk project directory, relative to the repo root.
    pub tolk_project_dir: &'static str,
    /// The converted Tolk entrypoint, relative to `tolk_project_dir`.
    pub tolk_entrypoint: &'static str,
}

/// Every Tier 2 ground-truth pair currently available in this workspace.
pub const TIER2_PAIRS: &[Tier2Pair] = &[
    Tier2Pair {
        func_file: "jetton-minter.fc",
        tolk_project_dir: "corpus/clean/converted/jetton-minter",
        tolk_entrypoint: "jetton-minter.tolk",
    },
    Tier2Pair {
        func_file: "jetton-wallet.fc",
        tolk_project_dir: "corpus/clean/converted/jetton-wallet",
        tolk_entrypoint: "jetton-wallet.tolk",
    },
    Tier2Pair {
        func_file: "nft-item.fc",
        tolk_project_dir: "corpus/clean/converted/nft-item",
        tolk_entrypoint: "nft-item.tolk",
    },
    Tier2Pair {
        func_file: "wallet3-code.fc",
        tolk_project_dir: "corpus/clean/converted/wallet3",
        tolk_entrypoint: "wallet3-code.tolk",
    },
];

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{discover_func_projects, discover_tact_projects, discover_tolk_projects};

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn discovers_every_real_tolk_project_including_multi_entrypoint_directories() {
        let root = repo_root().join("corpus/clean/acton-contracts");
        let projects = discover_tolk_projects(&[&root]);
        assert!(
            projects.iter().any(|p| p.label == "config/Config.tolk"),
            "{projects:?}"
        );
        let dns_entrypoints: Vec<_> = projects
            .iter()
            .filter(|p| p.label.starts_with("dns/"))
            .collect();
        assert_eq!(
            dns_entrypoints.len(),
            3,
            "dns/ has three real entrypoints, all should be discovered: {projects:?}"
        );
    }

    #[test]
    fn discovers_every_real_tact_project() {
        let root = repo_root().join("corpus-abi/clean/tact");
        if !root.is_dir() {
            println!("SKIP: {} not present", root.display());
            return;
        }
        let projects = discover_tact_projects(&root);
        assert!(
            projects.len() >= 15,
            "expected at least 15 real Tact contracts, found {}: {projects:?}",
            projects.len()
        );
    }

    #[test]
    fn discovers_every_real_func_project() {
        let root = repo_root().join("corpus-abi/clean/func");
        if !root.is_dir() {
            println!("SKIP: {} not present", root.display());
            return;
        }
        let projects = discover_func_projects(&root);
        assert!(
            projects
                .iter()
                .any(|p| p.label == "governed-jetton-wallet.fc"),
            "{projects:?}"
        );
        assert!(
            projects.iter().any(|p| p.label == "multisig.func"),
            "a real .func-extension file must be discovered too, not just .fc: {projects:?}"
        );
    }

    #[test]
    fn discovers_func_files_nested_in_subdirectories_too() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let nested = dir.path().join("registry").join("abc123");
        std::fs::create_dir_all(&nested).expect("creates nested dirs");
        std::fs::write(
            nested.join("contract.fc"),
            "_ get_thing() method_id { return 0; }",
        )
        .expect("writes a real file");
        std::fs::write(
            dir.path().join("flat.fc"),
            "_ get_flat() method_id { return 0; }",
        )
        .expect("writes a real flat file");

        let projects = discover_func_projects(dir.path());

        let nested_project = projects
            .iter()
            .find(|p| p.label == "registry/abc123/contract.fc")
            .expect("finds the nested file with a forward-slash label");
        assert_eq!(
            nested_project.project_dir,
            dir.path(),
            "project_dir stays the shared root, not the nested directory"
        );
        assert_eq!(
            nested_project.entrypoint,
            std::path::PathBuf::from("registry")
                .join("abc123")
                .join("contract.fc"),
            "entrypoint carries the full relative path so `project_dir.join(entrypoint)` still resolves"
        );
        assert!(
            projects.iter().any(|p| p.label == "flat.fc"),
            "a flat, non-nested file keeps its old, simple label: {projects:?}"
        );
    }
}
