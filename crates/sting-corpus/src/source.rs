//! Core types shared by every contract source
//! (`docs/DATA.md`).

use std::path::PathBuf;

use crate::FetchError;

/// Where a contract to analyze comes from — the type-level expression of
/// the distinction that matters most (`docs/DATA.md`):
/// [`Self::LocalFile`] is Tolk *source*, usable only for static
/// analysis; [`Self::LocalBoc`]/[`Self::OnChainAddress`] are compiled
/// *bytecode*, usable only for the emulator/fuzzer. `sting` never compiles
/// one into the other (see `docs/GLOSSARY.md`'s note that `sting` never
/// invokes the real Tolk compiler) — so a caller holding a `LocalFile`
/// cannot get an `EmulationTarget` from it, and vice versa. This is enforced
/// at the [`crate::CorpusController`] level, not just documented:
/// resolving a `LocalFile` to bytecode is a distinct, clearly-named error,
/// not a silent no-op or a surprising compile attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractTarget {
    /// A local `.tolk` source file — static analysis only.
    LocalFile(PathBuf),
    /// A local pair of already-compiled `code`/`data` cell `BoC`s, in the
    /// same base64-text file format as `corpus/fixtures/emulator/`'s
    /// fixtures (`docs/domains/emulator.md`) and the benchmark corpus
    /// (`docs/domains/precision-sources.md`) — one format, three
    /// consumers, per §5.3.
    LocalBoc {
        /// Path to the code cell `BoC`, base64-encoded text.
        code: PathBuf,
        /// Path to the data cell `BoC`, base64-encoded text.
        data: PathBuf,
    },
    /// An already-deployed contract's address, resolved over the network —
    /// bytecode only, dynamic-analysis-only (see this type's own doc comment).
    OnChainAddress(String),
}

/// A resolved contract's compiled code+data, in the same base64-`BoC`-text
/// representation `sting-emulator`'s `TvmEmulator::new` already expects
/// (`docs/domains/emulator.md`) — no further decoding needed by callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractCode {
    /// Base64-encoded `BoC` of the contract's code cell.
    pub code_boc_base64: String,
    /// Base64-encoded `BoC` of the contract's data cell.
    pub data_boc_base64: String,
}

/// A provider that can resolve an on-chain address to its compiled code+data
/// — the mocking boundary `docs/ARCHITECTURE.md` principle 1
/// calls for ("`&dyn ContractSource`", named there verbatim): production
/// code uses [`crate::ToncenterClient`]/[`crate::TonapiClient`]; tests for
/// anything built on top ([`crate::CorpusController`]) substitute a
/// fake with a scripted response, never a real network call.
pub trait ContractSource: Send + Sync {
    /// Human-readable name for diagnostics (e.g. `"toncenter v3"`).
    fn name(&self) -> &'static str;

    /// Resolves `address` (a raw `"<workchain>:<64 hex chars>"` or
    /// user-friendly form — whatever the provider's own API accepts) to its
    /// compiled code+data.
    ///
    /// # Errors
    /// See [`FetchError`].
    fn fetch(&self, address: &str) -> Result<ContractCode, FetchError>;
}
