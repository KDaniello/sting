//! Obtaining a target, in two steps, cheapest first.
//!
//! A scope entry names a repository; the run needs two different things from it, and the second is
//! far more expensive than the first. Listing what a commit contains answers "is there anything here
//! this analyser can read at all" and needs no file contents, so the clone is made without them.
//! Only a target that turned out to carry readable code is then cloned again, with contents.
//!
//! The order is not an optimisation. Most of the published scope is written in languages the static
//! contour does not read, and downloading a C++ node to discover that it is a C++ node would make
//! the reach measurement cost more than the hunt it precedes.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::HuntError;
use crate::scope::{Scope, Target};

/// A pinned listing of a target: what the commit contains, without the contents.
#[derive(Debug, Clone)]
pub struct Listing {
    /// The commit the reference resolved to. Recorded rather than assumed: a branch moves, and a
    /// finding that cannot name the commit it was found at is not reproducible.
    pub commit: String,
    /// Every path the commit contains, repository-relative, in `git` order.
    pub files: Vec<String>,
}

/// Clone `reference` of `url` without file contents, and list what it contains.
///
/// An existing clone in the cache is reused as it stands, and `refresh` is how a caller asks for the
/// reference to be resolved again. Reuse is the default because re-resolving on every run would
/// silently change the commit a report was made against; the cost is that a project migrating
/// between languages stays invisible until the cache is refreshed, which is why the commit sits in
/// every row of the report rather than only in the run that produced it.
///
/// # Errors
/// Returns [`HuntError`] when `git` is absent, when the clone or the listing fails, or when the
/// cache directory cannot be created.
pub fn list(url: &str, reference: &str, cache: &Path, refresh: bool) -> Result<Listing, HuntError> {
    let root = prepare(cache, &slug(url, reference, "list"))?;
    if refresh && root.join(".git").is_dir() {
        std::fs::remove_dir_all(&root).map_err(|source| HuntError::Cache {
            path: root.clone(),
            source,
        })?;
    }
    if !root.join(".git").is_dir() {
        clone(
            url,
            reference,
            &root,
            &["--filter=blob:none", "--no-checkout"],
        )?;
    }
    let commit = head_of(url, &root)?;
    // `git` escapes a path with a byte outside ASCII and wraps it in quotes unless told not to. A
    // quoted name matches no scope path and lands in a language bucket of its own, so a target whose
    // only Tolk file has an accent in its name would be reported as having nothing to read.
    let listing = git(
        url,
        "ls-tree",
        &[
            "-C",
            &root.to_string_lossy(),
            "-c",
            "core.quotePath=false",
            "ls-tree",
            "-r",
            "HEAD",
            "--name-only",
        ],
    )?;
    Ok(Listing {
        commit,
        files: listing.lines().map(str::to_owned).collect(),
    })
}

/// Clone the same reference again, with contents this time, and return where it landed.
///
/// A second clone rather than filling in the first: a contentless clone fetches what a checkout
/// needs one object at a time, and a repository of any size turns that into thousands of round trips
/// that fail as a unit. Called only for a target the reach pass found readable code in, which is
/// what keeps the download proportional to what is actually analysed.
///
/// The commit is checked against the one that was listed. A branch that moved between the two clones
/// would otherwise produce a report whose reach and whose findings describe different code.
///
/// # Errors
/// Returns [`HuntError`] when `git` is absent, when the clone fails, or when the reference resolved
/// to a different commit than the listing was taken at.
pub fn materialise(
    target: &Target,
    cache: &Path,
    refresh: bool,
    listed: &Listing,
) -> Result<PathBuf, HuntError> {
    let Scope::Repository {
        url,
        reference,
        paths,
        ..
    } = target.scope
    else {
        return Err(HuntError::NoRepository {
            target: target.name.to_owned(),
        });
    };
    let root = prepare(cache, &slug(url, reference, "files"))?;
    if refresh && root.join(".git").is_dir() {
        std::fs::remove_dir_all(&root).map_err(|source| HuntError::Cache {
            path: root.clone(),
            source,
        })?;
    }
    if !root.join(".git").is_dir() {
        // Whole, even for a scope that names four files inside a repository of 2702. Fetching only
        // the named paths was tried and measured: a contentless clone plus `checkout HEAD -- <paths>`
        // brings the cache from 45 MB to 1.4 MB, and then fails against this remote with `unable to
        // read sha1 file` — the promisor fetch a partial clone defers does not complete here,
        // reproduced on the node's repository twice. A cheap fetch that sometimes yields an empty
        // checkout is worse than an expensive one, because the run cannot tell the two apart from
        // the outside.
        let _ = paths;
        clone(url, reference, &root, &[])?;
    }
    let commit = head_of(url, &root)?;
    if commit != listed.commit {
        return Err(HuntError::ReferenceMoved {
            url: url.to_owned(),
            reference: reference.to_owned(),
            listed: listed.commit.clone(),
            fetched: commit,
        });
    }
    Ok(root)
}

/// Make sure the cache exists, and return the path a clone belongs at.
fn prepare(cache: &Path, name: &str) -> Result<PathBuf, HuntError> {
    std::fs::create_dir_all(cache).map_err(|source| HuntError::Cache {
        path: cache.to_path_buf(),
        source,
    })?;
    Ok(cache.join(name))
}

/// One shallow clone of one reference.
fn clone(url: &str, reference: &str, root: &Path, extra: &[&str]) -> Result<(), HuntError> {
    let root = root.to_string_lossy().into_owned();
    let mut args = vec!["clone", "--quiet", "--depth", "1", "--branch", reference];
    args.extend_from_slice(extra);
    args.push(url);
    args.push(&root);
    git(url, "clone", &args).map(|_| ())
}

/// The commit a clone is sitting on.
fn head_of(url: &str, root: &Path) -> Result<String, HuntError> {
    git(
        url,
        "rev-parse",
        &["-C", &root.to_string_lossy(), "rev-parse", "HEAD"],
    )
    .map(|out| out.trim().to_owned())
}

/// A directory name that is stable across runs and readable in a listing, so a cache can be
/// inspected by hand when a clone goes wrong.
fn slug(url: &str, reference: &str, kind: &str) -> String {
    let tail = url.trim_end_matches('/').trim_end_matches(".git");
    let mut out = String::new();
    for chunk in tail
        .rsplit('/')
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if !out.is_empty() {
            out.push('_');
        }
        out.push_str(chunk);
    }
    out.push('@');
    out.push_str(reference);
    out.push('.');
    out.push_str(kind);
    out.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '@') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Run `git` and return its stdout, turning a non-zero status into a named error rather than an
/// empty result.
fn git(url: &str, command: &str, args: &[&str]) -> Result<String, HuntError> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(HuntError::GitMissing)?;
    if !output.status.success() {
        return Err(HuntError::Git {
            command: command.to_owned(),
            url: url.to_owned(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::slug;

    /// Two targets in the same organisation, one repository at two references, and the listing and
    /// the contents of the same reference must all land in different directories: any collision
    /// would silently measure one target as another.
    #[test]
    fn a_slug_separates_the_repository_the_reference_and_the_kind() {
        let listed = slug("https://github.com/ston-fi/dex-core", "v1.0.0", "list");
        assert_eq!(listed, "ston-fi_dex-core@v1.0.0.list");
        assert_ne!(
            listed,
            slug("https://github.com/ston-fi/dex-core", "main", "list")
        );
        assert_ne!(
            listed,
            slug("https://github.com/torch-core/dex-core", "v1.0.0", "list")
        );
        assert_ne!(
            listed,
            slug("https://github.com/ston-fi/dex-core", "v1.0.0", "files")
        );
    }

    /// A reference with a separator in it would otherwise create a nested directory the cache never
    /// looks in again.
    #[test]
    fn a_slug_has_no_path_separator() {
        let s = slug(
            "https://github.com/ton-blockchain/ton.git",
            "refs/heads/master",
            "list",
        );
        assert!(!s.contains('/'), "{s}");
        assert!(!s.contains('\\'), "{s}");
    }
}
