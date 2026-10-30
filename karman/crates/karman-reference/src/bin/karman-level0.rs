//! Prints the corpus the reference produces, as the committed fixture is written.
//!
//! A tool rather than a run: it says what the reference did, and the shell decides where that
//! goes. Nothing in this workspace reads a file to run a simulation.

fn main() -> Result<(), String> {
    let corpus = karman_reference::generate()?;
    let printed =
        serde_json::to_string_pretty(&corpus).map_err(|e| format!("unprintable corpus: {e}"))?;
    println!("{printed}");
    Ok(())
}
