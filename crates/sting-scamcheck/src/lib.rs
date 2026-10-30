//! Rug/scam signals for TON jettons — `docs/domains/scamcheck.md`.
//!
//! Pure consumption of already-published `sting-emulator` (get-method/
//! message-send against arbitrary code+data) and the external `tonlib-core`
//! crate (cell/address parsing, already a workspace dependency) — no
//! changes to `sting-core`/`sting-emulator`/`sting-abi-*`/`sting-fuzz`
//! sources were needed to build this crate (verified against the real code
//! before writing anything here, see that document's §3). `sting-fuzz` in
//! particular is deliberately never a dependency: its `body`/`fixture`/
//! `harness` modules are hardcoded to one demo contract
//! (`corpus/fixtures/fuzz/BalanceLedger.tolk`), not a general-purpose
//! message-construction engine — reusing it here would have meant
//! generalizing it first, which is a core change, not a reuse.
//!
//! One signal per module, by design not yet behind a shared trait/registry
//! (`sting-detectors`' `Detector` trait only earned its keep once there were
//! several real detectors — the same "extend when a second real case
//! justifies it" discipline applies here with a single signal implemented
//! so far).

pub mod content;
pub mod domain_reputation;
pub mod gift_registry;
pub mod holder_concentration;
pub mod honeypot;
pub mod mint_rights;
pub mod namesakes;
pub mod nft_authenticity;
pub mod score;
// Re-exported rather than owned: decoding a get-method's result stack belongs to whatever calls
// get-methods, and a second decoder in a second crate is a second set of tag numbers to keep in
// step with the virtual machine.
pub use sting_emulator::stack;
pub mod ston_fi;
pub mod wallet_reputation;
