//! A wallet-side preview of one message (`docs/domains/service.md`).
//!
//! **Experimental proof-of-concept, not a runtime decoder.** Shows only
//! that a real TON Connect-shaped `sendTransaction` message's opcode can be
//! matched against a compiled contract's `incoming_internal` schema by
//! TL-B prefix — nothing more. A real runtime decoder would need to walk
//! the rest of the message body against the matched type's full field
//! layout (nested cells, refs, variable-width ints); that is explicitly
//! out of scope here and left as a documented gap, not silently implied.
//!
//! **The message below is real, not synthesized** — captured live from
//! mainnet 2026-07-22 via `toncenter.com/api/v3` (the same public API this
//! whole `abi-layer` branch already uses elsewhere, e.g.
//! `docs/domains/method-dictionary.md`), by the exact chain:
//! 1. `GET /api/v3/actions?limit=100&action_type=jetton_transfer` — found a
//!    real, live jetton transfer action (`trace_id`
//!    `qzNdbwV7QTKwHI5DBtnWiAO/+o5Ek+RgLFUXPcR4BpI=`).
//! 2. `GET /api/v3/transactions?hash=<tx hash from that trace>` — the
//!    sending wallet's own transaction, whose `out_msgs` carries the exact
//!    message the wallet sent *to* the jetton wallet contract: real
//!    `destination`, `value`, and `message_content.body` (a base64 BOC) —
//!    exactly TON Connect's `sendTransaction` `{address, amount, payload}`
//!    shape (format confirmed in
//!    `docs/POSITION.md`'s own research).
//!
//! Run: `cargo run --example wallet_preview -p sting-abi-cli` (needs
//! `STING_TOLKC_PATH`/`TOLK_STDLIB` — same env vars every other real-`tolk`
//! example/test in this workspace needs).

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use sting_abi_core::Extractor;
use sting_abi_tolk::TolkExtractor;
use sting_tolkc::TolkCompiler;
use tonlib_core::cell::BagOfCells;

/// A TON Connect `sendTransaction` request's one `messages[]` entry —
/// mirrors the real wire shape, not a type this crate needs to own
/// permanently (this example is the only place it's used).
struct TonConnectMessage {
    address: &'static str,
    amount: &'static str,
    payload: &'static str,
}

/// Real, live mainnet data — see this file's own module doc comment for
/// exactly how it was captured. `address` is the real jetton wallet
/// contract that received this message; `payload` is the real message
/// body it received, base64 BOC, byte-for-byte as `toncenter` reported it.
const REAL_MESSAGE: TonConnectMessage = TonConnectMessage {
    address: "0:B6A40EBC598D843471CFDADBE59A66C1F762237E23BE715DF6F8F104E1F5A339",
    amount: "50000000",
    payload: "te6cckEBAQEAVgAAqA+KfqVUbeTvyvvlfjmJaAgANyJXUioVfyiI4oWJJ4qb62UQLxQgd1MQxrHU5WgeaC0ANAxsspbp+rw72K/+H7B2CZzRbdrRn6pSsBp6lBRGprTCAhUWGCY=",
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() -> anyhow::Result<()> {
    println!("=== wallet preview (experimental proof of concept) ===\n");
    println!("Real TON Connect-shaped message:");
    println!("  address: {}", REAL_MESSAGE.address);
    println!("  amount:  {} nanoTON", REAL_MESSAGE.amount);
    println!("  payload: {} (base64 BOC)\n", REAL_MESSAGE.payload);

    // Step 1: get *a* receiver schema via the existing Tier 1 decoder —
    // deliberately *not* claiming `REAL_MESSAGE.address` is verified
    // against this exact compiled source (no RegistryController::verify
    // call here, honestly not attempted); this demo only shows that a
    // real payload's opcode *can* be matched against *a* real jetton-
    // wallet schema's `incoming_internal` prefixes — the TEP-74 `transfer`
    // opcode this payload carries is a de facto industry-wide standard
    // (confirmed identical across every jetton-wallet implementation this
    // workspace's own corpus has), so the match below is a real,
    // meaningful one, not a coincidence engineered for this demo.
    let root = repo_root();
    let project_dir = root.join("corpus/clean/tolk-bench/01_jetton");
    let entrypoint = Path::new("JettonWallet.tolk");
    let tolk_path = root
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !tolk_path.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tolk/ in .gitignore)",
            tolk_path.display()
        );
        return Ok(());
    }
    let extractor = TolkExtractor::new(TolkCompiler::new(tolk_path));
    let schema = extractor
        .extract(&project_dir, entrypoint)
        .context("corpus/clean/tolk-bench/01_jetton/JettonWallet.tolk is a real, previously-compile-verified contract")?;
    println!(
        "Receiver schema: real compiled {} ({} incoming_internal message types)\n",
        schema.contract_name.as_deref().unwrap_or("?"),
        schema.incoming_internal.len()
    );

    // Step 2: read the payload's first 32 bits — the TL-B opcode prefix
    // every TON message body convention puts there — via `tonlib-core`'s
    // real BoC deserializer, the same crate `sting-emulator` already uses
    // for cell work elsewhere in this workspace (`crates/sting-emulator/
    // src/tlb.rs`), not a hand-rolled parser.
    let root_cell = BagOfCells::parse_base64(REAL_MESSAGE.payload)
        .and_then(BagOfCells::single_root)
        .context("a real toncenter-reported message body must be a valid single-root BoC")?;
    let opcode = root_cell
        .parser()
        .load_u32(32)
        .context("a real jetton message body carries at least a 32-bit opcode prefix")?;
    println!("Payload's first 32 bits (opcode): 0x{opcode:08x}\n");

    // Step 3: match — by TL-B prefix only (bit length + value), the
    // explicitly narrow scope this demo commits to. No further fields are
    // decoded.
    let mut matched = false;
    for msg in &schema.incoming_internal {
        let ty = &schema.types[msg.ty.0 as usize];
        let Some(prefix) = &ty.prefix else { continue };
        if prefix.prefix_len == 32 && u32::try_from(prefix.prefix_num).ok() == Some(opcode) {
            println!(
                "MATCH: opcode 0x{opcode:08x} == {}'s TL-B prefix (0x{:08x}) — this real payload is, \
                 in principle, an instance of this schema's incoming_internal message type.",
                ty.name.as_deref().unwrap_or("<anonymous>"),
                prefix.prefix_num
            );
            matched = true;
        }
    }
    if !matched {
        println!(
            "NO MATCH — opcode 0x{opcode:08x} does not match any of this schema's \
             incoming_internal TL-B prefixes."
        );
    }

    println!(
        "\nHonest scope reminder: this only matched a 32-bit opcode prefix, not the full \
         message body. Decoding the rest (amount/destination/forward_payload fields inside the \
         cell) would need a real runtime decoder walking the matched type's full field layout — \
         explicitly out of scope for this proof-of-concept, see this file's own module doc comment."
    );
    Ok(())
}
