# The normalised schema

`ContractSchema` is the single artefact every extractor produces and every consumer reads. Why it has
exactly this shape, and what is deliberately not in it.

---

## 1. A problem nobody in TON has closed

TON has no ABI standard. Ethereum has one JSON format everything understands, from a wallet to an
explorer. TON has three languages, three incompatible compiler metadata formats, and a majority of
deployed code with no published source at all.

`ContractSchema` is an attempt at one answer for all three cases: **the same JSON whatever the data
came from**, carrying an explicit mark of how far it can be trusted.

---

## 2. The shape

```rust
pub struct ContractSchema {
    pub ir_schema_version: String,
    pub source_language: SourceLanguage,    // tolk | tact | func
    pub extraction_tier: ExtractionTier,    // compiled | heuristic | statistical
    pub contract_name: Option<String>,
    pub metadata: ContractMetadata,
    pub storage: Option<TypeRef>,
    pub types: Vec<NamedType>,
    pub incoming_internal: Vec<MessageRef>,
    pub incoming_external: Vec<MessageRef>,
    pub outgoing: Vec<MessageRef>,
    pub get_methods: Vec<GetMethod>,
    pub errors: Vec<ErrorCode>,
    pub confidence: Option<ExtractionConfidence>,
    pub field_confidence: Option<FieldConfidence>,
}
```

### 2.1 Everything indexes into the type table, never by name

Storage, every message, every parameter and return of a get method is a reference — an index into a
deduplicated table.

The reason is direct: two different types with the same name, from different files, would merge
silently. An index rules that out by construction rather than by a check.

### 2.2 Three tiers, and confidence belongs to exactly one

| Tier | How it was obtained | Confidence |
|---|---|---|
| compiled | a real compiler emitted structural metadata | none — a compiler's output does not need a trust score |
| heuristic | an AST heuristic over source | **required** |
| statistical | inferred from on-chain behaviour, with no source | none — this tier's trust model is unspecified |

The invariant "confidence is filled if and only if the tier is heuristic" is the type's one
cross-field rule, and it is **not enforced by a constructor**: the fields are public, this is
plain data that cannot refuse to be built in an invalid state. So there is an explicit validation call
every extractor makes before returning. The crate does not make it for them, and says so outright
rather than creating an illusion of protection.

### 2.3 Absent storage is a valid value rather than a placeholder

A project without a contract header compiles and genuinely has no declared storage type.
Distinguishing "no storage" from "could not extract" through an optional field would be impossible if
the empty case meant both — so the second is expressed by an extraction error rather than by an empty
value.

### 2.4 The type shapes, including two honest ones

Void, integers with a width, booleans, coins, addresses, cells, slices, builders, strings, bit
strings, a remaining tail, nullables, arrays, structures, unions, maps, and unknown.

**Remaining** — an unread slice tail — exists because on TON that is a real and frequent field shape
rather than an absence of information.

**Unknown** is an honest "the shape was not recognised". The subtlety that matters for measurement:
**"the source itself said unknown" and "the normaliser failed to recognise it"** produce the same
value, and only the second is a defect. The benchmark separates them by an explicit path that consults
the raw descriptor at the same index. Without that separation one real contract would show half
completeness instead of an honest full one.

---

## 3. The schema version moves separately

The schema's version is the version of **this** format, independent of the source compilers' own
metadata versions. Otherwise a change in somebody else's compiler would force a change to the contract
with our consumers without changing anything in substance.

It has been raised once, when the string shape was added. Both languages emit that kind — an
established fact rather than a hypothesis — the schema had no model for it, and **every string field
degraded silently to unknown**. A frequent case: content, name and metadata fields on jetton and NFT
contracts. Every golden fixture was regenerated and checked by hand; the only substantive change was
two fields moving from unknown to string.

---

## 4. How completeness is checked

**Structural completeness**: for each category, how many elements are normalised without loss against
what the compiler actually emitted.

| Category | Matched | Total | Completeness |
|---|---|---|---|
| errors | 898 | 898 | 100.0% |
| get methods | 154 | 154 | 100.0% |
| incoming external | 8 | 8 | 100.0% |
| incoming internal | 118 | 118 | 100.0% |
| outgoing | 71 | 71 | 100.0% |
| storage | 45 | 45 | 100.0% |

The corpus is 28 contracts in one language plus 19 in another. The source is `corpus-abi/baseline.json`,
a live gate.

### 4.1 How the last tenth of two categories was closed

The first run gave under ninety per cent on get methods and on storage. The whole deficit came down to
**one** root cause rather than to dozens of defects: the compiler really does emit short forms for
nullable primitives inside structures, instead of the wrapper every other nullable type uses.

That was **not** a matter of adding one match arm: the mapping built the type table strictly
positionally, with no way to synthesise an extra entry on the fly. It was classified in the risk table
as needing a structural change rather than as an unnoticed defect, and honestly deferred.

A systematic audit of the whole corpus later found not only that form but two previously unknown ones:
a full address union that really occurs in the standard library, and a null literal that is real and
appeared in no schema field of the audited corpus.

**The answer is a two-pass mapper.** The positional pass is unchanged, so every existing reference
stays valid. The second pass expands each short nullable into the wrapper, **reusing the existing
anonymous slot** when there is one — the frequent real case — and synthesising a new one only when
there is not. The address union maps straight to an address, a documented simplification, since the
schema does not yet distinguish "any address" from "internal address". The null literal is modelled
explicitly and degrades to unknown **deliberately**, rather than because it was forgotten.

The difference between "degrades deliberately" and "was forgotten" is not rhetorical here: the first
is checked by a test, the second is discovered by a user.

---

## 5. Two real defects found by the very first run

**A catch-all receiver broke the parse of the whole artefact.** That form matches any string, so the
compiler emits its kind **without** the text field. The model required the field, and deserialisation
failed on the **entire** document over one line, instead of losing information about one receiver.
Fixed by making the field optional, where its absence means catch-all.

**An error in somebody else's doc comment, found by tracing.** A descriptor in the compiler wrapper
claimed to represent a virtual-machine tuple. The real tuple is an alias for an array of unknowns, and
that descriptor is not involved at all. Found while working through two real get methods that both
return a tuple.

---

## 6. What is deliberately not in the schema

- **A TL-B output.** The format is reserved in the design and has no consumer. Declared and not
  implemented — and that is said rather than implied.
- **Distinguishing any address from an internal one.** The union collapses to an address; a documented
  simplification until a consumer needs the difference.
- **Field semantics.** The schema describes shape rather than meaning: it does not know that a field
  called total supply is a supply. A layer of meaning is a separate problem and outside its scope.
- **A compatibility promise.** Before publication, breaking changes are allowed without a major
  version.

---

## 7. What this document does not cover

It does not describe how each language's metadata is read — that is
[`extractors.md`](./extractors.md) — and it does not describe what the schema is served over, which is
[`service.md`](./service.md). It describes one artefact and the one invariant that cannot be enforced
by its type.
