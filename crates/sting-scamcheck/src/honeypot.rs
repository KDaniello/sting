//! Honeypot-transfer probe (`docs/domains/scamcheck.md`).
//!
//! Simulates a `transfer` (TEP-74, opcode `0x0f8a7ea5`) sent **from a real
//! holder's own wallet, using that wallet's real on-chain code+data** to
//! some other address (standing in for a DEX pool — "selling").
//!
//! **Corrected after a real false positive, not a hypothetical concern**:
//! an earlier version of this module hand-built a *synthetic* `Storage`
//! cell, guessing at the canonical TEP-74 reference wallet's layout
//! (`balance:Coins owner_address:MsgAddressInt jetton_master_address:
//! MsgAddressInt jetton_wallet_code:^Cell`). Run against a real,
//! definitely-not-a-scam jetton (USD₮) via `sting-scamcheck`'s own CLI,
//! that probe reverted with exit code 9 ("cannot parse a `MsgAddress`") at
//! VM step 2 — a false positive, not a real finding: TEP-74 standardizes
//! the *external* interface (get-methods, message opcodes), never the
//! *internal* storage layout, so any issuer (Tether included) is free to
//! implement it differently, and guessing wrong reads garbage as an
//! address. There is no way to make a synthetic-storage approach reliable
//! against an unknown implementation — the fix is to stop guessing:
//! probe a real, currently-deployed holder's wallet, using its actual
//! on-chain bytes, never fabricated ones. See `crate::holder_concentration::
//! fetch_top_holders` — the same real-holder data this module now consumes.
//!
//! **Why `TransactionEmulator`, not `TvmEmulator::send_internal_message`**:
//! the latter takes only a message body + attached value — it never models
//! a sender at all (confirmed by reading `sting_emulator`'s own source). A
//! TEP-74 wallet's `transfer` handler reads the message envelope's own
//! `src` to check against its stored owner — testing that path needs a
//! full `Message`+`ShardAccount` pair through `TransactionEmulator::
//! emulate_transaction`, built via the already-proven `sting_emulator::tlb`.
//!
//! **Residual limitation, stated explicitly — not "100% accurate" and
//! cannot be**: this proves whether *this specific tested holder* can
//! sell, using their *current* on-chain state. It cannot prove the absence
//! of a restriction targeting some *other* address this probe never
//! tests (e.g. a blacklist naming a specific DEX router, not any of the
//! holders checked) — the same fundamental limit any black-box dynamic
//! probe has (finds real behavior, does not prove universal absence of a
//! behavior). Testing several real holders (`crate` callers are expected
//! to loop, see `sting-scamcheck`'s own CLI) narrows this gap; it does not
//! close it.

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use num_bigint::BigUint;
use sting_emulator::tlb::{
    ActiveAccountState, InternalMessageParams, TlbBuildError, build_internal_message,
    build_shard_account,
};
use sting_emulator::{EmulatorError, TransactionEmulator};
use sting_emulator_sys::LibEmulator;
use thiserror::Error;
use tonlib_core::TonAddress;
use tonlib_core::TonHash;
use tonlib_core::cell::dict::predefined_readers::{
    key_reader_256bit, key_reader_u32, val_reader_cell, val_reader_ref_cell,
};
use tonlib_core::cell::{ArcCell, BagOfCells, Cell, TonCellError};
use tonlib_core::message::{JettonTransferMessage, TonMessage, TonMessageError};

/// Inputs for one honeypot probe — everything real, fetched by the caller
/// (typically via `crate::holder_concentration::fetch_top_holders` for the
/// wallet address/owner, and `sting_corpus` for that wallet's own
/// code+data), never fabricated here (see the module doc comment for why).
pub struct HoneypotProbe {
    /// The real holder wallet's own on-chain code cell.
    pub wallet_code: ArcCell,
    /// The real holder wallet's own on-chain data cell, as-is — never
    /// rebuilt or reinterpreted.
    pub wallet_data: ArcCell,
    /// The wallet's own real address (its `get_wallet_data()`/indexer-
    /// reported address, not derived from code+data here).
    pub wallet_address: TonAddress,
    /// The wallet's real owner — used as the probe message's `src`, so
    /// the standard sender check passes and isolates any *additional*
    /// restriction.
    pub owner: TonAddress,
    /// How much to attempt to transfer out — must not exceed the
    /// wallet's real balance, or the standard `balance >= 0` check (not a
    /// honeypot signal) would reject the transfer for an unrelated reason.
    pub transfer_amount: u64,
    /// Where the probe transfer is sent — e.g. a synthetic address
    /// standing in for a DEX pool ("selling").
    pub destination: TonAddress,
}

/// Outcome of one probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The compute phase completed with no uncaught exception **and**
    /// emitted at least one real output action (e.g. the `internal_transfer`
    /// a genuine transfer sends onward) — positive evidence the transfer
    /// actually happened, not just that nothing crashed.
    Executed,
    /// The compute phase completed with no uncaught exception, but emitted
    /// **zero** output actions — the message was silently accepted and
    /// dropped. A real, distinct honeypot pattern this module's earlier
    /// version could not have caught at all (it only checked for an
    /// exception, and a silent no-op throws none): "accept, never error,
    /// never actually move the balance".
    ExecutedSilently,
    /// The compute phase threw this exit code — consistent with (not
    /// proof of) an *additional* transfer restriction beyond the standard
    /// sender/balance checks, which this probe is built to avoid
    /// triggering (see the module doc comment) — seeing exit code `705`
    /// or `706` here would itself be suspicious (this probe's own inputs
    /// are wrong — e.g. `transfer_amount` exceeds the real balance — not
    /// the target).
    Reverted {
        /// The TVM exit code the compute phase terminated with.
        exit_code: i32,
    },
    /// This wallet's code hash is a TON "precompiled contract" (registered
    /// in the mainnet config's `ConfigParam 45`, run by real validators as
    /// native C++ instead of interpreted TVM bytecode — see
    /// `docs.ton.org/foundations/precompiled`). Found empirically, not
    /// assumed: probing a real USD₮ holder — the exact false positive this
    /// module's earlier synthetic-storage version also hit — reverted
    /// with exit code 9 ("cannot parse a `MsgAddress`") at VM step 2 even
    /// using 100% real on-chain bytes, while an otherwise-identical probe
    /// against a real, non-precompiled jetton holder (a STON.fi LP
    /// wallet) succeeded cleanly. This is a distinct, narrower failure
    /// mode from an actual transfer restriction — surfaced honestly as
    /// "cannot verify", not misreported as either `Executed` or
    /// `Reverted`. In practice this only affects the small, curated set
    /// of contracts validators have voted to precompile (major
    /// stablecoins, not the unknown/suspicious jettons this tool exists
    /// to check) — a real, narrow, disclosed gap, not a silent one.
    ///
    /// **Detected structurally, not by sniffing log text — a first attempt
    /// at this got it wrong**: an earlier version of this detection
    /// matched `vm_log`'s text for the string `"Unknown precompiled
    /// contract"`, reasoning from a line seen on the test process's own
    /// stdout while diagnosing the USD₮ false positive. That line is real,
    /// but it turned out to come from `libemulator`'s own internal
    /// diagnostic logger (a separate, global log sink the C++ library
    /// writes to directly), never from the structured `vm_log` string this
    /// module's caller actually receives back through the FFI boundary —
    /// confirmed by a real integration test failing (`Reverted { exit_code:
    /// 9 }`, not this variant) even though the exact expected line was
    /// visible in the test's own console output. The real fix: decode
    /// `ConfigParam 45`'s `HashmapE 256 PrecompiledSmc` from the real
    /// mainnet config `BoC` (`is_registered_precompiled_contract`,
    /// `tonlib_core::cell::dict`'s already-existing generic `Hashmap`
    /// decoder — no hand-rolled Patricia-trie parsing needed) and check the
    /// probed wallet's own real `cell_hash()` against it, before ever
    /// running the emulator at all — a structural fact about the chain
    /// config, not a fragile scrape of a diagnostic message never meant to
    /// be a stable API.
    PrecompiledContractUnsupported,
}

/// Errors running a probe.
#[derive(Debug, Error)]
pub enum HoneypotError {
    /// Serializing a real cell to base64 `BoC` failed.
    #[error("failed to serialize a cell: {0}")]
    Cell(#[source] TonCellError),
    /// Building the TEP-74 transfer message body failed.
    #[error("failed to build the probe transfer message body: {0}")]
    Message(#[source] TonMessageError),
    /// Building the `ShardAccount`/`Message` envelope failed.
    #[error("failed to build a ShardAccount/Message envelope: {0}")]
    Tlb(#[source] TlbBuildError),
    /// `transaction_emulator_create` or a setter on it failed.
    #[error("transaction emulator setup failed: {0}")]
    Setup(#[source] EmulatorError),
    /// `emulate_transaction` itself failed at the emulator layer.
    #[error("emulate_transaction call failed: {0}")]
    Emulator(#[source] EmulatorError),
    /// The call completed but did not succeed (rejected before the compute
    /// phase even started — a malformed probe, not a target-specific
    /// signal).
    #[error("emulate_transaction did not succeed: {error:?}")]
    TransactionFailed {
        /// The emulator-reported error, if any.
        error: Option<String>,
    },
    /// The transaction succeeded but returned no `vm_log` to interpret.
    #[error("emulate_transaction succeeded but returned no vm_log to interpret")]
    NoVmLog,
}

fn serialize_cell(cell: &Cell) -> Result<String, TonCellError> {
    let bytes = BagOfCells::from_root(cell.clone()).serialize(false)?;
    Ok(BASE64.encode(bytes))
}

/// `out_list_empty$_ = OutList 0;` — a genuinely empty cell (no bits, no
/// references), per `ton-blockchain/ton`'s `crypto/block/block.tlb`. Any
/// non-empty `OutList` has at least one reference (`out_list$_ {n:#}
/// prev:^(OutList n) action:OutAction = OutList (n + 1);`, the same
/// right-nested-list shape `stack.rs`'s own `VmStackList` decoder already
/// handles) — so "zero actions" is exactly "this cell has no content",
/// not something that needs the full recursive decode.
fn actions_are_empty(actions_boc_base64: &str) -> Result<bool, HoneypotError> {
    let bag = BagOfCells::parse_base64(actions_boc_base64).map_err(HoneypotError::Cell)?;
    let root = bag.single_root().map_err(HoneypotError::Cell)?;
    Ok(root.bit_len() == 0 && root.references().is_empty())
}

/// Whether `code_hash` is registered in the real mainnet config's
/// `ConfigParam 45` (`_ precompiled_contracts:(HashmapE 256 PrecompiledSmc)
/// = ConfigParam 45;`) — see [`ProbeOutcome::PrecompiledContractUnsupported`]'s
/// doc comment for why this, not `vm_log` text, is the real detection
/// mechanism.
///
/// **Corrected against the real fixture, not assumed from `block.tlb`
/// alone**: `ton-blockchain/ton`'s `crypto/block/block.tlb` defines the
/// full `ConfigParams` as `_ config_addr:bits256 config:^(Hashmap 32
/// ^Cell) = ConfigParams;`, so a first attempt skipped a leading 256-bit
/// `config_addr` before decoding the dict. Against the real
/// `mainnet-config.boc.base64` fixture (fetched live from toncenter's
/// `/api/v2/getConfigAll`) that failed (`"Not enough bits to read
/// (requested: 256, remaining: 2)"`) — a diagnostic dump of the root cell
/// showed `bit_len=2, refs=2`, i.e. toncenter's own `getConfigAll` already
/// returns the bare `Hashmap 32 ^Cell` config dict as the root, with no
/// `config_addr` wrapper at all. Decoding starts directly from the root
/// accordingly. Every config param is keyed by its number, each value
/// itself a further cell reference; key 45's cell is then decoded directly
/// as a `HashmapE 256 PrecompiledSmc` (no further indirection — this
/// param's own cell content, per its own TL-B definition, is the hashmap
/// root itself). Uses `tonlib_core::cell::dict`'s existing generic Hashmap
/// decoder throughout, not a hand-rolled one.
/// `ConfigParam` index for `precompiled_contracts:(HashmapE 256
/// PrecompiledSmc)` (`ton-blockchain/ton`'s `crypto/block/block.tlb`).
const PRECOMPILED_CONTRACTS_PARAM: u32 = 45;

fn is_registered_precompiled_contract(
    config_boc_base64: &str,
    code_hash: &TonHash,
) -> Result<bool, HoneypotError> {
    let config_bag = BagOfCells::parse_base64(config_boc_base64).map_err(HoneypotError::Cell)?;
    let config_root = config_bag.single_root().map_err(HoneypotError::Cell)?;
    let params: HashMap<u32, ArcCell> = config_root
        .parser()
        .load_dict_data(32, key_reader_u32, val_reader_ref_cell)
        .map_err(HoneypotError::Cell)?;

    let Some(param_45) = params.get(&PRECOMPILED_CONTRACTS_PARAM) else {
        // No precompiled contracts registered in this config at all.
        return Ok(false);
    };
    let precompiled: HashMap<TonHash, Cell> = param_45
        .parser()
        .load_dict(256, key_reader_256bit, val_reader_cell)
        .map_err(HoneypotError::Cell)?;
    Ok(precompiled.contains_key(code_hash))
}

/// Runs one [`HoneypotProbe`].
///
/// `block_unixtime`/`block_lt` anchor the account's own "last transaction"
/// state and the probe message's `created_at`/`created_lt` — pass real,
/// recent values (e.g. from a live block) so the simulated environment
/// resembles a real one, not a hand-picked edge case.
///
/// # Errors
/// See [`HoneypotError`].
pub fn probe_transfer(
    lib: &LibEmulator,
    config_boc_base64: &str,
    probe: &HoneypotProbe,
    block_unixtime: u32,
    block_lt: u64,
) -> Result<ProbeOutcome, HoneypotError> {
    if is_registered_precompiled_contract(config_boc_base64, &probe.wallet_code.cell_hash())? {
        return Ok(ProbeOutcome::PrecompiledContractUnsupported);
    }

    let code_boc_base64 = serialize_cell(&probe.wallet_code).map_err(HoneypotError::Cell)?;
    let data_boc_base64 = serialize_cell(&probe.wallet_data).map_err(HoneypotError::Cell)?;

    let shard_account = build_shard_account(&ActiveAccountState {
        address: probe.wallet_address.clone(),
        // Generous synthetic TON (gas) balance — gas is deliberately not
        // the thing under test, see the module doc comment. The wallet's
        // real on-chain TON balance is not used here: a real holder's
        // actual gas reserve is irrelevant to whether the *jetton wallet
        // code itself* restricts transfers, and using a low real balance
        // would risk an unrelated out-of-gas false signal.
        balance_nanotons: 1_000_000_000,
        code_boc_base64,
        data_boc_base64,
        last_transaction_lt: block_lt,
        last_transaction_hash: [0; 32],
        last_paid_unixtime: block_unixtime,
        storage_used_cells: 10,
        storage_used_bits: 5_000,
    })
    .map_err(HoneypotError::Tlb)?;

    let transfer_msg =
        JettonTransferMessage::new(&probe.destination, &BigUint::from(probe.transfer_amount));
    let body_cell = transfer_msg.build().map_err(HoneypotError::Message)?;
    let body_boc_base64 = serialize_cell(&body_cell).map_err(HoneypotError::Cell)?;

    let message_boc = build_internal_message(&InternalMessageParams {
        src: probe.owner.clone(),
        dest: probe.wallet_address.clone(),
        value_nanotons: 500_000_000, // generous — see the module doc comment
        bounce: true,
        fwd_fee_nanotons: 1_000_000,
        created_lt: block_lt + 1,
        created_at: block_unixtime,
        body_boc_base64,
    })
    .map_err(HoneypotError::Tlb)?;

    let txn_emulator =
        TransactionEmulator::new(lib, config_boc_base64, 0).map_err(HoneypotError::Setup)?;
    txn_emulator
        .set_unixtime(block_unixtime)
        .map_err(HoneypotError::Setup)?;
    txn_emulator
        .set_lt(block_lt + 1)
        .map_err(HoneypotError::Setup)?;

    let response = txn_emulator
        .emulate_transaction(&shard_account, &message_boc)
        .map_err(HoneypotError::Emulator)?;

    if !response.success {
        return Err(HoneypotError::TransactionFailed {
            error: response.error,
        });
    }
    let vm_log = response.vm_log.as_deref().ok_or(HoneypotError::NoVmLog)?;
    if let Some(exit_code) = parse_reverted_exit_code(vm_log) {
        return Ok(ProbeOutcome::Reverted { exit_code });
    }
    match &response.actions {
        Some(actions) if !actions_are_empty(actions)? => Ok(ProbeOutcome::Executed),
        _ => Ok(ProbeOutcome::ExecutedSilently),
    }
}

/// Parses whether the compute phase threw an uncaught exception, from
/// `vm_log`'s text — the format `sting-emulator`'s underlying
/// `libemulator` build actually produces for `TransactionEmulator::
/// emulate_transaction` specifically, verified empirically (not assumed,
/// and not carried over from a different call path — see the correction
/// note below) against a real forced revert in `tests/honeypot.rs`
/// (`insufficient_jetton_balance_produces_a_real_reverted_outcome`):
/// `"...execute THROWIFNOT 706\ndefault exception handler, terminating vm
/// with exit code 706"`. A successful transaction's `vm_log` ends in a
/// normal opcode line (e.g. `"execute implicit RET"`) with no such line.
///
/// **Correction, found by real testing, not assumed**: an earlier version
/// of this parser matched `"handling exception code "` instead — a string
/// that appears in a *different* emulator call path
/// (`TvmEmulator::run_get_method`, see `corpus/fixtures/emulator/
/// SOURCE.md`'s exit-code-9 note) but never in `emulate_transaction`'s own
/// `vm_log`. That bug made every `Reverted` case silently read as
/// `Executed` — caught only once a real forced-revert integration test
/// was added, not by the success-path test alone. `sting-emulator` does
/// not expose a structured field for this on
/// [`sting_emulator::TransactionResponse`] (only the external-message
/// rejection path has a `vm_exit_code` field) — parsing `vm_log` text
/// remains a known limitation, not a stable structured API; extend the
/// matched patterns if a real run is ever found with a differently
/// worded terminal line.
fn parse_reverted_exit_code(vm_log: &str) -> Option<i32> {
    const MARKER: &str = "terminating vm with exit code ";
    vm_log
        .lines()
        .find_map(|line| line.split(MARKER).nth(1)?.trim().parse::<i32>().ok())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use tonlib_core::cell::{BagOfCells, CellBuilder};

    use super::{actions_are_empty, is_registered_precompiled_contract, parse_reverted_exit_code};

    /// Real mainnet config, same fixture `tests/honeypot.rs` and
    /// `src/main.rs` already embed/read (`corpus/fixtures/emulator/
    /// SOURCE.md`) — read from disk here rather than `include_str!`,
    /// since this is a unit test, not the CLI binary that needs to run
    /// standalone.
    fn real_mainnet_config_boc_base64() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/fixtures/emulator/mainnet-config.boc.base64");
        std::fs::read_to_string(path).expect("read mainnet-config fixture")
    }

    #[test]
    fn a_real_usdt_wallet_code_hash_is_a_registered_precompiled_contract() {
        // `corpus/fixtures/emulator/SOURCE.md`'s own ground truth for
        // `usdt-holder-wallet.code.boc.base64`.
        let usdt_wallet_code_hash: tonlib_core::TonHash = tonlib_core::TonHash::from_hex(
            "89468F02C78E570802E39979C8516FC38DF07EA76A48357E0536F2BA7B3EE37B",
        )
        .expect("parse real USD₮ wallet code hash");
        let is_precompiled = is_registered_precompiled_contract(
            real_mainnet_config_boc_base64().trim(),
            &usdt_wallet_code_hash,
        )
        .expect("decode ConfigParam 45");
        assert!(
            is_precompiled,
            "USD₮'s real jetton wallet code hash must be found in the real config's \
             ConfigParam 45 — if this fails, either the fixture or the decoder regressed"
        );
    }

    #[test]
    fn a_real_non_precompiled_wallet_code_hash_is_not_registered() {
        // The real STON.fi LP holder wallet's own code hash
        // (`ston-fi-lp-holder-wallet.code.boc.base64`,
        // `corpus/fixtures/emulator/SOURCE.md`) — a genuine, currently
        // deployed jetton wallet's real code hash, not a sentinel value
        // that only exercises the trivial "definitely absent" branch.
        // `tests/honeypot.rs`'s own integration tests already confirm this
        // indirectly (a probe against this wallet reaches `Executed`/
        // `Reverted`, never `PrecompiledContractUnsupported`) — asserted
        // here directly, against the decoder alone, with no emulator
        // involved.
        let lp_holder_code_hash: tonlib_core::TonHash = tonlib_core::TonHash::from_hex(
            "0857CDD89C74844150CACD35CD020BEF9167CBE12CD330DFB8C779C14D6B1552",
        )
        .expect("parse real STON.fi LP holder wallet code hash");
        let is_precompiled = is_registered_precompiled_contract(
            real_mainnet_config_boc_base64().trim(),
            &lp_holder_code_hash,
        )
        .expect("decode ConfigParam 45");
        assert!(
            !is_precompiled,
            "the real STON.fi LP holder wallet's code hash must not be found in the real \
             config's ConfigParam 45 — if this fails, the decoder is over-matching"
        );
    }

    #[test]
    fn parses_a_real_observed_transaction_emulator_exception_log_line() {
        // Verbatim tail of a real `emulate_transaction` `vm_log` captured
        // while fixing this parser (`tests/honeypot.rs`'s forced-revert
        // test) — not a guessed format.
        let log =
            "execute THROWIFNOT 706\ndefault exception handler, terminating vm with exit code 706";
        assert_eq!(parse_reverted_exit_code(log), Some(706));
    }

    #[test]
    fn a_clean_log_with_no_exception_line_has_no_exit_code() {
        let log = "execute PUSHINT 0\nexecute implicit RET";
        assert_eq!(parse_reverted_exit_code(log), None);
    }

    #[test]
    fn a_genuinely_empty_cell_is_out_list_empty() {
        let cell = CellBuilder::new().build().expect("build empty cell");
        let boc = BagOfCells::from_root(cell)
            .serialize(false)
            .expect("serialize");
        let base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, boc);
        assert!(actions_are_empty(&base64).expect("decode"));
    }

    #[test]
    fn a_cell_with_a_reference_is_not_out_list_empty() {
        let mut builder = CellBuilder::new();
        let child: tonlib_core::cell::ArcCell =
            std::sync::Arc::new(CellBuilder::new().build().expect("build child"));
        builder.store_reference(&child).expect("store reference");
        let cell = builder.build().expect("build cell");
        let boc = BagOfCells::from_root(cell)
            .serialize(false)
            .expect("serialize");
        let base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, boc);
        assert!(!actions_are_empty(&base64).expect("decode"));
    }
}
