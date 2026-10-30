//! How alike two wallets' company is.
//!
//! Similarity over a *set* of counterparties, so the order of the interactions does not matter.
//! Where the order is the signal — the same sequence of actions repeated — the measure would be an
//! edit distance instead, and that is a different feature rather than a variant of this one.

use std::collections::HashSet;
use std::hash::{BuildHasher, Hash};

/// The size of the intersection over the size of the union, between zero and one.
///
/// Two empty sets score zero rather than one: knowing nobody is not something two wallets have in
/// common, and scoring it as perfect similarity would make every fresh address a twin of every
/// other fresh address.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "a count of counterparties is exact in a double, and the result is a ratio in [0,1]"
)]
pub fn jaccard<T: Eq + Hash, S: BuildHasher>(a: &HashSet<T, S>, b: &HashSet<T, S>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    intersection as f64 / union as f64
}

/// Every pair inside each group, and no pair across groups.
///
/// Comparing all pairs globally is quadratic in the number of wallets, which at the size of a
/// chain is not a slow computation but an impossible one. A group — a component of the funding
/// graph — is where a shared set of counterparties would mean something anyway.
#[must_use]
pub fn pairwise_jaccard_within_groups<T: Eq + Hash + Clone, S: BuildHasher>(
    groups: &[Vec<HashSet<T, S>>],
) -> Vec<Vec<f64>> {
    groups
        .iter()
        .map(|group| {
            let mut result = Vec::with_capacity(group.len() * group.len().saturating_sub(1) / 2);
            for i in 0..group.len() {
                for j in (i + 1)..group.len() {
                    result.push(jaccard(&group[i], &group[j]));
                }
            }
            result
        })
        .collect()
}
