use thiserror::Error;

/// The golden baseline could not answer for this key.
#[derive(Debug, Error)]
pub enum GoldenError {
    /// The baseline file itself is unreadable, so no key in it can be trusted.
    #[error("baseline is not valid TOML: {0}")]
    Parse(String),
    /// Nothing is pinned under that key — the measurement was never recorded, or was renamed
    /// without its reader.
    #[error("baseline has no key `{0}`")]
    Missing(String),
    /// The key holds something that is not a golden value; only integers compare exactly.
    #[error("baseline key `{0}` is not an integer")]
    NotAnInteger(String),
}

/// Reads one expected integer from baseline TOML text.
///
/// The caller owns the I/O: this crate never touches the filesystem, so tests pass the file in
/// with `include_str!`. Keys are dotted paths into tables: `"cpamm.golden_token_reserve"`.
///
/// # Errors
/// [`GoldenError`]: the text is not a document, the key names nothing, or what it names is not
/// an integer — only integers compare exactly, which is the whole point of a golden value.
pub fn expected(baseline_toml: &str, key: &str) -> Result<i128, GoldenError> {
    // `from_str` rather than `parse`: from toml 1.0 the `FromStr` on `Value` reads one *value*,
    // so a document opening with a table header is a value followed by content nobody asked for.
    let root: toml::Value = toml::from_str(baseline_toml)
        .map_err(|e: toml::de::Error| GoldenError::Parse(e.to_string()))?;
    let mut node = &root;
    for part in key.split('.') {
        node = node
            .get(part)
            .ok_or_else(|| GoldenError::Missing(key.to_string()))?;
    }
    node.as_integer()
        .map(i128::from)
        .ok_or_else(|| GoldenError::NotAnInteger(key.to_string()))
}

/// Asserts a measured value against the committed baseline.
///
/// A golden mismatch is information, not noise: either the change broke the mechanism, or it
/// legitimately moved a measured number — and then the baseline changes in the same commit,
/// with the reason in the message.
///
/// # Panics
///
/// Panics when the baseline cannot be read or the value differs from it.
pub fn assert_golden(baseline_toml: &str, key: &str, actual: i128) {
    let want = expected(baseline_toml, key).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        actual, want,
        "golden value `{key}` diverged from benches/baseline.toml"
    );
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE: &str = "[cpamm]\ngolden_token_reserve = 990255899\nname = \"cpamm\"\n";

    #[test]
    fn a_dotted_key_reaches_into_a_table() {
        assert_eq!(
            expected(BASELINE, "cpamm.golden_token_reserve").unwrap(),
            990_255_899
        );
    }

    #[test]
    fn a_missing_key_names_itself() {
        let error = expected(BASELINE, "cpamm.absent").unwrap_err();
        assert!(matches!(error, GoldenError::Missing(key) if key == "cpamm.absent"));
    }

    #[test]
    fn a_non_integer_value_is_not_a_golden_value() {
        assert!(matches!(
            expected(BASELINE, "cpamm.name"),
            Err(GoldenError::NotAnInteger(_))
        ));
    }

    #[test]
    fn a_broken_baseline_fails_before_any_comparison() {
        assert!(matches!(
            expected("= not toml", "any.key"),
            Err(GoldenError::Parse(_))
        ));
    }

    #[test]
    #[should_panic(expected = "diverged from benches/baseline.toml")]
    fn a_moved_value_fails_loudly() {
        assert_golden(BASELINE, "cpamm.golden_token_reserve", 0);
    }
}
