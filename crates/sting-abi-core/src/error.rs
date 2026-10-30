/// One error code reachable via `throw`/`assert` (or the source language's
/// equivalent) anywhere in the contract, as populated into
/// [`ContractSchema::errors`](crate::ContractSchema::errors). Direct
/// normalized form of Tolk's own `ThrownError::{PlainInt, Constant}`
/// (`crates/sting-tolkc/src/abi.rs`) — that split already matches what Tact
/// (`ABIError{message}`, keyed by numeric exit code, name not always
/// present) and `FunC` (usually not named at all) both need: a bare code with
/// an optional name, not two unrelated shapes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorCode {
    /// A bare integer exit code with no recovered name.
    Plain {
        /// The literal exit code.
        code: u32,
    },
    /// A named error constant.
    Named {
        /// The constant's declared/recovered name (e.g. `"ERROR_BAD_SEQNO"`).
        name: String,
        /// The resolved integer exit code.
        code: u32,
    },
}
