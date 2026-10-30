//! `sting-abi-decode` — a message body read as an operation with named values.
//!
//! What a chain gives a reader is a body: a run of bits opening with 32 that name an operation.
//! What an indexer gives is the same bits with the opcode pulled out as a number. Neither says what
//! the operation is, and that is the gap this crate closes: the opcode is looked up in the learned
//! operation dictionary, and the rest of the body is read against the fields that operation was
//! declared to carry.
//!
//! **A decode is a reading, not a fact.** An opcode is chosen by a contract's author rather than
//! derived from anything, so two contracts are free to use one number for two operations. When the
//! dictionary knows two, both are returned and neither is preferred: picking one would be inventing
//! the very fact the dataset says it does not have.
//!
//! **What this does not do.** It does not fetch anything — a body arrives as bytes, from wherever
//! its caller got them. It does not verify that the contract at the other end is the contract the
//! dictionary learned the opcode from, which is a claim about an address and not about a body. And
//! it stops at the first field whose shape the dataset states no reading for, reporting the fields
//! it did read and the one it stopped on, rather than skipping ahead and presenting a partial read
//! as a complete one.

use num_bigint::{BigInt, BigUint};
use sting_abi_dictionary::{FieldShape, OperationDictionary, OperationEntry};
use sting_address::TonAddress;
use tonlib_core::cell::{BagOfCells, CellParser};

/// How wide the opcode a body opens with is.
const OPCODE_BITS: usize = 32;

/// What a bounced message opens with.
///
/// Not an operation and not in any dictionary: it is TON's own message layout, and a body carrying
/// it is the message that was sent back rather than one somebody wrote. What follows the marker is
/// the first 256 bits of the body that bounced, which begins with its own opcode.
const BOUNCE_MARKER: u32 = 0xffff_ffff;

/// The operations TON itself defines, which no corpus declares because no contract has to.
///
/// Two so far, both read off the chain's own conventions rather than a contract: the bounce marker
/// above, and the plain text comment a wallet sends with a transfer. Each is here with what it is,
/// and nothing is added on a guess — an opcode common in traffic is not thereby a standard.
const STANDARD_OPERATIONS: &[(u32, &str)] = &[
    (BOUNCE_MARKER, "a bounced message"),
    (0x0000_0000, "a text comment"),
];

/// What TON's own conventions call this opcode, when they call it anything.
#[must_use]
pub fn standard_operation(opcode: u32) -> Option<&'static str> {
    STANDARD_OPERATIONS
        .iter()
        .find(|(known, _)| *known == opcode)
        .map(|(_, what)| *what)
}

/// What went wrong before any reading could happen.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The bytes are not a single-root bag of cells.
    #[error("the body is not a single-root bag of cells: {0}")]
    NotABody(String),
    /// The body is shorter than the opcode it must open with.
    ///
    /// A body of no bits at all is ordinary on TON — a plain transfer of value carries one — and is
    /// reported as such rather than as a failure of this crate.
    #[error("the body carries no opcode: {0} bit(s)")]
    NoOpcode(usize),
}

/// One value read off a body.
///
/// Numbers are carried as decimal strings rather than as machine integers: a TVM integer is up to
/// 257 bits wide, and a reader who is handed a truncated `amount` has been told something false.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    /// An unsigned integer, in decimal.
    Uint {
        /// The value.
        value: String,
    },
    /// A signed integer, in decimal.
    Int {
        /// The value.
        value: String,
    },
    /// An amount in nanotons or in a jetton's own units, in decimal.
    Coins {
        /// The value.
        value: String,
    },
    /// An address, in its raw `workchain:hex` form.
    ///
    /// Raw rather than user-friendly: the friendly form encodes flags a body does not carry, so
    /// rendering one would be adding a claim about bounceability nobody made.
    Address {
        /// The address.
        value: String,
    },
    /// A single bit.
    Bool {
        /// The value.
        value: bool,
    },
    /// A referenced cell, left unread, named by its hash.
    Cell {
        /// The cell's hash, in hex.
        hash: String,
    },
    /// An optional cell that was not there.
    NoCell,
    /// A fixed-width bitstring, in hex.
    Bits {
        /// The bits, in hex, byte-aligned as the parser returns them.
        hex: String,
    },
    /// Whatever was left of the body, measured rather than read.
    Remaining {
        /// How many bits were left.
        bits: usize,
        /// How many references were left.
        refs: usize,
    },
}

/// Why a reading stopped before the operation's last field.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum StoppedAt {
    /// The dataset states no reading for this field's shape.
    NoStatedReading {
        /// The field it stopped on.
        field: String,
        /// What the extractor called the shape.
        was: String,
    },
    /// The body ran out, or held something other than the field's shape.
    BodyDisagrees {
        /// The field it stopped on.
        field: String,
        /// What the cell parser said.
        detail: String,
    },
}

/// One candidate operation, as far as this body could be read as it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DecodedOperation {
    /// What the operation is called, as its contract declares it.
    pub name: String,
    /// The fields read, in the order the body carries them.
    pub fields: Vec<DecodedField>,
    /// Where the reading stopped, when it did not reach the end.
    pub stopped_at: Option<StoppedAt>,
    /// Which corpus contracts this operation was seen in.
    pub seen_in: Vec<String>,
}

/// One field read off a body.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DecodedField {
    /// The field's name as its contract declares it.
    pub name: String,
    /// What the body carried there.
    pub value: Value,
}

/// What a bounced body says about the message that bounced.
///
/// Its fields are deliberately not read. A bounce carries only the first 256 bits of the body it
/// returns, so every field past the first few is a field that was cut off — and a decoder that
/// presented those as values would be presenting the truncation as data.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Bounced {
    /// The opcode of the message that bounced.
    pub original_opcode: u32,
    /// What the dictionary calls that opcode, when it calls it anything.
    pub original_names: Vec<String>,
}

/// What a body turned out to be.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Decoded {
    /// The 32 bits the body opens with.
    pub opcode: u32,
    /// What TON's own conventions call this opcode, when they call it anything.
    pub standard: Option<String>,
    /// Every operation the dictionary knows for that opcode, each read as far as it goes.
    ///
    /// Empty when the dictionary knows the opcode as nothing at all — the `Unknown` answer, and the
    /// share of bodies that land here is what this dataset's coverage means.
    pub operations: Vec<DecodedOperation>,
    /// What bounced, when the body is a bounce.
    pub bounced: Option<Bounced>,
}

impl Decoded {
    /// Whether anything at all named this opcode: a dictionary, or the chain's own conventions.
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        self.operations.is_empty() && self.standard.is_none()
    }
}

/// Reads `body` as whatever operations `dictionary` knows its opcode to be.
///
/// The opcode is read from the body itself rather than taken from a caller's envelope: an indexer's
/// `opcode` field and the body it came with are two claims, and only one of them is the message.
///
/// # Errors
/// When the bytes are not a single-root bag of cells, or when the body is too short to carry an
/// opcode at all.
pub fn decode(body: &[u8], dictionary: &OperationDictionary) -> Result<Decoded, DecodeError> {
    let boc =
        BagOfCells::parse(body).map_err(|source| DecodeError::NotABody(source.to_string()))?;
    let root = boc
        .single_root()
        .map_err(|source| DecodeError::NotABody(source.to_string()))?;
    let mut opcode_parser = root.parser();
    let opcode = opcode_parser
        .load_u32(OPCODE_BITS)
        .map_err(|_| DecodeError::NoOpcode(root.bit_len()))?;

    let operations = dictionary
        .lookup(opcode)
        .map(|group| {
            group
                .iter()
                .map(|entry| read_as(root.parser(), entry))
                .collect()
        })
        .unwrap_or_default();
    let bounced = (opcode == BOUNCE_MARKER)
        .then(|| opcode_parser.load_u32(OPCODE_BITS).ok())
        .flatten()
        .map(|original_opcode| Bounced {
            original_opcode,
            original_names: dictionary
                .lookup(original_opcode)
                .map(|group| group.iter().map(|entry| entry.name.clone()).collect())
                .unwrap_or_default(),
        });
    Ok(Decoded {
        opcode,
        standard: standard_operation(opcode).map(str::to_string),
        operations,
        bounced,
    })
}

/// Reads one body as one candidate operation, from a parser positioned at the body's start.
fn read_as(mut parser: CellParser<'_>, entry: &OperationEntry) -> DecodedOperation {
    // The opcode again, and discarded: every candidate reads the same body from the same place, so
    // each gets its own parser rather than a shared one wound forward.
    let _ = parser.load_u32(OPCODE_BITS);

    let mut fields = Vec::new();
    let mut stopped_at = None;
    for field in &entry.fields {
        match read_field(&mut parser, &field.shape) {
            Ok(Some(value)) => fields.push(DecodedField {
                name: field.name.clone(),
                value,
            }),
            Ok(None) => {
                stopped_at = Some(StoppedAt::NoStatedReading {
                    field: field.name.clone(),
                    was: was_of(&field.shape).to_string(),
                });
                break;
            }
            Err(detail) => {
                stopped_at = Some(StoppedAt::BodyDisagrees {
                    field: field.name.clone(),
                    detail,
                });
                break;
            }
        }
    }

    DecodedOperation {
        name: entry.name.clone(),
        fields,
        stopped_at,
        seen_in: entry.seen_in.clone(),
    }
}

/// Reads one field, or says the shape has no stated reading (`Ok(None)`).
fn read_field(parser: &mut CellParser<'_>, shape: &FieldShape) -> Result<Option<Value>, String> {
    let value = match shape {
        FieldShape::Uint { bits } => Value::Uint {
            value: load_uint(parser, *bits)?.to_string(),
        },
        FieldShape::Int { bits } => Value::Int {
            value: load_int(parser, *bits)?.to_string(),
        },
        FieldShape::Coins => Value::Coins {
            value: parser
                .load_coins()
                .map_err(|error| stringify(&error))?
                .to_string(),
        },
        // Through the platform's own address model rather than the cell library's: one address
        // model, and the raw form is what a key and a log line carry.
        FieldShape::Address => Value::Address {
            value: TonAddress::from(parser.load_address().map_err(|error| stringify(&error))?)
                .to_raw_string(),
        },
        FieldShape::Bool => Value::Bool {
            value: parser.load_bit().map_err(|error| stringify(&error))?,
        },
        FieldShape::Cell => match parser
            .load_ref_cell_optional()
            .map_err(|error| stringify(&error))?
        {
            Some(cell) => Value::Cell {
                hash: hex::encode(cell.cell_hash().as_slice()),
            },
            None => Value::NoCell,
        },
        FieldShape::MaybeCell => match parser
            .load_maybe_cell_ref()
            .map_err(|error| stringify(&error))?
        {
            Some(cell) => Value::Cell {
                hash: hex::encode(cell.cell_hash().as_slice()),
            },
            None => Value::NoCell,
        },
        FieldShape::Bits { width } => Value::Bits {
            hex: hex::encode(
                &parser
                    .load_bits(usize::from(*width))
                    .map_err(|error| stringify(&error))?,
            ),
        },
        FieldShape::Remaining => {
            let rest = parser.load_remaining().map_err(|error| stringify(&error))?;
            Value::Remaining {
                bits: rest.bit_len(),
                refs: rest.references().len(),
            }
        }
        FieldShape::Opaque { .. } => return Ok(None),
    };
    Ok(Some(value))
}

/// An unsigned integer of `bits` bits.
fn load_uint(parser: &mut CellParser<'_>, bits: u16) -> Result<BigUint, String> {
    parser
        .load_uint(usize::from(bits))
        .map_err(|error| stringify(&error))
}

/// A signed integer of `bits` bits.
fn load_int(parser: &mut CellParser<'_>, bits: u16) -> Result<BigInt, String> {
    parser
        .load_int(usize::from(bits))
        .map_err(|error| stringify(&error))
}

/// What the extractor called a shape this dataset states no reading for.
fn was_of(shape: &FieldShape) -> &str {
    match shape {
        FieldShape::Opaque { was } => was,
        _ => "readable",
    }
}

/// The cell parser's own words, kept rather than replaced by ours.
fn stringify(error: &tonlib_core::cell::TonCellError) -> String {
    error.to_string()
}
