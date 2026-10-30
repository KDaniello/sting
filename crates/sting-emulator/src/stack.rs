//! Minimal decoder for a `run_get_method` response's `VmStack` (the `stack`
//! field of [`crate::RunGetMethodResponse`]) — `sting-emulator`
//! deliberately does not decode individual `VmStackValue`s beyond the
//! 24-bit depth prefix (see that crate's own doc comment on
//! `RunGetMethodResponse::stack_depth`), so any caller that needs an actual
//! return value writes its own decoder; this is that decoder, scoped to
//! exactly what `docs/domains/scamcheck.md` needs so far.
//!
//! Bit-level encoding verified directly against `ton-blockchain/ton`'s
//! `crypto/vm/stack.cpp` `StackEntry::serialize` (2026-07-23), not assumed
//! from memory: `vm_stk_null#00` (8-bit tag, no data, no reference),
//! `vm_stk_tinyint#01` (8-bit tag + 64-bit signed value, no reference),
//! `vm_stk_cell#03` (8-bit tag + one cell reference, no inline data beyond
//! the tag), `vm_stk_slice#04` (8-bit tag + the TL-B `VmCellSlice` shape —
//! `st_bits`/`end_bits`/`st_ref`/`end_ref` as 10/10/3/3 bits — + one cell
//! reference to the sliced cell). The outer list is a right-nested
//! structure (`vm_stk_cons#_ {n:#} rest:^(VmStackList n) tos:VmStackValue`):
//! each level's cell holds one value (`tos`) inline plus a reference to the
//! next level down (`rest`) — always the *first* reference in a level's
//! cell regardless of `tos`'s own type, since `rest` is declared before
//! `tos` in the TL-B record and reference fields are collected in
//! declaration order.
//!
//! The 257-bit full-int and NaN encodings (`vm_stk_int#0201_`/
//! `vm_stk_nan#02ff`, sharing an 8-bit `0x02` prefix disambiguated by the
//! next 7 bits) and the `builder`/`continuation`/`tuple` tags are
//! deliberately not decoded — no real get-method this crate calls has ever
//! needed one (`total_supply`/`mintable` on the one real jetton master
//! checked so far both fit the 64-bit tiny form, ground truth:
//! `corpus/fixtures/emulator/SOURCE.md`). Callers get
//! [`StackError::UnsupportedTag`] instead of a silently wrong decode.
//! Extend when a second real shape is needed, not speculatively — the same
//! discipline `sting_emulator::tlb`'s own module doc comment states.

use thiserror::Error;
use tonlib_core::cell::{ArcCell, BagOfCells, TonCellError};
use tonlib_core::tlb_types::block::msg_address::MsgAddress;

/// Errors decoding a `VmStack` `BoC`.
#[derive(Debug, Error)]
pub enum StackError {
    /// `stack_boc_base64` was not a well-formed single-root `BoC`.
    #[error("failed to parse stack as a BoC: {0}")]
    Boc(#[source] TonCellError),
    /// The stack has fewer than `needed` entries.
    #[error("stack has {actual} entries, need at least {needed}")]
    TooShort {
        /// The stack's actual depth.
        actual: u32,
        /// The minimum depth the caller needed.
        needed: u32,
    },
    /// A `VmStackValue` tag this decoder does not support (see this
    /// module's doc comment for which tags are covered and why), or a
    /// value of the wrong shape was asked to convert to something it is
    /// not (e.g. [`StackValue::as_msg_address`] on a non-slice value).
    #[error("unsupported VmStackValue tag {tag:#04x} at stack index {index}")]
    UnsupportedTag {
        /// The tag byte encountered (or a value's own tag, for a shape
        /// mismatch reported by a conversion method).
        tag: u8,
        /// Which stack index this happened at.
        index: u32,
    },
    /// A cell-level failure at a specific stack index — reading one while
    /// decoding, or building one while encoding an argument
    /// ([`encode_address_arg`]).
    #[error("cell error at stack index {index}: {source}")]
    Cell {
        /// Which stack index this happened at.
        index: u32,
        /// The underlying cell failure.
        #[source]
        source: TonCellError,
    },
}

/// One decoded stack entry, restricted to the shapes this module needs
/// (see the module doc comment).
#[derive(Debug, Clone)]
pub enum StackValue {
    /// `vm_stk_null#00`.
    Null,
    /// `vm_stk_tinyint#01` — the only integer shape decoded, see the module
    /// doc comment.
    Int(i64),
    /// `vm_stk_cell#03` — the referenced cell, undecoded.
    Cell(ArcCell),
    /// `vm_stk_slice#04` — the referenced base cell plus the bit offset
    /// (`st_bits`) into it where this slice's window actually starts.
    ///
    /// **Known limitation**: `end_bits`/`start_ref`/`end_ref` (the rest of
    /// the `VmCellSlice` shape) are read off the wire to stay correctly
    /// positioned but not kept here — only `start_bits` is. This is safe
    /// for [`StackValue::as_msg_address`] today because `MsgAddress` is
    /// self-delimiting (its own tag/length bits stop the read at the right
    /// place, confirmed against a real slice windowed into a larger shared
    /// cell in `tests/mint_rights.rs`). A *future* consumer that reads
    /// non-self-delimiting content (e.g. raw bytes of a `jetton_content`
    /// slice) from a [`StackValue::Slice`] must not reuse this shape as-is
    /// — it would silently read past the real end of the window into
    /// unrelated bits of the shared cell. Add `end_bits` when that
    /// consumer is real, not speculatively.
    Slice {
        /// The sliced cell's base (referenced) cell.
        cell: ArcCell,
        /// Bits to skip from the start of `cell` before this slice's own
        /// content begins.
        start_bits: usize,
    },
}

impl StackValue {
    /// Tag byte this value would have been encoded with — used only to
    /// build a useful [`StackError::UnsupportedTag`] when a conversion
    /// method is called on the wrong shape.
    fn tag(&self) -> u8 {
        match self {
            Self::Null => 0x00,
            Self::Int(_) => 0x01,
            Self::Cell(_) => 0x03,
            Self::Slice { .. } => 0x04,
        }
    }

    /// Reads this value as a `MsgAddress`, per TEP-74's `slice
    /// admin_address` convention.
    ///
    /// # Errors
    /// Returns [`StackError::UnsupportedTag`] if this is not a
    /// [`StackValue::Slice`]; see [`StackError`] for cell-parse failures.
    pub fn as_msg_address(&self, index: u32) -> Result<MsgAddress, StackError> {
        let Self::Slice { cell, start_bits } = self else {
            return Err(StackError::UnsupportedTag {
                tag: self.tag(),
                index,
            });
        };
        let mut parser = cell.parser();
        parser
            .skip_bits(*start_bits)
            .map_err(|source| StackError::Cell { index, source })?;
        parser
            .load_msg_address()
            .map_err(|source| StackError::Cell { index, source })
    }

    /// Reads this value as the 64-bit tiny int form (see the module doc
    /// comment for why only that int shape is supported).
    ///
    /// # Errors
    /// Returns [`StackError::UnsupportedTag`] (carrying this value's own
    /// real tag, not a placeholder) if this is not a [`StackValue::Int`].
    pub fn as_int(&self, index: u32) -> Result<i64, StackError> {
        match self {
            Self::Int(value) => Ok(*value),
            other => Err(StackError::UnsupportedTag {
                tag: other.tag(),
                index,
            }),
        }
    }
}

/// Decodes the `index`-th value (0 = the first value returned by the
/// get-method) of a `run_get_method` response's stack.
///
/// # Errors
/// See [`StackError`].
pub fn decode_entry(stack_boc_base64: &str, index: u32) -> Result<StackValue, StackError> {
    let bag = BagOfCells::parse_base64(stack_boc_base64).map_err(StackError::Boc)?;
    let root: ArcCell = bag.single_root().map_err(StackError::Boc)?.clone();

    let depth = {
        let mut p = root.parser();
        p.load_u32(24)
            .map_err(|source| StackError::Cell { index, source })?
    };
    if index >= depth {
        return Err(StackError::TooShort {
            actual: depth,
            needed: index + 1,
        });
    }

    let mut current = root;
    // This level's `tos` represents stack index `level - 1`; the outermost
    // cell (`level == depth`) holds the *last*-pushed value, i.e. the
    // highest index.
    let mut level = depth;
    let mut skip_depth_prefix = true;
    loop {
        let mut p = current.parser();
        if skip_depth_prefix {
            // Re-reads the same 24-bit prefix already read above — reading
            // is non-destructive to the underlying `Cell`, only to this
            // fresh `CellParser`'s own cursor, and only the root cell has
            // this prefix (`VmStackList n` does not re-store `n`).
            p.load_u32(24)
                .map_err(|source| StackError::Cell { index, source })?;
            skip_depth_prefix = false;
        }
        let this_index = level - 1;
        let tag = p.load_u8(8).map_err(|source| StackError::Cell {
            index: this_index,
            source,
        })?;

        let slice_start_bits = if tag == 0x04 {
            let start_bits = p.load_u16(10).map_err(|source| StackError::Cell {
                index: this_index,
                source,
            })?;
            let _end_bits = p.load_u16(10).map_err(|source| StackError::Cell {
                index: this_index,
                source,
            })?;
            let _start_ref = p.load_u8(3).map_err(|source| StackError::Cell {
                index: this_index,
                source,
            })?;
            let _end_ref = p.load_u8(3).map_err(|source| StackError::Cell {
                index: this_index,
                source,
            })?;
            Some(usize::from(start_bits))
        } else {
            None
        };
        let tiny_int = if tag == 0x01 {
            Some(p.load_i64(64).map_err(|source| StackError::Cell {
                index: this_index,
                source,
            })?)
        } else {
            None
        };

        // `rest` is always the first reference in field declaration order
        // (`rest:^(VmStackList n) tos:VmStackValue`), regardless of `tos`'s
        // own type — see the module doc comment.
        let rest = p.next_reference().map_err(|source| StackError::Cell {
            index: this_index,
            source,
        })?;
        let own_ref = if matches!(tag, 0x03 | 0x04) {
            Some(p.next_reference().map_err(|source| StackError::Cell {
                index: this_index,
                source,
            })?)
        } else {
            None
        };

        if this_index == index {
            return match tag {
                0x00 => Ok(StackValue::Null),
                0x01 => Ok(StackValue::Int(tiny_int.unwrap_or_default())),
                0x03 => own_ref
                    .map(StackValue::Cell)
                    .ok_or(StackError::UnsupportedTag {
                        tag,
                        index: this_index,
                    }),
                0x04 => match (own_ref, slice_start_bits) {
                    (Some(cell), Some(start_bits)) => Ok(StackValue::Slice { cell, start_bits }),
                    _ => Err(StackError::UnsupportedTag {
                        tag,
                        index: this_index,
                    }),
                },
                other => Err(StackError::UnsupportedTag {
                    tag: other,
                    index: this_index,
                }),
            };
        }
        current = rest;
        level -= 1;
    }
}

/// Encodes a single-argument `VmStack` `BoC` holding one address, for
/// calling a get-method that takes one `slice owner_address` parameter
/// (e.g. TEP-89's `get_wallet_address`) via
/// [`crate::TvmEmulator::run_get_method`]. The symmetric
/// counterpart of [`decode_entry`]'s slice handling — same `vm_stk_slice#04`
/// shape, `st_bits = 0` (the whole referenced cell is the address, nothing
/// windowed), terminated by an empty `vm_stk_nil` `rest`.
///
/// # Errors
/// Returns an error only if cell construction overflows (should not happen
/// for a single, small address value).
pub fn encode_address_arg(address: &tonlib_core::TonAddress) -> Result<String, StackError> {
    use tonlib_core::cell::CellBuilder;

    let map_cell = |source| StackError::Cell { index: 0, source };

    let mut addr_cell = CellBuilder::new();
    addr_cell.store_address(address).map_err(map_cell)?;
    let addr_cell = addr_cell.build().map_err(map_cell)?;
    let addr_bits = addr_cell.bit_len();

    let nil_cell = CellBuilder::new().build().map_err(map_cell)?;

    let addr_bits = u16::try_from(addr_bits).unwrap_or(u16::MAX);
    let mut slice_cell = CellBuilder::new();
    slice_cell.store_u8(8, 0x04).map_err(map_cell)?;
    slice_cell.store_u16(10, 0).map_err(map_cell)?; // st_bits: window starts at the cell's first bit
    slice_cell.store_u16(10, addr_bits).map_err(map_cell)?; // end_bits: window covers the whole cell
    slice_cell.store_u8(3, 0).map_err(map_cell)?; // st_ref
    slice_cell.store_u8(3, 0).map_err(map_cell)?; // end_ref
    slice_cell
        .store_reference(&nil_cell.into())
        .map_err(map_cell)?; // rest (ref[0])
    slice_cell
        .store_reference(&addr_cell.into())
        .map_err(map_cell)?; // this value's own ref (ref[1])
    let slice_cell = slice_cell.build().map_err(map_cell)?;

    let mut root = CellBuilder::new();
    root.store_u32(24, 1).map_err(map_cell)?; // depth = 1
    root.store_cell_data(&slice_cell).map_err(map_cell)?;
    root.store_references(slice_cell.references())
        .map_err(map_cell)?;
    let root = root.build().map_err(map_cell)?;

    let bytes = BagOfCells::from_root(root)
        .serialize(false)
        .map_err(map_cell)?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        bytes,
    ))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use tonlib_core::cell::{BagOfCells, CellBuilder};

    use super::{StackValue, decode_entry};

    /// Hand-builds a `VmStack` `BoC` holding exactly the shapes this module
    /// supports, in the same order TEP-74's `get_jetton_data()` returns
    /// them (`int, int, slice(addr_none), cell, cell`) — a synthetic input
    /// with a known-by-construction correct decode, independent of any
    /// real emulator run (that cross-check lives in
    /// `crates/sting-scamcheck/tests/mint_rights.rs`).
    fn build_stack_boc(values_top_to_bottom_reversed: bool) -> String {
        // Innermost first: index 4 (cell), 3 (cell), 2 (slice/addr_none),
        // 1 (int -1), 0 (int 1000) — built bottom-up since each level
        // references the level below it.
        assert!(
            !values_top_to_bottom_reversed,
            "kept as a documented parameter for clarity at call sites, not actually varied"
        );

        // addr_none slice: a cell whose only content is the 2-bit `00`
        // MsgAddress tag, referenced by a `vm_stk_slice#04` value with
        // `st_bits = 0`.
        let mut addr_none_cell = CellBuilder::new();
        addr_none_cell.store_bit(false).expect("build");
        addr_none_cell.store_bit(false).expect("build");
        let addr_none_cell = addr_none_cell.build().expect("build");

        // `vm_stk_nil#_ = VmStackList 0` — a genuinely empty cell, the
        // terminator every real `VmStack` ends with (even the bottommost
        // `vm_stk_cons` still has a `rest` ref, pointing at this).
        let nil_cell = CellBuilder::new().build().expect("build");

        let mut level0 = CellBuilder::new(); // index 0: tinyint 1000
        level0.store_u8(8, 0x01).expect("build");
        level0.store_i64(64, 1000).expect("build");
        level0.store_reference(&nil_cell.into()).expect("build");
        let level0 = level0.build().expect("build");

        let mut level1 = CellBuilder::new(); // index 1: tinyint -1
        level1.store_u8(8, 0x01).expect("build");
        level1.store_i64(64, -1).expect("build");
        level1.store_reference(&level0.into()).expect("build");
        let level1 = level1.build().expect("build");

        let mut level2 = CellBuilder::new(); // index 2: slice(addr_none)
        level2.store_u8(8, 0x04).expect("build");
        level2.store_u16(10, 0).expect("build"); // st_bits
        level2.store_u16(10, 2).expect("build"); // end_bits
        level2.store_u8(3, 0).expect("build"); // st_ref
        level2.store_u8(3, 0).expect("build"); // end_ref
        level2.store_reference(&level1.into()).expect("build");
        level2
            .store_reference(&addr_none_cell.into())
            .expect("build");
        let level2 = level2.build().expect("build");

        let mut level3 = CellBuilder::new(); // index 3: cell (opaque)
        level3.store_u8(8, 0x03).expect("build");
        level3.store_reference(&level2.into()).expect("build");
        let opaque_content = CellBuilder::new().build().expect("build");
        level3
            .store_reference(&opaque_content.into())
            .expect("build");
        let level3 = level3.build().expect("build");

        let mut root = CellBuilder::new(); // depth prefix + index 4: cell (opaque)
        root.store_u32(24, 5).expect("build");
        root.store_u8(8, 0x03).expect("build");
        root.store_reference(&level3.into()).expect("build");
        let opaque_content = CellBuilder::new().build().expect("build");
        root.store_reference(&opaque_content.into()).expect("build");
        let root = root.build().expect("build");

        let bytes = BagOfCells::from_root(root)
            .serialize(false)
            .expect("serialize");
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    }

    #[test]
    fn decodes_a_tiny_int_by_index() {
        let boc = build_stack_boc(false);
        let value = decode_entry(&boc, 0).expect("decode index 0");
        assert!(matches!(value, StackValue::Int(1000)));
        let value = decode_entry(&boc, 1).expect("decode index 1");
        assert!(matches!(value, StackValue::Int(-1)));
    }

    #[test]
    fn decodes_addr_none_slice_at_the_tep74_admin_address_index() {
        let boc = build_stack_boc(false);
        let value = decode_entry(&boc, 2).expect("decode index 2");
        let StackValue::Slice { .. } = &value else {
            panic!("expected a Slice value, got {value:?}");
        };
        let addr = value.as_msg_address(2).expect("parse MsgAddress");
        assert!(
            matches!(
                addr,
                tonlib_core::tlb_types::block::msg_address::MsgAddress::Ext(
                    tonlib_core::tlb_types::block::msg_address::MsgAddressExt::None(_)
                )
            ),
            "expected addr_none, got {addr:?}"
        );
    }

    #[test]
    fn out_of_range_index_is_a_clean_error_not_a_panic() {
        let boc = build_stack_boc(false);
        let err = decode_entry(&boc, 5).expect_err("index 5 is out of range for depth 5");
        assert!(matches!(
            err,
            super::StackError::TooShort {
                actual: 5,
                needed: 6
            }
        ));
    }
}
