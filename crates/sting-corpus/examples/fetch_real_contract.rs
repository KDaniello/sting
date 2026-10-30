//! A diagnostic (`docs/DATA.md`): fetches a real, currently-deployed mainnet
//! contract's code and data through
//! both `toncenter` and `tonapi` and cross-checks the result against the
//! same address's vendored fixture
//! (`corpus/fixtures/emulator/lp-jetton-master.*.boc.base64`) — proof that
//! both providers really do resolve to the same underlying bytes (despite
//! `tonapi` encoding them as hex and `toncenter` as base64, see
//! `sting_corpus::tonapi`'s module doc comment), not just that each client
//! compiles and returns *something*.
//!
//! Requires network access; not run by `cargo test` (an example, not a
//! test) — mirrors `sting-emulator`'s `run_get_jetton_data` example, which
//! is optional-DLL-gated the same way this is optional-network-gated.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "throwaway diagnostic binary, not shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::path::Path;

use sting_corpus::{ContractSource, TonapiClient, ToncenterClient};

const ADDRESS: &str = "0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6";

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures_dir = manifest_dir.join("../../corpus/fixtures/emulator");

    let expected_code =
        std::fs::read_to_string(fixtures_dir.join("lp-jetton-master.code.boc.base64"))
            .expect("read vendored fixture")
            .trim()
            .to_string();
    let expected_data =
        std::fs::read_to_string(fixtures_dir.join("lp-jetton-master.data.boc.base64"))
            .expect("read vendored fixture")
            .trim()
            .to_string();

    println!("fetching {ADDRESS} from toncenter v3...");
    let toncenter = ToncenterClient::new();
    let from_toncenter = toncenter.fetch(ADDRESS).expect("toncenter fetch failed");
    let toncenter_ok = from_toncenter.code_boc_base64 == expected_code
        && from_toncenter.data_boc_base64 == expected_data;
    println!(
        "  code matches fixture: {}, data matches fixture: {}",
        from_toncenter.code_boc_base64 == expected_code,
        from_toncenter.data_boc_base64 == expected_data
    );

    println!("fetching {ADDRESS} from tonapi v2...");
    let tonapi = TonapiClient::new();
    let from_tonapi = tonapi.fetch(ADDRESS).expect("tonapi fetch failed");
    let tonapi_ok = from_tonapi.code_boc_base64 == expected_code
        && from_tonapi.data_boc_base64 == expected_data;
    println!(
        "  code matches fixture: {}, data matches fixture: {}",
        from_tonapi.code_boc_base64 == expected_code,
        from_tonapi.data_boc_base64 == expected_data
    );

    println!(
        "  toncenter result == tonapi result (after tonapi's hex-to-base64 normalization): {}",
        from_toncenter == from_tonapi
    );

    if toncenter_ok && tonapi_ok {
        println!("\nRESULT: MATCH — both providers resolved to the exact vendored fixture bytes");
    } else {
        println!("\nRESULT: MISMATCH — see output above");
        std::process::exit(1);
    }
}
