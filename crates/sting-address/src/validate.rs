//! The cheap check that runs before an address becomes part of a URL.
//!
//! Not a parser and not a substitute for one: it verifies no checksum and
//! decides nothing about which form the text is in. What it rejects is the
//! character that cannot appear in any real address and can change the request
//! built around it — an unescaped `/` splitting a path segment, a `?` or `&`
//! adding a query parameter, a `#` truncating the path, whitespace ending the
//! request line.
//!
//! It exists because the alternative was measured: an address was interpolated
//! into a URL with no local check, and a typo came back as a status code after
//! a real round trip rather than as a refusal naming the address.

/// The union of every character either address form uses: raw, whose workchain
/// may be negative, and friendly, in base64 or base64url with padding.
fn is_address_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, ':' | '+' | '/' | '=' | '-' | '_')
}

/// Text refused before it could become part of a request.
#[derive(Debug, thiserror::Error)]
#[error(
    "{address:?} is not a valid TON address (expected raw `<workchain>:<64 hex chars>` \
     or a base64/base64url \"friendly\" address)"
)]
pub struct InvalidAddress {
    /// The text that was refused.
    pub address: String,
}

/// Refuses `address` locally when it is empty or carries a character no real
/// address form has.
///
/// # Errors
/// [`InvalidAddress`], carrying the text, so the message says what was wrong
/// rather than that something was.
pub fn validate_text(address: &str) -> Result<(), InvalidAddress> {
    if address.is_empty() || !address.chars().all(is_address_char) {
        return Err(InvalidAddress {
            address: address.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::validate_text;

    #[test]
    fn accepts_a_real_raw_address() {
        assert!(
            validate_text("0:83DFD552E63729B472FCBCC8C45EBCC6691702558B68EC7527E1BA403A0F31A8")
                .is_ok()
        );
    }

    #[test]
    fn accepts_a_negative_workchain() {
        assert!(
            validate_text("-1:0000000000000000000000000000000000000000000000000000000000000000")
                .is_ok()
        );
    }

    #[test]
    fn accepts_both_friendly_alphabets() {
        assert!(validate_text("EQD/+abc123+/def456==").is_ok());
        assert!(validate_text("EQD_-abc123_-def456").is_ok());
    }

    #[test]
    fn refuses_an_empty_address() {
        assert!(validate_text("").is_err());
    }

    #[test]
    fn refuses_the_characters_that_would_change_the_request() {
        // The concrete cases this check exists for: a second query parameter,
        // a truncated path, a request line that ends early.
        for address in ["0:aaaa?extra=1", "0:aaaa#frag", "0:aaaa bbbb", "0:aaaa&x=1"] {
            assert!(validate_text(address).is_err(), "{address}");
        }
    }

    #[test]
    fn the_refusal_carries_the_text_it_refused() {
        let error = validate_text("0:aaaa?extra=1").expect_err("refused");
        assert!(error.to_string().contains("0:aaaa?extra=1"));
    }
}
