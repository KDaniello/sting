//! Tier 3 (`sting-abi-dictionary`) pipeline: on-chain fetch (network, timed
//! but not looped — see below), `TvmEmulator` setup, and the per-entry
//! `run_get_method` loop, timed separately by replicating `probe()`'s own
//! logic externally (`TvmEmulator`/`MethodDictionary::probeable_entries`
//! are both public — no need to touch `sting-abi-dictionary`'s internals).

use std::path::Path;
use std::time::Instant;

use sting_abi_dictionary::MethodDictionary;
use sting_corpus::{
    ContractSource, ContractTarget, CorpusController, DiskCache, FetchOptions, TonapiClient,
    ToncenterClient,
};
use sting_emulator::TvmEmulator;
use sting_emulator_sys::LibEmulator;

use crate::report::{StageResult, skipped_stage, time_stage};

const COMPONENT: &str = "Tier 3 probe";

/// Real USDT jetton-master address, already used throughout this project's
/// live validation — the same already-trusted fixture, not a new one picked
/// for this crate.
const ADDRESS: &str = "EQCxE6mUtQJKFnGfaROTKOt1lZbDiiX1kCixRv7Nw2Id_sDs";

/// Everything [`profile`] needs once resource loading (dictionary,
/// `libemulator`, a real on-chain fetch) has succeeded — splitting this out
/// keeps [`profile`] itself under `clippy::too_many_lines`, one loaded
/// bundle in, one `Vec<StageResult>` out.
struct Loaded {
    dictionary: MethodDictionary,
    lib: LibEmulator,
    code_boc_base64: String,
    data_boc_base64: String,
    fetch_stage: StageResult,
}

/// Loads the dictionary, `libemulator`, and does the one real on-chain
/// fetch this whole module needs — `Err` carries the already-built
/// `skipped_stage` row explaining exactly which prerequisite was missing.
fn load(repo_root: &Path) -> Result<Loaded, StageResult> {
    let dictionary_path = repo_root.join("corpus-abi/method-dictionary.json");
    let dictionary = MethodDictionary::load(&dictionary_path).map_err(|_| {
        skipped_stage(
            COMPONENT,
            "fetch + probe",
            format!("could not load {}", dictionary_path.display()),
        )
    })?;

    let lib = sting_emulator::load_default_library().map_err(|_| {
        skipped_stage(
            COMPONENT,
            "fetch + probe",
            "could not load libemulator — see vendor/libemulator/ in .gitignore",
        )
    })?;
    let _ = sting_emulator::set_verbosity_level(&lib, 0);

    let cache_root = sting_corpus::DiskCache::default_root()
        .map_err(|_| skipped_stage(COMPONENT, "fetch + probe", "no platform cache dir"))?;
    let sources: Vec<Box<dyn ContractSource>> = vec![
        Box::new(ToncenterClient::new()),
        Box::new(TonapiClient::new()),
    ];
    let corpus = CorpusController::new(
        sources,
        DiskCache::new(cache_root, sting_corpus::DEFAULT_TTL),
    );

    // Timed once, not looped (`time_stage` would otherwise hammer a real
    // external API for the whole stage budget, and this crate's whole point
    // is to profile *our* code, not toncenter's/tonapi's own latency).
    // `calls: 1` makes this an honest single-sample measurement, not an
    // averaged one — the row's own note says so explicitly.
    let fetch_start = Instant::now();
    let code = corpus
        .resolve(
            &ContractTarget::OnChainAddress(ADDRESS.to_string()),
            FetchOptions::default(),
        )
        .map_err(|_| skipped_stage(COMPONENT, "fetch + probe", "real on-chain fetch failed"))?;
    let fetch_stage = StageResult {
        component: COMPONENT,
        stage: "on-chain fetch (network, real USDT address)",
        calls: 1,
        total: fetch_start.elapsed(),
        note: "single sample, not averaged — network latency, not our algorithm".to_string(),
    };

    Ok(Loaded {
        dictionary,
        lib,
        code_boc_base64: code.code_boc_base64,
        data_boc_base64: code.data_boc_base64,
        fetch_stage,
    })
}

/// Times `TvmEmulator::new` + `set_c7` — the per-probe setup `probe()`
/// itself only ever pays once per call, before its own `run_get_method`
/// loop.
fn profile_setup(loaded: &Loaded) -> StageResult {
    let mut idx = 0u32;
    time_stage(
        COMPONENT,
        "TvmEmulator::new + set_c7 (per-probe setup)",
        "real USDT code+data",
        || {
            if let Ok(emulator) = TvmEmulator::new(
                &loaded.lib,
                &loaded.code_boc_base64,
                &loaded.data_boc_base64,
            ) {
                let _ = emulator.set_c7(ADDRESS, 1_700_000_000 + idx, 0, None);
            }
            idx += 1;
        },
    )
}

/// Times the per-entry `run_get_method` loop against one shared,
/// already-`set_c7`'d emulator instance — mirrors `probe()`'s own real
/// call pattern (one setup, many `run_get_method` calls), so this number
/// is directly comparable to `probe()`'s real per-call cost.
fn profile_loop(loaded: &Loaded) -> Vec<StageResult> {
    let Ok(emulator) = TvmEmulator::new(
        &loaded.lib,
        &loaded.code_boc_base64,
        &loaded.data_boc_base64,
    ) else {
        return vec![skipped_stage(
            COMPONENT,
            "run_get_method loop",
            "TvmEmulator::new failed on the shared instance",
        )];
    };
    let _ = emulator.set_c7(ADDRESS, 1_700_000_000, 0, None);
    let Ok(empty_stack) = sting_emulator::empty_stack_boc() else {
        return vec![skipped_stage(
            COMPONENT,
            "run_get_method loop",
            "empty_stack_boc failed",
        )];
    };
    let probeable: Vec<i64> = loaded
        .dictionary
        .probeable_entries()
        .map(|(id, _)| id)
        .collect();

    let loop_stage = time_stage(
        COMPONENT,
        "run_get_method loop (all probeable entries)",
        format!("{} probeable dictionary entries/call", probeable.len()),
        || {
            for &id in &probeable {
                if let Ok(id_as_i32) = i32::try_from(id) {
                    let _ = emulator.run_get_method(id_as_i32, &empty_stack);
                }
            }
        },
    );
    let per_entry = StageResult {
        component: COMPONENT,
        stage: "run_get_method (single entry, derived)",
        calls: loop_stage.calls * u64::try_from(probeable.len().max(1)).unwrap_or(1),
        total: loop_stage.total,
        note: format!(
            "DERIVED: loop total / ({} entries x {} loop calls)",
            probeable.len(),
            loop_stage.calls
        ),
    };
    vec![loop_stage, per_entry]
}

pub fn profile(repo_root: &Path) -> Vec<StageResult> {
    let loaded = match load(repo_root) {
        Ok(loaded) => loaded,
        Err(skipped) => return vec![skipped],
    };

    let setup_stage = profile_setup(&loaded);
    let mut results = vec![loaded.fetch_stage.clone(), setup_stage];
    results.extend(profile_loop(&loaded));
    results
}
