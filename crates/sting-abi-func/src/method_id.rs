//! TON's standard get-method id convention — CRC16-derived from the method
//! name when no explicit numeric id is given in source. It is computed by
//! the standard CRC16 formula from the name, never searched for in the text
//! (`docs/domains/extractors.md`).

/// CRC-16/XMODEM (poly `0x1021`, init `0`, no reflection) — the specific
/// CRC16 variant TON's own tooling uses for this purpose.
fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            if crc & 0x8000 == 0 {
                crc <<= 1;
            } else {
                crc = (crc << 1) ^ 0x1021;
            }
        }
    }
    crc
}

/// The default TVM get-method id for a method named `name` with no explicit
/// numeric override — `(crc16_xmodem(name) & 0xffff) | 0x10000`. Verified
/// against a real, independently confirmed value: `tolk`
/// (which uses the identical convention for a `get fun` with no explicit
/// id) compiled a method named `getCounter` to method id `103307` — see
/// this module's own test.
#[must_use]
pub fn default_method_id(name: &str) -> i64 {
    let crc = crc16_xmodem(name.as_bytes());
    i64::from((u32::from(crc) & 0xffff) | 0x1_0000)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::default_method_id;

    /// Real ground truth, not a hand-derived expectation: `tolk` compiled a
    /// zero-arg `get fun getCounter(): int` (no explicit method id) to
    /// `tvm_method_id: 103307` — captured via a live `abi-layer extract`
    /// run against `corpus/fixtures/bridge/GvrShadowPattern.tolk` this same
    /// session (`crates/sting-abi-service/tests/real_end_to_end.rs` compiles
    /// this exact fixture). Tolk and `FunC` share the identical default
    /// get-method-id convention (TON's own, not per-language) so this is a
    /// real, independently-confirmed value, not a synthetic expectation.
    #[test]
    fn matches_a_real_tolk_compiled_value() {
        assert_eq!(default_method_id("getCounter"), 103_307);
    }

    #[test]
    fn is_always_in_the_documented_19_bit_unsigned_shape() {
        for name in ["seqno", "get_wallet_data", "royalty_params", ""] {
            let id = default_method_id(name);
            assert!((0x1_0000..=0x1_ffff).contains(&id), "{name:?} -> {id}");
        }
    }
}
