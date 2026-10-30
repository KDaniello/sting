//! Domain-reputation signal (`docs/domains/scamcheck.md`) — the weakest of
//! the three signals, **by design and stated
//! explicitly**, not just in the surrounding documentation: `SlowMist`'s
//! published analysis of `TonConnect`'s origin-forgery issue
//! (`https://slowmist.medium.com/risk-analysis-of-origin-forgery-in-the-tonconnect-sdk-e37c7d902b05`)
//! shows a wallet has no cryptographic guarantee that a `tonconnect-manifest.json`'s
//! claimed `url` is controlled by whoever actually served it — a malicious
//! dApp can simply put a real marketplace's domain in that field. This
//! module can therefore only ever answer "does this domain string *look
//! like* a known legitimate one", never "is this request really coming from
//! that domain" — [`DomainVerdict`] is named and documented to make that
//! limit impossible to miss at the call site, not softened into a plain
//! bool.
//!
//! **The starter domain list itself is genuinely uncertain, found and
//! disclosed during this module's own research, not assumed away**: MRKT
//! alone surfaced four different plausible web domains in search results
//! (`mrkt.xyz`, `mrkt.info`, `mrkt.land`, `mrkt.fun`) with no single
//! authoritative source distinguishing "the" official one from mirrors —
//! this is itself a real instance of the exact ambiguity this signal is
//! weakest against, kept in the list with that uncertainty stated rather
//! than silently picking one and presenting it as confirmed.

use thiserror::Error;

/// One legitimate marketplace/venue domain, with a confidence note — never
/// presented to a caller as uniformly "confirmed", per the module doc
/// comment.
#[derive(Debug, Clone, Copy)]
pub struct KnownDomain {
    /// The venue's human name.
    pub venue: &'static str,
    /// The domain itself (no scheme, no path — bare host, e.g.
    /// `"getgems.io"`).
    pub domain: &'static str,
    /// Whether this exact domain was corroborated by more than one
    /// independent source during this module's own research, or is a
    /// single-source, lower-confidence candidate (see the module doc
    /// comment's MRKT example).
    pub multi_source_confirmed: bool,
}

/// Starter list — **not exhaustive, not uniformly confirmed**, see the
/// module doc comment. `docs/domains/scamcheck.md` leaves
/// "exact legitimate domains" as an explicit open question; this is that
/// question's first, disclosed-uncertain answer, not its final resolution.
pub const KNOWN_DOMAINS: &[KnownDomain] = &[
    KnownDomain {
        venue: "Getgems",
        domain: "getgems.io",
        multi_source_confirmed: true,
    },
    KnownDomain {
        venue: "Fragment (official Telegram Gift venue)",
        domain: "fragment.com",
        multi_source_confirmed: true,
    },
    KnownDomain {
        venue: "Portals",
        domain: "portalsmarket.co",
        multi_source_confirmed: false,
    },
    KnownDomain {
        venue: "Tonnel",
        domain: "tonnelrelayer.com",
        multi_source_confirmed: false,
    },
    KnownDomain {
        venue: "MRKT",
        domain: "mrkt.xyz",
        multi_source_confirmed: false,
    },
    KnownDomain {
        venue: "MRKT",
        domain: "mrkt.info",
        multi_source_confirmed: false,
    },
    KnownDomain {
        venue: "MRKT",
        domain: "mrkt.land",
        multi_source_confirmed: false,
    },
    KnownDomain {
        venue: "MRKT",
        domain: "mrkt.fun",
        multi_source_confirmed: false,
    },
];

/// A domain string was empty or otherwise not comparable.
#[derive(Debug, Error)]
#[error("domain must be non-empty")]
pub struct EmptyDomainError;

/// The three-tier verdict — see the module doc comment for why this is
/// never a bare bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainVerdict {
    /// Exact, case-insensitive match against a [`KNOWN_DOMAINS`] entry.
    /// **Not proof the request truly originated there** — see the module
    /// doc comment's `SlowMist` citation.
    ExactMatch {
        /// Which known venue this exactly matches.
        venue: &'static str,
    },
    /// Close (small edit distance) but not exact — the shape of a
    /// typosquat, e.g. a homoglyph or an added/dropped hyphen.
    LooksLikeTyposquatOf {
        /// The known venue this domain resembles.
        venue: &'static str,
        /// The known domain it resembles.
        known_domain: &'static str,
        /// The Levenshtein edit distance between the two.
        distance: usize,
    },
    /// Neither an exact nor a close match to anything in the starter list —
    /// **not** evidence of anything, most real domains on the internet are
    /// simply not gift/NFT marketplaces at all.
    Unrecognized,
}

/// Levenshtein edit distance between two byte-lowercased strings — a
/// standard O(n·m) DP, no external crate needed for a comparison this
/// small (domain strings, not documents).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut row: Vec<usize> = (0..=m).collect();
    for i in 1..=n {
        let mut prev_diag = row[0];
        row[0] = i;
        for j in 1..=m {
            let temp = row[j];
            row[j] = if a[i - 1] == b[j - 1] {
                prev_diag
            } else {
                1 + row[j].min(row[j - 1]).min(prev_diag)
            };
            prev_diag = temp;
        }
    }
    row[m]
}

/// Below this edit distance (and not an exact match), a domain is
/// considered typosquat-shaped rather than merely unrelated — chosen as a
/// first, disclosed product guess (same honesty as `score.rs`'s own
/// weights, per that module's own commit message: "explicitly documented
/// as a first product estimate, not derived from external ground truth"),
/// not derived from a labeled typosquat dataset. Domains shorter than this
/// threshold could false-positive against short known domains; callers
/// comparing very short strings should sanity-check results themselves.
const TYPOSQUAT_DISTANCE_THRESHOLD: usize = 3;

/// Checks `domain` (bare host, no scheme/path — caller's responsibility to
/// strip those) against [`KNOWN_DOMAINS`].
///
/// # Errors
/// Returns [`EmptyDomainError`] if `domain` is empty after trimming.
pub fn check_domain(domain: &str) -> Result<DomainVerdict, EmptyDomainError> {
    let normalized = domain.trim().to_lowercase();
    if normalized.is_empty() {
        return Err(EmptyDomainError);
    }

    if let Some(known) = KNOWN_DOMAINS
        .iter()
        .find(|k| k.domain.eq_ignore_ascii_case(&normalized))
    {
        return Ok(DomainVerdict::ExactMatch { venue: known.venue });
    }

    let mut closest: Option<(&KnownDomain, usize)> = None;
    for known in KNOWN_DOMAINS {
        let distance = levenshtein(&normalized, known.domain);
        if distance <= TYPOSQUAT_DISTANCE_THRESHOLD
            && closest.is_none_or(|(_, best)| distance < best)
        {
            closest = Some((known, distance));
        }
    }

    Ok(match closest {
        Some((known, distance)) => DomainVerdict::LooksLikeTyposquatOf {
            venue: known.venue,
            known_domain: known.domain,
            distance,
        },
        None => DomainVerdict::Unrecognized,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::{DomainVerdict, check_domain, levenshtein};

    #[test]
    fn exact_match_is_case_insensitive() {
        let verdict = check_domain("GetGems.io").expect("check");
        assert_eq!(verdict, DomainVerdict::ExactMatch { venue: "Getgems" });
    }

    #[test]
    fn a_documented_real_phishing_shape_is_flagged_as_typosquat() {
        // `docs/POSITION.md` documents
        // real phishing kits impersonating Fragment — this is that shape
        // (an added hyphen), not a fabricated test string.
        let verdict = check_domain("fragm-ent.com").expect("check");
        assert!(matches!(
            verdict,
            DomainVerdict::LooksLikeTyposquatOf {
                venue: "Fragment (official Telegram Gift venue)",
                ..
            }
        ));
    }

    #[test]
    fn an_unrelated_domain_is_unrecognized_not_flagged() {
        let verdict = check_domain("example.com").expect("check");
        assert_eq!(verdict, DomainVerdict::Unrecognized);
    }

    #[test]
    fn empty_domain_is_a_clean_error() {
        assert!(check_domain("   ").is_err());
    }

    #[test]
    fn levenshtein_matches_known_values() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("getgems.io", "getgems.io"), 0);
    }
}
