//! What can go wrong on the way to a target, as types.
//!
//! Every variant exists so that a target which was not read cannot be printed beside one that was
//! read and found clean. A network that refused, a ref that moved, a `git` that is not installed, a
//! file the listing counted and the checkout does not carry — each of them ends the run with a name
//! and a non-zero status rather than an empty findings list.
//!
//! Nothing here covers writing the report out. That failure is the binary's to report, it is loud
//! on its own, and a variant nobody constructs is a promise the type stops keeping.

use std::path::PathBuf;

use thiserror::Error;

/// A step of the hunt that did not complete.
#[derive(Debug, Error)]
pub enum HuntError {
    /// `git` is the only way a target is obtained, so its absence is fatal rather than a target
    /// that quietly reports nothing.
    #[error("git is not on PATH, and every target is fetched with it: {0}")]
    GitMissing(#[source] std::io::Error),
    /// A `git` invocation returned non-zero. The command and its own message are carried through,
    /// because "clone failed" without the remote's reason is not a report.
    #[error("git {command} failed for {url}: {message}")]
    Git {
        /// The subcommand that failed.
        command: String,
        /// The remote it was aimed at.
        url: String,
        /// What `git` printed on stderr.
        message: String,
    },
    /// The cache directory could not be created or read.
    #[error("cannot use the target cache at {path}: {source}")]
    Cache {
        /// The directory the run was told to keep its clones in.
        path: PathBuf,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// The reference moved between the listing and the contents. Analysing the second while
    /// reporting the reach of the first would describe two different bodies of code as one.
    #[error("{url} moved from {listed} to {fetched} while {reference} was being read")]
    ReferenceMoved {
        /// The repository.
        url: String,
        /// The branch or tag that moved.
        reference: String,
        /// The commit the listing was taken at.
        listed: String,
        /// The commit the contents came from.
        fetched: String,
    },
    /// A published exclusion removed nothing at the pinned commit. A stale rule hides the day a
    /// project moves code back into scope, so the run refuses rather than quietly narrowing.
    #[error("{target}: the scope excludes {rules}, which {commit} does not contain")]
    StaleExclusion {
        /// The scope entry.
        target: String,
        /// The exclusions that matched nothing.
        rules: String,
        /// The commit the reference resolved to.
        commit: String,
    },
    /// The scope names a subtree that the pinned commit does not contain — the programme moved its
    /// code, and reporting against a path that no longer exists would be reporting against nothing.
    #[error("{target}: the scope names {path}, which {commit} does not contain")]
    ScopePathMissing {
        /// The scope entry.
        target: String,
        /// The subtree the published scope named.
        path: String,
        /// The commit the reference resolved to.
        commit: String,
    },
    /// The listing counted a file as in scope that the checkout does not carry. The two views of
    /// one commit disagree, and a file counted but never read is the silence this crate exists to
    /// refuse.
    #[error("{target}: {file} is in scope at {commit} but absent from the checkout")]
    ScopeFileMissing {
        /// The scope entry.
        target: String,
        /// The file the listing named.
        file: String,
        /// The commit both views claim to describe.
        commit: String,
    },
    /// A target with no repository was asked for its contents.
    #[error("{target}: the scope names no repository to read")]
    NoRepository {
        /// The scope entry.
        target: String,
    },
    /// A file inside a scope was found but could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
}
