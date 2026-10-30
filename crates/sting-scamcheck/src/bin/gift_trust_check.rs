//! `sting-scamcheck-gift` CLI — the Gift/NFT trust-layer's product wrapper
//! (`docs/domains/scamcheck.md`). Explicitly a thin
//! demo/CLI, not the public bot/API the TZ's §3.2 monetization plan
//! describes — building and hosting that (Telegram bot registration via
//! `BotFather`, a public HTTP service) needs credentials and infrastructure
//! nothing here has; this part is kept to a local, runnable CLI while the
//! three underlying signals themselves are real, tested logic rather than
//! stubs.
//!
//! A second binary in this crate (`src/bin/gift_trust_check.rs`, picked up
//! automatically by Cargo — no `Cargo.toml` change needed), not a change to
//! the existing `src/main.rs` (the jetton scamcheck CLI) — keeps this
//! session's Gift/NFT work isolated from that file while it evolves
//! independently in parallel.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests; this file is a binary entry point, not library code other crates depend on"
)]

use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use sting_corpus::ToncenterClient;
use sting_emulator::TvmEmulator;
use sting_scamcheck::domain_reputation::{self, DomainVerdict};
use sting_scamcheck::nft_authenticity::{self, AuthenticityVerdict};
use sting_scamcheck::wallet_reputation;
use tonlib_core::TonAddress;

/// Same fixed `set_c7` values `src/main.rs` already uses for the jetton
/// CLI — `get_nft_data()`/`get_collection_data()` need a plausible
/// `MYADDR`, not an accurate balance or the real current time.
const FIXED_UNIXTIME: u32 = 1_784_240_154;
const FIXED_BALANCE: u64 = 1_000_000_000;

#[derive(Parser)]
#[command(
    name = "sting-scamcheck-gift",
    about = "Non-custodial Gift/NFT trust-layer demo CLI (docs/domains/scamcheck.md) \
             — §0.1: one input, one structured answer across all applicable signals, never a bare yes/no."
)]
struct Cli {
    /// An NFT item OR collection address to check (friendly `EQ.../UQ...`
    /// or raw `0:<hex>` form). Tried as an item first (`get_nft_data()`);
    /// if that get-method is not implemented, tried as a collection
    /// directly (`get_collection_data()`).
    address: String,
    /// A marketplace/dApp domain to check for the domain-reputation signal
    /// (§1.2 — bare host, no scheme). Optional: this signal does not apply
    /// to every input.
    #[arg(long)]
    domain: Option<String>,
    /// A wallet/relayer address to check for the behavioral-reputation
    /// signal (§1.3). Optional, same reason.
    #[arg(long)]
    wallet: Option<String>,
    /// How many recent events to fetch for the wallet-reputation signal.
    #[arg(long, default_value_t = 20)]
    wallet_events: usize,
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
    use sting_corpus::ContractSource;
    ToncenterClient::new()
        .fetch(address)
        .with_context(|| format!("fetching {address} from toncenter"))
}

fn build_emulator<'a>(
    lib: &'a sting_emulator_sys::LibEmulator,
    address_hex: &str,
    code_data: &sting_corpus::ContractCode,
) -> Result<TvmEmulator<'a>> {
    let emulator = TvmEmulator::new(lib, &code_data.code_boc_base64, &code_data.data_boc_base64)
        .context("tvm_emulator_create failed")?;
    emulator
        .set_c7(address_hex, FIXED_UNIXTIME, FIXED_BALANCE, None)
        .context("tvm_emulator_set_c7 failed")?;
    Ok(emulator)
}

/// §0.1's own three-tier taxonomy for the authenticity signal, printed
/// honestly — `NotInRegistry` is never printed as "fake", only as
/// "unverified by this instance's starter registry" (§0.2 point 3).
fn print_authenticity(verdict: &AuthenticityVerdict, collection_address: &TonAddress) {
    match verdict {
        AuthenticityVerdict::KnownGood(name) => {
            println!(
                "collection authenticity: KNOWN GOOD — matches curated registry entry {name:?} \
                 ({})",
                collection_address.to_hex()
            );
        }
        AuthenticityVerdict::NotInRegistry => {
            println!(
                "collection authenticity: NOT IN STARTER REGISTRY ({}) — this is NOT evidence of \
                 a copycat, only that this instance's small curated list (docs/research/\
                 gift-nft-trust-layer-tz.md §1.1) does not yet include it",
                collection_address.to_hex()
            );
        }
    }
}

fn print_domain(domain: &str) {
    match domain_reputation::check_domain(domain) {
        Ok(DomainVerdict::ExactMatch { venue }) => {
            println!("domain: EXACT MATCH — {venue} ({domain:?})");
        }
        Ok(DomainVerdict::LooksLikeTyposquatOf {
            venue,
            known_domain,
            distance,
        }) => {
            println!(
                "domain: LOOKS LIKE TYPOSQUAT of {venue} ({known_domain:?}, edit distance \
                 {distance}) — reminder: this is a string-similarity heuristic only, NOT proof of \
                 origin (docs/domains/scamcheck.md — TonConnect manifests are \
                 not cryptographically bound to their claimed domain)"
            );
        }
        Ok(DomainVerdict::Unrecognized) => {
            println!(
                "domain: UNRECOGNIZED ({domain:?}) — not a signal either way, most domains are \
                 simply not gift/NFT marketplaces"
            );
        }
        Err(err) => println!("domain: SKIPPED ({err})"),
    }
}

fn print_wallet(wallet: &str, limit: usize) {
    let Ok(address) = wallet.parse::<TonAddress>() else {
        println!("wallet: SKIPPED (could not parse {wallet:?} as a TON address)");
        return;
    };
    match wallet_reputation::fetch_wallet_reputation(&address, limit) {
        Ok(signal) => {
            println!(
                "wallet reputation ({wallet}): {} events fetched, {} unique counterparties, \
                 scam-flagged={}, window=[{:?}..{:?}] (page-scoped, NOT true account age — \
                 docs/domains/scamcheck.md)",
                signal.events_fetched,
                signal.unique_counterparties,
                signal.any_event_flagged_scam,
                signal.oldest_timestamp_in_page,
                signal.newest_timestamp_in_page,
            );
        }
        Err(err) => println!("wallet reputation: SKIPPED ({err})"),
    }
}

fn run(cli: &Cli) -> Result<()> {
    let lib = sting_emulator::load_default_library().context(
        "loading libemulator — set STING_LIBEMULATOR_PATH or place it next to this binary, \
         see docs/domains/emulator.md",
    )?;

    let input_address: TonAddress = cli
        .address
        .parse()
        .with_context(|| format!("parsing address {:?}", cli.address))?;
    let input_hex = input_address.to_hex();

    println!("=== fetching {input_hex} ===");
    let code_data = fetch_code_data(&input_hex)?;
    let emulator = build_emulator(&lib, &input_hex, &code_data)?;

    let collection_address = match nft_authenticity::item_collection_address(&emulator) {
        Ok(resolved) => {
            println!(
                "input is an NFT item — resolved collection: {}",
                resolved.to_hex()
            );
            resolved
        }
        Err(item_err) => {
            println!(
                "input is not readable as an NFT item ({item_err}) — trying it directly as a \
                 collection"
            );
            input_address.clone()
        }
    };

    let collection_hex = collection_address.to_hex();
    let collection_code_data = if collection_hex == input_hex {
        code_data
    } else {
        fetch_code_data(&collection_hex)?
    };
    let collection_emulator = build_emulator(&lib, &collection_hex, &collection_code_data)?;
    let data = nft_authenticity::collection_data(&collection_emulator)
        .context("get_collection_data failed")?;
    println!("collection content: {:?}", data.content);
    println!("collection next_item_index: {}", data.next_item_index);

    let verdict = nft_authenticity::check_registry(&collection_address);
    print_authenticity(&verdict, &collection_address);

    if let Some(domain) = &cli.domain {
        print_domain(domain);
    } else {
        println!("domain: not checked (pass --domain to check one)");
    }

    if let Some(wallet) = &cli.wallet {
        print_wallet(wallet, cli.wallet_events);
    } else {
        println!("wallet: not checked (pass --wallet to check one)");
    }

    Ok(())
}
