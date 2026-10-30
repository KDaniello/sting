//! Raw `FunC` type text → [`TypeShape`] — the same precision level as the
//! proven precedent this extractor is built against
//! (`ton-blockchain/verifier`'s `getterParser.ts`, which works directly off
//! token text and kind rather than off a resolved type system, and marks the
//! same boundary in its own source). `FunC` has no real static type system
//! beyond a
//! handful of primitives plus untyped tensors/tuples, so this is
//! deliberately a flat text mapper, not a recursive type-expression
//! parser — good enough to match the precedent, not claiming more.

use sting_abi_core::TypeShape;

/// Maps one raw `FunC` type token/expression's source text to a
/// [`TypeShape`] — `text` is the verbatim span of a `primitive_type`,
/// `type_identifier`, `hole_type`, or a whole parenthesized tensor/tuple
/// type's text. Unrecognized or structurally complex text (tensors,
/// tuples, `var`/`_` holes, `cont` continuations, anything not in
/// `FunC`'s fixed primitive set) maps to [`TypeShape::Unknown`] rather
/// than guessed — the same choice `getterParser.ts` makes by only
/// special-casing punctuation.
#[must_use]
pub fn map_type_text(text: &str) -> TypeShape {
    match text.trim() {
        "int" => TypeShape::Int {
            bits: None,
            signed: true,
        },
        "cell" => TypeShape::Cell,
        "slice" => TypeShape::Slice,
        "builder" => TypeShape::Builder,
        _ => TypeShape::Unknown,
    }
}

/// Splits a raw return/parameter type text into its top-level components —
/// `"(int, slice, cell)"` → `["int", "slice", "cell"]`, `"int"` → `["int"]`.
/// Parenthesis-depth-aware (a nested tensor like `"(int, (int, int))"`
/// splits into exactly two top-level components, not three) since `FunC`
/// get-methods commonly return multiple stack values as a tensor type — a
/// very common real shape (`get_wallet_data() method_id { return
/// load_data(); }` returning `(int, slice, slice, cell)`, confirmed in
/// the research corpus), not an edge case to collapse into one
/// [`TypeShape::Unknown`]. A caller interns each returned component
/// separately, then wraps them in a [`TypeShape::Tensor`] when there is
/// more than one — this function only splits text, it has no
/// [`sting_abi_core::TypeRef`] table to intern into.
#[must_use]
pub fn split_top_level_components(text: &str) -> Vec<&str> {
    let trimmed = text.trim();
    let Some(inner) = trimmed.strip_prefix('(').and_then(|t| t.strip_suffix(')')) else {
        return vec![trimmed];
    };
    if inner.trim().is_empty() {
        return Vec::new(); // `()` — unit, no components at all
    }

    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (index, ch) in inner.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(inner[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(inner[start..].trim());
    parts
}

/// Maps a `load_*`/`~load_*` method-call name (from a storage/message
/// parsing chain, e.g. `cs~load_uint(32)`) to the [`TypeShape`] it reads —
/// a different, narrower mapping than [`map_type_text`]: `FunC`'s stdlib
/// load functions encode width/signedness in the *call*, not in a
/// declared type, so recovering a storage field's type means reading the
/// call, not a type annotation (`docs/abi-layer/blocks/
/// 04-func-heuristic-extractor.md` §4.3).
#[must_use]
pub fn map_load_call(method_name: &str, first_arg_literal: Option<u32>) -> TypeShape {
    // `FunC`'s own widest width-bearing load (`load_bits`/`load_uint`/...)
    // is bounded by a slice's max 1023 bits — always fits `u16`, but the
    // literal itself is parsed as `u32` (this crate's general integer
    // literal type), so narrow it defensively rather than truncate
    // silently on some future out-of-range source.
    let bits = first_arg_literal.and_then(|w| u16::try_from(w).ok());
    match method_name {
        "load_uint" | "preload_uint" => TypeShape::Int {
            bits,
            signed: false,
        },
        "load_int" | "preload_int" => TypeShape::Int { bits, signed: true },
        // `load_grams`/`preload_grams` are a real `FunC` stdlib alias for
        // `load_coins`/`preload_coins` (both read the same `VarUInteger16`
        // encoding) — confirmed real usage, not a hypothetical:
        // `wallet-contract/func/simple-subscription-plugin.fc`'s real
        // `load_storage()` calls `ds~load_grams()` for its `amount` field.
        "load_coins" | "load_grams" | "preload_coins" | "preload_grams" => TypeShape::Coins,
        "load_msg_addr" | "preload_msg_addr" => TypeShape::Address,
        "load_ref" | "preload_ref" => TypeShape::Cell,
        "load_bits" | "preload_bits" => TypeShape::Bits {
            width: bits.unwrap_or(0),
        },
        _ => TypeShape::Unknown,
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
    use super::{map_load_call, map_type_text, split_top_level_components};
    use sting_abi_core::TypeShape;

    #[test]
    fn splits_top_level_tensor_components() {
        assert_eq!(
            split_top_level_components("(int, slice, cell)"),
            vec!["int", "slice", "cell"]
        );
    }

    #[test]
    fn a_single_type_is_not_split() {
        assert_eq!(split_top_level_components("int"), vec!["int"]);
    }

    #[test]
    fn nested_tensors_only_split_at_the_top_level() {
        assert_eq!(
            split_top_level_components("(int, (int, int), cell)"),
            vec!["int", "(int, int)", "cell"]
        );
    }

    #[test]
    fn unit_has_zero_components() {
        assert_eq!(split_top_level_components("()"), Vec::<&str>::new());
        assert_eq!(split_top_level_components("(  )"), Vec::<&str>::new());
    }

    #[test]
    fn primitives_map_directly() {
        assert_eq!(
            map_type_text("int"),
            TypeShape::Int {
                bits: None,
                signed: true
            }
        );
        assert_eq!(map_type_text("cell"), TypeShape::Cell);
        assert_eq!(map_type_text("slice"), TypeShape::Slice);
        assert_eq!(map_type_text("builder"), TypeShape::Builder);
    }

    #[test]
    fn unrecognized_text_is_unknown_not_guessed() {
        assert_eq!(map_type_text("(int, int)"), TypeShape::Unknown);
        assert_eq!(map_type_text("tuple"), TypeShape::Unknown);
        assert_eq!(map_type_text("_"), TypeShape::Unknown);
        assert_eq!(map_type_text("var"), TypeShape::Unknown);
    }

    #[test]
    fn load_calls_map_by_call_not_by_declared_type() {
        assert_eq!(
            map_load_call("load_uint", Some(64)),
            TypeShape::Int {
                bits: Some(64),
                signed: false
            }
        );
        assert_eq!(
            map_load_call("load_int", Some(32)),
            TypeShape::Int {
                bits: Some(32),
                signed: true
            }
        );
        assert_eq!(map_load_call("load_coins", None), TypeShape::Coins);
        assert_eq!(
            map_load_call("load_grams", None),
            TypeShape::Coins,
            "load_grams is a real FunC stdlib alias for load_coins"
        );
        assert_eq!(map_load_call("preload_coins", None), TypeShape::Coins);
        assert_eq!(map_load_call("preload_grams", None), TypeShape::Coins);
        assert_eq!(map_load_call("load_msg_addr", None), TypeShape::Address);
        assert_eq!(map_load_call("load_ref", None), TypeShape::Cell);
        assert_eq!(
            map_load_call("load_bits", Some(8)),
            TypeShape::Bits { width: 8 }
        );
        assert_eq!(map_load_call("load_dict", None), TypeShape::Unknown);
    }
}
