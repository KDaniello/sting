//! `sting-scamcheck` CLI — the product wrapper that proves the library is a
//! real, runnable thing against a live jetton rather than a set of functions
//! tested in isolation.
//!
//! Deliberately a single binary inside this crate, not a new
//! `sting-scamcheck-cli` crate — `sting-cli`/`sting-abi-cli` are separate
//! crates because each already wraps *several* library crates; this
//! product has exactly one, so a second crate today would be the same
//! premature split already argued against
//! for the workspace as a whole.
//!
//! Fetches a real jetton by address (`sting_corpus::ToncenterClient`, the
//! same provider every other real fixture in this crate's tests was
//! fetched from), runs all four real signals against it, and prints a
//! composite [`sting_scamcheck::score::ScamCheckReport`].
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests; this file is the `main.rs` entry point, not library code other crates depend on"
)]
#![allow(
    clippy::cast_precision_loss,
    reason = "human-facing percentages for terminal output, same rationale holder_concentration.rs's own top_holders_share() already documents"
)]

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use sting_corpus::{ContractSource, ToncenterClient};
use sting_emulator::TvmEmulator;
use sting_scamcheck::stack;
use sting_scamcheck::{holder_concentration, honeypot, mint_rights, score, ston_fi};
use tonlib_core::TonAddress;

/// Same fixed `set_c7` unixtime/balance `sting_pipeline::EmulationPoolController`
/// already uses for every no-argument get-method call in this workspace —
/// `get_jetton_data()` needs a plausible `MYADDR`, not an accurate balance
/// or the real current time (see that controller's own doc comment).
const FIXED_UNIXTIME: u32 = 1_784_240_154;
const FIXED_BALANCE: u64 = 1_000_000_000;

/// A real mainnet blockchain config, embedded at compile time — the same
/// fixture `crates/sting-scamcheck/tests/honeypot.rs` already uses, fetched
/// live from toncenter on 2026-07-17/18 (`corpus/fixtures/emulator/SOURCE.md`).
/// Embedded (not re-fetched at runtime) so this binary runs standalone —
/// a known limitation: this config goes stale as the real chain's
/// consensus parameters change over time, acceptable for a first cut, not
/// silently pretended otherwise.
const MAINNET_CONFIG_BOC_BASE64: &str =
    include_str!("../../../corpus/fixtures/emulator/mainnet-config.boc.base64");

/// TEP-74 `get_wallet_data()`'s `balance` — index 0, same shape convention
/// as `get_jetton_data()`'s own index 0 (`sting_scamcheck::mint_rights`'s
/// doc comment).
const WALLET_BALANCE_STACK_INDEX: u32 = 0;

#[derive(Parser)]
#[command(
    name = "sting-scamcheck",
    about = "Real, runnable jetton rug/scam signal checker (docs/domains/scamcheck.md)"
)]
struct Cli {
    /// The jetton master address to check (friendly `EQ...`/`UQ...` or raw
    /// `0:<hex>` form).
    jetton_master: String,
    /// A STON.fi pool address for this jetton, if known — enables the
    /// LP-concentration check. No automatic router-based pool
    /// discovery yet (see `ston_fi.rs`'s own scope note) — the caller
    /// supplies it.
    #[arg(long)]
    ston_fi_pool: Option<String>,
    /// A candidate address to check LP ownership share for — only used if
    /// `--ston-fi-pool` is also given.
    #[arg(long)]
    lp_candidate: Option<String>,
    /// How many top holders to fetch for the concentration check.
    #[arg(long, default_value_t = 10)]
    top_holders: usize,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn fetch_code_data(address: &str) -> Result<sting_corpus::ContractCode> {
    ToncenterClient::new()
        .fetch(address)
        .with_context(|| format!("fetching {address} from toncenter"))
}

/// Mint rights plus the one extra `get_jetton_data()` field the holder
/// share needs (`total_supply`) — one real jetton master, checked twice (once via
/// `mint_rights::check_mint_rights`, once directly here), not a refactor
/// of already-reviewed `mint_rights.rs` to share one response.
fn check_master(master_emulator: &TvmEmulator<'_>) -> Result<(mint_rights::MintRightsSignal, i64)> {
    let mint_rights_signal =
        mint_rights::check_mint_rights(master_emulator).context("check_mint_rights failed")?;
    println!(
        "mint rights: {}",
        if mint_rights_signal.renounced() {
            "RENOUNCED (admin_address is addr_none)".to_string()
        } else {
            format!(
                "NOT renounced (admin_address = {:?})",
                mint_rights_signal.admin_address
            )
        }
    );

    let jetton_data_response = master_emulator
        .run_get_method_no_args("get_jetton_data")
        .context("get_jetton_data call failed")?;
    let jetton_data_stack = jetton_data_response
        .stack
        .as_deref()
        .context("get_jetton_data succeeded but returned no stack")?;
    let total_supply = stack::decode_entry(jetton_data_stack, 0)
        .context("decoding get_jetton_data index 0 (total_supply)")?
        .as_int(0)
        .context("get_jetton_data index 0 (total_supply) was not an Int")?;
    println!("total_supply: {total_supply}");

    Ok((mint_rights_signal, total_supply))
}

/// The honeypot probe, against up to `max_holders` **real** holders —
/// no synthetic storage (see `honeypot.rs`'s own doc comment for why:
/// guessing a non-standard wallet's layout produced a real false positive
/// against USD₮). Every holder of the same jetton runs the identical
/// `wallet_code`, just different data/balance/owner — testing several
/// guards against one specific address being individually blocked, not
/// against needing different code coverage.
fn check_honeypot(
    lib: &sting_emulator_sys::LibEmulator,
    holders: &[holder_concentration::Holder],
    max_holders: usize,
) -> Option<bool> {
    let mut any_suspicious = false;
    let mut attempted = 0;
    let mut conclusive = 0;
    for holder in holders.iter().filter(|h| h.balance > 0).take(max_holders) {
        let wallet_hex = holder.wallet_address.to_hex();
        // Every failure below is this *one* holder's problem (a bad fetch,
        // an unparseable cell, a probe setup error) — not a reason to lose
        // the composite report entirely. `run()` already computed
        // mint-rights/holder-concentration before this function is even
        // called; an early `?` here would have discarded those too.
        let outcome = (|| -> Result<honeypot::ProbeOutcome> {
            let wallet_code_data = fetch_code_data(&wallet_hex)?;
            let bag =
                tonlib_core::cell::BagOfCells::parse_base64(&wallet_code_data.code_boc_base64)
                    .context("parsing holder wallet code")?;
            let wallet_code = bag.single_root().context("single root code")?.clone();
            let bag =
                tonlib_core::cell::BagOfCells::parse_base64(&wallet_code_data.data_boc_base64)
                    .context("parsing holder wallet data")?;
            let wallet_data = bag.single_root().context("single root data")?.clone();

            let half_balance = (holder.balance / 2).max(1);
            let transfer_amount = u64::try_from(half_balance).unwrap_or(u64::MAX);
            let probe = honeypot::HoneypotProbe {
                wallet_code,
                wallet_data,
                wallet_address: holder.wallet_address.clone(),
                owner: holder.owner.clone(),
                transfer_amount,
                destination: TonAddress::new(0, [0x22; 32].into()),
            };
            honeypot::probe_transfer(
                lib,
                MAINNET_CONFIG_BOC_BASE64.trim(),
                &probe,
                FIXED_UNIXTIME,
                50_000_000_000_000,
            )
            .context("probe_transfer failed outright")
        })();
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(err) => {
                println!("honeypot probe ({wallet_hex}): SKIPPED, {err}");
                continue;
            }
        };
        println!("honeypot probe ({wallet_hex}): {outcome:?}");
        attempted += 1;
        match outcome {
            // A real, positive-or-negative signal about this jetton's
            // transfer behavior — counts toward the composite verdict.
            honeypot::ProbeOutcome::Executed => conclusive += 1,
            honeypot::ProbeOutcome::ExecutedSilently | honeypot::ProbeOutcome::Reverted { .. } => {
                conclusive += 1;
                any_suspicious = true;
            }
            // Not a signal about *this* jetton at all — the emulator
            // could not run its real wallet code, precompiled contracts
            // aside. Must not be counted as "not suspicious" (that would
            // hide a real gap) nor as "suspicious" (that would falsely
            // flag major precompiled stablecoins like USD₮ — the exact
            // false positive `honeypot.rs`'s own doc comment documents).
            honeypot::ProbeOutcome::PrecompiledContractUnsupported => {
                println!(
                    "honeypot probe ({wallet_hex}): INCONCLUSIVE — this wallet is a TON \
                     precompiled contract, cannot verify via emulated TVM"
                );
            }
        }
    }
    if attempted == 0 {
        println!("honeypot probe: SKIPPED, no real holder with a non-zero balance found");
        return None;
    }
    if conclusive == 0 {
        println!(
            "honeypot probe: INCONCLUSIVE, every tested holder's wallet is a TON precompiled contract"
        );
        return None;
    }
    Some(any_suspicious)
}

/// The LP concentration, run only if `--ston-fi-pool` was given — returns the
/// candidate's share of the pool's LP supply, if `--lp-candidate` was
/// also given.
fn check_ston_fi(lib: &sting_emulator_sys::LibEmulator, cli: &Cli) -> Result<Option<f64>> {
    let Some(pool_address) = &cli.ston_fi_pool else {
        return Ok(None);
    };
    println!("=== fetching STON.fi pool {pool_address} ===");
    let pool_address: TonAddress = pool_address
        .parse()
        .with_context(|| format!("parsing pool address {pool_address:?}"))?;
    let pool_hex = pool_address.to_hex();
    let pool_code = fetch_code_data(&pool_hex)?;
    let pool_emulator =
        TvmEmulator::new(lib, &pool_code.code_boc_base64, &pool_code.data_boc_base64)
            .context("tvm_emulator_create for the pool failed")?;
    pool_emulator
        .set_c7(&pool_hex, FIXED_UNIXTIME, FIXED_BALANCE, None)
        .context("tvm_emulator_set_c7 for the pool failed")?;

    let pool_total_supply =
        ston_fi::pool_total_supply(&pool_emulator).context("ston_fi::pool_total_supply failed")?;
    println!("pool LP total_supply: {pool_total_supply}");

    let Some(candidate) = &cli.lp_candidate else {
        return Ok(None);
    };
    let candidate: TonAddress = candidate
        .parse()
        .with_context(|| format!("parsing LP candidate address {candidate:?}"))?;
    let lp_wallet = ston_fi::candidate_lp_wallet_address(&pool_emulator, &candidate)
        .context("ston_fi::candidate_lp_wallet_address failed")?;
    let lp_wallet_hex = lp_wallet.to_hex();
    println!("candidate's LP wallet: {lp_wallet_hex}");

    let Ok(lp_wallet_code) = fetch_code_data(&lp_wallet_hex) else {
        // Never deployed (candidate never provided liquidity) — a real,
        // expected outcome, not a tool failure.
        println!(
            "candidate's LP wallet {lp_wallet_hex} is not a deployed account — \
             candidate likely never provided liquidity to this pool"
        );
        return Ok(Some(0.0));
    };
    let lp_wallet_emulator = TvmEmulator::new(
        lib,
        &lp_wallet_code.code_boc_base64,
        &lp_wallet_code.data_boc_base64,
    )
    .context("tvm_emulator_create for the LP wallet failed")?;
    lp_wallet_emulator
        .set_c7(&lp_wallet_hex, FIXED_UNIXTIME, FIXED_BALANCE, None)
        .context("tvm_emulator_set_c7 for the LP wallet failed")?;
    let wallet_data_response = lp_wallet_emulator
        .run_get_method_no_args("get_wallet_data")
        .context("get_wallet_data call failed")?;
    let Some(wallet_stack) = wallet_data_response.stack.as_deref() else {
        return Ok(None);
    };
    let balance = stack::decode_entry(wallet_stack, WALLET_BALANCE_STACK_INDEX)
        .ok()
        .and_then(|v| v.as_int(WALLET_BALANCE_STACK_INDEX).ok());
    match balance {
        Some(balance) if pool_total_supply > 0 => {
            let share = balance as f64 / pool_total_supply as f64;
            println!(
                "candidate's LP balance: {balance} ({:.2}% of pool LP supply)",
                share * 100.0
            );
            Ok(Some(share))
        }
        _ => {
            println!("candidate's LP wallet exists but balance was not decodable");
            Ok(None)
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let jetton_master: TonAddress = cli
        .jetton_master
        .parse()
        .with_context(|| format!("parsing jetton master address {:?}", cli.jetton_master))?;
    let jetton_master_hex = jetton_master.to_hex();

    let lib = sting_emulator::load_default_library().context(
        "loading libemulator — set STING_LIBEMULATOR_PATH or place it next to this binary, \
         see docs/domains/emulator.md",
    )?;

    println!("=== fetching jetton master {jetton_master_hex} ===");
    let master_code = fetch_code_data(&jetton_master_hex)?;
    let master_emulator = TvmEmulator::new(
        &lib,
        &master_code.code_boc_base64,
        &master_code.data_boc_base64,
    )
    .context("tvm_emulator_create for the jetton master failed")?;
    master_emulator
        .set_c7(&jetton_master_hex, FIXED_UNIXTIME, FIXED_BALANCE, None)
        .context("tvm_emulator_set_c7 for the jetton master failed")?;

    let (mint_rights_signal, total_supply) = check_master(&master_emulator)?;
    let total_supply_u128 = u128::try_from(total_supply).context("total_supply was negative")?;

    // Holder concentration: fetched once, and the real (address, owner,
    // balance) list is reused by the honeypot probe below — the same real
    // holders, one HTTP call rather than two.
    let holders = holder_concentration::fetch_top_holders(&jetton_master, cli.top_holders);
    let holder_share = match &holders {
        Ok(holders) => {
            let sum: u128 = holders.iter().map(|h| h.balance).sum();
            let share = if total_supply_u128 == 0 {
                0.0
            } else {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "human-facing ratio, same rationale as holder_concentration.rs's own top_holders_share()"
                )]
                let ratio = sum as f64 / total_supply_u128 as f64;
                ratio
            };
            println!(
                "top {} holders: {} fetched, {:.2}% of total supply",
                cli.top_holders,
                holders.len(),
                share * 100.0
            );
            Some(share)
        }
        Err(err) => {
            println!("top holders: FAILED ({err})");
            None
        }
    };

    // The honeypot probe, against up to three of the same real holders just
    // fetched — no synthetic wallet, see honeypot.rs's own doc comment for
    // the false positive that replaced.
    let honeypot_reverted = match &holders {
        Ok(holders) => check_honeypot(&lib, holders, 3),
        Err(_) => None,
    };

    // STON.fi LP concentration, only if a pool was given.
    let lp_owner_share = check_ston_fi(&lib, cli)?;

    let report = score::ScamCheckReport {
        mint_rights_renounced: Some(mint_rights_signal.renounced()),
        honeypot_reverted,
        top_holders_share: holder_share,
        lp_owner_share,
    };
    println!();
    println!("=== composite report ===");
    println!("{report:#?}");
    println!(
        "risk level: {:?} ({} points)",
        report.risk_level(),
        report.risk_points()
    );

    Ok(())
}
