//! The method-id dictionary — this crate's entire "knowledge base": real
//! `(tvm_method_id -> name)` pairs learned from real Tier 1/Tier 2
//! extractions elsewhere in this workspace (`corpus/clean/*`, `corpus-abi/
//! clean/tact/`, and, over time, real registry verifications).
//! Direct parallel to Ethereum's `4byte.directory`: EVM function selectors
//! and TON get-method ids are both short hashes of a name (`keccak256(sig)
//! [:4]` vs. `crc16(name)`), and in both ecosystems the only way to reverse
//! that hash back into a name is a crowd-sourced dictionary of names
//! actually seen somewhere — there is no other way, the hash is one-way by
//! design.

use std::collections::BTreeMap;

use sting_abi_core::ContractSchema;

use crate::provenance::{CorpusSource, DatasetProvenance};

/// One real `(name, tvm_method_id)` pair, with provenance.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DictionaryEntry {
    /// The get-method's real name, as declared in the source it was learned
    /// from.
    pub name: String,
    /// How many parameters this get-method takes, in the source it was
    /// learned from — Tier 3 v1 only probes entries with `parameter_count
    /// == 0` (see `probe`'s own module doc comment for why: a real
    /// TVM call with no arguments supplied can't be told apart from "method
    /// doesn't exist" for a parameterized method, empirically confirmed
    /// against a real `get_wallet_address(owner)` call during design).
    /// Parameterized entries are still recorded here (informational, and
    /// ready for a future extension that supplies real argument values),
    /// just never probed.
    pub parameter_count: usize,
    /// Which contract labels (e.g. `"config/Config.tolk"`,
    /// `"jetton-minter/jetton-minter.tolk"`) this exact `(name,
    /// parameter_count)` pair was observed in — kept for transparency (a
    /// name seen in 40 real contracts is a stronger signal than one seen in
    /// a single synthetic fixture), not for anything the probing logic
    /// itself branches on.
    pub seen_in: Vec<String>,
    /// Heuristic classification of the real contract this entry was learned
    /// from, inferred from `seen_in`'s first label — see
    /// [`ContractKind::from_label`]. Exists to guard against a real,
    /// empirically confirmed risk (`docs/domains/abi-ir.md`,
    /// case `hikkB9r4zttC`): TON's `tvm_method_id` space is flat, not
    /// namespaced per contract class, so the *same* 32-bit-derived id can
    /// mean something real but different in a jetton contract than in a
    /// highload wallet — [`crate::probe::probe`] flags, rather than
    /// silently trusts, a probed target where recognized entries disagree
    /// on kind.
    pub contract_kind: ContractKind,
}

/// A heuristic classification of what kind of real contract a
/// [`DictionaryEntry`] was learned from — **not authoritative** (a real
/// project could be misnamed, or genuinely implement more than one
/// standard), only real enough to catch the concrete collision risk this
/// type exists for (see [`DictionaryEntry::contract_kind`]'s own doc
/// comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContractKind {
    /// Label contains "jetton" (TEP-74 minter/wallet family).
    Jetton,
    /// Label contains "nft" (TEP-62 collection/item family).
    Nft,
    /// Label contains "wallet" or "highload" — standard/highload wallet
    /// contracts, not the jetton-wallet half of TEP-74 (checked first, so
    /// a label like "jetton-wallet" resolves to [`Self::Jetton`], the more
    /// specific match, not this one).
    Wallet,
    /// Label contains "dns" (TON DNS resolver/collection family).
    Dns,
    /// No known keyword matched — genuinely unclassified, not evidence of
    /// anything. [`crate::probe::probe`] never treats two [`Self::Other`]
    /// entries as conflicting with each other, only a specific kind against
    /// another specific kind.
    Other,
}

impl ContractKind {
    /// Classifies a contract label by keyword, case-insensitively — first
    /// match in this fixed order wins.
    #[must_use]
    pub fn from_label(label: &str) -> Self {
        let lower = label.to_ascii_lowercase();
        if lower.contains("jetton") {
            Self::Jetton
        } else if lower.contains("nft") {
            Self::Nft
        } else if lower.contains("wallet") || lower.contains("highload") {
            Self::Wallet
        } else if lower.contains("dns") {
            Self::Dns
        } else {
            Self::Other
        }
    }
}

/// A real `tvm_method_id` collided during dictionary construction — either
/// two **different names** hashed to the same id, or the **same name** was
/// learned with two different `parameter_count`s (legal in TON: dispatch is
/// purely by numeric id, so nothing stops the same name from taking a
/// different real arity in a different contract). CRC16's 16-bit space
/// (`docs/abi-layer/critical` method-id convention, `(crc16(name) & 0xffff)
/// | 0x10000`) is small enough that a same-name collision is a real,
/// expected possibility once the dictionary has on the order of hundreds of
/// entries (birthday-paradox math: ~9% chance of at least one collision
/// already at ~110 entries), not a bug when it happens. [`MethodDictionary`]
/// keeps every colliding entry rather than silently picking one — an
/// ambiguous id must be reported as ambiguous to a caller, never resolved by
/// guessing.
pub type CollisionGroup = Vec<DictionaryEntry>;

/// This file format's own version.
///
/// A dictionary outlives the build that produced it — it is committed, shipped and probed against
/// months later — so the file says which shape it is in. A file with no version is a file whose
/// reader has to guess, and the guess is silent.
pub const DICTIONARY_SCHEMA_VERSION: &str = "1.1";

/// The command that rebuilds this file, named in the file itself.
///
/// A published artefact whose reader cannot tell how to reproduce it is an artefact nobody can
/// check. The command is here so that the file answers that question about itself.
pub const DICTIONARY_BUILT_BY: &str = "abi-layer dictionary-build";

/// Real `(tvm_method_id -> one-or-more-candidate-names)` knowledge base.
///
/// Ordered rather than hashed, and that is a property of the artefact rather than a preference: the
/// dictionary is published, and a published file has to rebuild byte for byte from the corpus it
/// was built from. A hash map serialises in whatever order it happens to iterate, which made two
/// builds of the same corpus produce two different files that meant the same thing — a diff nobody
/// can review and a rebuild nobody can verify.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MethodDictionary {
    /// What this file says about itself: its shape, its licence, its corpora.
    #[serde(flatten)]
    provenance: DatasetProvenance,
    entries: BTreeMap<i64, CollisionGroup>,
}

impl Default for MethodDictionary {
    fn default() -> Self {
        Self {
            provenance: DatasetProvenance::new(DICTIONARY_SCHEMA_VERSION, DICTIONARY_BUILT_BY),
            entries: BTreeMap::new(),
        }
    }
}

impl MethodDictionary {
    /// An empty dictionary.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What this file says about itself.
    #[must_use]
    pub fn provenance(&self) -> &DatasetProvenance {
        &self.provenance
    }

    /// Which version of the file format this dictionary is in.
    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.provenance.schema_version
    }

    /// What a consumer may do with this data.
    #[must_use]
    pub fn licence(&self) -> &str {
        &self.provenance.licence
    }

    /// The command that rebuilds this file.
    #[must_use]
    pub fn built_by(&self) -> &str {
        &self.provenance.built_by
    }

    /// Which corpora this dictionary was learned from.
    #[must_use]
    pub fn sources(&self) -> &[CorpusSource] {
        &self.provenance.sources
    }

    /// Records which corpora this dictionary was learned from.
    pub fn record_sources(&mut self, sources: impl IntoIterator<Item = CorpusSource>) {
        self.provenance.record_sources(sources);
    }

    /// Learns every get-method in `schema`, tagging each with `source_label`
    /// for provenance. Safe to call repeatedly over an entire corpus —
    /// re-seeing the same `(id, name)` pair just appends `source_label` to
    /// that entry's `seen_in` (deduplicated), it does not create a
    /// duplicate entry or inflate a collision count.
    pub fn learn_from_schema(&mut self, schema: &ContractSchema, source_label: &str) {
        for method in &schema.get_methods {
            let Some(id) = method.tvm_method_id else {
                continue;
            };
            self.learn_one(id, &method.name, method.parameters.len(), source_label);
        }
    }

    /// Matches on `(name, parameter_count)`, not `name` alone — TON dispatch
    /// is purely by numeric id, so the same name is legally free to appear
    /// with a different real arity in a different contract. Treating those
    /// as the same entry would silently keep whichever `parameter_count` was
    /// learned first and could offer a method up as "unambiguous,
    /// zero-argument" in [`Self::probeable_entries`] even though most real
    /// instances of that name actually take arguments. Recording them as
    /// separate [`DictionaryEntry`] values under the same id instead makes
    /// the id's group `len() > 1`, which the existing collision machinery
    /// already excludes from probing — an arity disagreement is handled by
    /// the same "ambiguous, don't guess" rule as a true name collision.
    fn learn_one(&mut self, id: i64, name: &str, parameter_count: usize, source_label: &str) {
        let group = self.entries.entry(id).or_default();
        if let Some(existing) = group
            .iter_mut()
            .find(|e| e.name == name && e.parameter_count == parameter_count)
        {
            if !existing.seen_in.iter().any(|s| s == source_label) {
                existing.seen_in.push(source_label.to_string());
            }
            return;
        }
        group.push(DictionaryEntry {
            name: name.to_string(),
            parameter_count,
            seen_in: vec![source_label.to_string()],
            contract_kind: ContractKind::from_label(source_label),
        });
    }

    /// Puts every list into one order, so that a build of one corpus produces one file.
    ///
    /// Two things arrive in whatever order the walk happened to take them: which contracts a name
    /// was seen in, and which names collided on one id. Neither carries meaning in its order, and
    /// both would otherwise make a rebuild differ from the file it was meant to reproduce.
    ///
    /// Called by [`Self::save`] rather than left to the caller: an order a writer has to remember
    /// is an order the next writer forgets.
    fn sort(&mut self) {
        for group in self.entries.values_mut() {
            for entry in group.iter_mut() {
                entry.seen_in.sort();
                entry.seen_in.dedup();
            }
            group.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then(left.parameter_count.cmp(&right.parameter_count))
            });
        }
    }

    /// Every `(id, candidates)` pair — `candidates` has more than one entry
    /// exactly when that id is a real collision (see [`CollisionGroup`]).
    pub fn iter(&self) -> impl Iterator<Item = (i64, &CollisionGroup)> {
        self.entries.iter().map(|(id, group)| (*id, group))
    }

    /// Every **unambiguous** (`candidates.len() == 1`), **zero-argument**
    /// entry, ready to probe — the only kind [`mod@crate::probe`] can safely
    /// call (see that module's own doc comment).
    pub fn probeable_entries(&self) -> impl Iterator<Item = (i64, &DictionaryEntry)> {
        self.entries
            .iter()
            .filter_map(|(id, group)| match group.as_slice() {
                [only] if only.parameter_count == 0 => Some((*id, only)),
                _ => None,
            })
    }

    /// How many distinct ids are known (collisions count once).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` iff [`Self::len`] is 0.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every id where more than one distinct name was learned — real
    /// collisions, worth surfacing explicitly rather than only discovering
    /// them by accident while probing.
    pub fn collisions(&self) -> impl Iterator<Item = (i64, &CollisionGroup)> {
        self.entries
            .iter()
            .filter(|(_, g)| g.len() > 1)
            .map(|(id, g)| (*id, g))
    }
}

/// Errors loading/saving a [`MethodDictionary`].
#[derive(Debug, thiserror::Error)]
pub enum DictionaryIoError {
    /// Reading the dictionary file failed.
    #[error("failed to read {path}: {source}")]
    Read {
        /// The path that failed to read.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The dictionary file was not valid JSON / didn't match the expected
    /// shape.
    #[error("failed to parse {path} as a MethodDictionary: {source}")]
    Parse {
        /// The path that failed to parse.
        path: std::path::PathBuf,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
    },
    /// Writing the dictionary file failed.
    #[error("failed to write {path}: {source}")]
    Write {
        /// The path that failed to write.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Serializing the dictionary to JSON failed.
    #[error("failed to serialize the dictionary: {0}")]
    Serialize(#[source] serde_json::Error),
}

impl MethodDictionary {
    /// Loads a dictionary previously written by [`Self::save`].
    ///
    /// # Errors
    /// See [`DictionaryIoError`]'s variants.
    pub fn load(path: &std::path::Path) -> Result<Self, DictionaryIoError> {
        let text = std::fs::read_to_string(path).map_err(|source| DictionaryIoError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        serde_json::from_str(&text).map_err(|source| DictionaryIoError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Writes this dictionary as pretty JSON — the committed artifact
    /// consumed by [`Self::load`], in one order.
    ///
    /// Takes `&mut self` because ordering the lists is part of writing them: the file is published,
    /// and a published file has to rebuild byte for byte from the corpus it was built from.
    ///
    /// # Errors
    /// See [`DictionaryIoError`]'s variants.
    pub fn save(&mut self, path: &std::path::Path) -> Result<(), DictionaryIoError> {
        self.sort();
        let text = serde_json::to_string_pretty(self).map_err(DictionaryIoError::Serialize)?;
        std::fs::write(path, text).map_err(|source| DictionaryIoError::Write {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_abi_core::{
        ContractMetadata, ContractSchema, ExtractionTier, GetMethod, Param, SourceLanguage, TypeRef,
    };

    use super::{ContractKind, MethodDictionary};

    fn schema_with_methods(methods: Vec<GetMethod>) -> ContractSchema {
        ContractSchema {
            ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
            source_language: SourceLanguage::Tolk,
            extraction_tier: ExtractionTier::Compiled,
            contract_name: None,
            metadata: ContractMetadata::default(),
            storage: None,
            types: Vec::new(),
            incoming_internal: Vec::new(),
            incoming_external: Vec::new(),
            outgoing: Vec::new(),
            get_methods: methods,
            errors: Vec::new(),
            confidence: None,
            field_confidence: None,
        }
    }

    #[test]
    fn learning_the_same_method_twice_does_not_duplicate_or_collide() {
        let mut dict = MethodDictionary::new();
        let schema = schema_with_methods(vec![GetMethod {
            tvm_method_id: Some(106_029),
            name: "get_jetton_data".to_string(),
            parameters: Vec::new(),
            return_ty: Some(TypeRef(0)),
        }]);
        dict.learn_from_schema(&schema, "contract_a");
        dict.learn_from_schema(&schema, "contract_b");

        assert_eq!(dict.len(), 1);
        assert_eq!(dict.collisions().count(), 0);
        let (_, group) = dict.iter().next().expect("one entry");
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].seen_in, vec!["contract_a", "contract_b"]);
    }

    #[test]
    fn from_label_classifies_by_keyword_jetton_first() {
        assert_eq!(
            ContractKind::from_label("jetton-minter/jetton-minter.tolk"),
            ContractKind::Jetton
        );
        // "jetton-wallet" contains both "jetton" and "wallet" — the more
        // specific match must win, checked first.
        assert_eq!(
            ContractKind::from_label("jetton-wallet/jetton-wallet.tolk"),
            ContractKind::Jetton
        );
        assert_eq!(
            ContractKind::from_label("nft-v1.1/NftItem.tolk"),
            ContractKind::Nft
        );
        assert_eq!(
            ContractKind::from_label("highload-v3.1/messages.tolk"),
            ContractKind::Wallet
        );
        assert_eq!(
            ContractKind::from_label("w5.2/WalletV5.tolk"),
            ContractKind::Wallet
        );
        assert_eq!(
            ContractKind::from_label("dns/DnsItem.tolk"),
            ContractKind::Dns
        );
        assert_eq!(
            ContractKind::from_label("config/Config.tolk"),
            ContractKind::Other
        );
    }

    #[test]
    fn learning_from_a_schema_records_the_label_derived_contract_kind() {
        let mut dict = MethodDictionary::new();
        dict.learn_one(
            106_029,
            "get_jetton_data",
            0,
            "jetton-minter/JettonMinter.tolk",
        );
        let (_, group) = dict.iter().next().expect("one entry");
        assert_eq!(group[0].contract_kind, ContractKind::Jetton);
    }

    #[test]
    fn two_different_names_at_the_same_id_are_a_real_collision_not_silently_resolved() {
        let mut dict = MethodDictionary::new();
        dict.learn_one(999, "method_one", 0, "a");
        dict.learn_one(999, "method_two", 0, "b");

        assert_eq!(dict.len(), 1);
        assert_eq!(dict.collisions().count(), 1);
        // A colliding id must never appear as probeable — ambiguous, not
        // resolvable by guessing.
        assert_eq!(dict.probeable_entries().count(), 0);
    }

    #[test]
    fn the_same_name_learned_with_two_different_arities_is_ambiguous_not_probeable() {
        // Legal in TON (dispatch is purely by numeric id): the same name can
        // be zero-argument in one real contract and take an argument in
        // another. Silently keeping the first-seen `parameter_count` would
        // risk offering this id up as "unambiguous, zero-argument" even
        // though most real instances actually take arguments.
        let mut dict = MethodDictionary::new();
        dict.learn_one(555, "get_thing", 0, "contract_a");
        dict.learn_one(555, "get_thing", 1, "contract_b");

        assert_eq!(dict.len(), 1);
        let (_, group) = dict.iter().next().expect("one id, two entries");
        assert_eq!(group.len(), 2, "distinct arities must not be merged");
        assert_eq!(
            dict.probeable_entries().count(),
            0,
            "an arity disagreement must be treated as ambiguous, not probed"
        );
    }

    #[test]
    fn parameterized_methods_are_recorded_but_not_probeable() {
        let mut dict = MethodDictionary::new();
        let schema = schema_with_methods(vec![GetMethod {
            tvm_method_id: Some(103_289),
            name: "get_wallet_address".to_string(),
            parameters: vec![Param {
                name: "owner_address".to_string(),
                ty: TypeRef(0),
            }],
            return_ty: Some(TypeRef(0)),
        }]);
        dict.learn_from_schema(&schema, "jetton-wallet");

        assert_eq!(dict.len(), 1);
        assert_eq!(
            dict.probeable_entries().count(),
            0,
            "parameterized entries must not be probeable"
        );
    }

    #[test]
    fn round_trips_through_json() {
        let mut dict = MethodDictionary::new();
        dict.learn_one(106_029, "get_jetton_data", 0, "usdt");
        let dir = tempfile::tempdir().expect("scratch dir");
        let path = dir.path().join("dict.json");
        dict.save(&path).expect("save");
        let loaded = MethodDictionary::load(&path).expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.probeable_entries().count(), 1);
    }
}
