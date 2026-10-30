//! `sting-abi-dictionary` — bytecode-only get-method recognition via a
//! learned method-id dictionary, no source ever read. Implements Tier 3 of
//! `docs/domains/abi-ir.md`'s tier model.
//!
//! **What this is, precisely** — a direct parallel to Ethereum's
//! `4byte.directory`, not a schema-inference engine: TON get-method ids are
//! `(crc16(name) & 0xffff) | 0x10000` (`docs/abi-layer/critical` method-id
//! convention, confirmed against real `tolk` output elsewhere in this
//! workspace) — a one-way hash of the name, same shape as an EVM function
//! selector (`keccak256(signature)[:4]`). There is no way to invert a hash
//! back into a name computationally; the only way, in either ecosystem, is
//! a dictionary of real names seen somewhere. [`MethodDictionary`] is that
//! dictionary, built from every real `ContractSchema` this workspace's own
//! Tier 1/Tier 2 extractors have ever produced; [`mod@probe`] is the "try
//! calling every dictionary entry against real on-chain bytecode and see
//! which ones respond" lookup, using the already-real, already-tested
//! `sting-emulator`.
//!
//! **What this is not** — see `docs/domains/method-dictionary.md`
//! and the design discussion that preceded this crate: pure bytecode has no
//! field *names*/*semantics*, full stop, for anything not already in the
//! dictionary. This module recovers **get-method presence + name**, for
//! **zero-argument, unambiguous** dictionary entries only (see [`mod@probe`]'s
//! own doc comment for exactly why parameterized entries and hash
//! collisions are excluded, not silently guessed through). It does not
//! recover storage layout, message-body fields, or anything about a
//! contract with no name in the dictionary at all — that remains the
//! genuinely unsolved, information-theoretic ceiling.

//! **The second dataset the same corpus gives.** A get method is called by an id derived from its
//! name; a message is dispatched by the 32 bits its body opens with, and those bits are not derived
//! from anything — an author picks them. [`OperationDictionary`] is what those numbers were seen to
//! mean, learned from the same schemas as the method dictionary: an opcode, the operation it names,
//! and the fields that operation carries in the order a body carries them.

pub mod dictionary;
pub mod operations;
pub mod probe;
pub mod provenance;
pub mod tact;

pub use dictionary::{
    CollisionGroup, DICTIONARY_BUILT_BY, DICTIONARY_SCHEMA_VERSION, DictionaryEntry,
    DictionaryIoError, MethodDictionary,
};
pub use operations::{
    FieldShape, OPERATION_BUILT_BY, OPERATION_SCHEMA_VERSION, OperationDictionary, OperationEntry,
    OperationField, OperationGroup,
};
pub use probe::{ProbeError, ProbeReport, ProbeTarget, RecognizedMethod, probe};
pub use provenance::{CorpusSource, DATASET_LICENCE, DatasetProvenance, UNSTATED};
pub use tact::{TactSignature, TactVerdict, signature_of};
