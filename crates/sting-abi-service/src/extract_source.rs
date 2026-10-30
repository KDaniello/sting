//! Turning an HTTP request's inline `source` text into the
//! `(project_dir, entrypoint)` pair every [`sting_abi_core::Extractor`]
//! actually needs (`docs/API.md`'s
//! `sting-abi-cli` does the same translation, but from a real file path —
//! HTTP has no filename at all, just `{source, lang}`).

use sting_abi_core::SourceLanguage;

/// Picks a synthetic entrypoint filename for source received inline over
/// HTTP. Never used for language *detection* — every route in this crate
/// requires an explicit `lang`, see `routes/extract.rs`'s own doc comment
/// for why `"auto"` is rejected outright here, unlike `sting-abi-cli`
/// where a real filename to detect from actually exists.
#[must_use]
pub fn synthetic_entrypoint_name(lang: SourceLanguage) -> &'static str {
    match lang {
        SourceLanguage::Tolk => "contract.tolk",
        SourceLanguage::Tact => "contract.tact",
        SourceLanguage::Func => "contract.fc",
    }
}
