//! The verification ladder as code.
//!
//! karman/docs/VERIFICATION.md defines four levels; the first three are executable and live here, one
//! module per level: [`differential`] is level 0, [`replay`] is level 1, [`facts`] is level 2.
//! Level 3 — preregistered prediction — is a process, not a program, and has no module by
//! design.
//!
//! The crate is a harness library: it drives mechanisms through the core's public API and
//! renders verdicts. It holds no mechanism knowledge of its own — a new mechanism gets verified
//! by these same harnesses or it does not merge.

pub mod differential;
pub mod facts;
pub mod replay;

/// The outcome of one executable verification check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The check passed within its preregistered tolerance.
    Confirmed,
    /// The check failed; the detail names the first divergence.
    Diverged {
        /// Where agreement ended and what the two sides said there.
        detail: String,
    },
}
