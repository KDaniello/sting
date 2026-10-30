# Source

`sarif-schema-2.1.0.json` — the official SARIF 2.1.0 JSON Schema, fetched
2026-07-17 from
`raw.githubusercontent.com/schemastore/schemastore/master/src/schemas/json/sarif-2.1.0.json`
(SchemaStore's mirror; its own `$id` field points back at the canonical
`oasis-tcs/sarif-spec` schema URL, confirming it is an unmodified copy).
Used by `tests/sarif_schema_validation.rs` to validate `sting-report`'s
actual SARIF output against the real schema (`git show a7280d2:docs/security/blocks/02-detectors-v1.md`'s
Definition of Done), not just trust that `serde-sarif`'s generated types are
correct by construction.
