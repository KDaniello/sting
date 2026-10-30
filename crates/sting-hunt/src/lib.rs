//! Running the detectors against code nobody wrote for this repository.
//!
//! The corpus the detectors are measured on was assembled here and labelled here, which answers one
//! question — whether the measure agrees with itself — and leaves a second one open: whether it
//! survives contact with code written by people who never heard of it. This crate is that second
//! question, aimed at the published bug bounty scopes of the TON ecosystem.
//!
//! It is not a scanner. A scanner reads whatever it is pointed at; this reads only what a published
//! programme has permitted, because the permission is the difference between research and
//! trespass. The permitted set is [`scope::SCOPE`], it is committed, and a target absent from it is
//! read for calibration and never reported.
//!
//! The first thing the run produces is not findings but reach: how much of each published scope the
//! static contour can read at all. Most of the ecosystem's bounty-covered code is written in
//! languages this analyser does not parse, and a findings table printed without that number invites
//! the reader to mistake "not examined" for "examined and clean". Reach is measured from the pinned
//! commit rather than declared: a language typed into a table stops being true the day a project
//! migrates, and this counts instead of asserting.
//!
//! What it does not do is notice a migration on its own. A cached clone is reused so that two
//! reports printed beside each other describe the same commits, which means the reach of a target
//! is as old as its cache until `--refresh` resolves the references again. The commit sits in every
//! row of the report for exactly that reason: a stale number is legible as a stale commit.

pub mod error;
pub mod fetch;
pub mod hunt;
pub mod reach;
pub mod report;
pub mod scope;

pub use error::HuntError;
pub use hunt::{Hit, Report, TargetRun, Unparsed, Unreadable, run};
pub use reach::{Reach, ReachSummary};
pub use scope::{Programme, SCOPE, Scope, Target, Tier};
