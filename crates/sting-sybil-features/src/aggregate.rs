//! How much a wallet moved, to how many people, and how many of them it had never met.
//!
//! The least chain-specific group of the vocabulary: counts and moments over a wallet's own
//! events. What is sent and what is received are kept apart throughout — averaging them together
//! describes a wallet that does not exist.

use std::collections::HashSet;

use sting_sybil_types::{OwnerId, TonAddress};

/// One thing that happened to a wallet.
pub struct WalletEvent {
    /// Who was on the other side.
    pub counterparty: OwnerId,
    /// How much moved.
    pub amount: u128,
    /// True when the wallet was the sender.
    pub sent: bool,
    /// Which jetton, or none for native TON.
    ///
    /// Carried rather than inferred: whatever produced the event knows which channel it came
    /// from, and reconstructing that from the amount would be a guess dressed as a fact.
    pub jetton_master: Option<TonAddress>,
}

impl WalletEvent {
    /// A native transfer.
    #[must_use]
    pub fn native(counterparty: OwnerId, amount: u128, sent: bool) -> Self {
        Self {
            counterparty,
            amount,
            sent,
            jetton_master: None,
        }
    }

    /// A jetton transfer.
    #[must_use]
    pub fn jetton(
        counterparty: OwnerId,
        amount: u128,
        sent: bool,
        jetton_master: TonAddress,
    ) -> Self {
        Self {
            counterparty,
            amount,
            sent,
            jetton_master: Some(jetton_master),
        }
    }
}

/// Counts, degrees, amounts and novelty for one wallet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AggregateFeatures {
    /// How many events in total.
    pub tx_count: u32,
    /// How many were incoming.
    pub in_deg: u32,
    /// How many were outgoing.
    pub out_deg: u32,
    /// Outgoing over incoming, smoothed by one.
    ///
    /// The smoothing is not cosmetic: a wallet that only sent would divide by zero, and the
    /// infinity that produced would go on to destroy every scaler downstream of it.
    pub deg_ratio: f64,
    /// How many distinct senders.
    pub n_unique_in: u32,
    /// How many distinct recipients.
    pub n_unique_out: u32,
    /// Mean amount sent.
    pub amount_mean_sent: f64,
    /// Variance of amounts sent.
    pub amount_var_sent: f64,
    /// Mean amount received.
    pub amount_mean_recv: f64,
    /// Variance of amounts received.
    pub amount_var_recv: f64,
    /// Smallest amount sent.
    pub amount_min_sent: f64,
    /// Largest amount sent.
    pub amount_max_sent: f64,
    /// Median amount sent.
    pub amount_median_sent: f64,
    /// Smallest amount received.
    pub amount_min_recv: f64,
    /// Largest amount received.
    pub amount_max_recv: f64,
    /// Median amount received.
    pub amount_median_recv: f64,
    /// How many distinct jettons were involved.
    pub n_jetton_types: u16,
    /// What share of the events were with somebody not seen before.
    pub novelty_ratio: f64,
}

/// Every figure above, from one wallet's events.
///
/// An empty input is a wallet that did nothing, not a failure: every count is zero and no
/// statistic is a division by nothing.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "amounts and counts become doubles because that is what a feature vector is; the precision a jetton amount loses past 2^53 is far below what a model reads"
)]
pub fn extract_aggregate(events: &[WalletEvent]) -> AggregateFeatures {
    let mut unique_in = HashSet::new();
    let mut unique_out = HashSet::new();
    let mut sent_amounts = Vec::new();
    let mut recv_amounts = Vec::new();
    let mut seen = HashSet::new();
    let mut novel_count = 0u32;
    let mut jetton_types = HashSet::new();

    for event in events {
        if event.sent {
            unique_out.insert(event.counterparty);
            sent_amounts.push(event.amount as f64);
        } else {
            unique_in.insert(event.counterparty);
            recv_amounts.push(event.amount as f64);
        }
        if seen.insert(event.counterparty) {
            novel_count += 1;
        }
        if let Some(master) = event.jetton_master {
            jetton_types.insert(master);
        }
    }

    let in_deg = u32::try_from(recv_amounts.len()).unwrap_or(u32::MAX);
    let out_deg = u32::try_from(sent_amounts.len()).unwrap_or(u32::MAX);

    AggregateFeatures {
        tx_count: u32::try_from(events.len()).unwrap_or(u32::MAX),
        in_deg,
        out_deg,
        deg_ratio: f64::from(out_deg) / (f64::from(in_deg) + 1.0),
        n_unique_in: u32::try_from(unique_in.len()).unwrap_or(u32::MAX),
        n_unique_out: u32::try_from(unique_out.len()).unwrap_or(u32::MAX),
        amount_mean_sent: mean_or_zero(&sent_amounts),
        amount_var_sent: variance_or_zero(&sent_amounts),
        amount_mean_recv: mean_or_zero(&recv_amounts),
        amount_var_recv: variance_or_zero(&recv_amounts),
        amount_min_sent: min_or_zero(&sent_amounts),
        amount_max_sent: max_or_zero(&sent_amounts),
        amount_median_sent: median_or_zero(&sent_amounts),
        amount_min_recv: min_or_zero(&recv_amounts),
        amount_max_recv: max_or_zero(&recv_amounts),
        amount_median_recv: median_or_zero(&recv_amounts),
        n_jetton_types: u16::try_from(jetton_types.len()).unwrap_or(u16::MAX),
        novelty_ratio: if events.is_empty() {
            0.0
        } else {
            f64::from(novel_count) / events.len() as f64
        },
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a count of one wallet's events is exact in a double"
)]
fn mean_or_zero(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a count of one wallet's events is exact in a double"
)]
fn variance_or_zero(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mean = mean_or_zero(xs);
    xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64
}

fn min_or_zero(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().copied().fold(f64::INFINITY, f64::min)
    }
}

fn max_or_zero(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }
}

/// The median, over values that came from unsigned amounts and therefore cannot be NaN.
fn median_or_zero(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut sorted = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[mid - 1], sorted[mid])
    } else {
        sorted[mid]
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "these are exact values a feature computes from small integers, not the result of accumulated arithmetic"
)]
mod tests {
    use super::*;
    use sting_sybil_types::TonAddress;

    fn owner(byte: u8) -> OwnerId {
        OwnerId(TonAddress::new(0, [byte; 32]))
    }

    fn jetton(byte: u8) -> TonAddress {
        TonAddress::new(0, [100 + byte; 32])
    }

    #[test]
    fn the_amount_statistics_read_off_known_values() {
        let events = vec![
            WalletEvent::native(owner(1), 10, true),
            WalletEvent::native(owner(2), 50, true),
            WalletEvent::native(owner(3), 30, true),
        ];
        let agg = extract_aggregate(&events);
        assert_eq!(agg.amount_min_sent, 10.0);
        assert_eq!(agg.amount_max_sent, 50.0);
        assert_eq!(agg.amount_median_sent, 30.0);
        // The receiving side is empty, and empty means zero rather than a NaN nothing downstream
        // would survive.
        assert_eq!(agg.amount_min_recv, 0.0);
        assert_eq!(agg.amount_max_recv, 0.0);
        assert_eq!(agg.amount_median_recv, 0.0);
    }

    #[test]
    fn an_even_count_takes_the_middle_two() {
        let events = vec![
            WalletEvent::native(owner(1), 10, false),
            WalletEvent::native(owner(2), 20, false),
            WalletEvent::native(owner(3), 30, false),
            WalletEvent::native(owner(4), 40, false),
        ];
        let agg = extract_aggregate(&events);
        assert_eq!(agg.amount_median_recv, 25.0);
    }

    #[test]
    fn distinct_jettons_are_counted_and_native_transfers_are_not() {
        let events = vec![
            WalletEvent::native(owner(1), 5, true),
            WalletEvent::jetton(owner(2), 10, true, jetton(1)),
            WalletEvent::jetton(owner(3), 20, false, jetton(1)),
            WalletEvent::jetton(owner(4), 30, true, jetton(2)),
        ];
        let agg = extract_aggregate(&events);
        assert_eq!(agg.n_jetton_types, 2);
    }

    #[test]
    fn a_wallet_that_only_moved_tons_holds_no_jettons() {
        let events = vec![WalletEvent::native(owner(1), 5, true)];
        assert_eq!(extract_aggregate(&events).n_jetton_types, 0);
    }
}
