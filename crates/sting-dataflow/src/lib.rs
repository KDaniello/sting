//! Generic dataflow framework: `Semilattice`/`Transfer`/`WorklistSolver`,
//! independent of Tolk specifics.
//!
//! Deliberately has **zero** dependency on `sting-ast` or any Tolk concept
//! (see `docs/ARCHITECTURE.md` §1.1's dependency graph: the arrow runs
//! `sting-detectors -> sting-dataflow`, never the reverse). That is what lets
//! the lattice/solver be unit-tested on a synthetic toy CFG, as done below,
//! without parsing anything — a real Tolk-specific `Transfer` implementation
//! (for the `ID`/`UBM` detectors, see `docs/domains/detectors.md`) belongs to
//! the detector crate and plugs into this framework from outside.

use std::collections::{HashMap, VecDeque};

/// A join-semilattice over dataflow facts of type `T`: a partial order with a
/// least element ([`bottom`](Semilattice::bottom)) and a least-upper-bound
/// operation ([`join`](Semilattice::join)) — exactly what a forward "may"
/// dataflow analysis (e.g. reaching definitions, the classic textbook
/// example exercised by this crate's test) merges control-flow-graph
/// predecessors' facts with.
///
/// `T: PartialEq` is required at the trait level (not just where the solver
/// uses it) because [`leq`](Semilattice::leq)'s default implementation is
/// defined in terms of it: `a <= b` iff `join(a, b) == b`, the standard
/// lattice-theoretic definition, so a correct `join` gives a correct `leq`
/// for free.
pub trait Semilattice<T: PartialEq> {
    /// The least element: `join(bottom(), x) == x` for all `x`.
    fn bottom(&self) -> T;

    /// The least upper bound of `a` and `b`.
    fn join(&self, a: &T, b: &T) -> T;

    /// Whether `a` is below-or-equal-to `b` in the partial order. Overridable
    /// for lattices where a direct comparison is cheaper than a full `join`
    /// (e.g. bitset subset check vs. bitset union); the default is always
    /// correct, just not always fastest.
    fn leq(&self, a: &T, b: &T) -> bool {
        self.join(a, b) == *b
    }
}

/// Computes the dataflow fact flowing *out* of one CFG block, given the fact
/// flowing *in*. `Self::Block` is whatever per-block payload the caller's
/// [`Cfg`] carries — this crate never interprets it, only threads it through
/// to the caller-supplied `Transfer` implementation (that is how Tolk-specific
/// analyses plug in without this crate knowing about Tolk).
pub trait Transfer<T> {
    /// The per-block payload type this transfer function knows how to
    /// interpret (e.g., for a real Tolk analysis, a list of statements).
    type Block;

    /// `state_in` is the join of `OUT` over every predecessor of the block
    /// (or [`Semilattice::bottom`] for the entry block). Returns the block's
    /// `OUT`.
    fn transfer(&self, state_in: &T, block: &Self::Block) -> T;
}

/// A control-flow-graph block identifier: a dense index into a [`Cfg`]'s
/// block list.
pub type BlockId = usize;

/// A malformed [`Cfg`] construction request — a caller bug in how the graph
/// was assembled (mismatched list lengths, an out-of-range entry or
/// successor id), not a runtime condition [`Cfg::new`] can recover from on
/// its own. Previously this was `assert!`/`assert_eq!` inside [`Cfg::new`]
/// — real, unrecoverable panics that bypassed this workspace's
/// `unwrap_used`/`expect_used`/`panic` deny lints entirely (those lints
/// don't see `assert!`), so nothing forced a caller to reckon with this
/// failure mode. [`Cfg::new`] returning `Result<Self, CfgError>` puts that
/// decision back in the caller's hands — today's one real caller
/// (`sting-detectors::cfg::build`) still treats it as "no CFG could be
/// built, skip this function's dataflow-based finding" rather than
/// crashing the whole `sting lint` batch, matching the fail-closed pattern
/// already used elsewhere in this codebase (e.g. `BridgeController`
/// failing closed to `Inconclusive`). See
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CfgError {
    /// `blocks` and `successors` had different lengths — [`Cfg::new`]
    /// requires exactly one successor list per block.
    #[error(
        "one successor list per block is required: {blocks} blocks, {successors} successor lists"
    )]
    LengthMismatch {
        /// `blocks.len()`.
        blocks: usize,
        /// `successors.len()`.
        successors: usize,
    },
    /// `entry` was not a valid index into `blocks`.
    #[error("entry block id {entry} out of range for {block_count} blocks")]
    EntryOutOfRange {
        /// The out-of-range entry id that was passed in.
        entry: BlockId,
        /// `blocks.len()`.
        block_count: usize,
    },
    /// One of `successors`' entries pointed at a block id not present in
    /// `blocks`.
    #[error("successor id {successor} out of range for {block_count} blocks")]
    SuccessorOutOfRange {
        /// The out-of-range successor id that was passed in.
        successor: BlockId,
        /// `blocks.len()`.
        block_count: usize,
    },
}

/// A minimal control-flow graph: blocks with a payload of caller-chosen type
/// `B`, plus successor edges. Predecessor edges are derived once at
/// construction ([`Cfg::new`]) rather than recomputed by the solver on every
/// lookup or duplicated by every caller.
#[derive(Debug, Clone)]
pub struct Cfg<B> {
    entry: BlockId,
    blocks: Vec<B>,
    successors: Vec<Vec<BlockId>>,
    predecessors: Vec<Vec<BlockId>>,
}

impl<B> Cfg<B> {
    /// Builds a CFG from `blocks` (indexed by [`BlockId`], i.e. position in
    /// the `Vec`) and each block's `successors`, both required to have the
    /// same length. `entry` must be a valid index into `blocks`.
    ///
    /// # Errors
    /// Returns [`CfgError`] if `blocks.len() != successors.len()`, if
    /// `entry` is out-of-range, or if any successor id is out-of-range —
    /// all three are caller bugs in how the graph was assembled, but this
    /// crate has no way to know whether a given caller wants to treat that
    /// as fatal (`.expect()`, now visible to this workspace's
    /// `expect_used` deny lint, unlike the `assert!`s this replaced) or
    /// recoverable (see [`CfgError`]'s own doc comment for how today's one
    /// real caller handles it).
    pub fn new(
        entry: BlockId,
        blocks: Vec<B>,
        successors: Vec<Vec<BlockId>>,
    ) -> Result<Self, CfgError> {
        if blocks.len() != successors.len() {
            return Err(CfgError::LengthMismatch {
                blocks: blocks.len(),
                successors: successors.len(),
            });
        }
        if entry >= blocks.len() {
            return Err(CfgError::EntryOutOfRange {
                entry,
                block_count: blocks.len(),
            });
        }

        let mut predecessors = vec![Vec::new(); blocks.len()];
        for (from, succs) in successors.iter().enumerate() {
            for &to in succs {
                if to >= blocks.len() {
                    return Err(CfgError::SuccessorOutOfRange {
                        successor: to,
                        block_count: blocks.len(),
                    });
                }
                predecessors[to].push(from);
            }
        }

        Ok(Self {
            entry,
            blocks,
            successors,
            predecessors,
        })
    }

    /// The designated entry block's id.
    ///
    /// Not read by [`WorklistSolver::solve`] itself: every block with no
    /// predecessors (the entry included, by construction) already gets
    /// [`Semilattice::bottom`] as its `IN` fact, with no special case needed.
    /// Exposed anyway because a `Cfg` genuinely has a distinguished entry —
    /// a real caller building one from a Tolk function body needs to know
    /// where it starts (`docs/domains/detectors.md`'s `LEP` detector,
    /// the first consumer) — and a future analysis with a non-bottom initial
    /// fact (e.g. "parameters are live at function entry") will need to look
    /// it up specifically rather than relying on the predecessor-less
    /// coincidence.
    #[must_use]
    pub fn entry(&self) -> BlockId {
        self.entry
    }

    /// Ids of every block with no outgoing edges — the exit points a "does
    /// this hold on every path out of the function" analysis needs to check
    /// (the `LEP` and `UBM` detectors, `docs/domains/detectors.md`).
    pub fn exit_blocks(&self) -> impl Iterator<Item = BlockId> + '_ {
        self.successors
            .iter()
            .enumerate()
            .filter(|(_, succs)| succs.is_empty())
            .map(|(id, _)| id)
    }

    /// Number of blocks in this CFG. Block ids are dense `0..block_count()`
    /// (see [`Cfg::new`]), so this is what lets a caller iterate every block
    /// for a decoration pass run *after* [`WorklistSolver::solve`] — e.g.
    /// the `UBM` detector re-inspecting each block's contents now that it
    /// knows the reachability fact flowing into it.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// The payload of `block` (e.g. the statement list a Tolk-specific
    /// `Transfer` interprets).
    ///
    /// # Panics
    /// Panics if `block` is out of range — same contract as [`Cfg::new`]'s
    /// own panics: an out-of-range id is a caller bug, not a runtime
    /// condition to recover from (`docs/ARCHITECTURE.md` §3.2).
    #[must_use]
    pub fn block(&self, block: BlockId) -> &B {
        &self.blocks[block]
    }

    /// Ids of every block with an edge directly into `block` — lets a
    /// caller derive a block's `IN` fact (the join of its predecessors'
    /// `OUT`) after the fact, since [`WorklistSolver::solve`] itself only
    /// returns `OUT` per block (the `UBM` detector needs exactly
    /// this: solve for "was state saved by the time we reach the block",
    /// then re-scan the block's own statements starting from that `IN`
    /// value to find the exact risky call).
    ///
    /// # Panics
    /// Panics if `block` is out of range — see [`Cfg::block`].
    #[must_use]
    pub fn predecessors(&self, block: BlockId) -> &[BlockId] {
        &self.predecessors[block]
    }
}

/// Solves a forward dataflow problem over a [`Cfg`] to a fixpoint, using the
/// classic worklist algorithm: repeatedly recompute a block's `OUT` from its
/// (possibly stale) predecessors' `OUT`s, and only re-enqueue its successors
/// when `OUT` actually changed. Terminates because `T` is finite-height in
/// every real use (the solver itself does not assume or check this — an
/// ever-growing `T` under `join` would simply never converge, same as any
/// other dataflow solver).
pub struct WorklistSolver;

impl WorklistSolver {
    /// Runs the algorithm, returning each block's `OUT` fact.
    ///
    /// `IN` for the entry block is [`Semilattice::bottom`]; `IN` for every
    /// other block is the join of `OUT` over all of its predecessors, which
    /// is [`Semilattice::bottom`] again for an (unreachable) block with none.
    pub fn solve<T, L, Tr>(lattice: &L, transfer: &Tr, cfg: &Cfg<Tr::Block>) -> HashMap<BlockId, T>
    where
        T: Clone + PartialEq,
        L: Semilattice<T>,
        Tr: Transfer<T>,
    {
        let n = cfg.blocks.len();
        let mut out: Vec<T> = (0..n).map(|_| lattice.bottom()).collect();
        let mut worklist: VecDeque<BlockId> = (0..n).collect();
        let mut queued = vec![true; n];

        while let Some(block_id) = worklist.pop_front() {
            queued[block_id] = false;

            let in_fact = cfg.predecessors[block_id]
                .iter()
                .fold(lattice.bottom(), |acc, &pred| {
                    lattice.join(&acc, &out[pred])
                });

            let new_out = transfer.transfer(&in_fact, &cfg.blocks[block_id]);
            if new_out != out[block_id] {
                out[block_id] = new_out;
                for &succ in &cfg.successors[block_id] {
                    if !queued[succ] {
                        queued[succ] = true;
                        worklist.push_back(succ);
                    }
                }
            }
        }

        out.into_iter().enumerate().collect()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Cfg, CfgError, Semilattice, Transfer, WorklistSolver};

    /// Set-union semilattice: `bottom = {}`, `join = union`. The textbook
    /// lattice for "may" analyses like reaching definitions.
    struct SetUnion;
    impl Semilattice<BTreeSet<&'static str>> for SetUnion {
        fn bottom(&self) -> BTreeSet<&'static str> {
            BTreeSet::new()
        }
        fn join(
            &self,
            a: &BTreeSet<&'static str>,
            b: &BTreeSet<&'static str>,
        ) -> BTreeSet<&'static str> {
            a.union(b).copied().collect()
        }
    }

    /// Each block unconditionally "generates" (adds) exactly the one
    /// definition named by its payload, and kills nothing — the simplest
    /// possible non-trivial reaching-definitions transfer function, chosen so
    /// the expected fixpoint is hand-verifiable.
    struct GenOnly;
    impl Transfer<BTreeSet<&'static str>> for GenOnly {
        type Block = &'static str;
        fn transfer(
            &self,
            state_in: &BTreeSet<&'static str>,
            block: &Self::Block,
        ) -> BTreeSet<&'static str> {
            let mut out = state_in.clone();
            out.insert(block);
            out
        }
    }

    fn set(defs: &[&'static str]) -> BTreeSet<&'static str> {
        defs.iter().copied().collect()
    }

    #[test]
    fn leq_default_matches_join_based_definition() {
        let lattice = SetUnion;
        let a = set(&["d0"]);
        let b = set(&["d0", "d1"]);
        assert!(lattice.leq(&a, &b), "{{d0}} <= {{d0,d1}}");
        assert!(!lattice.leq(&b, &a), "{{d0,d1}} is not <= {{d0}}");
        assert!(lattice.leq(&a, &a), "leq must be reflexive");
    }

    /// Classic reaching-definitions worklist fixpoint over a 5-node diamond
    /// CFG — the toy-CFG unit test `docs/domains/dataflow-cfg.md` calls for:
    ///
    /// ```text
    /// B0 -> B1 -> B2 -\
    ///        \         -> B4
    ///         -> B3 --/
    /// ```
    ///
    /// Each block Bn generates exactly `dn`; expected `OUT` sets below were
    /// hand-computed (see the module-level walk in the implementation PR),
    /// not derived from the solver itself — an independent oracle is the
    /// point of this test.
    #[test]
    fn reaching_definitions_over_a_diamond_cfg_reaches_the_expected_fixpoint() {
        let cfg = Cfg::new(
            0,
            vec!["d0", "d1", "d2", "d3", "d4"],
            vec![
                vec![1],    // B0 -> B1
                vec![2, 3], // B1 -> B2, B3
                vec![4],    // B2 -> B4
                vec![4],    // B3 -> B4
                vec![],     // B4 (exit)
            ],
        )
        .expect("well-formed diamond CFG");

        let result = WorklistSolver::solve(&SetUnion, &GenOnly, &cfg);

        assert_eq!(result[&0], set(&["d0"]));
        assert_eq!(result[&1], set(&["d0", "d1"]));
        assert_eq!(result[&2], set(&["d0", "d1", "d2"]));
        assert_eq!(result[&3], set(&["d0", "d1", "d3"]));
        assert_eq!(
            result[&4],
            set(&["d0", "d1", "d2", "d3", "d4"]),
            "B4 merges both B2 and B3's OUT"
        );
    }

    #[test]
    fn a_single_block_with_no_predecessors_gets_bottom_as_its_in_fact() {
        let cfg = Cfg::new(0, vec!["only"], vec![vec![]]).expect("single-block CFG");
        let result = WorklistSolver::solve(&SetUnion, &GenOnly, &cfg);
        assert_eq!(result[&0], set(&["only"]));
    }

    #[test]
    fn cfg_new_rejects_an_out_of_range_successor() {
        assert_eq!(
            Cfg::new(0, vec!["a", "b"], vec![vec![5], vec![]]).unwrap_err(),
            CfgError::SuccessorOutOfRange {
                successor: 5,
                block_count: 2,
            }
        );
    }

    #[test]
    fn cfg_new_rejects_an_out_of_range_entry() {
        assert_eq!(
            Cfg::new(2, vec!["a", "b"], vec![vec![], vec![]]).unwrap_err(),
            CfgError::EntryOutOfRange {
                entry: 2,
                block_count: 2,
            }
        );
    }

    #[test]
    fn cfg_new_rejects_a_blocks_successors_length_mismatch() {
        assert_eq!(
            Cfg::new(0, vec!["a", "b"], vec![vec![]]).unwrap_err(),
            CfgError::LengthMismatch {
                blocks: 2,
                successors: 1,
            }
        );
    }

    #[test]
    fn entry_accessor_returns_the_constructed_entry_id() {
        let cfg = Cfg::new(1, vec!["a", "b", "c"], vec![vec![], vec![0, 2], vec![]])
            .expect("well-formed 3-block CFG");
        assert_eq!(cfg.entry(), 1);
    }

    #[test]
    fn exit_blocks_returns_every_block_with_no_successors() {
        // Same diamond as the reaching-definitions test: B0 -> B1 -> {B2,B3} -> B4.
        // Only B4 has no outgoing edge.
        let cfg = Cfg::new(
            0,
            vec!["d0", "d1", "d2", "d3", "d4"],
            vec![vec![1], vec![2, 3], vec![4], vec![4], vec![]],
        )
        .expect("well-formed diamond CFG");
        let exits: BTreeSet<_> = cfg.exit_blocks().collect();
        assert_eq!(exits, BTreeSet::from([4]));
    }

    #[test]
    fn exit_blocks_returns_multiple_ids_for_multiple_dead_ends() {
        // B0 -> B1, B0 -> B2 (both B1 and B2 are exits, e.g. two branches
        // that each end in their own `return`).
        let cfg = Cfg::new(0, vec!["a", "b", "c"], vec![vec![1, 2], vec![], vec![]])
            .expect("well-formed 3-block CFG");
        let exits: BTreeSet<_> = cfg.exit_blocks().collect();
        assert_eq!(exits, BTreeSet::from([1, 2]));
    }

    #[test]
    fn block_count_matches_the_number_of_blocks_constructed() {
        let cfg = Cfg::new(0, vec!["a", "b", "c"], vec![vec![1, 2], vec![], vec![]])
            .expect("well-formed 3-block CFG");
        assert_eq!(cfg.block_count(), 3);
    }

    #[test]
    fn block_returns_the_payload_at_that_id() {
        let cfg = Cfg::new(0, vec!["a", "b", "c"], vec![vec![1, 2], vec![], vec![]])
            .expect("well-formed 3-block CFG");
        assert_eq!(*cfg.block(1), "b");
    }

    #[test]
    fn predecessors_returns_every_block_with_an_edge_into_the_given_block() {
        // B0 -> B1, B0 -> B2, B1 -> B3, B2 -> B3 (diamond): B3's
        // predecessors are B1 and B2.
        let cfg = Cfg::new(
            0,
            vec!["a", "b", "c", "d"],
            vec![vec![1, 2], vec![3], vec![3], vec![]],
        )
        .expect("well-formed diamond CFG");
        let preds: BTreeSet<_> = cfg.predecessors(3).iter().copied().collect();
        assert_eq!(preds, BTreeSet::from([1, 2]));
        assert!(
            cfg.predecessors(0).is_empty(),
            "the entry block has no predecessors in this graph"
        );
    }
}
