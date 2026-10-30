//! Extraction-accuracy harness — analog of the security branch's
//! `sting-bench`, but the metric is schema-extraction accuracy per tier
//! (structural completeness for Tier 1, precision/recall for Tier 2), not
//! defect precision/recall. Full methodology:
//! `docs/domains/precision-sources.md`.
//!
//! `docs/domains/precision-sources.md`'s deliverable.

pub mod corpus;
pub mod report;
pub mod throughput;
pub mod tier1;
pub mod tier2;
