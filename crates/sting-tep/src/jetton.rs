//! TEP-74: the four jetton messages, and how a body of each is read.
//!
//! The opcodes are byte-exact from the standard. The bodies are read through
//! `tonlib-core`'s own cell parser rather than a bit reader written here: a
//! hand-rolled reader of `VarUInteger 16` is a defect that surfaces as a
//! transfer counted at the wrong size.
//!
//! **The addresses in these values are owners, never jetton wallets.** That is
//! the distinction the whole standard turns on: the envelope of a message
//! carries wallet contracts, and the body carries the people. A decoder that
//! blurred the two would produce a graph of the wrong thing that looks exactly
//! like the right one — hence [`sting_address::OwnerId`] and
//! [`sting_address::JettonWalletAddr`], which no conversion joins.

use num_bigint::BigUint;
use sting_address::TonAddress;
use tonlib_core::cell::{BagOfCells, CellParser};

/// `transfer`: an owner tells their own jetton wallet to send.
pub const OP_TRANSFER: u32 = 0x0f8a_7ea5;
/// `internal_transfer`: one jetton wallet to another. The only message of a
/// transfer that is always sent.
pub const OP_INTERNAL_TRANSFER: u32 = 0x178d_4519;
/// `transfer_notification`: the receiving wallet tells its owner — sent only
/// when the transfer carried a forward amount, so a graph built on it is a
/// graph missing everything that did not.
pub const OP_TRANSFER_NOTIFICATION: u32 = 0x7362_d09c;
/// `excesses`: change returned.
pub const OP_EXCESSES: u32 = 0xd532_76db;

/// What one jetton message says.
///
/// Only the fields this platform's consumers read: `response_destination`,
/// `forward_ton_amount`, `forward_payload` and `custom_payload` are in the
/// standard and are deliberately not read here. Naming what is skipped is the
/// difference between a partial decoder and a decoder that looks complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JettonOp {
    /// An owner to their own wallet. `destination` is the new owner directly,
    /// not a wallet contract, so it needs no resolution.
    Transfer {
        /// The correlating id, echoed by every message of this transfer.
        query_id: u64,
        /// How much, in the jetton's own units.
        amount: u128,
        /// Who receives — an owner.
        destination: TonAddress,
    },
    /// One wallet to another.
    InternalTransfer {
        /// The correlating id.
        query_id: u64,
        /// How much.
        amount: u128,
        /// Who sent — an owner.
        from: TonAddress,
    },
    /// A wallet to its owner, and only when a forward amount was attached.
    TransferNotification {
        /// The correlating id.
        query_id: u64,
        /// How much.
        amount: u128,
        /// Who sent — an owner.
        sender: TonAddress,
    },
    /// Change returned to whoever paid.
    Excesses {
        /// The correlating id.
        query_id: u64,
    },
}

impl JettonOp {
    /// The id every message of one transfer carries, which is what correlates
    /// them into a trace.
    #[must_use]
    pub const fn query_id(&self) -> u64 {
        match *self {
            Self::Transfer { query_id, .. }
            | Self::InternalTransfer { query_id, .. }
            | Self::TransferNotification { query_id, .. }
            | Self::Excesses { query_id } => query_id,
        }
    }
}

/// Reads `body_boc` as the message `op_code` claims it is.
///
/// The opcode is checked against the body's own first 32 bits rather than
/// trusted: the caller read it from an indexer's field, and a body that
/// disagrees with its envelope is a body nobody should act on.
///
/// # Errors
/// When the bytes are not a single-root bag of cells, when the body's opcode
/// disagrees with the one claimed, when a field does not have the shape the
/// standard gives it, or when the opcode is none of the four. Every one of
/// those is a message the caller skips rather than guesses at — an exotic
/// jetton, a truncated body, or another standard's message entirely.
pub fn decode_jetton_body(op_code: u32, body_boc: &[u8]) -> anyhow::Result<JettonOp> {
    let boc = BagOfCells::parse(body_boc)?;
    let root = boc.single_root()?;
    let mut parser = root.parser();

    let body_op_code = parser.load_u32(32)?;
    anyhow::ensure!(
        body_op_code == op_code,
        "the body's opcode ({body_op_code:#x}) is not the one claimed ({op_code:#x})"
    );

    match op_code {
        OP_TRANSFER => Ok(JettonOp::Transfer {
            query_id: parser.load_u64(64)?,
            amount: load_amount(&mut parser)?,
            destination: load_owner(&mut parser)?,
        }),
        OP_INTERNAL_TRANSFER => Ok(JettonOp::InternalTransfer {
            query_id: parser.load_u64(64)?,
            amount: load_amount(&mut parser)?,
            from: load_owner(&mut parser)?,
        }),
        OP_TRANSFER_NOTIFICATION => Ok(JettonOp::TransferNotification {
            query_id: parser.load_u64(64)?,
            amount: load_amount(&mut parser)?,
            sender: load_owner(&mut parser)?,
        }),
        OP_EXCESSES => Ok(JettonOp::Excesses {
            query_id: parser.load_u64(64)?,
        }),
        other => anyhow::bail!("opcode {other:#x} is none of TEP-74's four messages"),
    }
}

/// `amount:(VarUInteger 16)` — the same encoding as `Coins`, which
/// `load_coins` already implements: four bits of length, then that many bytes.
fn load_amount(parser: &mut CellParser) -> anyhow::Result<u128> {
    to_u128(&parser.load_coins()?)
}

fn load_owner(parser: &mut CellParser) -> anyhow::Result<TonAddress> {
    Ok(parser.load_address()?.into())
}

fn to_u128(value: &BigUint) -> anyhow::Result<u128> {
    let bytes = value.to_bytes_be();
    anyhow::ensure!(
        bytes.len() <= 16,
        "the amount does not fit in 128 bits: {} bytes",
        bytes.len()
    );
    let mut buffer = [0_u8; 16];
    buffer[16 - bytes.len()..].copy_from_slice(&bytes);
    Ok(u128::from_be_bytes(buffer))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use tonlib_core::cell::CellBuilder;

    use super::{
        JettonOp, OP_EXCESSES, OP_INTERNAL_TRANSFER, OP_TRANSFER, OP_TRANSFER_NOTIFICATION,
        decode_jetton_body,
    };
    use sting_address::TonAddress;

    fn transfer_body(op_code: u32, amount: u128, owner: TonAddress) -> Vec<u8> {
        let mut builder = CellBuilder::new();
        builder.store_u32(32, op_code).expect("opcode");
        builder.store_u64(64, 42).expect("query id");
        builder
            .store_coins(&num_bigint::BigUint::from(amount))
            .expect("amount");
        builder
            .store_address(&owner.into())
            .expect("the owner's address");
        let cell = builder.build().expect("builds");
        tonlib_core::cell::BagOfCells::from_root(cell)
            .serialize(false)
            .expect("serialises")
    }

    #[test]
    fn a_transfer_reads_back_the_amount_and_the_owner_it_was_built_with() {
        let owner = TonAddress::new(0, [0x11; 32]);
        let body = transfer_body(OP_TRANSFER, 1_000_000_000, owner);
        assert_eq!(
            decode_jetton_body(OP_TRANSFER, &body).expect("reads"),
            JettonOp::Transfer {
                query_id: 42,
                amount: 1_000_000_000,
                destination: owner,
            }
        );
    }

    #[test]
    fn the_other_three_messages_read_their_own_shapes() {
        let owner = TonAddress::new(-1, [0x22; 32]);
        let internal = transfer_body(OP_INTERNAL_TRANSFER, 7, owner);
        assert_eq!(
            decode_jetton_body(OP_INTERNAL_TRANSFER, &internal).expect("reads"),
            JettonOp::InternalTransfer {
                query_id: 42,
                amount: 7,
                from: owner,
            }
        );

        let notification = transfer_body(OP_TRANSFER_NOTIFICATION, 8, owner);
        assert_eq!(
            decode_jetton_body(OP_TRANSFER_NOTIFICATION, &notification).expect("reads"),
            JettonOp::TransferNotification {
                query_id: 42,
                amount: 8,
                sender: owner,
            }
        );

        let mut builder = CellBuilder::new();
        builder.store_u32(32, OP_EXCESSES).expect("opcode");
        builder.store_u64(64, 42).expect("query id");
        let cell = builder.build().expect("builds");
        let excesses = tonlib_core::cell::BagOfCells::from_root(cell)
            .serialize(false)
            .expect("serialises");
        assert_eq!(
            decode_jetton_body(OP_EXCESSES, &excesses).expect("reads"),
            JettonOp::Excesses { query_id: 42 }
        );
    }

    #[test]
    fn a_body_disagreeing_with_its_envelope_is_refused_rather_than_read() {
        let body = transfer_body(OP_TRANSFER, 1, TonAddress::new(0, [0x33; 32]));
        let error = decode_jetton_body(OP_INTERNAL_TRANSFER, &body)
            .expect_err("the claimed opcode is not the body's");
        let message = error.to_string();
        assert!(message.contains("0xf8a7ea5"), "{message}");
        assert!(message.contains("0x178d4519"), "{message}");
    }

    #[test]
    fn an_opcode_from_another_standard_is_an_error_rather_than_a_guess() {
        let mut builder = CellBuilder::new();
        builder.store_u32(32, 0xdead_beef).expect("opcode");
        let cell = builder.build().expect("builds");
        let body = tonlib_core::cell::BagOfCells::from_root(cell)
            .serialize(false)
            .expect("serialises");
        assert!(decode_jetton_body(0xdead_beef, &body).is_err());
    }

    #[test]
    fn bytes_that_are_not_a_bag_of_cells_are_refused() {
        assert!(decode_jetton_body(OP_TRANSFER, &[0x00, 0x01, 0x02]).is_err());
    }

    #[test]
    fn every_message_carries_the_id_that_correlates_it() {
        assert_eq!(JettonOp::Excesses { query_id: 9 }.query_id(), 9);
    }
}
