//! How 128-bit integers cross a serialisation boundary.
//!
//! As a decimal string, always. JSON's number type cannot carry 128 bits — `serde_json` refuses
//! to emit them, and a JavaScript consumer would silently corrupt anything past 2^53. Reading
//! also accepts a plain integer, so a hand-written fixture stays readable for values that fit.

/// A `u128` that may be absent, in the same form when it is there.
///
/// For a field that states a value only to depart from something already known — a value whose
/// absence is not zero and not a default, but a deferral to whatever the fact is.
pub mod option_u128_str {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Carries the string encoding through the optional layer, which `serde` cannot reach into.
    struct Decimal(u128);

    impl Serialize for Decimal {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_str(&self.0)
        }
    }

    /// # Errors
    ///
    /// Propagates whatever the serializer reports.
    pub fn serialize<S: Serializer>(
        value: &Option<u128>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(inner) => serializer.serialize_some(&Decimal(*inner)),
            None => serializer.serialize_none(),
        }
    }

    /// # Errors
    ///
    /// Fails on anything the inner encoding refuses.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u128>, D::Error> {
        #[derive(Deserialize)]
        struct Read(#[serde(with = "super::u128_str")] u128);

        Ok(Option::<Read>::deserialize(deserializer)?.map(|read| read.0))
    }
}

/// A `u128` as a decimal string: the encoding of every quantity.
pub mod u128_str {
    use std::fmt;

    use serde::{Deserializer, Serializer, de};

    /// # Errors
    ///
    /// Propagates whatever the serializer reports while writing the string.
    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    /// # Errors
    ///
    /// Fails when the input is neither a decimal string nor a non-negative integer within range.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        struct Visitor;

        impl de::Visitor<'_> for Visitor {
            type Value = u128;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a non-negative integer, or a decimal string for 128-bit range")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<u128, E> {
                v.parse().map_err(E::custom)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<u128, E> {
                Ok(u128::from(v))
            }

            fn visit_u128<E: de::Error>(self, v: u128) -> Result<u128, E> {
                Ok(v)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// An `i128` as a decimal string: the encoding of every change, and of any measured value a
/// report carries at full width.
pub mod i128_str {
    use std::fmt;

    use serde::{Deserializer, Serializer, de};

    /// # Errors
    ///
    /// Propagates whatever the serializer reports while writing the string.
    pub fn serialize<S: Serializer>(value: &i128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    /// # Errors
    ///
    /// Fails when the input is neither a decimal string nor an integer within range.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i128, D::Error> {
        struct Visitor;

        impl de::Visitor<'_> for Visitor {
            type Value = i128;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an integer, or a decimal string for 128-bit range")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<i128, E> {
                v.parse().map_err(E::custom)
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<i128, E> {
                Ok(i128::from(v))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<i128, E> {
                Ok(i128::from(v))
            }

            fn visit_i128<E: de::Error>(self, v: i128) -> Result<i128, E> {
                Ok(v)
            }
        }

        deserializer.deserialize_any(Visitor)
    }

    /// The same encoding for `Option<i128>`: a decimal string, or `null` when absent.
    pub mod opt {
        use serde::{Deserialize, Deserializer, Serializer};

        /// # Errors
        ///
        /// Propagates whatever the serializer reports while writing the string or the null.
        pub fn serialize<S: Serializer>(
            value: &Option<i128>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match value {
                Some(v) => serializer.collect_str(v),
                None => serializer.serialize_none(),
            }
        }

        /// # Errors
        ///
        /// Fails when a present value is not a valid `i128` encoding.
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<i128>, D::Error> {
            #[derive(Deserialize)]
            struct Wrapper(#[serde(with = "super")] i128);
            Ok(Option::<Wrapper>::deserialize(deserializer)?.map(|w| w.0))
        }
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Unsigned(#[serde(with = "u128_str")] u128);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Signed(#[serde(with = "i128_str")] i128);

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Optional(#[serde(with = "i128_str::opt")] Option<i128>);

    #[test]
    fn the_full_unsigned_range_survives_the_boundary() {
        for value in [0, 1, u128::MAX] {
            let json = serde_json::to_string(&Unsigned(value)).unwrap();
            assert_eq!(json, format!("\"{value}\""));
            assert_eq!(
                serde_json::from_str::<Unsigned>(&json).unwrap(),
                Unsigned(value)
            );
        }
    }

    #[test]
    fn the_full_signed_range_survives_the_boundary() {
        for value in [i128::MIN, -1, 0, i128::MAX] {
            let json = serde_json::to_string(&Signed(value)).unwrap();
            assert_eq!(json, format!("\"{value}\""));
            assert_eq!(
                serde_json::from_str::<Signed>(&json).unwrap(),
                Signed(value)
            );
        }
    }

    #[test]
    fn a_plain_json_number_is_accepted_on_the_way_in() {
        assert_eq!(serde_json::from_str::<Unsigned>("7").unwrap(), Unsigned(7));
        assert_eq!(serde_json::from_str::<Signed>("-7").unwrap(), Signed(-7));
    }

    #[test]
    fn a_negative_number_is_not_a_quantity() {
        assert!(serde_json::from_str::<Unsigned>("-1").is_err());
        assert!(serde_json::from_str::<Unsigned>("\"-1\"").is_err());
    }

    #[test]
    fn an_absent_optional_stays_absent() {
        for value in [None, Some(i128::MIN), Some(i128::MAX)] {
            let json = serde_json::to_string(&Optional(value)).unwrap();
            assert_eq!(
                serde_json::from_str::<Optional>(&json).unwrap(),
                Optional(value)
            );
        }
        assert_eq!(serde_json::to_string(&Optional(None)).unwrap(), "null");
    }
}
