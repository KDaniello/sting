//! `Tact` pipeline: `tact.js` (Node.js) subprocess compile vs. pure-Rust
//! `abi_to_schema` mapping, timed separately — same reasoning as
//! [`crate::tolk`].

use std::path::Path;

use sting_abi_bench::corpus::discover_tact_projects;
use sting_abi_tact::{ContractAbi, TactCompiler};

use crate::report::{StageResult, skipped_stage, time_stage};

const COMPONENT: &str = "Tact";

pub fn profile(repo_root: &Path) -> Vec<StageResult> {
    let script = repo_root.join("vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js");
    if !script.is_file() {
        return vec![skipped_stage(
            COMPONENT,
            "compile + abi_to_schema",
            format!(
                "{} not present locally (vendor/tact/ is .gitignored)",
                script.display()
            ),
        )];
    }

    let compiler = TactCompiler::new(script);
    let projects = discover_tact_projects(&repo_root.join("corpus-abi/clean/tact"));

    let mut compilable = Vec::new();
    let mut abis: Vec<ContractAbi> = Vec::new();
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
            "no real Tact project compiled successfully",
        )];
    }

    let mut idx = 0usize;
    let compile_stage = time_stage(
        COMPONENT,
        "compile (tact.js/Node.js subprocess)",
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
            let _ = sting_abi_tact::abi_to_schema(&abis[idx % abis.len()]);
            idx += 1;
        },
    );

    vec![compile_stage, mapping_stage]
}
