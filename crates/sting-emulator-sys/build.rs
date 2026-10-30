//! Lifts the `libemulator` pin out of this crate's own manifest and into the
//! compiled crate, so `[package.metadata.libemulator]` is the only place the
//! expected build is written down.
//!
//! A constant in the source and a table in the manifest would be the same fact
//! spelled twice, and two spellings of one fact drift. Reading the manifest
//! here removes the second spelling rather than testing that it matches.

use std::path::Path;
use std::process::ExitCode;

/// Keys lifted into the build, each becoming `STING_LIBEMULATOR_<KEY>`.
const PINNED_KEYS: [&str; 4] = ["commit", "date", "release", "source"];

fn main() -> ExitCode {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => dir,
        Err(error) => return fail(&format!("CARGO_MANIFEST_DIR is unreadable: {error}")),
    };
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    println!("cargo::rerun-if-changed={}", manifest_path.display());

    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => return fail(&format!("{}: {error}", manifest_path.display())),
    };
    let manifest = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(error) => return fail(&format!("{}: {error}", manifest_path.display())),
    };
    let pin = manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("libemulator"));
    let Some(pin) = pin else {
        return fail("[package.metadata.libemulator] is missing: the pin is the version");
    };

    for key in PINNED_KEYS {
        let Some(value) = pin.get(key).and_then(toml::Value::as_str) else {
            return fail(&format!(
                "[package.metadata.libemulator] has no string {key:?}"
            ));
        };
        println!(
            "cargo::rustc-env=STING_LIBEMULATOR_{}={value}",
            key.to_uppercase()
        );
    }
    ExitCode::SUCCESS
}

/// Reports why the pin could not be read and stops the build. A build script
/// that cannot read the pin must not produce a crate that claims one.
fn fail(reason: &str) -> ExitCode {
    println!("cargo::error=libemulator pin unreadable — {reason}");
    ExitCode::FAILURE
}
