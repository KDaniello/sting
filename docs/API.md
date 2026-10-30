# API

Every contract that crosses a boundary: the shared types, the three command lines, the HTTP protocol,
the output formats and the diagnostic codes. How the tree is built: [`ARCHITECTURE.md`](./ARCHITECTURE.md).
What proves it: [`TESTING.md`](./TESTING.md).

Everything below is read off the real signatures in `crates/`, inconvenient details included.

---

## 1. Shared types

### 1.1 `Finding` — the unit of a security result

```rust
pub struct Finding {
    pub detector_id: DetectorId,                          // &'static str, not an enum — see below
    pub severity: Severity,                               // Info < Low < Medium < High < Critical
    pub message: String,                                  // what is wrong, in the terms of this code
    pub remediation: String,                              // what to change — a required field
    pub location: Location,                               // file, line and column, 1-indexed
    pub confidence: Option<Confidence>,                   // Some only after the scoring pass
    pub dynamic_verification: Option<DynamicVerification>,// Some only after the bridge pass
    pub suggestion: Option<Suggestion>,                   // a machine-applicable edit, §1.4
}
```

**`remediation` is required, not optional.** The split from `message` is deliberate: `message`
explains the defect and `remediation` tells the developer what to change — the same pair as `rustc`'s
`error:` and `help:`. It reaches SARIF through two standard channels, `properties.remediation` on
each result and `ReportingDescriptor.help` on each rule.

**`detector_id` is a string rather than a `DefectClass`.** The TONScanner taxonomy is a chosen
starting scope, not a ceiling: `EPS` and `UDA` are real registered detectors that are not in
`DefectClass` at all, and `DEPR` is not about vulnerabilities. Restricting the id to an enumeration
would have made them inexpressible. `Finding::defect_class()` recovers the class where one exists.

**There is no separate `id` field.** The benchmark matches findings to the reference by
`(file, detector_id, line_range)`, and that tuple already identifies a finding uniquely.
`stable_id()` builds a deduplication key on demand.

### 1.2 Refusal and uncertainty are expressed by type

```rust
pub enum DynamicVerification {
    Confirmed(ProofOfConcept),  // the emulator reproduced the predicted behaviour
    Refuted,                    // the pattern matched and the behaviour is safe
    Inconclusive(String),       // it cannot be checked, and this is why
}

pub struct ProofOfConcept {
    pub expected: String,       // what the static pass predicted
    pub actual: String,         // what the emulator observed
    pub steps: Vec<String>,     // enough for a person to repeat it by hand
}
```

`Refuted` **does not delete** the finding — the report lowers its rendered severity. The difference
matters: deleting it would hide from the user both that the pattern is in the code and that it was
checked.

`Inconclusive` carries its reason as a string because the reasons are many and they are substantive.
A confirmed example: findings that hinge on `in.senderAddress` cannot be reached through
`TvmEmulator::send_internal_message` at all — written down rather than disguised as `Refuted`.

The same principle in two other contours:

| Type | Crate | What it expresses |
|---|---|---|
| `ProbeOutcome::PrecompiledContractUnsupported` | `sting-scamcheck` | the bytecode is registered in `ConfigParam 45` and cannot run under an emulator at all |
| `ProbeOutcome::ExecutedSilently` | `sting-scamcheck` | success, `exit_code = 0`, and zero output actions — accepted and swallowed |
| `MatchStatus` | `sting-abi-registry` | whether the rebuilt bytecode matched the on-chain code |
| `ExtractionTier::Heuristic` with a required `confidence` | `sting-abi-core` | the schema came from a heuristic rather than from a compiler |

### 1.3 `Confidence` — a range guaranteed by its constructor

```rust
Confidence::new(f32) -> Result<Confidence, ConfidenceRangeError>  // rejects outside [0,1] and NaN
Confidence::saturating(f32) -> Confidence                          // clamps; NaN becomes 0.0
```

The two constructors are not duplicates. `new` is for outside or unchecked input. `saturating` is for
a caller whose value is bounded **by construction** — a weighted sum where every term is already in
range and the weights total 1.0 — and who still wants protection from a floating-point edge without a
`Result` that neither side expects to see used.

### 1.4 `Suggestion` — a machine-applicable edit, and why almost nobody has one

```rust
pub struct Suggestion { pub start_byte: usize, pub end_byte: usize, pub replacement: String }
```

**Exactly two detectors fill it: `DEPR` and `SUA`.** That is a boundary, not an omission.

A finding whose fix needs human judgement about intent may not carry a `Suggestion` — which check to
add, which variable was meant, whether the shadowing was deliberate. A plausible-looking wrong check
inserted automatically is worse than no finding at all, because it manufactures the feeling of a
solved problem. A deprecated stdlib name and `x =+ 1` written for `x += 1` are the only two where a
single deterministic correct replacement exists. It is the same line `cargo fix` and `clippy --fix`
draw.

### 1.5 `ContractSchema` — the unit of an abi-layer result

```rust
pub struct ContractSchema {
    pub ir_schema_version: String,          // version of THIS schema, not of the source compiler
    pub source_language: SourceLanguage,    // tolk | tact | func
    pub extraction_tier: ExtractionTier,    // compiled | heuristic | statistical
    pub contract_name: Option<String>,
    pub metadata: ContractMetadata,         // author, version, description from the header
    pub storage: Option<TypeRef>,           // None is a real value, not a placeholder
    pub types: Vec<NamedType>,              // deduplicated table; every other field indexes into it
    pub incoming_internal: Vec<MessageRef>,
    pub incoming_external: Vec<MessageRef>,
    pub outgoing: Vec<MessageRef>,
    pub get_methods: Vec<GetMethod>,
    pub errors: Vec<ErrorCode>,
    pub confidence: Option<ExtractionConfidence>,   // Some if and only if tier = heuristic
    pub field_confidence: Option<FieldConfidence>,  // per-category breakdown, optional for Tier 2 too
}
```

**Everything indexes into `types[]` and never by name.** Otherwise two different types with the same
name, from different files, would merge silently.

**`storage: None` is a real value rather than "not extracted yet".** A project without a `contract{}`
header compiles under Tolk and genuinely has no declared storage type.

**One cross-field invariant** — `confidence` is filled if and only if `extraction_tier == Heuristic`
— is not enforced by a constructor, because this is plain-data with public fields, and is checked by
an explicit `validate()` every extractor calls before returning. The crate does not call it for them:
a structure with public fields cannot refuse to be built in an invalid state.

**`ir_schema_version` moves independently** of Tolk's `abi_schema_version` and of Tact's
`ContractABI` version. It has been raised once, when `TypeShape::Str` was added: both languages emit
that kind, the schema had no model for it, and every string field was degrading silently to
`Unknown`.

---

## 2. `sting` — the security CLI

```
sting <COMMAND>
  -v, -vv                 debug / trace (RUST_LOG overrides it entirely)
  --color auto|always|never
```

| Command | What it does | Key flags |
|---|---|---|
| `lint <path>` | runs the detectors over a file or a directory | `--format human\|sarif\|json`, `--output`, `--config`, `--min-severity`, `--fix` |
| `scan <address>` | dynamic examination of a deployed contract | `--get-method` (repeatable), `--testnet`, `--force-refresh`, `--format`, `--output` |
| `verify <path> <address>` | closes static onto dynamic: lint, hypothesis, emulator, scoring | `--hypotheses <file.toml>` (required), `--baseline`, `--min-severity` |
| `bench` | precision and recall over the labelled corpus | `--corpus`, `--report`, `--baseline`, `--update-baseline` |
| `fetch <address>` | downloads code and data for an on-chain address | `--out`, `--testnet`, `--force-refresh` |
| `explain <detector_id>` | a detector's profile: what it looks for, its measured accuracy, its limits | — |
| `config dump` | the effective configuration after defaults and file are merged | `--config` |
| `doctor` | the four versions an answer depends on: emulator, global version, compiler, grammar | — |
| `fuzz <target>` | a thin wrapper over `cargo fuzz run`, not an implementation | `--time`, `-- <libFuzzer args>` |

### 2.1 Three things these commands deliberately do not do

**`fetch` does not give you source.** It downloads bytecode. A deployed contract has no recoverable
source through this command: what `fetch` brings back is input for the emulator and the fuzzer, never
for the static detectors. If you have both the source and your own deployed copy, you pass the
`.tolk` to `lint` and the address to `fetch` **separately**; `sting` never compiles one into the
other.

**`scan` runs no detectors and no dynamic bridge.** An address has no source, so there is no `Finding`
to build a hypothesis from. Deriving a hypothesis from bytecode alone is not attempted: general
recovery of a message dispatch convention is an openly unsolved research problem, not a corner cut.

**`verify` does not infer the hypothesis from the source.** `--hypotheses` is a TOML file written by
a person who already knows the answer, one section per defect class. A finding whose class the file
does not cover still reaches the report and is still scored, on static evidence alone, exactly as in
`lint`.

### 2.2 Exit codes, and one real hole

| Situation | Code |
|---|---|
| success | `0` |
| execution error: no files, broken config, network | `1` |
| `scan` found at least one finding | `1` |
| `doctor` could not read one of the four versions | `1` |
| **`lint` found at least one finding** | **`0`** |

The last row is **a real integration defect, not a design**. `sting lint` returns `0` however many
findings it produced, because no path in its command branch fails. A linter that always returns zero
cannot gate anybody's CI — the most obvious way to use it does not work today without a wrapper that
parses SARIF. The fix is a `--fail-on <severity>` flag, and it is named here rather than hidden.

### 2.3 Output formats

**`human`** is more than `file:line:col: [ID] Severity: message`. Under the location line it prints
the source line with `^^^^^` under the exact column range, and always a `= help: {remediation}` line.
The snippet is skipped when `Location.file` is not physically readable at render time — `scan`
findings use the address itself as `file`, and there is nothing to read.

**`sarif`** is SARIF 2.1.0 through `serde-sarif` rather than hand-assembled JSON.
`tool.driver.name = "sting"`, `ruleId` is `sting/<CODE>`. Every result carries
`properties.confidence` and `properties.dynamicVerification`
(`confirmed`/`refuted`/`inconclusive`/`not-attempted`). The output is validated against the real
SARIF 2.1.0 JSON schema in a test, rather than assumed correct because the structure is typed.

**`json`** is a flat array of findings, not SARIF — a separate format for a consumer who does not want
SARIF scaffolding.

**How the default is chosen**: an explicit `--format` wins; otherwise `sarif` when `--output` is
given; otherwise `human` on an interactive terminal; otherwise `sarif`.

**A clean stdout is an invariant.** `scan`'s status lines are printed only when the format resolves
to `human`. Printing them unconditionally once wedged plain text in front of a JSON payload and
corrupted it for every downstream parser — found on a real run, not in review.

### 2.4 `Sting.toml`

```toml
[detectors]
enable = ["GVR", "LEP"]     # a non-empty list acts as a whitelist
disable = ["ID"]            # always subtracted

min_severity = "high"       # an optional global floor

[[suppressions.rule]]
detector = "LEP"
file = "contracts/legacy.tolk"
line = 42
reason = "wrapped by an external library; the caller calls end_parse"
```

The file is looked up upward from the current directory, the same convention as `Cargo.toml` and
`Acton.toml`.

**All four fields of a suppression are required, `reason` included.** The comparable mechanism in
Misti does not ask for one. An unexplained suppression is exactly the class of ageing unaccountable
exception this mechanism should make harder rather than easier. The inline equivalent is
`// sting:allow(ID1,ID2) reason` on the line immediately before the finding, and a line **without a
reason is not recognised as a suppression at all** — the finding stays visible. Unused rules produce
a warning.

**Detector codes are not validated in `sting-core`.** `StingConfig::parse` is purely structural:
`sting-core` does not depend on `sting-detectors` and cannot know the universe of valid ids. The
check lives where the registered set is known. The opposite was implemented once and was a defect —
the config rejected a legitimate non-taxonomy id, found by running `sting config dump` rather than by
a test.

**Designed and without a consumer**: `[detectors.rules.<ID>].severity_override`, `[corpus]`,
`[report]`. Listed here honestly rather than presented as working.

---

## 3. `abi-layer` — the extraction CLI

| Command | What it does |
|---|---|
| `extract <path>` | a normalised `ContractSchema` from one entry point. `--lang tolk\|tact\|func` overrides detection by extension |
| `bench` | Tier 1 structural completeness, Tier 2 precision and recall, and throughput against the real corpus |
| `dictionary-build` | rebuilds the method-id dictionary from the whole real Tolk, Tact and FunC corpus |
| `dictionary-probe <address>` | calls every known method id against real on-chain code and data — **no source is read at all** |
| `dictionary-verify` | rebuilds the dictionary into a scratch file and compares it with the committed one, byte for byte |
| `dictionary-audit --sample <n>` | recompiles a sample of the compiler-extracted entries and checks them against tolk's own `abi.json` |
| `catalogue-fetch --checkout <clone>` | rereads `ton-blockchain/abis`'s two lists from a checkout, with the commit read out of it |
| `catalogue-diff` | the difference from that catalogue in both directions ([`domains/method-dictionary.md`](./domains/method-dictionary.md) §10) |
| `decode --body <base64>` / `--hash <tx>` | what operation a message is, with its fields ([`domains/decode.md`](./domains/decode.md)) |
| `decode-sample --transactions <n>` | how much of real mainnet traffic the datasets can read, in three shares |
| `catalogue-operations --checkout <clone>` | an operation dictionary learned from the foundation catalogue's own type files |
| `sweep --db <sqlite>` | a mainnet walk deduplicated by code hash, probing every new code |
| `coverage-report --db <sqlite>` | aggregate coverage plus the top candidates for extending the dictionary |
| `tact-share --db <sqlite>` | how many unique on-chain codes declare themselves Tact ([`domains/method-dictionary.md`](./domains/method-dictionary.md) §8) |

**`--format tlb` is reserved and not implemented** — declared in the schema design, no consumer yet.

**`sweep` puts real sustained load on a public third-party service.** `--pilot-size` defaults to
5000; before going past a pilot, read [`DATA.md`](./DATA.md) §4.

**`--reprobe` exists because a gap was found**, not because it was designed: a dictionary that grew
**after** a code was first probed never got a second chance at that code, since an already-probed
code was not re-examined. The flag re-probes previously unrecognised codes and leaves recognised ones
alone.

---

## 4. `sting-abi-service` — HTTP

The one containerised contour. `axum` and `tokio`; compilation moves to a blocking pool, and a batch
is parallelised with `rayon` inside it.

| Method | Path | Rate limited | What it does |
|---|---|---|---|
| `POST` | `/v1/extract` | yes | a schema from one submitted source |
| `POST` | `/v1/extract/batch` | yes | up to `batch_max_items` per request |
| `POST` | `/v1/verify` | yes | compares rebuilt bytecode with the on-chain code at an address |
| `POST` | `/v1/probe` | yes | probes method ids at an address, without source |
| `POST` | `/v1/decode` | yes | what a message is, by transaction hash or by body ([`domains/decode.md`](./domains/decode.md)) |
| `GET` | `/v1/schema/{address}` | no | a previously verified schema from storage |
| `GET` | `/v1/health` | no | liveness |
| `GET` | `/v1/version` | no | the service and schema versions |

**The split into two sub-routers is deliberate**: the rate limit applies to the extraction paths and
never to `/v1/health` or `/v1/version` — otherwise a monitoring system would lose the ability to poll
the service exactly when the service is under load.

### 4.1 One shape of error

Every unsuccessful response is `{"error": {"code": "…", "message": "…"}}`, a malformed body and an
oversized one included: the framework's own rejection bodies do not leak out unstructured.

| Code | HTTP | Meaning |
|---|---|---|
| `invalid_request` | 400 | malformed JSON, unknown `lang`, oversized batch — the client's error |
| `unsupported_language` | 400 | no extractor is registered for it |
| `compilation_failed` | 400 | the compiler ran and produced a genuine compilation error |
| `payload_too_large` | 413 | the body is larger than `max_body_bytes` |
| `rate_limited` | 429 | this address has spent its budget |
| `timeout` | 504 | extraction did not fit the budget |
| `no_verified_source` | 404 | no verified source for this address — an honest error, **never an empty `200`** |
| `on_chain_resolution_failed` | 502 | an upstream failure, or the address does not exist |
| `registry_unavailable` / `tier3_unavailable` | 503 | the subsystem is not configured on this deployment — a deployment gap, not a bad request |
| `decode_unavailable` | 503 | no operation dataset on this deployment — the same deployment gap as the two above |
| `unauthorized` | 401 | this deployment names keys and the request presented none, or one it does not know |
| `quota_exhausted` | 429 | the key is known and has spent its period's budget — **not** the same refusal as `rate_limited` |
| `result_too_large` | 507 | the answer is larger than `max_result_bytes`, and is refused rather than truncated |
| `internal_error` | 500 | our defect; the client gets a general message and no internal paths or state |

**Not knowing is not an error.** `POST /v1/decode` answers an opcode nothing names with `200` and
`unknown: true`. A `404` would tell the caller their question was wrong when it was this platform's
knowledge that ran out, and it would hide the share the product is measured by inside an error code.

**`rate_limited` and `quota_exhausted` are two limits, not one.** The first bounds a burst from one
address, the second a period for one caller however many addresses it comes from. A caller told only
`429` cannot tell "slow down" from "come back tomorrow".

**Separating `invalid_request` from `internal_error` is not pedantry.** `internal_error` means "our
defect", and reusing it for a client's mistake would mislead whoever reads the logs, systematically.

### 4.2 Three independent limits, not one

```rust
max_body_bytes:      1_000_000
compile_timeout:     60s        // kills the compiler subprocess — real enforcement
request_timeout:     90s        // = compile_timeout + 30s of headroom
batch_max_items:     100
max_result_bytes:    1_000_000  // what the service will send back
rate_limit:          60 requests / 60 seconds per address
quota:               10 000 requests / 24 hours per key, when keys are named
```

**The result size is the third limit and neither of the others stands in for it.** A small request
answered quickly can still name a message whose reading is long, and a caller who asked for a decode
did not ask to be handed a megabyte. Over the limit the request fails with `result_too_large` rather
than being answered with a cut-off reading that looks whole.

**Keys are a decision a deployment makes.** `STING_API_KEYS` empty leaves the service open, which is
what it has been since it was written; naming keys turns both the key check and the quota on at once,
because a key that is optional is a key it is cheaper not to present.

`compile_timeout` and `request_timeout` are **separate fields rather than one derived from the
other**. The first kills the compiler process and frees the blocking thread; the second is the
asynchronous backstop for the case where that mechanism does not return, and the only bound on a
batch, whose total duration a per-item timeout does not limit.

**The 60 and 90 were not chosen by eye.** The first load run, at 20 and 25, showed that a batch of
**three** Tact contracts reliably produced `504`. Not because of routing: parallelism worked and the
container reached 550% CPU. The budget had been calibrated without the real cost of a cold `tact.js`
start under contention, p99 near 11.8 seconds for a single call. The current numbers come from the
second run, where every request finished `200`.

**The caveat that remains**: those defaults were verified at a batch size near three, not at the full
hundred under concurrency. The report was not re-run with the new defaults, and the point is open.

---

## 5. `sting-scamcheck` — risk signals

Two binaries in one crate.

```
sting-scamcheck <jetton-address>
gift_trust_check <collection-or-item-address> [--domain X] [--wallet Y]
```

Both need a real `libemulator` through `STING_LIBEMULATOR_PATH`.

### 5.1 A composite score rather than a single-factor verdict

```rust
pub struct ScamCheckReport {
    pub mint_rights_renounced: Option<bool>,  // from get_jetton_data(): admin_address == addr_none
    pub honeypot_reverted: Option<bool>,      // a transfer probe against REAL holders
    pub top_holders_share: Option<f64>,       // supply concentration
    pub lp_owner_share: Option<f64>,          // the owner's share of the liquidity pool
}
```

| Signal | Points |
|---|---|
| mint rights not renounced | 1 |
| the transfer probe reverted or passed silently | **3** |
| a share above 0.5 (each of the two shares) | 2 |
| a share above 0.2 (each of the two shares) | 1 |

`0` is `Low`, `1–2` is `Medium`, `3` or more is `High`.

**The honeypot weighs triple because it is the only one that observes restricting *behaviour*
directly** rather than a proxy for it. The other three are indirect.

**The `Option` in all four fields is substantive.** `None` means "could not check", and it is never
folded into `Some(false)`, which would read as "clean". Concretely: if every tested holder returned
`PrecompiledContractUnsupported`, `honeypot_reverted` stays `None`.

A live example of why the composite is needed: one jetton had mint rights **renounced** and its
transfer probe reverted anyway, with `exit_code: 714`. A single-factor check would have called it
clean.

### 5.2 The Gift and NFT branch

`content.rs` is a TEP-64 decoder written by hand, because the crate that would have provided one does
not; then `gift_registry.rs`, `nft_authenticity.rs`, `domain_reputation.rs`, `wallet_reputation.rs`.

The starter registry of Gift collections holds **nine entries, marked as non-exhaustive**, and the
CLI prints `NOT IN STARTER REGISTRY` rather than `FAKE`. The difference is semantic, not stylistic.

A real finding from assembling that registry, and not one the specification anticipated: all nine
confirmed collections share **one** `code_hash` — the official Telegram Gift contract, different from
a generic marketplace collection. It is recorded as a second, independent and cheaper signal that
strengthens the address check without replacing it.

`domain_reputation` computes an edit distance to known legitimate domains. Verified live:
`fragm-ent.com` against `fragment.com`, distance 1. The honest caveat sits inside it — one
marketplace has **four** plausible domains and no authoritative source for which is canonical, so all
four are recorded with `multi_source_confirmed: false`.

---

## 6. What this API does not promise

- **`sting lint` does not gate CI on its own** — §2.2.
- **None of the three command lines promises interface stability.** `version = "0.0.0"`,
  `publish = false`.
- **`ir_schema_version` is a format version, not a compatibility promise**: before publication,
  breaking changes are allowed without a major bump.
- **The HTTP service has not been measured over a long run.** The one load run was local, on one
  machine, and uptime cannot be measured that way.
- **Tolk through HTTP in the container has not been tested at all** — `vendor/` holds only a Windows
  build of the compiler, and there is no Linux binary to mount. The native Tolk throughput number was
  measured by a different method and does not compare one-to-one with the HTTP numbers.

---

## 7. What this document does not cover

It does not say why a boundary is where it is — that is [`ARCHITECTURE.md`](./ARCHITECTURE.md). It
does not say what any of these commands is measured to achieve: precision, recall, coverage and
throughput live in [`TESTING.md`](./TESTING.md) beside the corpus that produced them. And it names no
accuracy number, for the same reason `ARCHITECTURE.md` does not: a contract is stable, a measurement
moves, and a document that mixes them goes stale at the speed of the faster one.
