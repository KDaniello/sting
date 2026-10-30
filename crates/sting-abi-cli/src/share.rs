//! One reading of "what share of that is this", used by every command here that reports one.
//!
//! Three commands measured shares and each carried its own copy of the same six lines, down to the
//! same lint exception. A rounding rule copied three times is three rules that agree until one of
//! them is edited.

/// A share in per cent, to one decimal, and zero when there is nothing to take a share of.
///
/// Both counts are counts of things this tree has read — files, entries, messages — so the
/// precision `as f64` loses above 2^52 is a bound none of them reaches.
#[allow(clippy::cast_precision_loss)]
pub fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    (part as f64 / whole as f64 * 1000.0).round() / 10.0
}
