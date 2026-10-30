//! The semantic layer over Tolk: types, call targets, reachability.
//!
//! The third level of the static contour. The first two — `tree-sitter` for
//! syntax and `sting-dataflow` for control flow — existed already, and what was
//! missing between them was semantics. That is a measured limit rather than a
//! guess: `AC` sits at 32.8% because it recognizes authorization by four
//! literal strings, and delegation into a method by a bare name matching
//! anywhere in the project.
//!
//! # Where the layer gets its knowledge
//!
//! The order of trust is followed literally: **ask the compiler first, infer
//! only where it is silent.** The reverse order is the road to a private
//! understanding of the language that diverges from the compiler's.
//!
//! The real `tolk`'s `symbolTypes.json` gives more than was expected:
//!
//! - a type table (`unique_types`) with real kinds, unions with TL-B prefixes
//!   included;
//! - struct fields with their types, so the type of `storage.counter` is known
//!   exactly;
//! - function signatures with parameter and result types;
//! - declaration spans, by which an AST node is matched to a function;
//! - the import graph.
//!
//! What it does not hold is the types of locals, of calls, and the call graph.
//! That is the layer's work.
//!
//! **The finding that shaped the inference**: the compiler hands parameters
//! back *lowered*. For `fun onInternalMessage(in: InMessage)` the table holds a
//! parameter named `in.body` of type `slice`, and `in` itself is not there at
//! all.
//!
//! Only **`in.body`** is lowered — the one path that really arrives on the
//! stack. `in.senderAddress` is absent from the parameter table even in
//! contracts that use it heavily: checked on
//! `corpus/clean/tolk-bench/01_jetton`, and pinned by the test
//! `sender_address_resolves_only_when_stdlib_sources_are_given`. The first
//! version of this comment claimed the opposite — that the compiler reports the
//! type of `in.senderAddress` directly — a generalisation from one example,
//! found by review.
//!
//! The type of `in.senderAddress` comes from elsewhere: `struct InMessage` is
//! declared in `tolk-stdlib/common.tolk`, which the compiler does compile
//! (`file_id` 0) and does not surface in the artefact's `declarations`. So the
//! fields are read from the stdlib's **source**, and for that its files have to
//! be handed to the model alongside the project's own. Without them the layer
//! answers honestly that it does not know.
//!
//! # The boundary of honesty
//!
//! Full Tolk type inference means rewriting the compiler's front end, and it is
//! not part of this layer. "I do not know" is therefore expressed by a type
//! rather than by `Option::None`: [`ResolvedType::Unknown`] carries an
//! [`UnknownReason`], and [`ResolvedType::Ambiguous`] is deliberately separate
//! from it — for a consumer these are different situations, and collapsing them
//! into one is exactly what costs `AC` its precision.
//!
//! # Who consumes it
//!
//! Nobody yet, and that is deliberate: the layer exists and is tested, and
//! moving the detectors onto it is separate work. Changing the layer and the
//! detectors at once would move `corpus/baseline.json`'s numbers in a way
//! nobody could explain.

mod model;
mod types;

pub use model::{
    FileSemantics, FunctionRef, ResolvedCall, SemanticModel, SourceUnit, UnresolvedCall,
};
pub use types::{ResolvedType, TypeRef, UnknownReason};
