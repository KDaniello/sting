//! Builds the two `BoC`s [`crate::TransactionEmulator::emulate_transaction`]
//! needs that this crate had no way to construct before: a `ShardAccount`
//! (an account's on-chain state, not just its code+data — `TvmEmulator`
//! only ever needed the latter) and an inbound `Message` (a real envelope
//! around a body — `CommonMsgInfo` + optional `init` + `body`, not just a
//! bare body the way `TvmEmulator::send_internal_message` takes it).
//!
//! Exists because `TvmEmulator::send_internal_message` does not populate
//! the full TVM `c7` "in-message parameters" tuple — in particular
//! `GETORIGINALFWDFEE` (`in.originalForwardFee` on the Tolk side) reads
//! uninitialized state there and throws exit code 7 ("intermediate value is
//! not a tuple"), confirmed empirically against a real deployed jetton
//! wallet while investigating a real finding ('s real-target audit work). Any contract logic that inspects
//! its own forward fee — the standard TEP-74 gas-sufficiency check among
//! them — needs the fuller `TransactionEmulator` path this module feeds.
//!
//! TL-B shapes below are transcribed directly from
//! `ton-blockchain/ton`'s `crypto/block/block.tlb` (`ShardAccount`,
//! `Account`, `StorageInfo`, `StorageUsed`, `AccountStorage`,
//! `CurrencyCollection`, `AccountState`, `StateInit`, `CommonMsgInfo`,
//! `Message`), not reconstructed from memory — every field ordering and bit
//! width here has a named counterpart in that file. Only the two shapes a
//! real caller needs are covered: an **already-deployed, active, ordinary**
//! account (no split accounts, no frozen/uninit accounts, no libraries), and
//! an **internal** message with **inline body carried as a reference**
//! (`Either X ^X` = right) — the shapes every real `sting` scenario so far
//! has needed. Extend when a second real shape is needed, not speculatively.

use num_bigint::BigUint;
use tonlib_core::TonAddress;
use tonlib_core::cell::{ArcCell, BagOfCells, Cell, CellBuilder, TonCellError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

/// Errors building a `ShardAccount`/`Message` `BoC`.
#[derive(Debug, thiserror::Error)]
pub enum TlbBuildError {
    /// A `BoC` argument (code/data/body) was not valid base64, or did not
    /// parse as a single-root `BoC`.
    #[error("failed to parse {field} as a single-root BoC: {source}")]
    InvalidBoc {
        /// Which field failed (`"code"`, `"data"`, `"body"`).
        field: &'static str,
        /// The underlying parse failure.
        #[source]
        source: TonCellError,
    },
    /// Cell construction overflowed (should not happen for the fixed, small
    /// shapes this module builds — surfaced rather than unwrapped anyway,
    /// per `docs/ARCHITECTURE.md` §3.2).
    #[error("cell builder error: {0}")]
    CellBuilder(#[source] TonCellError),
}

impl From<TonCellError> for TlbBuildError {
    fn from(source: TonCellError) -> Self {
        Self::CellBuilder(source)
    }
}

fn parse_single_root(field: &'static str, boc_base64: &str) -> Result<ArcCell, TlbBuildError> {
    BagOfCells::parse_base64(boc_base64)
        .and_then(BagOfCells::single_root)
        .map_err(|source| TlbBuildError::InvalidBoc { field, source })
}

fn serialize_root(cell: Cell) -> Result<String, TlbBuildError> {
    let bytes = BagOfCells::from_root(cell)
        .serialize(false)
        .map_err(TlbBuildError::CellBuilder)?;
    Ok(BASE64.encode(bytes))
}

/// `var_uint$_ {n:#} len:(#< n) value:(uint (len * 8)) = VarUInteger n;` for
/// an arbitrary `n` — [`tonlib_core::cell::CellBuilder::store_coins`] already
/// covers the one fixed instantiation every other caller in this workspace
/// needs (`VarUInteger 16` = `Grams`); [`ShardAccount`]'s `StorageUsed`
/// needs `VarUInteger 7` (`cells`/`bits` counts), which nothing in
/// `tonlib-core` exposes.
fn store_var_uint(builder: &mut CellBuilder, n: u32, value: u64) -> Result<(), TlbBuildError> {
    debug_assert!(n >= 2, "VarUInteger n requires n >= 2 to be meaningful");
    let length_bits = (u32::BITS - (n - 1).leading_zeros()) as usize;
    let byte_len = if value == 0 {
        0
    } else {
        ((u64::BITS - value.leading_zeros()).div_ceil(8)) as usize
    };
    // `byte_len` is the byte width of a `u64`, so it is always in `0..=8` —
    // well within `u8`, provably, not just assumed.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "byte_len is in 0..=8 by construction (byte width of a u64)"
    )]
    let byte_len_u8 = byte_len as u8;
    builder.store_u8(length_bits, byte_len_u8)?;
    if byte_len > 0 {
        let bytes = value.to_be_bytes();
        builder.store_slice(&bytes[8 - byte_len..])?;
    }
    Ok(())
}

/// What [`build_shard_account`] needs to describe an already-deployed,
/// active, ordinary account (see this module's doc comment for the shapes
/// deliberately not covered).
#[derive(Debug, Clone)]
pub struct ActiveAccountState {
    /// The account's own address (`Account.addr`).
    pub address: TonAddress,
    /// Current balance, in nanotons.
    pub balance_nanotons: u64,
    /// Base64 `BoC` of the account's code cell (single root).
    pub code_boc_base64: String,
    /// Base64 `BoC` of the account's data cell (single root).
    pub data_boc_base64: String,
    /// The logical time of this account's own last transaction — what a
    /// real fetch (e.g. toncenter's `last_transaction_lt`) gives you.
    /// Written into `AccountStorage.last_trans_lt` as-is; `ShardAccount`'s
    /// own (distinct) `last_trans_lt` field is derived as `this - 1`, not
    /// taken as a second parameter — `emulator/emulator-extern.cpp`'s
    /// `block::Account::unpack` rejects the input outright unless
    /// `AccountStorage.last_trans_lt > ShardAccount.last_trans_lt` (strict:
    /// `std::max(storage.last_trans_lt, 1) > acc_info.last_trans_lt` in
    /// `crypto/block/transaction.cpp`), confirmed empirically — an equal
    /// pair (the first, wrong guess here) fails with "Can't unpack shard
    /// account" despite every earlier field parsing cleanly. A real node's
    /// `ShardAccount.last_trans_lt` records the transaction *before* the
    /// account's current recorded state, so `- 1` is the correct relation,
    /// not just a value that happens to satisfy the check.
    pub last_transaction_lt: u64,
    /// `ShardAccount.last_trans_hash`.
    pub last_transaction_hash: [u8; 32],
    /// `StorageInfo.last_paid` — unix time storage was last paid up to.
    /// Passing the same value as the emulation's own `unixtime`
    /// (`TransactionEmulator::set_unixtime`) means zero elapsed time, so the
    /// storage phase accrues no `due_payment` — isolates whatever contract
    /// logic is under test from an unrelated, separately-estimated real
    /// accrual (see this module's doc comment).
    pub last_paid_unixtime: u32,
    /// `StorageUsed.cells`/`.bits` — approximate persisted-state size.
    /// Only feeds the storage phase's fee estimate (not read by
    /// `checkAmountIsEnoughToTransfer`-style contract-level checks, which
    /// budget storage cost from their own constants instead, e.g.
    /// `fees-management.tolk`'s `STORAGE_SIZE_MAX_WALLET_*`) — a rough
    /// estimate is fine when the caller only cares about contract-level
    /// logic, not the storage phase itself.
    pub storage_used_cells: u64,
    /// See [`Self::storage_used_cells`].
    pub storage_used_bits: u64,
}

/// Builds a `ShardAccount` `BoC` (base64) for an active, ordinary account —
/// what [`crate::TransactionEmulator::emulate_transaction`]'s
/// `shard_account_boc` argument needs. See this module's doc comment for the
/// exact TL-B this follows and the shapes deliberately not covered (frozen/
/// uninit/nonexistent accounts, libraries, split accounts).
///
/// # Errors
/// See [`TlbBuildError`].
pub fn build_shard_account(state: &ActiveAccountState) -> Result<String, TlbBuildError> {
    let code = parse_single_root("code", &state.code_boc_base64)?;
    let data = parse_single_root("data", &state.data_boc_base64)?;

    // StateInit: no fixed_prefix_length, no special (TickTock), code + data
    // both present, no libraries.
    let mut state_init = CellBuilder::new();
    state_init.store_bit(false)?; // fixed_prefix_length: Nothing
    state_init.store_bit(false)?; // special: Nothing
    state_init.store_bit(true)?; // code: Just
    state_init.store_reference(&code)?;
    state_init.store_bit(true)?; // data: Just
    state_init.store_reference(&data)?;
    state_init.store_bit(false)?; // library: Nothing

    // AccountState: account_active$1 _:StateInit — StateInit's own fields
    // are inlined directly after the tag (matches the `_:StateInit` anonymous
    // field convention), not nested under a second reference.
    let mut account_state = CellBuilder::new();
    account_state.store_bit(true)?; // account_active$1
    account_state.store_cell(&state_init.build()?)?;

    // CurrencyCollection: grams=balance, no extra currencies.
    let mut balance = CellBuilder::new();
    balance.store_coins(&BigUint::from(state.balance_nanotons))?;
    balance.store_bit(false)?; // ExtraCurrencyCollection: empty HashmapE

    // AccountStorage: last_trans_lt, balance, state.
    let mut storage = CellBuilder::new();
    storage.store_u64(64, state.last_transaction_lt)?;
    storage.store_cell(&balance.build()?)?;
    storage.store_cell(&account_state.build()?)?;

    // StorageInfo: used, storage_extra (none), last_paid, due_payment (none).
    let mut storage_info = CellBuilder::new();
    store_var_uint(&mut storage_info, 7, state.storage_used_cells)?;
    store_var_uint(&mut storage_info, 7, state.storage_used_bits)?;
    storage_info.store_bits(3, &[0])?; // storage_extra_none$000
    storage_info.store_u32(32, state.last_paid_unixtime)?;
    storage_info.store_bit(false)?; // due_payment: Nothing

    // Account: account$1 addr storage_stat storage.
    let mut account = CellBuilder::new();
    account.store_bit(true)?; // account$1
    account.store_address(&state.address)?;
    account.store_cell(&storage_info.build()?)?;
    account.store_cell(&storage.build()?)?;

    // ShardAccount: account_descr$_ account:^Account last_trans_hash last_trans_lt.
    // Must be strictly less than AccountStorage's own last_trans_lt above —
    // see `ActiveAccountState::last_transaction_lt`'s doc comment.
    let mut shard_account = CellBuilder::new();
    shard_account.store_child(account.build()?)?;
    shard_account.store_slice(&state.last_transaction_hash)?;
    shard_account.store_u64(64, state.last_transaction_lt.saturating_sub(1))?;

    serialize_root(shard_account.build()?)
}

/// What [`build_internal_message`] needs — an ordinary internal message to
/// an already-deployed contract (no `init`), body carried as a reference
/// (`Either X ^X` = right — the shape every body this workspace has built so
/// far needs once wrapped in a real envelope; see this module's doc
/// comment).
#[derive(Debug, Clone)]
pub struct InternalMessageParams {
    /// `CommonMsgInfo.src`.
    pub src: TonAddress,
    /// `CommonMsgInfo.dest`.
    pub dest: TonAddress,
    /// `CommonMsgInfo.value.grams`, in nanotons.
    pub value_nanotons: u64,
    /// `CommonMsgInfo.bounce`.
    pub bounce: bool,
    /// `CommonMsgInfo.fwd_fee` — what `GETORIGINALFWDFEE`/
    /// `in.originalForwardFee` reads on the Tolk side. The one field this
    /// module exists to let a caller control at all (`TvmEmulator::
    /// send_internal_message` cannot set this).
    pub fwd_fee_nanotons: u64,
    /// `CommonMsgInfo.created_lt`.
    pub created_lt: u64,
    /// `CommonMsgInfo.created_at`.
    pub created_at: u32,
    /// Base64 `BoC` of the message body (single root) — e.g. from
    /// `sting_pipeline`'s `build_scenario_body`, or hand-built the same way.
    pub body_boc_base64: String,
}

/// Builds an internal `Message` `BoC` (base64) wrapping `params.body` in a
/// real `CommonMsgInfo` envelope — what
/// [`crate::TransactionEmulator::emulate_transaction`]'s `message_boc`
/// argument needs, for the shape covered (ordinary internal message, no
/// `init`, body-as-reference — see this module's doc comment).
///
/// # Errors
/// See [`TlbBuildError`].
pub fn build_internal_message(params: &InternalMessageParams) -> Result<String, TlbBuildError> {
    let body = parse_single_root("body", &params.body_boc_base64)?;

    let mut info = CellBuilder::new();
    info.store_bit(false)?; // int_msg_info$0
    info.store_bit(true)?; // ihr_disabled: true (IHR unused in practice)
    info.store_bit(params.bounce)?;
    info.store_bit(false)?; // bounced: this is not itself a bounce
    info.store_address(&params.src)?;
    info.store_address(&params.dest)?;
    info.store_coins(&BigUint::from(params.value_nanotons))?; // value.grams
    info.store_bit(false)?; // value.other: empty ExtraCurrencyCollection
    store_var_uint(&mut info, 16, 0)?; // extra_flags: VarUInteger 16, 0
    info.store_coins(&BigUint::from(params.fwd_fee_nanotons))?; // fwd_fee
    info.store_u64(64, params.created_lt)?;
    info.store_u32(32, params.created_at)?;

    let mut message = CellBuilder::new();
    message.store_cell(&info.build()?)?;
    message.store_bit(false)?; // init: Nothing (already-deployed destination)
    message.store_bit(true)?; // body: Either X ^X = right (ref)
    message.store_reference(&body)?;

    serialize_root(message.build()?)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{
        ActiveAccountState, InternalMessageParams, TlbBuildError, build_internal_message,
        build_shard_account, store_var_uint,
    };
    use tonlib_core::TonAddress;
    use tonlib_core::cell::{BagOfCells, CellBuilder};

    fn a_leaf_cell_boc() -> String {
        // A trivial single-cell BoC to stand in for "some code"/"some data"
        // — content is irrelevant to this module (it never inspects code/
        // data, only stores them as references), so a minimal real BoC is
        // enough to exercise the parse-then-reference path honestly.
        let mut b = CellBuilder::new();
        b.store_u32(32, 0xDEAD_BEEF).unwrap();
        let cell = b.build().unwrap();
        let bytes = BagOfCells::from_root(cell).serialize(false).unwrap();
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    }

    fn a_body_boc() -> String {
        let mut b = CellBuilder::new();
        b.store_u32(32, 0x1234_5678).unwrap();
        let cell = b.build().unwrap();
        let bytes = BagOfCells::from_root(cell).serialize(false).unwrap();
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    }

    fn dummy_address(byte: u8) -> TonAddress {
        TonAddress::new(0, [byte; 32].into())
    }

    #[test]
    fn store_var_uint_encodes_zero_as_a_bare_zero_length() {
        let mut b = CellBuilder::new();
        store_var_uint(&mut b, 7, 0).expect("store_var_uint(0) must succeed");
        let cell = b.build().expect("build");
        // VarUInteger 7: length field is 3 bits; value 0 has byte_len 0, so
        // the whole encoding is exactly 3 zero bits, nothing else.
        assert_eq!(cell.bit_len(), 3);
    }

    #[test]
    fn store_var_uint_round_trips_a_small_value() {
        let mut b = CellBuilder::new();
        store_var_uint(&mut b, 7, 300).expect("store_var_uint(300) must succeed");
        let cell = b.build().expect("build");
        // 300 needs 2 bytes -> 3-bit length field (=2) + 16 value bits = 19 bits.
        assert_eq!(cell.bit_len(), 19);
        let mut parser = cell.parser();
        let len = parser.load_u8(3).expect("load length");
        assert_eq!(len, 2);
        let value = parser.load_u16(16).expect("load value");
        assert_eq!(value, 300);
    }

    #[test]
    fn build_shard_account_produces_a_parseable_single_root_boc() {
        let state = ActiveAccountState {
            address: dummy_address(0xAB),
            balance_nanotons: 974_433,
            code_boc_base64: a_leaf_cell_boc(),
            data_boc_base64: a_leaf_cell_boc(),
            last_transaction_lt: 81_006_661_000_009,
            last_transaction_hash: [0x11; 32],
            last_paid_unixtime: 1_784_428_400,
            storage_used_cells: 3,
            storage_used_bits: 1026,
        };
        let boc = build_shard_account(&state).expect("build_shard_account must succeed");
        let root = BagOfCells::parse_base64(&boc)
            .and_then(BagOfCells::single_root)
            .expect("result must parse as a single-root BoC");
        // Account (1 ref) + last_trans_hash (256 bits) + last_trans_lt (64 bits).
        assert_eq!(root.references().len(), 1);
        assert_eq!(root.bit_len(), 256 + 64);
        // The referenced Account cell must itself carry exactly 2 references
        // (code, data) — confirms the StateInit refs threaded through
        // AccountState/AccountStorage/StorageInfo correctly, not silently
        // dropped or duplicated.
        assert_eq!(root.references()[0].references().len(), 2);
    }

    #[test]
    fn build_shard_account_rejects_an_invalid_code_boc() {
        let state = ActiveAccountState {
            address: dummy_address(0x01),
            balance_nanotons: 1,
            code_boc_base64: "not a boc".to_string(),
            data_boc_base64: a_leaf_cell_boc(),
            last_transaction_lt: 1,
            last_transaction_hash: [0; 32],
            last_paid_unixtime: 1,
            storage_used_cells: 1,
            storage_used_bits: 1,
        };
        let err = build_shard_account(&state).expect_err("malformed code BoC must be rejected");
        assert!(matches!(
            err,
            TlbBuildError::InvalidBoc { field: "code", .. }
        ));
    }

    #[test]
    fn build_internal_message_produces_a_parseable_single_root_boc_with_one_ref() {
        let params = InternalMessageParams {
            src: dummy_address(0x01),
            dest: dummy_address(0x02),
            value_nanotons: 80_000_000,
            bounce: true,
            fwd_fee_nanotons: 6_000_000,
            created_lt: 81_006_661_000_010,
            created_at: 1_784_428_400,
            body_boc_base64: a_body_boc(),
        };
        let boc = build_internal_message(&params).expect("build_internal_message must succeed");
        let root = BagOfCells::parse_base64(&boc)
            .and_then(BagOfCells::single_root)
            .expect("result must parse as a single-root BoC");
        // info (inlined) + init:Nothing (1 bit) + body:right (1 bit), 1 ref (body).
        assert_eq!(root.references().len(), 1);
        let mut parser = root.parser();
        assert!(!parser.load_bit().expect("int_msg_info tag")); // int_msg_info$0
        assert!(parser.load_bit().expect("ihr_disabled"));
        assert!(parser.load_bit().expect("bounce"));
        assert!(!parser.load_bit().expect("bounced"));
    }

    #[test]
    fn build_internal_message_rejects_an_invalid_body_boc() {
        let params = InternalMessageParams {
            src: dummy_address(0x01),
            dest: dummy_address(0x02),
            value_nanotons: 1,
            bounce: false,
            fwd_fee_nanotons: 0,
            created_lt: 0,
            created_at: 0,
            body_boc_base64: "not a boc".to_string(),
        };
        let err = build_internal_message(&params).expect_err("malformed body BoC must be rejected");
        assert!(matches!(
            err,
            TlbBuildError::InvalidBoc { field: "body", .. }
        ));
    }
}
