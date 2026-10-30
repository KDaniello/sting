//! Ground truth: the `manifest.yaml` format from `docs/domains/precision-sources.md`
//! §2, shared verbatim with the golden fixtures
//! (`docs/domains/detectors.md`). This module is the first actual *parser* for
//! that format; before it, the format existed only as documentation.

use std::path::Path;

use serde::Deserialize;

/// One `<name>.manifest.yaml` file's contents, exactly the shape documented
/// in `docs/domains/precision-sources.md`
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// The detector id expected to fire (e.g. `"GVR"`) — same string as
    /// [`sting_core::DetectorId`], but owned: manifests are loaded at
    /// runtime, not known at compile time.
    pub defect_class: String,
    /// Where the defect is, in the *original single file's own* line/column
    /// space (translated into a merged project's coordinate space by
    /// `crate::corpus` when the manifest's file is one of several sibling
    /// files merged into one project).
    pub location: ManifestLocation,
    /// Human-readable description (not consumed for matching, only for
    /// report output).
    #[allow(dead_code, reason = "kept for report readability, not matching logic")]
    pub description: String,
    /// Optional citation (`CertiK` writeup, etc.) — not consumed, kept for
    /// round-tripping the format the fixtures already commit to.
    #[serde(default)]
    #[allow(dead_code, reason = "kept for report readability, not matching logic")]
    pub source: Option<String>,
}

/// `manifest.location`.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestLocation {
    /// The file this location is in, as named in the manifest (a bare
    /// filename, matched against a project's sibling `.tolk` files — see
    /// `crate::corpus`).
    pub file: String,
    /// 1-indexed line, in `file`'s own text (not a merged project's).
    pub line: u32,
    /// 1-indexed column. Not used for matching (`docs/domains/precision-sources.md`
    /// — an exact column match is not required — kept for report
    /// output and for cross-checking against a `Finding`'s own column when
    /// present.
    pub column: u32,
}

/// Errors from [`load`].
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The file could not be read.
    #[error("failed to read manifest {path}: {source}")]
    Io {
        /// The manifest path that failed.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file's contents are not valid YAML, or don't match the expected
    /// shape.
    #[error("failed to parse manifest {path}: {source}")]
    Parse {
        /// The manifest path that failed.
        path: String,
        /// Underlying parse error.
        #[source]
        source: serde_norway::Error,
    },
}

/// Loads and parses one `.manifest.yaml` file.
///
/// # Errors
/// Returns [`ManifestError`] if the file cannot be read or is not valid YAML
/// matching [`Manifest`]'s shape.
pub fn load(path: &Path) -> Result<Manifest, ManifestError> {
    let text = std::fs::read_to_string(path).map_err(|source| ManifestError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_norway::from_str(&text).map_err(|source| ManifestError::Parse {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::Manifest;

    #[test]
    fn parses_a_real_block_02_manifest_shape() {
        // Byte-for-byte the shape committed in
        // corpus/fixtures/detectors/GVR/positive/01-shadowed-owner-simple.manifest.yaml
        let yaml = r#"
defect_class: GVR
location: { file: "01-shadowed-owner-simple.tolk", line: 12, column: 9 }
description: "local variable shadows storage field"
source: "CertiK: Secure Smart Contract Programming in FunC"
"#;
        let manifest: Manifest = serde_norway::from_str(yaml).expect("valid manifest");
        assert_eq!(manifest.defect_class, "GVR");
        assert_eq!(manifest.location.file, "01-shadowed-owner-simple.tolk");
        assert_eq!(manifest.location.line, 12);
        assert_eq!(manifest.location.column, 9);
    }

    #[test]
    fn source_field_is_optional() {
        let yaml = r#"
defect_class: LEP
location: { file: "x.tolk", line: 1, column: 1 }
description: "d"
"#;
        let manifest: Manifest = serde_norway::from_str(yaml).expect("valid manifest");
        assert!(manifest.source.is_none());
    }
}
