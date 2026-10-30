//! `Tolk` pipeline: `tolk` subprocess compile vs. pure-Rust `abi_to_schema`
//! mapping, timed separately — the subprocess's own CPU time is not "our
//! algorithm" to optimize, only the mapping step is.

use std::path::Path;

use sting_abi_bench::corpus::discover_tolk_projects;
use sting_tolkc::{AbiJson, TolkCompiler};

use crate::report::{StageResult, skipped_stage, time_stage};

const COMPONENT: &str = "Tolk";

pub fn profile(repo_root: &Path) -> Vec<StageResult> {
    let tolk_path = repo_root
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !tolk_path.is_file() {
        return vec![skipped_stage(
            COMPONENT,
            "compile + abi_to_schema",
            format!(
                "{} not present locally (vendor/tolk/ is .gitignored)",
                tolk_path.display()
            ),
        )];
    }

    let compiler = TolkCompiler::new(tolk_path);
    let projects = discover_tolk_projects(&[
        &repo_root.join("corpus/clean/acton-contracts"),
        &repo_root.join("corpus/clean/tolk-bench"),
    ]);

    // Compile every project once up front: (a) filters out the two known
    // `@gen`-path-mapping projects this compiler can't resolve standalone
    // (`docs/ROADMAP.md`'s already-documented limitation, not a
    // new finding here), (b) caches every `AbiJson` so the mapping stage
    // times pure-Rust work only, never a second round of subprocess calls.
    let mut compilable = Vec::new();
    let mut abis: Vec<AbiJson> = Vec::new();
    for p in &projects {
        if let Ok(output) = compiler.compile(&p.project_dir, &p.entrypoint) {
            abis.push(output.abi);
            compilable.push(p.clone());
        }
    }

    if compilable.is_empty() {
        return vec![skipped_stage(
            COMPONENT,
            "compile + abi_to_schema",
            "no real Tolk project compiled successfully",
        )];
    }

    let mut idx = 0usize;
    let compile_stage = time_stage(
        COMPONENT,
        "compile (tolk subprocess)",
        format!("{} real projects", compilable.len()),
        || {
            let p = &compilable[idx % compilable.len()];
            let _ = compiler.compile(&p.project_dir, &p.entrypoint);
            idx += 1;
        },
    );

    let mut idx = 0usize;
    let mapping_stage = time_stage(
        COMPONENT,
        "abi_to_schema (Rust mapping)",
        format!("{} real ABIs", abis.len()),
        || {
            let _ = sting_abi_tolk::abi_to_schema(&abis[idx % abis.len()]);
            idx += 1;
        },
    );

    vec![compile_stage, mapping_stage]
}
