//! TEP-64 content-cell decoder (`docs/domains/scamcheck.md`,
//! open question: `tonlib-core` 0.26.11 has no built-in decoder for this —
//! confirmed by checking its `message`/`meta` modules directly before writing
//! this, not assumed — so this is hand-built, the same way `honeypot.rs`
//! hand-built TEP-74's storage layout instead of assuming a helper existed).
//!
//! Scope, stated explicitly: only the **off-chain** (`0x01`-prefixed) and
//! **bare/relative** (no prefix byte at all — the common TEP-62 convention
//! for an item's `individual_content`, meant to be resolved against the
//! collection's own base URI) shapes are decoded. **On-chain** content
//! (`0x00`-prefixed `HashmapE 256` dictionary of per-key snake cells) is
//! deliberately unsupported — every real fixture this crate has seen so far
//! (see `corpus/fixtures/emulator/SOURCE.md`'s NFT entries) is off-chain, and
//! decoding a dictionary correctly needs real ground truth to verify against,
//! which none of tonight's fixtures provide. Extend when a real on-chain
//! case is found, not speculatively — the same discipline `stack.rs`'s own
//! module doc comment already states for its own unsupported shapes.

use thiserror::Error;
use tonlib_core::cell::{ArcCell, TonCellError};

/// A decoded TEP-64 content cell, restricted to the two shapes this module
/// supports (see the module doc comment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NftContent {
    /// `0x01`-prefixed — an absolute URI, the remaining bytes decoded as
    /// UTF-8.
    OffChainUri(String),
    /// No recognized prefix byte (not `0x00`, not `0x01`) — the whole byte
    /// sequence decoded as UTF-8 as-is. Per TEP-62 convention this is
    /// typically a *relative* path meant to be resolved against the
    /// collection's own base URI, but this module does not perform that
    /// resolution itself (the caller has the collection's URI already, from
    /// its own `get_collection_data()` call, and can join the two as
    /// appropriate for its own use — string-joining URLs is a caller-level
    /// policy, not a content-decoding concern).
    Bare(String),
}

/// Errors decoding a TEP-64 content cell.
#[derive(Debug, Error)]
pub enum ContentError {
    /// A cell-level read failure while walking the snake chain.
    #[error("cell error reading snake-format content: {0}")]
    Cell(#[source] TonCellError),
    /// The snake chain's bytes were not valid UTF-8.
    #[error("content bytes are not valid UTF-8: {0}")]
    Utf8(#[source] std::str::Utf8Error),
    /// The content cell used the on-chain (`0x00`-prefixed, dictionary)
    /// shape — deliberately unsupported, see the module doc comment.
    #[error(
        "on-chain (0x00-prefixed dictionary) TEP-64 content is not supported by this decoder \
         (docs/domains/scamcheck.md) — only off-chain/bare shapes are"
    )]
    OnChainUnsupported,
    /// A snake-format cell had more than one reference — not a valid snake
    /// continuation (which uses exactly zero or one).
    #[error("snake-format cell at depth {depth} has {references} references, expected 0 or 1")]
    MalformedSnakeBranching {
        /// How many snake cells had already been walked before this one.
        depth: usize,
        /// The actual reference count found.
        references: usize,
    },
}

/// Reads a "snake format" cell chain (this cell's own bytes, followed by the
/// single referenced continuation cell's bytes, recursively) into one byte
/// buffer. Every TEP-64 content cell — on-chain values, off-chain URIs, and
/// bare relative paths alike — shares this same underlying snake shape; only
/// the leading prefix byte's *meaning* differs, decided by the caller.
fn read_snake_bytes(root: &ArcCell) -> Result<Vec<u8>, ContentError> {
    let mut bytes = Vec::new();
    let mut current = root.clone();
    let mut depth = 0usize;
    loop {
        let refs = current.references();
        if refs.len() > 1 {
            return Err(ContentError::MalformedSnakeBranching {
                depth,
                references: refs.len(),
            });
        }
        let mut parser = current.parser();
        let n = parser.remaining_bytes();
        let chunk = parser.load_bytes(n).map_err(ContentError::Cell)?;
        bytes.extend_from_slice(&chunk);

        match refs.first() {
            Some(next) => {
                current = next.clone();
                depth += 1;
            }
            None => break,
        }
    }
    Ok(bytes)
}

/// Decodes `root` as a TEP-64 content cell.
///
/// # Errors
/// See [`ContentError`]. Returns [`ContentError::OnChainUnsupported`] for a
/// `0x00`-prefixed cell rather than attempting (and likely getting wrong) a
/// dictionary decode.
pub fn decode_content(root: &ArcCell) -> Result<NftContent, ContentError> {
    let bytes = read_snake_bytes(root)?;
    match bytes.first() {
        Some(0x00) => Err(ContentError::OnChainUnsupported),
        Some(0x01) => {
            let text = std::str::from_utf8(&bytes[1..]).map_err(ContentError::Utf8)?;
            Ok(NftContent::OffChainUri(text.to_owned()))
        }
        _ => {
            let text = std::str::from_utf8(&bytes).map_err(ContentError::Utf8)?;
            Ok(NftContent::Bare(text.to_owned()))
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use tonlib_core::cell::CellBuilder;

    use super::{NftContent, decode_content};

    #[test]
    fn decodes_an_off_chain_uri_with_the_0x01_prefix() {
        let mut cell = CellBuilder::new();
        let mut bytes = vec![0x01u8];
        bytes.extend_from_slice(b"https://example.com/meta.json");
        cell.store_slice(&bytes).expect("build");
        let cell = cell.build().expect("build");
        let content = decode_content(&std::sync::Arc::new(cell)).expect("decode");
        assert_eq!(
            content,
            NftContent::OffChainUri("https://example.com/meta.json".to_owned())
        );
    }

    #[test]
    fn decodes_a_bare_relative_path_with_no_prefix_byte() {
        let mut cell = CellBuilder::new();
        cell.store_slice(b"0/meta.json").expect("build");
        let cell = cell.build().expect("build");
        let content = decode_content(&std::sync::Arc::new(cell)).expect("decode");
        assert_eq!(content, NftContent::Bare("0/meta.json".to_owned()));
    }

    #[test]
    fn on_chain_prefix_is_an_explicit_error_not_a_silent_misdecode() {
        let mut cell = CellBuilder::new();
        cell.store_slice(&[0x00u8, 0xAB]).expect("build");
        let cell = cell.build().expect("build");
        let err = decode_content(&std::sync::Arc::new(cell)).expect_err("must reject on-chain");
        assert!(matches!(err, super::ContentError::OnChainUnsupported));
    }

    #[test]
    fn walks_a_two_cell_snake_chain() {
        let mut tail = CellBuilder::new();
        tail.store_slice(b"world").expect("build");
        let tail = tail.build().expect("build");

        let mut head = CellBuilder::new();
        let mut bytes = vec![0x01u8];
        bytes.extend_from_slice(b"hello ");
        head.store_slice(&bytes).expect("build");
        head.store_reference(&std::sync::Arc::new(tail))
            .expect("build");
        let head = head.build().expect("build");

        let content = decode_content(&std::sync::Arc::new(head)).expect("decode");
        assert_eq!(content, NftContent::OffChainUri("hello world".to_owned()));
    }
}
