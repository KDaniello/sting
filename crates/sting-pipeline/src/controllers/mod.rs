//! Controllers: the orchestration layer over detectors/emulator/reporting
//! (`docs/ARCHITECTURE.md`). No controller here knows about
//! `clap` — every input/output is a plain Rust type, testable directly.

pub mod bridge;
pub mod doctor;
pub mod fix;
pub mod hypothesis_spec;
pub mod lint;
pub mod pool;
pub mod report;
pub mod scoring;
