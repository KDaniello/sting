//! `BridgeController` — `docs/ARCHITECTURE.md`,
//! `docs/domains/emulator.md`.
//!
//! One small strategy function per [`sting_core::DefectClass`], rather than
//! one universal verifier: the hypotheses are different in kind. Each strategy
//! needs more than a bare `Finding` to build its
//! scenario — which op code reaches the flagged code path, which field
//! values to use — information a `Finding`'s `(file, line, message)` alone
//! does not carry and which is not derived from source here (see
//! [`Hypothesis`]'s doc comment for why that is a confirmed out-of-scope
//! problem rather than an oversight). Callers supply it explicitly via
//! [`Hypothesis`].

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use num_bigint::BigUint;
use sting_core::{DynamicVerification, ProofOfConcept};
use tonlib_core::cell::{BagOfCells, CellBuilder};

use super::pool::{EmulationTarget, EmulatorProvider};

/// One field to encode into a synthetic message body after its leading
/// 32-bit op code, in order — deliberately a small closed set (not a
/// generic cell-builder DSL): every fixture verified against
/// reads its body as `op:32` followed by a short flat sequence of these
/// primitive shapes (`corpus/fixtures/bridge/*.tolk`), and a richer DSL
/// would be speculative surface with no real caller yet.
#[derive(Debug, Clone)]
pub enum ScenarioField {
    /// A 32-bit unsigned field (`loadUint(32)` on the Tolk side).
    U32(u32),
    /// A 64-bit unsigned field (`loadUint(64)` on the Tolk side).
    U64(u64),
    /// A `coins` (`VarUInteger16`) field (`loadCoins()` on the Tolk side).
    Coins(u64),
    /// Raw trailing bytes, appended as-is — used to build `LEP`'s
    /// "unexpected tail" scenario.
    Raw(Vec<u8>),
}

/// Errors building a scenario message body.
#[derive(Debug, thiserror::Error)]
#[error("failed to build scenario message body")]
pub struct BuildScenarioError;

/// Builds the base64-`BoC` message body `op` followed by `fields`, in order
/// — the same "structured stub, serialized to a real `BoC` before the
/// emulator ever sees it" approach `sting-fuzz` already uses
/// (`docs/domains/emulator.md`), reused here because the
/// underlying need (a small typed body, not raw bytes) is identical.
///
/// # Errors
/// Returns [`BuildScenarioError`] if the resulting cell would overflow (not
/// expected for the fixed-shape, small scenarios built here).
pub fn build_scenario_body(
    op: u32,
    fields: &[ScenarioField],
) -> Result<String, BuildScenarioError> {
    let mut builder = CellBuilder::new();
    builder.store_u32(32, op).map_err(|_| BuildScenarioError)?;
    for field in fields {
        match field {
            ScenarioField::U32(v) => {
                builder.store_u32(32, *v).map_err(|_| BuildScenarioError)?;
            }
            ScenarioField::U64(v) => {
                builder.store_u64(64, *v).map_err(|_| BuildScenarioError)?;
            }
            ScenarioField::Coins(v) => {
                builder
                    .store_coins(&BigUint::from(*v))
                    .map_err(|_| BuildScenarioError)?;
            }
            ScenarioField::Raw(bytes) => {
                builder.store_slice(bytes).map_err(|_| BuildScenarioError)?;
            }
        }
    }
    let cell = builder.build().map_err(|_| BuildScenarioError)?;
    let bytes = BagOfCells::from_root(cell)
        .serialize(false)
        .map_err(|_| BuildScenarioError)?;
    Ok(BASE64.encode(bytes))
}

/// Decodes up to `count` `vm_stk_tinyint`-tagged 64-bit integers off the top
/// of a get-method's returned `VmStack` `BoC`, in return order.
///
/// Only the `tinyint` shape (`libemulator`'s own encoding for small
/// in-range integers — confirmed empirically by reading real returned stacks
/// byte by byte rather than assumed
/// from a spec) is supported; any other `VmStackValue` tag stops decoding
/// and returns `None` — matching `docs/domains/emulator.md`'s existing
/// stance of not reimplementing a full `VmStack` decoder, extended just
/// enough for the fixed-shape small-int results the fixtures return.
#[must_use]
pub fn decode_stack_ints(stack_boc_base64: &str, count: usize) -> Option<Vec<i64>> {
    let mut values = Vec::with_capacity(count);
    if count == 0 {
        return Some(values);
    }

    let bag = BagOfCells::parse_base64(stack_boc_base64).ok()?;
    let root = bag.single_root().ok()?;
    let mut parser = root.parser();
    let _depth = parser.load_u32(24).ok()?;
    if parser.load_u8(8).ok()? != 0x01 {
        return None;
    }
    values.push(parser.load_i64(64).ok()?);

    let mut current = root.references().first()?.clone();
    for _ in 1..count {
        let mut p = current.parser();
        if p.load_u8(8).ok()? != 0x01 {
            return None;
        }
        values.push(p.load_i64(64).ok()?);
        current = current.references().first()?.clone();
    }
    Some(values)
}

/// The extra, `Finding`-independent information each strategy needs to
/// build its scenario (`docs/domains/emulator.md`).
///
/// This is a deliberate, documented deviation from
/// `docs/ARCHITECTURE.md`'s illustrative sketch
/// (`verify(&self, finding, source, target)` — no separate hypothesis
/// parameter, implying the strategy derives its scenario from `source`
/// directly). Investigated directly: deriving "which op code reaches this
/// specific flagged function" from
/// source alone is a genuine, deep problem (equivalent to solving the
/// contract's own message-dispatch convention in general — not a fixed
/// v1-scope-style narrowing, an open research-grade question). Every
/// `BridgeController` caller supplies the hypothesis
/// explicitly instead, exactly as `corpus/fixtures/bridge/*.tolk` were
/// purpose-built with a *known* op-code-to-function mapping: a fixture with
/// known reachability rather than a gamble on general derivation.
///
/// `get_method` fields are owned `String`, not `&'static str`: every caller
/// originally supplied a string literal (a hardcoded test or demo
/// scenario), for which `&'static str` was the more precise type — but
/// `sting-cli`'s `verify` command ('s direction 3)
/// builds a `Hypothesis` from a user-supplied TOML file at runtime, which
/// can never produce a `'static` string without leaking memory. Changed
/// once a second, genuinely different kind of caller needed it, not
/// speculatively.
#[derive(Debug, Clone)]
pub enum Hypothesis {
    /// `GVR`: the op code that reaches the shadowed-variable code path, and
    /// the get-method that reads the (allegedly unaffected) storage field back.
    Gvr {
        /// Op code to send.
        trigger_op: u32,
        /// Get-method name to read the field both before and after.
        get_method: String,
    },
    /// `LEP`: the op code that reaches the un-`assertEnd`-ed function, the
    /// fields it actually expects, and extra trailing bytes to append after
    /// them.
    Lep {
        /// Op code to send.
        trigger_op: u32,
        /// The fields the function actually reads, in order.
        fields: Vec<ScenarioField>,
        /// Extra bytes appended after `fields` — the "tail" the static
        /// finding predicts will be silently ignored.
        trailing_bytes: Vec<u8>,
    },
    /// `ID`: the op code that writes two known, distinct field values, and
    /// the get-method that reads them back.
    Id {
        /// Op code to send.
        save_op: u32,
        /// First field's value (sent in this position).
        field_a: u32,
        /// Second field's value (sent in this position).
        field_b: u32,
        /// Get-method that reads both fields back, in the same declared order.
        get_method: String,
    },
    /// `UBM`: the op code that reaches the commit-then-send path, and the
    /// `coins` amount to mint.
    Ubm {
        /// Op code to send.
        mint_op: u32,
        /// Amount encoded as the message's `coins` field.
        mint_amount: u64,
    },
}

/// Resolves a [`sting_core::Finding`] + [`Hypothesis`] into a
/// [`DynamicVerification`] by driving `provider` against `target`
/// (`docs/ARCHITECTURE.md`).
pub struct BridgeController<'a, P: EmulatorProvider> {
    provider: &'a P,
}

impl<'a, P: EmulatorProvider> BridgeController<'a, P> {
    /// Builds a controller driving `provider`.
    #[must_use]
    pub fn new(provider: &'a P) -> Self {
        Self { provider }
    }

    /// Attempts to dynamically verify `hypothesis` against `target`.
    ///
    /// This does not take a `Finding` directly (contrast
    /// `docs/ARCHITECTURE.md`'s sketch) — callers already know
    /// which `Hypothesis` variant corresponds to their finding's
    /// `DefectClass` (see [`Hypothesis`]'s doc comment); routing on a
    /// `Finding` here would just be a second, redundant dispatch.
    #[must_use]
    pub fn verify(&self, hypothesis: &Hypothesis, target: &EmulationTarget) -> DynamicVerification {
        match hypothesis {
            Hypothesis::Gvr {
                trigger_op,
                get_method,
            } => self.verify_gvr(*trigger_op, get_method, target),
            Hypothesis::Lep {
                trigger_op,
                fields,
                trailing_bytes,
            } => self.verify_lep(*trigger_op, fields, trailing_bytes, target),
            Hypothesis::Id {
                save_op,
                field_a,
                field_b,
                get_method,
            } => self.verify_id(*save_op, *field_a, *field_b, get_method, target),
            Hypothesis::Ubm {
                mint_op,
                mint_amount,
            } => self.verify_ubm(*mint_op, *mint_amount, target),
        }
    }

    /// `GVR` (`docs/domains/emulator.md`): read the
    /// flagged field before, send the message the static pattern claims
    /// "should" update it, read again — `Confirmed` if genuinely unchanged
    /// (byte-identical returned stack), `Refuted` if it did change.
    fn verify_gvr(
        &self,
        trigger_op: u32,
        get_method: &str,
        target: &EmulationTarget,
    ) -> DynamicVerification {
        let Ok(before) = self.provider.run_get_method(target, get_method) else {
            return DynamicVerification::Inconclusive("failed to read baseline state".to_string());
        };
        let Ok(body) = build_scenario_body(trigger_op, &[]) else {
            return DynamicVerification::Inconclusive(
                "failed to build scenario message".to_string(),
            );
        };
        let Ok(send) = self
            .provider
            .send_internal_message(target, &body, 1_000_000_000)
        else {
            return DynamicVerification::Inconclusive(
                "send_internal_message call failed".to_string(),
            );
        };
        if !send.success || !send.accepted || send.vm_exit_code != Some(0) {
            return DynamicVerification::Inconclusive(
                "scenario message did not complete cleanly".to_string(),
            );
        }
        let after_target = target.with_data(send.new_data_boc_base64.unwrap_or_default());
        let Ok(after) = self.provider.run_get_method(&after_target, get_method) else {
            return DynamicVerification::Inconclusive("failed to re-read state".to_string());
        };
        if before.stack_boc_base64 == after.stack_boc_base64 {
            DynamicVerification::Confirmed(ProofOfConcept {
                expected: format!(
                    "`{get_method}` unchanged despite a message that visually looks like it should update it"
                ),
                actual: format!("`{get_method}` returned identical state before and after"),
                steps: vec![format!("sent op=0x{trigger_op:08x}")],
            })
        } else {
            DynamicVerification::Refuted
        }
    }

    /// `LEP` (`docs/domains/emulator.md`): send `fields`
    /// followed by `trailing_bytes` the function does not expect —
    /// `Confirmed` if the call still completes cleanly (the tail was
    /// silently ignored, not rejected), `Refuted` if it was rejected.
    fn verify_lep(
        &self,
        trigger_op: u32,
        fields: &[ScenarioField],
        trailing_bytes: &[u8],
        target: &EmulationTarget,
    ) -> DynamicVerification {
        let mut all_fields = fields.to_vec();
        all_fields.push(ScenarioField::Raw(trailing_bytes.to_vec()));
        let Ok(body) = build_scenario_body(trigger_op, &all_fields) else {
            return DynamicVerification::Inconclusive(
                "failed to build scenario message".to_string(),
            );
        };
        let Ok(send) = self
            .provider
            .send_internal_message(target, &body, 1_000_000_000)
        else {
            return DynamicVerification::Inconclusive(
                "send_internal_message call failed".to_string(),
            );
        };
        if send.success && send.accepted && send.vm_exit_code == Some(0) {
            DynamicVerification::Confirmed(ProofOfConcept {
                expected: "trailing data after the expected fields is silently ignored".to_string(),
                actual: format!(
                    "message with {} extra trailing byte(s) after the expected fields was accepted cleanly",
                    trailing_bytes.len()
                ),
                steps: vec![format!(
                    "sent op=0x{trigger_op:08x} with {} extra trailing byte(s)",
                    trailing_bytes.len()
                )],
            })
        } else {
            DynamicVerification::Refuted
        }
    }

    /// `ID` (`docs/domains/emulator.md`): write two
    /// known, distinct field values, read them back — `Confirmed` if they
    /// come back in a different order than sent, `Refuted` if they match.
    fn verify_id(
        &self,
        save_op: u32,
        field_a: u32,
        field_b: u32,
        get_method: &str,
        target: &EmulationTarget,
    ) -> DynamicVerification {
        let fields = vec![ScenarioField::U32(field_a), ScenarioField::U32(field_b)];
        let Ok(body) = build_scenario_body(save_op, &fields) else {
            return DynamicVerification::Inconclusive(
                "failed to build scenario message".to_string(),
            );
        };
        let Ok(send) = self
            .provider
            .send_internal_message(target, &body, 1_000_000_000)
        else {
            return DynamicVerification::Inconclusive(
                "send_internal_message call failed".to_string(),
            );
        };
        if !send.success || !send.accepted || send.vm_exit_code != Some(0) {
            return DynamicVerification::Inconclusive(
                "save message did not complete cleanly".to_string(),
            );
        }
        let after_target = target.with_data(send.new_data_boc_base64.unwrap_or_default());
        let Ok(read) = self.provider.run_get_method(&after_target, get_method) else {
            return DynamicVerification::Inconclusive(
                "failed to read back saved fields".to_string(),
            );
        };
        let Some(stack) = read.stack_boc_base64 else {
            return DynamicVerification::Inconclusive("get-method returned no stack".to_string());
        };
        let Some(values) = decode_stack_ints(&stack, 2) else {
            return DynamicVerification::Inconclusive(
                "failed to decode returned fields".to_string(),
            );
        };
        // `decode_stack_ints` reads *top-of-stack first* (its own doc
        // comment). A Tolk `return (a, b)` pushes `a` then `b` in that
        // order, so `b` ends up on top — `values[0]` is therefore the
        // *second*-declared return value, `values[1]` the first. Confirmed
        // against a real emulator run, not assumed
        // (`corpus/fixtures/bridge/IdSwappedPattern.*`,
        // `crates/sting-pipeline/tests/bridge_id_end_to_end.rs`): getting
        // this backwards silently cancelled out against the very swap bug
        // this strategy exists to detect on that fixture's first real
        // end-to-end run — a `Confirmed`-worthy scenario came back
        // `Refuted` until this was traced down to the actual raw VM stack.
        let [read_b, read_a] = values[..] else {
            return DynamicVerification::Inconclusive(
                "expected exactly two returned fields".to_string(),
            );
        };
        if read_a == i64::from(field_a) && read_b == i64::from(field_b) {
            DynamicVerification::Refuted
        } else {
            DynamicVerification::Confirmed(ProofOfConcept {
                expected: format!("read back (a={field_a}, b={field_b}) in the order written"),
                actual: format!(
                    "read back (a={read_a}, b={read_b}) — order does not match what was written"
                ),
                steps: vec![format!("sent op=0x{save_op:08x} a={field_a} b={field_b}")],
            })
        }
    }

    /// `UBM` (`docs/domains/emulator.md` §7): send a
    /// mint-shaped message — `Confirmed` if storage committed *and* an
    /// outbound action was queued (the unrevertable-commit-then-send
    /// pattern, `docs/domains/detectors.md`), `Inconclusive` if the
    /// scenario never reached that path at all (distinct from `Refuted`:
    /// nothing risky was even attempted, so nothing was safely handled either).
    fn verify_ubm(
        &self,
        mint_op: u32,
        mint_amount: u64,
        target: &EmulationTarget,
    ) -> DynamicVerification {
        let fields = vec![ScenarioField::Coins(mint_amount)];
        let Ok(body) = build_scenario_body(mint_op, &fields) else {
            return DynamicVerification::Inconclusive(
                "failed to build scenario message".to_string(),
            );
        };
        let Ok(send) = self
            .provider
            .send_internal_message(target, &body, 2_000_000_000)
        else {
            return DynamicVerification::Inconclusive(
                "send_internal_message call failed".to_string(),
            );
        };
        if !send.success || !send.accepted || send.vm_exit_code != Some(0) {
            return DynamicVerification::Inconclusive(
                "mint message did not complete cleanly".to_string(),
            );
        }
        let state_changed = send
            .new_data_boc_base64
            .as_deref()
            .is_some_and(|new_data| new_data != target.data_boc_base64);
        if state_changed && send.has_actions {
            DynamicVerification::Confirmed(ProofOfConcept {
                expected:
                    "storage commits, then a bounceable send is queued, with no compensating \
                           onBouncedMessage handler to revert it if the send bounces"
                        .to_string(),
                actual: "storage changed and an outbound action was queued in the same call, with \
                         no revert"
                    .to_string(),
                steps: vec![format!("sent mint op=0x{mint_op:08x} amount={mint_amount}")],
            })
        } else {
            DynamicVerification::Inconclusive(
                "scenario did not reach the commit-then-send path".to_string(),
            )
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
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::super::pool::{EmulationError, GetMethodOutcome, SendOutcome};
    use super::*;

    /// A scripted [`EmulatorProvider`] — `docs/ARCHITECTURE.md`
    /// principle 1's mocking boundary in action: `BridgeController`'s own
    /// tests never touch a real `libemulator`. `send`/`get` responses are
    /// each a separate queue, popped in call order — sufficient because
    /// every strategy's call sequence (which of `send_internal_message`/
    /// `run_get_method`, and how many) is fixed and known here.
    #[derive(Default)]
    struct ScriptedProvider {
        sends: Mutex<VecDeque<SendOutcome>>,
        gets: Mutex<VecDeque<GetMethodOutcome>>,
    }

    impl EmulatorProvider for ScriptedProvider {
        fn send_internal_message(
            &self,
            _target: &EmulationTarget,
            _body: &str,
            _amount: u64,
        ) -> Result<SendOutcome, EmulationError> {
            self.sends
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| panic!("ScriptedProvider: unexpected send_internal_message call"))
        }

        fn run_get_method(
            &self,
            _target: &EmulationTarget,
            _method: &str,
        ) -> Result<GetMethodOutcome, EmulationError> {
            self.gets
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| panic!("ScriptedProvider: unexpected run_get_method call"))
        }

        fn check_structural_validity(
            &self,
            _target: &EmulationTarget,
        ) -> Result<(), EmulationError> {
            panic!(
                "ScriptedProvider: check_structural_validity is not used by any BridgeController strategy"
            )
        }
    }

    fn dummy_target() -> EmulationTarget {
        EmulationTarget {
            code_boc_base64: "code".to_string(),
            data_boc_base64: "data".to_string(),
            address: "0:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
        }
    }

    fn clean_send(new_data: &str, has_actions: bool) -> SendOutcome {
        SendOutcome {
            success: true,
            accepted: true,
            vm_exit_code: Some(0),
            new_data_boc_base64: Some(new_data.to_string()),
            has_actions,
        }
    }

    fn clean_get(stack: &str) -> GetMethodOutcome {
        GetMethodOutcome {
            success: true,
            vm_exit_code: Some(0),
            stack_boc_base64: Some(stack.to_string()),
        }
    }

    #[test]
    fn gvr_confirms_when_the_field_is_byte_identical_before_and_after() {
        let provider = ScriptedProvider::default();
        provider.gets.lock().unwrap().push_back(clean_get("stackA"));
        provider
            .sends
            .lock()
            .unwrap()
            .push_back(clean_send("data2", false));
        provider.gets.lock().unwrap().push_back(clean_get("stackA")); // unchanged
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Gvr {
                trigger_op: 0x1234_5678,
                get_method: "getCounter".to_string(),
            },
            &dummy_target(),
        );
        assert!(matches!(result, DynamicVerification::Confirmed(_)));
    }

    #[test]
    fn gvr_refutes_when_the_field_actually_changed() {
        let provider = ScriptedProvider::default();
        provider.gets.lock().unwrap().push_back(clean_get("stackA"));
        provider
            .sends
            .lock()
            .unwrap()
            .push_back(clean_send("data2", false));
        provider.gets.lock().unwrap().push_back(clean_get("stackB")); // changed
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Gvr {
                trigger_op: 0x1234_5678,
                get_method: "getCounter".to_string(),
            },
            &dummy_target(),
        );
        assert_eq!(result, DynamicVerification::Refuted);
    }

    #[test]
    fn lep_confirms_when_the_tail_is_accepted_cleanly() {
        let provider = ScriptedProvider::default();
        provider
            .sends
            .lock()
            .unwrap()
            .push_back(clean_send("data2", false));
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Lep {
                trigger_op: 0x1234_5678,
                fields: vec![ScenarioField::U32(42)],
                trailing_bytes: vec![0xde, 0xad],
            },
            &dummy_target(),
        );
        assert!(matches!(result, DynamicVerification::Confirmed(_)));
    }

    #[test]
    fn lep_refutes_when_the_tail_is_rejected() {
        let provider = ScriptedProvider::default();
        provider.sends.lock().unwrap().push_back(SendOutcome {
            success: true,
            accepted: false,
            vm_exit_code: Some(9),
            new_data_boc_base64: None,
            has_actions: false,
        });
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Lep {
                trigger_op: 0x1234_5678,
                fields: vec![ScenarioField::U32(42)],
                trailing_bytes: vec![0xde, 0xad],
            },
            &dummy_target(),
        );
        assert_eq!(result, DynamicVerification::Refuted);
    }

    /// Builds a `VmStack`-shaped `BoC` carrying exactly two `tinyint`-tagged
    /// 64-bit values — the one shape [`decode_stack_ints`] actually
    /// understands (see that function's own doc comment), needed here
    /// because (unlike `GVR`'s tests, which only ever compare two stacks
    /// for byte-identity and never decode either one) `ID`'s strategy
    /// genuinely decodes the returned stack.
    ///
    /// Named `top`/`second` deliberately, not `a`/`b`: `decode_stack_ints`
    /// reads *top-of-stack first* (confirmed against a real emulator run,
    /// see [`verify_id`]'s own comment at the call site) — `top` becomes
    /// `values[0]`, `second` becomes `values[1]`. For a Tolk `return (a,
    /// b)`, `top` is `b` (pushed last) and `second` is `a` (pushed first),
    /// *not* the other way around — naming the parameters `a`/`b` here
    /// previously hid exactly this reversal and let a wrong `verify_id`
    /// implementation pass its own tests while failing for real.
    fn stack_of_two_ints(top: i64, second: i64) -> String {
        // Three cells chained by reference, not two — confirmed against a
        // *real* `libemulator` `getFields()` response
        // (`corpus/fixtures/bridge/IdSwappedPattern.*`) during this
        // function's own development, not assumed from
        // `decode_stack_ints`'s doc comment alone: the real `VmStack`
        // encoding always has one more reference past the last value read
        // (almost certainly a "rest of stack" terminator in the real
        // encoding), which `decode_stack_ints` relies on being present —
        // *even for the very last value `count` asks for* — as part of its
        // own walk. A 2-cell version (no terminator) is not a realistic
        // stack and `decode_stack_ints` correctly refuses to decode it.
        let terminator = CellBuilder::new()
            .build()
            .expect("builds an empty terminator cell");

        let mut middle = CellBuilder::new();
        middle.store_u8(8, 0x01).expect("stores the tinyint tag");
        middle
            .store_i64(64, second)
            .expect("stores the second value");
        middle
            .store_child(terminator)
            .expect("links the terminator cell");
        let middle_cell = middle.build().expect("builds the middle cell");

        let mut outer = CellBuilder::new();
        outer.store_u32(24, 0).expect("stores the depth field");
        outer.store_u8(8, 0x01).expect("stores the tinyint tag");
        outer.store_i64(64, top).expect("stores the top value");
        outer
            .store_child(middle_cell)
            .expect("links the second value's cell");
        let outer_cell = outer.build().expect("builds the outer cell");

        let bytes = BagOfCells::from_root(outer_cell)
            .serialize(false)
            .expect("serializes to bytes");
        BASE64.encode(bytes)
    }

    #[test]
    fn id_confirms_when_the_fields_come_back_swapped() {
        // The contract returned `(a, b) = (field_b, field_a)` — a genuine
        // swap. On the wire that means top-of-stack (`b`'s position) holds
        // `field_a`'s value and the value below it (`a`'s position) holds
        // `field_b`'s — see `stack_of_two_ints`'s own doc comment for why
        // this is *not* the same as passing `(field_a, field_b)` in
        // top-to-bottom order.
        let provider = ScriptedProvider::default();
        provider
            .sends
            .lock()
            .unwrap()
            .push_back(clean_send("data2", false));
        provider.gets.lock().unwrap().push_back(GetMethodOutcome {
            success: true,
            vm_exit_code: Some(0),
            stack_boc_base64: Some(stack_of_two_ints(11, 22)),
        });
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Id {
                save_op: 0x1234_5678,
                field_a: 11,
                field_b: 22,
                get_method: "getFields".to_string(),
            },
            &dummy_target(),
        );
        assert!(matches!(result, DynamicVerification::Confirmed(_)));
    }

    #[test]
    fn id_refutes_when_the_fields_come_back_in_the_same_order() {
        // The contract returned `(a, b) = (field_a, field_b)` correctly —
        // top-of-stack (`b`'s position) holds `field_b`'s value, the value
        // below it (`a`'s position) holds `field_a`'s.
        let provider = ScriptedProvider::default();
        provider
            .sends
            .lock()
            .unwrap()
            .push_back(clean_send("data2", false));
        provider.gets.lock().unwrap().push_back(GetMethodOutcome {
            success: true,
            vm_exit_code: Some(0),
            stack_boc_base64: Some(stack_of_two_ints(22, 11)),
        });
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Id {
                save_op: 0x1234_5678,
                field_a: 11,
                field_b: 22,
                get_method: "getFields".to_string(),
            },
            &dummy_target(),
        );
        assert_eq!(result, DynamicVerification::Refuted);
    }

    #[test]
    fn id_is_inconclusive_when_the_save_message_does_not_complete_cleanly() {
        let provider = ScriptedProvider::default();
        provider.sends.lock().unwrap().push_back(SendOutcome {
            success: true,
            accepted: false,
            vm_exit_code: Some(9),
            new_data_boc_base64: None,
            has_actions: false,
        });
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Id {
                save_op: 0x1234_5678,
                field_a: 100,
                field_b: 200,
                get_method: "getFields".to_string(),
            },
            &dummy_target(),
        );
        assert!(matches!(result, DynamicVerification::Inconclusive(_)));
    }

    #[test]
    fn ubm_confirms_when_state_changed_and_an_action_was_queued() {
        let provider = ScriptedProvider::default();
        provider
            .sends
            .lock()
            .unwrap()
            .push_back(clean_send("new-data", true));
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Ubm {
                mint_op: 0x4d49_4e54,
                mint_amount: 1_000_000_000,
            },
            &dummy_target(),
        );
        assert!(matches!(result, DynamicVerification::Confirmed(_)));
    }

    #[test]
    fn ubm_is_inconclusive_when_no_action_was_queued() {
        let provider = ScriptedProvider::default();
        provider
            .sends
            .lock()
            .unwrap()
            .push_back(clean_send("new-data", false));
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Ubm {
                mint_op: 0x4d49_4e54,
                mint_amount: 1_000_000_000,
            },
            &dummy_target(),
        );
        assert!(matches!(result, DynamicVerification::Inconclusive(_)));
    }

    #[test]
    fn ubm_is_inconclusive_when_the_call_itself_fails_cleanly() {
        let provider = ScriptedProvider::default();
        provider.sends.lock().unwrap().push_back(SendOutcome {
            success: true,
            accepted: false,
            vm_exit_code: Some(73),
            new_data_boc_base64: None,
            has_actions: false,
        });
        let bridge = BridgeController::new(&provider);
        let result = bridge.verify(
            &Hypothesis::Ubm {
                mint_op: 0x4d49_4e54,
                mint_amount: 1_000_000_000,
            },
            &dummy_target(),
        );
        assert!(matches!(result, DynamicVerification::Inconclusive(_)));
    }

    #[test]
    fn build_scenario_body_encodes_op_and_fields_into_a_real_boc() {
        let boc = build_scenario_body(
            0x1234_5678,
            &[ScenarioField::U32(1), ScenarioField::Coins(1_000)],
        )
        .expect("builds");
        assert!(!boc.is_empty());
        // Round-trips through BagOfCells without error -- a real BoC, not a
        // placeholder string.
        assert!(BagOfCells::parse_base64(&boc).is_ok());
    }

    #[test]
    fn decode_stack_ints_reads_zero_values_without_touching_the_cell() {
        assert_eq!(decode_stack_ints("irrelevant", 0), Some(vec![]));
    }
}
