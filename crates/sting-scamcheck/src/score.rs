//! Composite scoring (`docs/domains/scamcheck.md`) —
//! combines whichever of the other modules' signals a caller has actually
//! computed into one report and a coarse risk level.
//!
//! **Weights/thresholds below are this module's own first-cut product
//! judgment, not derived from any external ground truth** — flagged
//! honestly, the same way `sting-pipeline::ScoringController`'s detector
//! weights (`docs/ARCHITECTURE.md`) are an explicit,
//! tunable product choice, not a provable constant. §6 of the design
//! document this implements calls for validating against a labeled
//! held-out set (TVL-collapse-based ground truth) before trusting these
//! numbers in production — that validation has not been done; these
//! thresholds are a starting point rather than a finished calibration.
//!
//! **Every field is `Option`, on purpose**: a caller may not have run
//! every signal (e.g. `ston_fi`'s LP-concentration primitives require a
//! caller-supplied candidate address to check, per that module's own
//! scope note — there is no "the" LP holder resolved automatically). A
//! missing signal contributes zero risk points rather than being treated
//! as either "clean" or "suspicious" — an honest "unknown", not a guess in
//! either direction.
//!
//! **Deliberate design choice, confirmed on review, not an unexamined
//! artifact**: unweighted summation means three independently-weak
//! signals (e.g. non-renounced mint rights + moderate holder
//! concentration + moderate LP concentration, `1+1+1=3`) reach the same
//! `RiskLevel::High` as a single reverted honeypot probe (`3` on its
//! own). This is intentional for a first cut — "several independent weak
//! signs" and "one strong sign" are both treated as "stop treating this
//! as routine", the same coarse bar — rather than trying to encode a
//! finer distinction the data in §6 hasn't yet justified.

/// One assembled report — inputs computed elsewhere in this crate, never
/// re-derived here.
#[derive(Debug, Clone, Default)]
pub struct ScamCheckReport {
    /// From [`crate::mint_rights::MintRightsSignal::renounced`].
    pub mint_rights_renounced: Option<bool>,
    /// From [`crate::honeypot::ProbeOutcome`] — `true` if the probe
    /// reverted *or* completed silently with zero output actions
    /// (`ProbeOutcome::Reverted`/`ExecutedSilently`; consistent with, not
    /// proof of, a transfer restriction — see that enum's own doc
    /// comments). `None` when every tested holder came back
    /// `ProbeOutcome::PrecompiledContractUnsupported` (or the signal was
    /// never run at all) — an honest "couldn't verify", not a "clean"
    /// `Some(false)`.
    pub honeypot_reverted: Option<bool>,
    /// From [`crate::holder_concentration::HolderConcentrationSignal::top_holders_share`].
    pub top_holders_share: Option<f64>,
    /// A candidate address's share of a STON.fi pool's own LP supply —
    /// computed by a caller from [`crate::ston_fi::pool_total_supply`] and
    /// a resolved LP-wallet balance (that cross-contract balance fetch is
    /// itself out of `ston_fi`'s scope, see that module's own doc
    /// comment) — this field only records the resulting ratio, however
    /// obtained.
    pub lp_owner_share: Option<f64>,
}

/// A coarse risk level — not a substitute for reading the underlying
/// signals, which carry far more information than three buckets can.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    /// No signal present triggered a risk point.
    Low,
    /// Some risk points, below the `High` threshold.
    Medium,
    /// Enough risk points that this should not be treated as routine.
    High,
}

impl ScamCheckReport {
    /// Sums risk points across whichever signals are present, per the
    /// weights documented on this module's own doc comment (honeypot
    /// weighted highest — it is the only signal that comes from directly
    /// observing restrictive *behavior*, not an indirect proxy for it).
    #[must_use]
    pub fn risk_points(&self) -> u32 {
        let mut points = 0;
        if self.mint_rights_renounced == Some(false) {
            points += 1;
        }
        if self.honeypot_reverted == Some(true) {
            points += 3;
        }
        points += Self::share_points(self.top_holders_share);
        points += Self::share_points(self.lp_owner_share);
        points
    }

    /// Both concentration ratios use the same coarse bucketing: `> 0.5`
    /// (a single party could unilaterally move the majority of supply/
    /// liquidity) scores highest, `> 0.2` scores lower, anything else
    /// scores nothing.
    fn share_points(share: Option<f64>) -> u32 {
        match share {
            Some(share) if share > 0.5 => 2,
            Some(share) if share > 0.2 => 1,
            _ => 0,
        }
    }

    /// The coarse [`RiskLevel`] for [`Self::risk_points`].
    #[must_use]
    pub fn risk_level(&self) -> RiskLevel {
        match self.risk_points() {
            0 => RiskLevel::Low,
            1..=2 => RiskLevel::Medium,
            _ => RiskLevel::High,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RiskLevel, ScamCheckReport};

    #[test]
    fn no_signals_present_is_low_risk_not_a_guess() {
        let report = ScamCheckReport::default();
        assert_eq!(report.risk_points(), 0);
        assert_eq!(report.risk_level(), RiskLevel::Low);
    }

    #[test]
    fn a_reverted_honeypot_probe_alone_reaches_high() {
        let report = ScamCheckReport {
            honeypot_reverted: Some(true),
            ..ScamCheckReport::default()
        };
        assert_eq!(report.risk_points(), 3);
        assert_eq!(report.risk_level(), RiskLevel::High);
    }

    #[test]
    fn non_renounced_mint_rights_alone_is_only_medium() {
        let report = ScamCheckReport {
            mint_rights_renounced: Some(false),
            ..ScamCheckReport::default()
        };
        assert_eq!(report.risk_points(), 1);
        assert_eq!(report.risk_level(), RiskLevel::Medium);
    }

    #[test]
    fn renounced_mint_rights_contributes_no_points() {
        let report = ScamCheckReport {
            mint_rights_renounced: Some(true),
            ..ScamCheckReport::default()
        };
        assert_eq!(report.risk_points(), 0);
    }

    #[test]
    fn high_holder_concentration_alone_is_medium() {
        let report = ScamCheckReport {
            top_holders_share: Some(0.6),
            ..ScamCheckReport::default()
        };
        assert_eq!(report.risk_points(), 2);
        assert_eq!(report.risk_level(), RiskLevel::Medium);
    }

    #[test]
    fn moderate_holder_concentration_scores_less_than_high() {
        let report = ScamCheckReport {
            top_holders_share: Some(0.3),
            ..ScamCheckReport::default()
        };
        assert_eq!(report.risk_points(), 1);
    }

    #[test]
    fn low_holder_concentration_scores_nothing() {
        let report = ScamCheckReport {
            top_holders_share: Some(0.1),
            ..ScamCheckReport::default()
        };
        assert_eq!(report.risk_points(), 0);
    }

    /// Two independently-weak signals (`1 + 1 = 2`) stay `Medium` — the
    /// 2/3 boundary tested from the *compounding* direction, not just via
    /// a single dominant signal (`high_holder_concentration_alone_is_medium`
    /// reaches 2 from one source; this reaches it from two).
    #[test]
    fn two_weak_signals_compound_to_medium_not_high() {
        let report = ScamCheckReport {
            mint_rights_renounced: Some(false), // 1
            top_holders_share: Some(0.3),       // 1
            ..ScamCheckReport::default()
        };
        assert_eq!(report.risk_points(), 2);
        assert_eq!(report.risk_level(), RiskLevel::Medium);
    }

    /// Three independently-weak signals (`1 + 1 + 1 = 3`) reach `High` —
    /// the same level a single reverted honeypot probe reaches alone,
    /// confirmed as the module's deliberate design choice (see the module
    /// doc comment), not tested anywhere else in this file.
    #[test]
    fn three_weak_signals_compound_to_high_same_as_one_strong_signal() {
        let report = ScamCheckReport {
            mint_rights_renounced: Some(false), // 1
            top_holders_share: Some(0.3),       // 1
            lp_owner_share: Some(0.3),          // 1
            ..ScamCheckReport::default()
        };
        assert_eq!(report.risk_points(), 3);
        assert_eq!(report.risk_level(), RiskLevel::High);
    }

    #[test]
    fn multiple_suspicious_signals_compound_to_high() {
        let report = ScamCheckReport {
            mint_rights_renounced: Some(false),
            honeypot_reverted: Some(false),
            top_holders_share: Some(0.55),
            lp_owner_share: Some(0.9),
        };
        // 1 (mint) + 0 (honeypot clean) + 2 (holders) + 2 (LP) = 5
        assert_eq!(report.risk_points(), 5);
        assert_eq!(report.risk_level(), RiskLevel::High);
    }
}
