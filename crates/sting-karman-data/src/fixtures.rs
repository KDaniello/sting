use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

/// A fixture could not be read or written. The path is in the message: a data error a user
/// cannot locate is a support ticket, not an error message.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// The file could not be reached, read or replaced.
    #[error("cannot read fixture {path}: {source}")]
    Io {
        /// The path the operation was attempted on.
        path: String,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// The bytes were reachable but were not the value the caller asked for.
    #[error("fixture {path} is not valid JSON for the expected type: {source}")]
    Schema {
        /// The path the bytes came from.
        path: String,
        /// Where the shape stopped matching.
        #[source]
        source: serde_json::Error,
    },
}

/// Reads one typed fixture from a JSON file.
///
/// # Errors
/// [`FixtureError`] when the file cannot be read, or holds something that is not this fixture.
pub fn read_json_fixture<T: DeserializeOwned>(path: &Path) -> Result<T, FixtureError> {
    let display = path.display().to_string();
    let text = fs::read_to_string(path).map_err(|source| FixtureError::Io {
        path: display.clone(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| FixtureError::Schema {
        path: display,
        source,
    })
}

/// Writes one typed fixture as pretty-printed JSON, with the crash-safe discipline of
/// [`write_text_artifact`].
///
/// # Errors
/// [`FixtureError`] when the value cannot be rendered, or the file cannot be written.
pub fn write_json_fixture<T: Serialize>(path: &Path, value: &T) -> Result<(), FixtureError> {
    let display = path.display().to_string();
    let text = serde_json::to_string_pretty(value).map_err(|source| FixtureError::Schema {
        path: display.clone(),
        source,
    })?;
    write_text_artifact(path, &text)
}

/// Writes a text artifact — a report, a rendered summary — creating the parent directory when
/// absent.
///
/// The write goes to a temporary neighbour and is then renamed over the target, so a crash
/// leaves either the previous file or the new one, never half of either.
///
/// # Errors
/// [`FixtureError::Io`] when the directory cannot be made, the neighbour cannot be written, or
/// the rename does not happen.
pub fn write_text_artifact(path: &Path, text: &str) -> Result<(), FixtureError> {
    let display = path.display().to_string();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| FixtureError::Io {
            path: display.clone(),
            source,
        })?;
    }
    let tmp = temp_neighbour(path);
    fs::write(&tmp, text).map_err(|source| FixtureError::Io {
        path: display.clone(),
        source,
    })?;
    fs::rename(&tmp, path).map_err(|source| FixtureError::Io {
        path: display,
        source,
    })
}

/// The staging path for one target: the target's own name with `.tmp` appended.
///
/// Appended rather than substituted for the extension, because a report is written as `x.md`
/// and as `x.json` into the same directory — replacing the extension would give both writes the
/// same staging file and let one overwrite the other's bytes.
fn temp_neighbour(path: &Path) -> PathBuf {
    let mut name = path.file_name().map_or_else(OsString::new, OsString::from);
    name.push(".tmp");
    path.with_file_name(name)
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        value: i64,
    }

    /// A directory of this test's own, so a parallel run of the suite cannot collide with it.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("karman-fixture-tests").join(name);
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_fixture_round_trips() {
        let path = scratch("round-trip").join("sample.json");
        write_json_fixture(&path, &Sample { value: 7 }).unwrap();
        assert_eq!(
            read_json_fixture::<Sample>(&path).unwrap(),
            Sample { value: 7 }
        );
    }

    #[test]
    fn a_missing_parent_directory_is_created() {
        let path = scratch("nested").join("a").join("b").join("sample.json");
        write_json_fixture(&path, &Sample { value: 1 }).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn two_artifacts_sharing_a_stem_do_not_share_a_staging_file() {
        let dir = scratch("same-stem");
        write_text_artifact(&dir.join("report.md"), "markdown").unwrap();
        write_text_artifact(&dir.join("report.json"), "{}").unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("report.md")).unwrap(),
            "markdown"
        );
        assert_eq!(fs::read_to_string(dir.join("report.json")).unwrap(), "{}");
    }

    #[test]
    fn no_staging_file_survives_a_successful_write() {
        let dir = scratch("no-leftovers");
        write_text_artifact(&dir.join("report.md"), "markdown").unwrap();
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn a_fixture_of_the_wrong_shape_is_a_schema_error_naming_the_path() {
        let path = scratch("wrong-shape").join("sample.json");
        write_text_artifact(&path, "{\"value\": \"not a number\"}").unwrap();
        let error = read_json_fixture::<Sample>(&path).unwrap_err();
        assert!(matches!(&error, FixtureError::Schema { .. }));
        assert!(error.to_string().contains("sample.json"));
    }

    #[test]
    fn an_absent_fixture_is_an_io_error_naming_the_path() {
        let path = scratch("absent").join("sample.json");
        let error = read_json_fixture::<Sample>(&path).unwrap_err();
        assert!(matches!(&error, FixtureError::Io { .. }));
        assert!(error.to_string().contains("sample.json"));
    }
}
