//! Controllers — orchestrates extract → normalize → verify → serve across
//! `sting-abi-tolk`/`sting-abi-tact`/`sting-abi-func`/`sting-abi-registry`,
//! testable without an HTTP server (`docs/domains/service.md`).
//!
//! `ExtractController` is implemented; `RegistryController`'s pipeline wiring
//! is not.

mod extract_controller;

pub use extract_controller::{ExtractController, LanguageHint, PipelineError};
