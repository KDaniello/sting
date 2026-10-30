//! Parses the standard `Retry-After` HTTP header, so that a server stating
//! its own delay is respected rather than guessed at (`docs/domains/chain.md`).
//!
//! Only the delay-seconds form (`Retry-After: 120`) is read, not the HTTP-date
//! form (`Retry-After: Fri, 31 Dec 2026 23:59:59 GMT`): both providers send
//! the numeric form when they send the header at all. Toncenter currently
//! sends none on its `429`s — confirmed by reading real response headers — so
//! this parser exists for the path to be real and tested rather than because
//! that provider needs it today.

use std::time::Duration;

/// Extracts and parses a `Retry-After` header from `headers`, if present and
/// in the numeric-seconds form.
pub(crate) fn parse(headers: &ureq::http::HeaderMap) -> Option<Duration> {
    let value = headers.get("Retry-After")?;
    let text = value.to_str().ok()?;
    let seconds: u64 = text.trim().parse().ok()?;
    Some(Duration::from_secs(seconds))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::parse;

    #[test]
    fn parses_a_present_numeric_retry_after_header() {
        let mut headers = ureq::http::HeaderMap::new();
        headers.insert("Retry-After", "120".parse().expect("valid header value"));
        assert_eq!(parse(&headers), Some(std::time::Duration::from_secs(120)));
    }

    #[test]
    fn returns_none_when_the_header_is_absent() {
        let headers = ureq::http::HeaderMap::new();
        assert_eq!(parse(&headers), None);
    }

    #[test]
    fn returns_none_for_the_http_date_form_not_supported_by_this_parser() {
        let mut headers = ureq::http::HeaderMap::new();
        headers.insert(
            "Retry-After",
            "Fri, 31 Dec 2026 23:59:59 GMT"
                .parse()
                .expect("valid header value"),
        );
        assert_eq!(parse(&headers), None);
    }
}
