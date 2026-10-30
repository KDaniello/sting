//! The address itself: a workchain and a hash, and the ways in and out.

use std::fmt;

/// An account's address, always in its raw form.
///
/// Never stored or compared as a user-friendly string: one account has four
/// valid friendly spellings, so equality over text answers whether two
/// renderings match rather than whether two addresses do.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct TonAddress {
    /// The workchain, `-1` for the masterchain and `0` for the basechain.
    pub workchain: i32,
    /// The account's 32-byte hash.
    pub hash: [u8; 32],
}

/// An address that could not be read.
#[derive(Debug, thiserror::Error)]
pub enum AddressError {
    /// The text was not an address in any form this crate reads.
    #[error("not a TON address: {0}")]
    Unreadable(#[from] tonlib_core::TonAddressParseError),
}

impl TonAddress {
    /// The address with this workchain and hash.
    #[must_use]
    pub const fn new(workchain: i32, hash: [u8; 32]) -> Self {
        Self { workchain, hash }
    }

    /// The canonical string form, `"<workchain>:<64 hex chars>"` — what a
    /// database key and a log line carry.
    #[must_use]
    pub fn to_raw_string(&self) -> String {
        format!("{}:{}", self.workchain, hex::encode(self.hash))
    }

    /// Reads the raw form.
    ///
    /// # Errors
    /// [`AddressError`] when the text is not one.
    pub fn from_raw_string(text: &str) -> Result<Self, AddressError> {
        Ok(Self::from(tonlib_core::TonAddress::from_hex_str(text)?))
    }

    /// Reads a user-friendly form: `EQ…`, `UQ…`, `kQ…`, base64 or base64url.
    ///
    /// Through `tonlib-core` rather than here: the friendly form carries a
    /// CRC16 over flag bits, and a hand-rolled implementation of that is a
    /// source of bugs too subtle to catch by reading.
    ///
    /// # Errors
    /// [`AddressError`] when the text is not one, checksum included.
    pub fn from_user_friendly(text: &str) -> Result<Self, AddressError> {
        Ok(Self::from(tonlib_core::TonAddress::from_base64_url(text)?))
    }

    /// Reads whichever form the text is in.
    ///
    /// # Errors
    /// [`AddressError`] when it is neither.
    pub fn parse(text: &str) -> Result<Self, AddressError> {
        if text.contains(':') {
            Self::from_raw_string(text)
        } else {
            Self::from_user_friendly(text)
        }
    }
}

impl fmt::Display for TonAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_raw_string())
    }
}

impl From<tonlib_core::TonAddress> for TonAddress {
    fn from(address: tonlib_core::TonAddress) -> Self {
        Self {
            workchain: address.workchain,
            hash: address.hash_part.into(),
        }
    }
}

impl From<TonAddress> for tonlib_core::TonAddress {
    fn from(address: TonAddress) -> Self {
        Self::new(address.workchain, address.hash.into())
    }
}

/// Serialised as the raw string, which is the form a database key and a
/// snapshot already carry — a byte array would be a second spelling of the
/// same address in the same file.
impl serde::Serialize for TonAddress {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_raw_string())
    }
}

impl<'de> serde::Deserialize<'de> for TonAddress {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
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
    use super::TonAddress;

    #[test]
    fn the_raw_form_survives_a_round_trip() {
        let address = TonAddress::new(0, [0x22; 32]);
        assert_eq!(
            TonAddress::from_raw_string(&address.to_raw_string()).expect("reads back"),
            address
        );
    }

    #[test]
    fn the_masterchain_survives_a_round_trip_with_its_negative_workchain() {
        let address = TonAddress::new(-1, [0x33; 32]);
        assert_eq!(
            TonAddress::from_raw_string(&address.to_raw_string()).expect("reads back"),
            address
        );
    }

    #[test]
    fn all_four_friendly_spellings_of_one_account_read_back_as_one_address() {
        // Bounceable or not, mainnet or testnet: four renderings, one account.
        // This is why an address is never compared as text.
        let hash = [0x11_u8; 32];
        let expected = TonAddress::new(0, hash);
        for non_bounceable in [false, true] {
            for testnet in [false, true] {
                let rendering = tonlib_core::TonAddress::new(0, hash.into())
                    .to_base64_url_flags(non_bounceable, testnet);
                assert_eq!(
                    TonAddress::from_user_friendly(&rendering)
                        .unwrap_or_else(|error| panic!("{rendering}: {error}")),
                    expected
                );
            }
        }
    }

    #[test]
    fn parse_reads_either_form_without_being_told_which() {
        let raw = TonAddress::new(0, [0x44; 32]);
        let friendly = tonlib_core::TonAddress::from(raw).to_base64_url();
        assert_eq!(TonAddress::parse(&raw.to_raw_string()).expect("raw"), raw);
        assert_eq!(TonAddress::parse(&friendly).expect("friendly"), raw);
    }

    #[test]
    fn text_that_is_not_an_address_is_refused_rather_than_guessed_at() {
        assert!(TonAddress::from_user_friendly("not-an-address").is_err());
        assert!(TonAddress::parse("0:nothex").is_err());
    }

    #[test]
    fn an_address_serialises_as_the_string_a_database_key_already_carries() {
        let address = TonAddress::new(-1, [0x55; 32]);
        let json = serde_json::to_string(&address).expect("serialises");
        assert_eq!(json, format!("\"{}\"", address.to_raw_string()));
        assert_eq!(
            serde_json::from_str::<TonAddress>(&json).expect("reads back"),
            address
        );
    }

    proptest::proptest! {
        /// Any real workchain and hash survives the round trip.
        #[test]
        fn any_address_survives_the_raw_round_trip(
            workchain in -1_i32..=0,
            hash in proptest::array::uniform32(0_u8..=255),
        ) {
            let address = TonAddress::new(workchain, hash);
            let parsed = TonAddress::from_raw_string(&address.to_raw_string())
                .expect("reads back");
            proptest::prop_assert_eq!(address, parsed);
        }
    }
}
