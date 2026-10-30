//! The chain's global version, read out of the config an emulation runs against.
//!
//! An emulation is a statement about semantics, and semantics belong to a
//! version. Global versions 12 through 15 changed what a contract does: the
//! bounce format carries `extra_flags`, `SENDMSG` stopped honouring a
//! user-supplied `fwd_fee` as a lower bound, publishing a library became
//! governance-only, action fines are collected when the phase fails. A run that
//! does not say which of those it emulated cannot be checked by anyone.
//!
//! The version is not a setting this tree chooses. It is written in the chain
//! config, parameter 8, and the only honest thing to do is read it and refuse a
//! config that carries a version this tree has not been ruled against.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tonlib_core::cell::BagOfCells;
use tonlib_core::cell::dict::predefined_readers::{key_reader_u32, val_reader_ref_cell};

/// The global version every emulation in this tree is ruled against.
///
/// **14, and it is read rather than chosen**: the vendored mainnet config
/// carries it, and every emulation this tree has ever run has been against that
/// config without saying so. Ruled against means each detector carries a verdict
/// for versions 12 through 15 (`docs/domains/detectors.md` §9). A config
/// carrying a different version is refused by every path that hands one to the
/// emulator, because the detectors' answers would otherwise be about semantics
/// nobody checked them against.
pub const PINNED_GLOBAL_VERSION: u32 = 14;

/// Parameter 8 of the chain config, which carries the version and the
/// capability flags.
const CONFIG_PARAM_GLOBAL_VERSION: u32 = 8;

/// The tag `ConfigParam 8` opens with: `capabilities#c4`.
const CAPABILITIES_TAG: u8 = 0xc4;

/// Why a config's global version could not be read, or why it is refused.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The config text was not base64.
    #[error("the config is not base64: {0}")]
    Base64(#[source] base64::DecodeError),
    /// The bytes were not a bag of cells, or the dictionary would not parse.
    #[error("the config is not a readable bag of cells: {0}")]
    Cell(#[source] tonlib_core::cell::TonCellError),
    /// The dictionary has no parameter 8. A config without it names no version
    /// at all, which is not a version this tree may guess at.
    #[error(
        "the config carries no parameter {CONFIG_PARAM_GLOBAL_VERSION}, so it names no global version"
    )]
    NoGlobalVersion,
    /// Parameter 8 did not open with the tag its TL-B gives it.
    #[error(
        "parameter {CONFIG_PARAM_GLOBAL_VERSION} opens with {found:#04x}, not {CAPABILITIES_TAG:#04x}"
    )]
    UnexpectedTag {
        /// The tag byte that was there.
        found: u8,
    },
    /// The config names a version this tree has not been ruled against.
    #[error(
        "the config pins global version {found}, and this tree is ruled against {PINNED_GLOBAL_VERSION}: \
         emulating semantics nobody checked the detectors against would answer a question nobody asked"
    )]
    UnexpectedVersion {
        /// The version the config carries.
        found: u32,
    },
}

/// Reads the global version out of a base64 chain config.
///
/// # Errors
/// See [`ConfigError`]. A config that cannot be read is an error rather than a
/// default: the whole point is that the version is never assumed.
pub fn global_version_of(config_boc_base64: &str) -> Result<u32, ConfigError> {
    let bytes = BASE64
        .decode(config_boc_base64.trim())
        .map_err(ConfigError::Base64)?;
    let boc = BagOfCells::parse(&bytes).map_err(ConfigError::Cell)?;
    let root = boc.single_root().map_err(ConfigError::Cell)?;

    // The config arrives as the dictionary's own root cell, not as a `Maybe`
    // wrapping one, so the data is loaded directly rather than through a
    // presence bit.
    let params = root
        .parser()
        .load_dict_data(32, key_reader_u32, val_reader_ref_cell)
        .map_err(ConfigError::Cell)?;

    let param = params
        .get(&CONFIG_PARAM_GLOBAL_VERSION)
        .ok_or(ConfigError::NoGlobalVersion)?;

    let mut parser = param.parser();
    let tag = parser.load_u8(8).map_err(ConfigError::Cell)?;
    if tag != CAPABILITIES_TAG {
        return Err(ConfigError::UnexpectedTag { found: tag });
    }
    parser.load_u32(32).map_err(ConfigError::Cell)
}

/// Reads the global version and refuses one this tree has not been ruled
/// against.
///
/// # Errors
/// See [`ConfigError`].
pub fn pinned_global_version_of(config_boc_base64: &str) -> Result<u32, ConfigError> {
    let found = global_version_of(config_boc_base64)?;
    if found == PINNED_GLOBAL_VERSION {
        Ok(found)
    } else {
        Err(ConfigError::UnexpectedVersion { found })
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{ConfigError, PINNED_GLOBAL_VERSION, global_version_of, pinned_global_version_of};

    /// The real mainnet config this workspace vendors as a fixture.
    fn mainnet_config() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/fixtures/emulator/mainnet-config.boc.base64");
        std::fs::read_to_string(path).expect("the config fixture is vendored")
    }

    #[test]
    fn the_real_mainnet_config_names_its_global_version() {
        let version = global_version_of(&mainnet_config()).expect("the config parses");
        assert_eq!(
            version, PINNED_GLOBAL_VERSION,
            "the pin is what the vendored config actually carries, not a number chosen for it"
        );
    }

    #[test]
    fn a_config_that_is_not_base64_is_an_error_rather_than_a_default() {
        let error = global_version_of("not base64 at all!!").expect_err("must refuse");
        assert!(matches!(error, ConfigError::Base64(_)), "got {error:?}");
    }

    #[test]
    fn a_version_the_tree_is_not_ruled_against_is_refused_by_name() {
        // The refusal has to name both numbers: "wrong version" without saying
        // which is a message nobody can act on.
        let error = ConfigError::UnexpectedVersion { found: 15 };
        let text = error.to_string();
        assert!(
            text.contains("15"),
            "the found version must be named: {text}"
        );
        assert!(
            text.contains(&PINNED_GLOBAL_VERSION.to_string()),
            "the expected version must be named: {text}"
        );
    }

    #[test]
    fn the_pinned_reader_accepts_the_config_the_pin_was_taken_from() {
        assert_eq!(
            pinned_global_version_of(&mainnet_config()).expect("the pin matches the fixture"),
            PINNED_GLOBAL_VERSION
        );
    }
}
