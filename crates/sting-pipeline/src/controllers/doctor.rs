//! `DoctorController` — the four versions a run's answer depends on.
//!
//! An emulation is a statement about semantics, and the semantics belong to a
//! version. A run that does not say which version it emulated is the same
//! defect as a parse that does not say it failed: the answer looks like an
//! answer and cannot be checked.
//!
//! Four components decide what this tree reads and what it executes: the
//! emulator library, the Tolk compiler it compares itself against, the grammar
//! it parses with, and the chain's global version the emulation pins. Each is
//! reported with where the number came from, and an unknown one is a refusal
//! rather than a blank in a table.

use sting_tolkc::TolkCompiler;

/// What one component reports about itself.
#[derive(Debug, Clone)]
pub struct ComponentVersion {
    /// The component, as a reader names it.
    pub name: &'static str,
    /// The version it reports, or why it could not be read.
    pub state: VersionState,
    /// Where the number came from — a file, a call, a pin.
    pub source: String,
}

/// A version that is known, or the reason it is not.
#[derive(Debug, Clone)]
pub enum VersionState {
    /// The component reported a version.
    Known(String),
    /// It could not be read, with the reason.
    Unknown(String),
}

impl VersionState {
    /// Whether this is a version rather than a reason for its absence.
    #[must_use]
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(_))
    }
}

/// Every component's version, in the order a reader needs them.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    /// The components, in report order.
    pub components: Vec<ComponentVersion>,
}

impl DoctorReport {
    /// Whether every component reported a version.
    ///
    /// A caller is expected to refuse the run when this is false. One unknown
    /// version is enough: a report that names three and shrugs at the fourth
    /// says less than it appears to.
    #[must_use]
    pub fn all_known(&self) -> bool {
        self.components
            .iter()
            .all(|component| component.state.is_known())
    }

    /// The components whose version could not be read.
    pub fn unknown(&self) -> impl Iterator<Item = &ComponentVersion> {
        self.components
            .iter()
            .filter(|component| !component.state.is_known())
    }
}

/// Collects the four versions.
pub struct DoctorController;

impl DoctorController {
    /// Reads every component's version.
    ///
    /// Nothing here fails: a component that cannot be read reports why, because
    /// the reason is the useful half of the answer. Whether that is fatal is
    /// the caller's decision through [`DoctorReport::all_known`].
    #[must_use]
    pub fn report(&self) -> DoctorReport {
        DoctorReport {
            components: vec![
                Self::emulator(),
                Self::compiler(),
                Self::grammar(),
                Self::global_version(),
            ],
        }
    }

    /// `libemulator`, as the loaded library reports itself.
    ///
    /// There is no semantic version: TON's emulator ABI moves per commit, so
    /// the commit hash and its date are the version.
    fn emulator() -> ComponentVersion {
        let Some(path) = sting_emulator::vendored_or_default_library_path() else {
            return ComponentVersion {
                name: "libemulator",
                state: VersionState::Unknown(
                    "no library beside the executable, at STING_LIBEMULATOR_PATH, or in vendor/"
                        .to_string(),
                ),
                source: "vendor/libemulator, gitignored".to_string(),
            };
        };
        // Through `sting-emulator`'s safe loader rather than the raw one: this
        // crate has no right to `unsafe`, and the trust boundary is asserted
        // once, there (`docs/ARCHITECTURE.md` §1.3).
        let state = match sting_emulator::load_library(&path) {
            Ok(lib) => {
                let version = lib.version();
                // The commit is always the pinned one — loading refuses any
                // other build — so what this line adds is the release that
                // commit was published in, which is the name a person knows it
                // by.
                VersionState::Known(format!(
                    "commit {} ({}), built {}",
                    version.commit_hash,
                    sting_emulator_sys::PINNED_RELEASE,
                    version.commit_date
                ))
            }
            Err(error) => VersionState::Unknown(error.to_string()),
        };
        ComponentVersion {
            name: "libemulator",
            state,
            source: path.display().to_string(),
        }
    }

    /// The vendored Tolk compiler, through `tolk --version`.
    fn compiler() -> ComponentVersion {
        let Some(compiler) = TolkCompiler::vendored_or_default() else {
            return ComponentVersion {
                name: "Tolk compiler",
                state: VersionState::Unknown(
                    "no compiler beside the executable, at STING_TOLKC_PATH, or in vendor/"
                        .to_string(),
                ),
                source: "vendor/tolk, gitignored".to_string(),
            };
        };
        let source = compiler.executable_path().display().to_string();
        let state = match compiler.version() {
            Ok(version) => VersionState::Known(format!(
                "{} (commit {}, built {})",
                version.version, version.build_commit, version.build_date
            )),
            Err(error) => VersionState::Unknown(error.to_string()),
        };
        ComponentVersion {
            name: "Tolk compiler",
            state,
            source,
        }
    }

    /// The grammar every parse of Tolk source goes through.
    ///
    /// It is vendored and patched, so the published version alone would be a
    /// half-truth: what is compiled in is a fork of it.
    fn grammar() -> ComponentVersion {
        ComponentVersion {
            name: "Tolk grammar",
            state: VersionState::Known(format!(
                "{}, vendored fork",
                sting_ast::GRAMMAR_UPSTREAM_VERSION
            )),
            source: "vendor/tree-sitter-tolk".to_string(),
        }
    }

    /// The chain's global version an emulation is ruled against.
    ///
    /// A constant rather than a read of some particular config: this is what
    /// the tree claims to have checked its detectors against. What a given run
    /// actually emulates is read from that run's own config and refused when
    /// the two differ ([`sting_emulator::pinned_global_version_of`]).
    fn global_version() -> ComponentVersion {
        ComponentVersion {
            name: "global version",
            state: VersionState::Known(sting_emulator::PINNED_GLOBAL_VERSION.to_string()),
            source: "read from the vendored mainnet config, parameter 8".to_string(),
        }
    }
}
