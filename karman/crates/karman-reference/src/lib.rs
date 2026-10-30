//! Where the ladder meets what it models: the implementation itself, executed, and one episode
//! of a market that actually happened.
//!
//! Level 0 is the corpus — the same transitions through the reference's bytecode and through
//! this workspace's mechanism. Level 1 is the episode — the actions a chain accepted, in the
//! order it accepted them, against the rates it charged.
//!
//! The reference is Morpho Blue's deployed bytecode, run on `revm`. It is vendored rather than
//! compiled — this repository has no Solidity toolchain, and the code that actually runs is a
//! stronger reference than a local build of the source that claims to produce it. What makes it
//! checkable is its provenance: an address, a block and a code hash, which anyone may put to a
//! node.
//!
//! The corpus is generated once and committed. A differential test that re-derived its
//! expectations on every run would be pinned to whatever produced them and could not be read by
//! anyone checking the claim; a committed corpus is a statement about the reference that stands
//! on its own, and regenerating it asks a second question — whether the reference still says
//! this — which is not the same question as whether the mechanism agrees.
//!
//! What the corpus reaches, and what it does not. Every one of the eight conversions between
//! assets and shares is stated by some case, in both directions and on both sides of the market,
//! and so are both branches of a liquidation and the write-off at the end of one. What it cannot
//! reach is the actor-less half of the port: a case is one agent's action, and interest is
//! therefore diffed as it is charged inside a transition rather than on its own.
//!
//! A composition root, and the only place in the workspace where a mechanism, a verification
//! harness and a foreign engine meet.

mod episode;
mod evm;
mod script;

use sting_karman_mech_lending::{LendingAction, LendingEvent, LendingState, Wad};
use sting_karman_types::{AgentId, Amount, Tick, Wide};
use serde::{Deserialize, Serialize};

pub use crate::{
    episode::{
        acted_on, charged_at, closing, decode, opening, Closing, Episode, EpisodeOrigin,
        RecordedPrice, EPISODE,
    },
    script::{ACTORS, FEE_RECIPIENT, LLTV, RATE},
};

/// One transition, and what the reference made of it.
pub type Case =
    sting_karman_verify::differential::DifferentialCase<LendingState, LendingAction, LendingEvent>;

/// What the collateral was worth from `from` onwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quoted {
    /// The first tick this price answered for.
    pub from: Tick,
    /// The price, at the oracle's own scale.
    pub price: Wide,
}

/// Where the reference came from, in enough detail to be checked against a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// The chain the code was read from.
    pub chain_id: u64,
    /// The address it was read at.
    pub address: String,
    /// The block it was read at.
    pub block: u64,
    /// The hash of the code that was read, as the chain itself reports it.
    pub code_hash: String,
}

/// The market the corpus was generated against, the paths it was driven with, and the cases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Corpus {
    /// Which code answered.
    pub reference: Provenance,
    /// The threshold the market was opened at.
    pub lltv: Amount<Wad>,
    /// Who the market pays its fee to.
    pub fee_recipient: AgentId,
    /// The rate the model answered with, unchanged for the whole corpus: what level 0 is
    /// checking is the arithmetic around a rate, not which rate a curve would have produced.
    pub rate: Amount<Wad>,
    /// What the oracle answered, and from when.
    pub prices: Vec<Quoted>,
    /// The transitions, each carrying the state it starts from.
    pub cases: Vec<Case>,
}

/// The vendored reference, as committed.
const VENDORED: &str = include_str!("../../../testdata/reference/morpho-blue.json");

/// The corpus, as committed.
const COMMITTED: &str = include_str!("../../../testdata/level0/morpho-blue.json");

/// The reference's runtime code and where it came from.
///
/// # Errors
///
/// Fails when the vendored artifact cannot be read, or when the code in it is not the code its
/// own hash names — which would mean the artifact was edited without its provenance.
pub fn vendored() -> Result<(Vec<u8>, Provenance), String> {
    #[derive(Deserialize)]
    struct Artifact {
        chain_id: u64,
        address: String,
        block: u64,
        code_hash: String,
        runtime_code: String,
    }

    let artifact: Artifact = serde_json::from_str(VENDORED)
        .map_err(|e| format!("the vendored reference is not readable: {e}"))?;
    let code = unhex(artifact.runtime_code.trim_start_matches("0x"))?;
    let hashed = format!("{:?}", revm::primitives::keccak256(&code));
    if hashed != artifact.code_hash {
        return Err(format!(
            "the vendored code hashes to {hashed}, and calls itself {}",
            artifact.code_hash
        ));
    }
    Ok((
        code,
        Provenance {
            chain_id: artifact.chain_id,
            address: artifact.address,
            block: artifact.block,
            code_hash: artifact.code_hash,
        },
    ))
}

fn unhex(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err(String::from("the vendored code is half a byte long"));
    }
    (0..hex.len() / 2)
        .map(|at| {
            u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16)
                .map_err(|e| format!("the vendored code is not hexadecimal: {e}"))
        })
        .collect()
}

/// Runs the reference through the program and reports what it did.
///
/// # Errors
///
/// Fails when the reference cannot be stood up, or when a step the corpus depends on succeeding
/// does not: a corpus generated from a machine that half worked would claim agreement with
/// nothing.
pub fn generate() -> Result<Corpus, String> {
    let (code, reference) = vendored()?;
    script::run(&code, reference)
}

/// The corpus as committed, read without touching the filesystem.
///
/// # Errors
///
/// Fails when the committed corpus is not readable.
pub fn committed() -> Result<Corpus, String> {
    serde_json::from_str(COMMITTED)
        .map_err(|e| format!("the committed corpus is not readable: {e}"))
}

#[cfg(test)]
mod tests {
    use sting_karman_core::{PathSource, SourceError};
    use sting_karman_mech_lending::{Collateral, FixedRate, Lending, Loan, MarketParams, Price};
    use sting_karman_types::{MarketId, SecondsPerTick};
    use sting_karman_verify::{differential::differential, Verdict};

    use super::*;

    /// The oracle of the replay: the same schedule the reference was driven with, read back.
    #[derive(Debug, Clone)]
    struct Quotes(Vec<Quoted>);

    impl PathSource<Price<Collateral, Loan>> for Quotes {
        fn value_at(&self, now: Tick) -> Result<Price<Collateral, Loan>, SourceError> {
            self.0
                .iter()
                .rev()
                .find(|quoted| quoted.from <= now)
                .map(|quoted| Price::wider(quoted.price))
                .ok_or_else(|| SourceError::new("the corpus quoted no price for this tick"))
        }
    }

    fn mechanism(corpus: &Corpus) -> Lending<Quotes, FixedRate> {
        let params = MarketParams { lltv: corpus.lltv, fee_recipient: corpus.fee_recipient };
        Lending::new(SecondsPerTick(std::num::NonZeroU64::new(1).unwrap()))
            .with_market(MarketId(0), params, Quotes(corpus.prices.clone()), FixedRate(corpus.rate))
            .unwrap()
    }

    /// The vendored artifact is the code it says it is, before anything is asked of it.
    #[test]
    fn the_vendored_reference_is_the_code_it_names() {
        let (code, provenance) = vendored().unwrap();
        assert_eq!(code.len(), 15_623);
        assert_eq!(provenance.chain_id, 1);
    }

    /// The reference still says what the committed corpus says it said.
    ///
    /// The slower of the two questions, and the one that needs the machine. It fails when the
    /// vendored code changes, when the program that drives it changes, or when the corpus was
    /// edited by hand — and each of those is something a reader of the corpus is entitled to
    /// assume did not happen.
    #[test]
    fn the_reference_still_says_what_the_corpus_says() {
        let generated = generate().expect("the reference could not be run");
        let committed = committed().expect("the committed corpus could not be read");
        assert_eq!(generated.cases.len(), committed.cases.len(), "the corpus changed size");
        for (generated, committed) in generated.cases.iter().zip(&committed.cases) {
            assert_eq!(
                generated, committed,
                "case `{}` is not what was committed",
                committed.probes
            );
        }
        assert_eq!(generated, committed);
    }

    /// The gate: zero divergences on the corpus.
    #[test]
    fn the_mechanism_agrees_with_the_reference() {
        let corpus = committed().expect("the committed corpus could not be read");
        assert!(!corpus.cases.is_empty(), "an empty corpus confirms nothing");
        assert_eq!(differential(&mechanism(&corpus), corpus.cases.clone()), Verdict::Confirmed);
    }

    /// A corpus that agreed with anything would prove nothing, so the same corpus is put to a
    /// mechanism the reference does not describe: one whose threshold lets a position borrow
    /// more than the reference allows.
    #[test]
    fn a_mechanism_the_reference_does_not_describe_diverges() {
        let corpus = committed().expect("the committed corpus could not be read");
        let generous = Corpus {
            lltv: corpus.lltv.checked_add(Amount::new(100_000_000_000_000_000)).unwrap(),
            ..corpus.clone()
        };
        let verdict = differential(&mechanism(&generous), corpus.cases);
        assert!(matches!(verdict, Verdict::Diverged { .. }), "a looser market was called the same");
    }
}
