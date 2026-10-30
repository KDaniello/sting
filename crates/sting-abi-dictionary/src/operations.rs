//! Real `(message opcode -> one-or-more-candidate-operations)` knowledge base.
//!
//! The same idea as the method dictionary and a different fact. A get method is called by an id
//! derived from its name; a message is dispatched by the 32 bits its body opens with, and those
//! bits are not derived from anything — a contract's author picks them. So an opcode is not a hash
//! to be reversed but a number to be recognised, and recognising it means having seen it declared
//! in a contract somebody wrote.
//!
//! That makes the wire format readable: an opcode names an operation, and the operation names its
//! fields in order, so a body can be read as values with names rather than as a run of bits.
//!
//! What this does not do: it does not decode. It carries what a body is shaped like and leaves
//! reading one to its caller, because a dictionary that also parses is a dictionary whose consumers
//! have to link a cell library to look a number up. It also does not claim an opcode means one
//! thing: an author is free to reuse a number another contract already uses, so an entry is a list
//! of candidates rather than an answer, exactly as with a method-id collision.

use std::collections::BTreeMap;

use sting_abi_core::{ContractSchema, NamedType, TlbPrefix, TypeShape};

use crate::provenance::{CorpusSource, DatasetProvenance};

/// This file format's own version.
pub const OPERATION_SCHEMA_VERSION: &str = "1.0";

/// The command that rebuilds this file, named in the file itself.
///
/// The same walk that builds the method dictionary: one extraction, two datasets, because
/// extraction is what costs the minutes.
pub const OPERATION_BUILT_BY: &str = "abi-layer dictionary-build";

/// How wide a prefix has to be to be a message opcode.
///
/// A TL-B prefix is not always an opcode: a struct embedded as one arm of a union nested inside a
/// message carries a narrower one, and counting those as operations would fill the dictionary with
/// numbers no message ever opens with.
const OPCODE_BITS: u32 = 32;

/// How many names for a name this crate follows before deciding the table points at itself.
const MAX_ALIAS_DEPTH: usize = 16;

/// What one field of a message body is, as far as reading it off the wire goes.
///
/// The extractor's own type model is richer than this on purpose — it describes a language, this
/// describes a wire. Everything it cannot state as a fixed reading is [`FieldShape::Opaque`], which
/// carries the language-level name of what it gave up on rather than dropping the field: a decoder
/// that stops at a field it cannot read is honest, and one that silently skips it is not.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldShape {
    /// A fixed-width unsigned integer.
    Uint {
        /// Its width in bits.
        bits: u16,
    },
    /// A fixed-width signed integer.
    Int {
        /// Its width in bits.
        bits: u16,
    },
    /// The `coins` (`VarUInteger 16`) primitive.
    Coins,
    /// A TON address.
    Address,
    /// A single bit read as true or false.
    Bool,
    /// A reference to another cell, left unread.
    Cell,
    /// A cell that may not be there: one bit, and a reference when that bit is set.
    ///
    /// TL-B calls this `(Maybe ^Cell)`, and it is what an optional cell field compiles to. An
    /// optional address is deliberately not this shape: TON's own address encoding already carries
    /// an absent form, so `address?` is one address on the wire rather than a bit and an address.
    MaybeCell,
    /// A fixed-width opaque bitstring.
    Bits {
        /// Its width in bits.
        width: u16,
    },
    /// Whatever bits and references are left — the trailing payload of a body.
    Remaining,
    /// Something this format does not state a reading for, named as the extractor named it.
    Opaque {
        /// The language-level shape the extractor found.
        was: String,
    },
}

/// One named field of a message body, in declaration order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OperationField {
    /// The field's name as its contract declares it.
    pub name: String,
    /// What reading it off the wire takes.
    pub shape: FieldShape,
}

/// One candidate operation for an opcode: what it is called, what it carries, where it was seen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OperationEntry {
    /// The message type's name as declared.
    pub name: String,
    /// Its fields, in the order the body carries them.
    pub fields: Vec<OperationField>,
    /// Which corpus contracts this operation was seen in.
    pub seen_in: Vec<String>,
}

/// Every candidate operation one opcode has, in name order.
pub type OperationGroup = Vec<OperationEntry>;

/// Real `(opcode -> one-or-more-candidate-operations)` knowledge base.
///
/// Ordered rather than hashed, for the reason the method dictionary is: the file is published, and
/// a published file has to rebuild byte for byte from the corpus it was built from.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OperationDictionary {
    /// What this file says about itself: its shape, its licence, its corpora.
    #[serde(flatten)]
    provenance: DatasetProvenance,
    entries: BTreeMap<u32, OperationGroup>,
}

impl Default for OperationDictionary {
    fn default() -> Self {
        Self {
            provenance: DatasetProvenance::new(OPERATION_SCHEMA_VERSION, OPERATION_BUILT_BY),
            entries: BTreeMap::new(),
        }
    }
}

impl OperationDictionary {
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

    /// The same dictionary, rebuilt by a command other than the default one.
    ///
    /// A published artefact whose reader cannot tell how to reproduce it is an artefact nobody can
    /// check, and two datasets of one shape are not always written by one command.
    #[must_use]
    pub fn built_by(mut self, command: &str) -> Self {
        self.provenance.built_by = command.to_string();
        self
    }

    /// The same dictionary under a licence this repository does not set.
    ///
    /// Learning from somebody else's published sources does not make what is learned ours to
    /// relicense. A dictionary built from another repository says so in its own header.
    #[must_use]
    pub fn under_licence(mut self, licence: &str) -> Self {
        self.provenance = self.provenance.under_licence(licence);
        self
    }

    /// What a consumer may do with this data.
    #[must_use]
    pub fn licence(&self) -> &str {
        &self.provenance.licence
    }

    /// Records which corpora this dictionary was learned from.
    pub fn record_sources(&mut self, sources: impl IntoIterator<Item = CorpusSource>) {
        self.provenance.record_sources(sources);
    }

    /// How many distinct opcodes it knows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it knows nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every opcode and its candidates, in numeric order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &OperationGroup)> {
        self.entries.iter().map(|(opcode, group)| (*opcode, group))
    }

    /// What an opcode might be, or nothing when no corpus contract declares it.
    #[must_use]
    pub fn lookup(&self, opcode: u32) -> Option<&OperationGroup> {
        self.entries.get(&opcode)
    }

    /// Every opcode more than one operation claims.
    ///
    /// An opcode is chosen by a contract's author rather than derived from anything, so two authors
    /// choosing the same number is ordinary rather than exceptional — and a consumer told one name
    /// for an opcode two contracts declare has been told something this dataset does not know.
    pub fn collisions(&self) -> impl Iterator<Item = (u32, &OperationGroup)> {
        self.iter().filter(|(_, group)| group.len() > 1)
    }

    /// Learns every incoming message `schema` declares, tagged with `source_label`.
    ///
    /// Incoming only: an outgoing message is the same wire format seen from the other end, and the
    /// contract that sends one rarely declares what its recipient calls it.
    pub fn learn_from_schema(&mut self, schema: &ContractSchema, source_label: &str) {
        let messages = schema
            .incoming_internal
            .iter()
            .chain(schema.incoming_external.iter());
        for message in messages {
            // Through an alias first: a contract is free to name the set of messages it accepts,
            // and a name for a type is not a type — a wallet's external message reaches here as an
            // alias, and reading the alias itself would learn nothing.
            let Some(declared) = resolve_alias(schema, message.ty.0 as usize) else {
                continue;
            };
            // A contract names one type as what it accepts, and that type is either one message or
            // the set of them. A union carries no prefix of its own — its arms do — so reading only
            // the named type would learn nothing from every contract that declares its messages the
            // way a wallet does.
            if let TypeShape::Union { variants } = &declared.shape {
                for variant in variants {
                    let Some(arm) = resolve_alias(schema, variant.ty.0 as usize) else {
                        continue;
                    };
                    self.learn_message(schema, arm, variant.prefix, source_label);
                }
            } else if let Some(prefix) = declared.prefix {
                self.learn_message(schema, declared, prefix, source_label);
            }
        }
    }

    /// Learns one message type under the prefix that dispatches to it.
    fn learn_message(
        &mut self,
        schema: &ContractSchema,
        declared: &NamedType,
        prefix: TlbPrefix,
        source_label: &str,
    ) {
        if prefix.prefix_len != OPCODE_BITS {
            return;
        }
        let Ok(opcode) = u32::try_from(prefix.prefix_num) else {
            return;
        };
        let Some(name) = declared.name.clone() else {
            return;
        };
        let fields = fields_of(schema, declared);
        self.learn_one(opcode, name, fields, source_label);
    }

    /// Matches on the whole shape, not on the name alone: two contracts declaring one opcode under
    /// one name with different fields have declared two different operations, and merging them
    /// would produce an entry neither of them would recognise.
    fn learn_one(
        &mut self,
        opcode: u32,
        name: String,
        fields: Vec<OperationField>,
        source_label: &str,
    ) {
        let group = self.entries.entry(opcode).or_default();
        if let Some(existing) = group
            .iter_mut()
            .find(|entry| entry.name == name && entry.fields == fields)
        {
            if !existing.seen_in.iter().any(|seen| seen == source_label) {
                existing.seen_in.push(source_label.to_string());
            }
            return;
        }
        group.push(OperationEntry {
            name,
            fields,
            seen_in: vec![source_label.to_string()],
        });
    }

    /// Takes in everything another dictionary knows, keeping both sides' provenance.
    ///
    /// What comes out is a lookup rather than a dataset: the header stays this dictionary's, and a
    /// merge of two files under two licences is not a file — which is why the two are kept apart on
    /// disk and joined only in memory, by whoever is answering a question with both.
    pub fn absorb(&mut self, other: &Self) {
        for (opcode, group) in other.iter() {
            for entry in group {
                for contract in &entry.seen_in {
                    self.learn_one(opcode, entry.name.clone(), entry.fields.clone(), contract);
                }
            }
        }
    }

    /// Puts every list into one order, so that a build of one corpus produces one file.
    fn sort(&mut self) {
        for group in self.entries.values_mut() {
            for entry in group.iter_mut() {
                entry.seen_in.sort();
                entry.seen_in.dedup();
            }
            group.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then(left.fields.len().cmp(&right.fields.len()))
            });
        }
    }

    /// Reads a dictionary written by [`Self::save`].
    ///
    /// # Errors
    /// See [`crate::DictionaryIoError`]'s variants.
    pub fn load(path: &std::path::Path) -> Result<Self, crate::DictionaryIoError> {
        let text =
            std::fs::read_to_string(path).map_err(|source| crate::DictionaryIoError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        serde_json::from_str(&text).map_err(|source| crate::DictionaryIoError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Writes this dictionary as pretty JSON, in one order.
    ///
    /// Takes `&mut self` because ordering the lists is part of writing them: the file is published,
    /// and a published file has to rebuild byte for byte from the corpus it was built from.
    ///
    /// # Errors
    /// See [`crate::DictionaryIoError`]'s variants.
    pub fn save(&mut self, path: &std::path::Path) -> Result<(), crate::DictionaryIoError> {
        self.sort();
        let text =
            serde_json::to_string_pretty(self).map_err(crate::DictionaryIoError::Serialize)?;
        std::fs::write(path, text).map_err(|source| crate::DictionaryIoError::Write {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// The type an index names, with any chain of aliases followed to what it stands for.
///
/// Bounded rather than recursive without end: a type table is somebody else's output, and a cycle
/// in one is a file this crate declines to loop on.
fn resolve_alias(schema: &ContractSchema, index: usize) -> Option<&NamedType> {
    let mut at = index;
    for _ in 0..MAX_ALIAS_DEPTH {
        let named = schema.types.get(at)?;
        match named.shape {
            TypeShape::Alias { target } => at = target.0 as usize,
            _ => return Some(named),
        }
    }
    None
}

/// Reads one message type's fields into the flat, wire-level form this dataset carries.
fn fields_of(schema: &ContractSchema, declared: &NamedType) -> Vec<OperationField> {
    let TypeShape::Struct { fields } = &declared.shape else {
        return Vec::new();
    };
    fields
        .iter()
        .map(|field| OperationField {
            name: field.name.clone(),
            shape: shape_of(schema, field.ty.0 as usize),
        })
        .collect()
}

/// What reading one field takes, resolved through aliases down to a wire-level shape.
///
/// An alias is followed because it is a name for a type rather than a type: `Timestamp = uint32` is
/// four bytes on the wire whatever it is called. Everything with a structure of its own — a struct,
/// a union, a map, an optional — stops here as [`FieldShape::Opaque`], because reading it takes a
/// decision this dataset does not make: how the language it came from lays it out.
fn shape_of(schema: &ContractSchema, index: usize) -> FieldShape {
    let Some(named) = schema.types.get(index) else {
        return FieldShape::Opaque {
            was: "missing".to_string(),
        };
    };
    match &named.shape {
        TypeShape::Int { bits, signed } => match bits {
            Some(bits) if *signed => FieldShape::Int { bits: *bits },
            Some(bits) => FieldShape::Uint { bits: *bits },
            None => FieldShape::Opaque {
                was: "int of no declared width".to_string(),
            },
        },
        TypeShape::Bool => FieldShape::Bool,
        TypeShape::Coins => FieldShape::Coins,
        TypeShape::Address => FieldShape::Address,
        TypeShape::Cell | TypeShape::CellPacked { .. } => FieldShape::Cell,
        TypeShape::Bits { width } => FieldShape::Bits { width: *width },
        TypeShape::Remaining | TypeShape::Slice => FieldShape::Remaining,
        TypeShape::Alias { target } => shape_of(schema, target.0 as usize),
        // An optional is two different wire forms depending on what is optional, and only two of
        // them are stated here. Both were read off the standard jetton transfer this dataset
        // learned from `corpus/clean/acton-contracts`: `sendExcessesTo: address?` is TEP-74's
        // `response_destination:MsgAddress`, and `customPayload: cell?` is its
        // `custom_payload:(Maybe ^Cell)`.
        TypeShape::Nullable { inner } => match shape_of(schema, inner.0 as usize) {
            FieldShape::Address => FieldShape::Address,
            FieldShape::Cell => FieldShape::MaybeCell,
            _ => FieldShape::Opaque {
                was: "nullable".to_string(),
            },
        },
        TypeShape::Enum { encoded_as, .. } => shape_of(schema, encoded_as.0 as usize),
        other => FieldShape::Opaque {
            was: shape_name(other).to_string(),
        },
    }
}

/// What the extractor called a shape this dataset does not read.
const fn shape_name(shape: &TypeShape) -> &'static str {
    match shape {
        TypeShape::Void => "void",
        TypeShape::Builder => "builder",
        TypeShape::Str => "string",
        TypeShape::Nullable { .. } => "nullable",
        TypeShape::Array { .. } => "array",
        TypeShape::LispList { .. } => "list",
        TypeShape::Map { .. } => "map",
        TypeShape::Struct { .. } => "struct",
        TypeShape::Union { .. } => "union",
        TypeShape::Tensor { .. } => "tensor",
        _ => "unknown",
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
        ContractMetadata, ContractSchema, ExtractionTier, FieldRef, IR_SCHEMA_VERSION, MessageRef,
        NamedType, SourceLanguage, TlbPrefix, TypeRef, TypeShape,
    };

    use super::{FieldShape, OperationDictionary};

    /// A schema declaring one incoming message of `name` at `opcode`, carrying `fields` whose types
    /// are appended to the type table in order.
    fn schema_with_message(
        name: &str,
        opcode: u64,
        prefix_len: u32,
        fields: Vec<(&str, TypeShape)>,
    ) -> ContractSchema {
        let mut types = vec![NamedType {
            name: Some(name.to_string()),
            description: None,
            prefix: Some(TlbPrefix {
                prefix_num: opcode,
                prefix_len,
            }),
            shape: TypeShape::Struct { fields: Vec::new() },
        }];
        let mut refs = Vec::new();
        for (field_name, shape) in fields {
            types.push(NamedType {
                name: None,
                description: None,
                prefix: None,
                shape,
            });
            refs.push(FieldRef {
                name: field_name.to_string(),
                ty: TypeRef(u32::try_from(types.len() - 1).unwrap()),
            });
        }
        types[0].shape = TypeShape::Struct { fields: refs };

        ContractSchema {
            ir_schema_version: IR_SCHEMA_VERSION.to_string(),
            source_language: SourceLanguage::Tolk,
            extraction_tier: ExtractionTier::Compiled,
            contract_name: None,
            metadata: ContractMetadata::default(),
            storage: None,
            types,
            incoming_internal: vec![MessageRef { ty: TypeRef(0) }],
            incoming_external: Vec::new(),
            outgoing: Vec::new(),
            get_methods: Vec::new(),
            errors: Vec::new(),
            confidence: None,
            field_confidence: None,
        }
    }

    #[test]
    fn one_operation_seen_twice_is_one_entry_with_two_contracts() {
        let schema = schema_with_message("AskToBurn", 0x595f_07bc, 32, Vec::new());
        let mut dictionary = OperationDictionary::new();
        dictionary.learn_from_schema(&schema, "wallet_a.tolk");
        dictionary.learn_from_schema(&schema, "wallet_b.tolk");

        let group = dictionary.lookup(0x595f_07bc).expect("the opcode");
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].seen_in, vec!["wallet_a.tolk", "wallet_b.tolk"]);
        assert_eq!(dictionary.collisions().count(), 0);
    }

    #[test]
    fn two_contracts_claiming_one_opcode_are_two_candidates() {
        let mut dictionary = OperationDictionary::new();
        dictionary.learn_from_schema(
            &schema_with_message("AskToTransfer", 0x0f8a_7ea5, 32, Vec::new()),
            "ours.tolk",
        );
        dictionary.learn_from_schema(
            &schema_with_message("JettonTransfer", 0x0f8a_7ea5, 32, Vec::new()),
            "theirs.tolk",
        );

        assert_eq!(dictionary.collisions().count(), 1);
        assert_eq!(dictionary.lookup(0x0f8a_7ea5).expect("the opcode").len(), 2);
    }

    #[test]
    fn a_prefix_narrower_than_an_opcode_is_not_an_operation() {
        let mut dictionary = OperationDictionary::new();
        dictionary.learn_from_schema(
            &schema_with_message("ConfigProposal", 0xf3, 8, Vec::new()),
            "config.tolk",
        );

        assert!(dictionary.is_empty());
    }

    #[test]
    fn an_optional_address_is_an_address_and_an_optional_cell_is_a_maybe() {
        let mut schema = schema_with_message(
            "AskToTransfer",
            0x0f8a_7ea5,
            32,
            vec![
                ("sendExcessesTo", TypeShape::Nullable { inner: TypeRef(4) }),
                ("customPayload", TypeShape::Nullable { inner: TypeRef(5) }),
                ("unreadable", TypeShape::Nullable { inner: TypeRef(6) }),
            ],
        );
        // What each optional wraps, appended after the three optionals themselves.
        for shape in [TypeShape::Address, TypeShape::Cell, TypeShape::Str] {
            schema.types.push(NamedType {
                name: None,
                description: None,
                prefix: None,
                shape,
            });
        }

        let mut dictionary = OperationDictionary::new();
        dictionary.learn_from_schema(&schema, "wallet.tolk");

        let group = dictionary.lookup(0x0f8a_7ea5).expect("the opcode");
        let shapes: Vec<FieldShape> = group[0]
            .fields
            .iter()
            .map(|field| field.shape.clone())
            .collect();
        assert_eq!(
            shapes,
            vec![
                FieldShape::Address,
                FieldShape::MaybeCell,
                FieldShape::Opaque {
                    was: "nullable".to_string()
                },
            ]
        );
    }
}
