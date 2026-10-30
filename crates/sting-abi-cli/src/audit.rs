//! What the compiler says against what the dictionary says, on a sample of entries.
//!
//! The dictionary is learned from three languages, and only one of them has an authority to be
//! checked against: a Tolk contract's method ids are emitted by the compiler itself, in the
//! `abi.json` it writes beside the compiled code. Tact and `FunC` entries are derived by this
//! repository, and a check of a derivation against itself is not a check.
//!
//! So a sample of the Tolk entries is recompiled and compared against that artefact, read straight
//! out of the compiler's own output rather than through the extractor that built the dictionary.
//! The point is not to confirm the formula — the formula is TON's — but to catch a committed file
//! that has drifted from the corpus it claims to describe.
//!
//! What this does not do: it says nothing about the Tact and `FunC` entries, which are the larger
//! part of the dataset, and nothing about names the compiler emits that this dictionary never
//! learned. It answers one question — do the entries that came from a compiler still match that
//! compiler — and the answer is a percentage with the divergences named.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context as _;
use sting_abi_bench::corpus;
use sting_abi_dictionary::MethodDictionary;
use sting_tolkc::TolkCompiler;

/// How far the sample may diverge from the compiler before the dataset is not fit to publish.
///
/// One entry in a hundred is a corpus that moved under a committed file; more than that is a build
/// that no longer describes what it was built from.
const MAX_DIVERGENCE_PCT: f64 = 1.0;

/// One thing the dictionary claims: this contract declares this name at this id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Claim {
    /// The contract the name was seen in, as the dictionary records it.
    label: String,
    /// The get-method name.
    name: String,
    /// The id the dictionary carries for it.
    id: i64,
}

/// A claim the compiler did not confirm, and what it said instead.
#[derive(Debug, Clone)]
struct Divergence {
    /// The claim that failed.
    claim: Claim,
    /// What the compiler's own artefact says.
    found: String,
}

/// Which of `total` claims a sample of `sample_size` takes.
///
/// Spread across the whole list rather than by a stride computed with integer division: a stride of
/// `total / sample_size` is 1 whenever the sample is more than half the list, which quietly turns
/// "a sample spread across the corpus" into "the first hundred of one corpus".
fn sample_positions(total: usize, sample_size: usize) -> Vec<usize> {
    (0..sample_size)
        .map(|position| position * total / sample_size)
        .collect()
}

/// Checks a sample of the compiler-extracted entries against the compiler.
///
/// The sample is spread across the claims in contract order rather than taken from the front,
/// because the front of a sorted list is one corpus and a sample of one corpus measures nothing.
/// It is spread by position rather than drawn at random for the same reason the dataset is sorted:
/// an audit whose sample changes between runs is an audit nobody can reproduce.
///
/// # Errors
/// When the dictionary cannot be read, when no Tolk compiler is reachable, when fewer claims exist
/// than were asked for, or when the divergence exceeds one per cent.
pub fn run(dictionary_path: &Path, sample_size: usize) -> anyhow::Result<()> {
    anyhow::ensure!(sample_size > 0, "a sample of nothing checks nothing");
    let dictionary = MethodDictionary::load(dictionary_path)
        .with_context(|| format!("loading {}", dictionary_path.display()))?;
    let compiler = TolkCompiler::vendored_or_default().context(
        "no Tolk compiler is reachable: see docs/domains/extractors.md for vendoring it",
    )?;

    let projects: BTreeMap<String, corpus::Tier1Project> =
        corpus::discover_tolk_projects(&crate::dictionary::tolk_roots())
            .into_iter()
            .map(|project| (project.label.clone(), project))
            .collect();

    let mut claims: Vec<Claim> = dictionary
        .iter()
        .flat_map(|(id, group)| {
            group.iter().flat_map(move |entry| {
                entry.seen_in.iter().map(move |label| Claim {
                    label: label.clone(),
                    name: entry.name.clone(),
                    id,
                })
            })
        })
        .filter(|claim| projects.contains_key(&claim.label))
        .collect();
    claims.sort();

    anyhow::ensure!(
        claims.len() >= sample_size,
        "only {} compiler-extracted claim(s) exist in {}, {sample_size} were asked for",
        claims.len(),
        dictionary_path.display()
    );
    let sample: Vec<&Claim> = sample_positions(claims.len(), sample_size)
        .into_iter()
        .map(|position| &claims[position])
        .collect();

    let mut per_contract: BTreeMap<&str, BTreeMap<String, i64>> = BTreeMap::new();
    let mut divergences: Vec<Divergence> = Vec::new();
    for claim in &sample {
        // One contract compiles once, however many of its methods the sample names.
        if !per_contract.contains_key(claim.label.as_str()) {
            let project = &projects[&claim.label];
            let output = compiler
                .compile(&project.project_dir, &project.entrypoint)
                .with_context(|| format!("recompiling {}", claim.label))?;
            per_contract.insert(
                claim.label.as_str(),
                output
                    .abi
                    .get_methods
                    .into_iter()
                    .map(|method| (method.name, i64::from(method.tvm_method_id)))
                    .collect(),
            );
        }
        let methods = &per_contract[claim.label.as_str()];
        match methods.get(&claim.name) {
            Some(id) if *id == claim.id => {}
            Some(id) => divergences.push(Divergence {
                claim: (*claim).clone(),
                found: format!("the compiler puts it at {id}"),
            }),
            None => divergences.push(Divergence {
                claim: (*claim).clone(),
                found: "the compiler declares no such method".to_string(),
            }),
        }
    }

    let divergence_pct = crate::share::percent(divergences.len(), sample.len());
    println!(
        "checked {} of {} compiler-extracted claim(s) across {} contract(s) against tolk's own abi.json",
        sample.len(),
        claims.len(),
        per_contract.len()
    );
    for divergence in &divergences {
        println!(
            "  {} claims {} at {} — {}",
            divergence.claim.label, divergence.claim.name, divergence.claim.id, divergence.found
        );
    }
    println!(
        "divergence {divergence_pct:.1}% ({} of {}), threshold {MAX_DIVERGENCE_PCT:.1}%",
        divergences.len(),
        sample.len()
    );
    anyhow::ensure!(
        divergence_pct <= MAX_DIVERGENCE_PCT,
        "the dataset diverges from the compiler by {divergence_pct:.1}%, above {MAX_DIVERGENCE_PCT:.1}%"
    );
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::sample_positions;

    #[test]
    fn a_sample_of_most_of_the_list_still_reaches_its_end() {
        let positions = sample_positions(115, 100);

        assert_eq!(positions.len(), 100);
        assert_eq!(positions[0], 0);
        assert_eq!(*positions.last().unwrap(), 113);
    }

    #[test]
    fn a_sample_never_repeats_a_position_it_does_not_have_to() {
        let positions = sample_positions(1_000, 10);

        assert_eq!(
            positions,
            vec![0, 100, 200, 300, 400, 500, 600, 700, 800, 900]
        );
    }
}
