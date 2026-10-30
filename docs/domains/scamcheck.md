# Risk signals

Risk signals for jettons and for gift collections. The youngest contour and the only one aimed at an
end user rather than at a developer.

---

## 1. What is built on other people's code and what is ours

This crate is **pure consumption** of what already existed: the emulator wrapper for get methods and
message sending against arbitrary code and data, the chain client for loading by address, an external
crate for cells and addresses. Checked before the first line was written: not one change was needed
in the core, the emulator or the extraction crates.

**The fuzzing crate is deliberately never a dependency.** Its body, fixture and harness modules are
hard-wired to one demonstration contract; it is not a general message-construction engine. Reusing it
would first require generalising it — a change to a core rather than reuse, and the difference matters
here.

**There is no shared trait or registry of signals, and that is a decision rather than an omission.**
The detector trait in the security contour paid for itself only after several real detectors. The same
discipline — widen when a second real case justifies it — is applied here.

---

## 2. Four jetton signals

| Module | Signal | How it is obtained |
|---|---|---|
| mint rights | minting is not renounced | the standard data call: the admin address is absent |
| honeypot | transfers are restricted | **a transfer probe against real holders** through a real virtual machine |
| holder concentration | supply concentration | the share of the top holders |
| pool concentration | liquidity concentration | the owner's share of the liquidity pool |

### 2.1 The honeypot is the only signal that observes behaviour

The other three are indirect proxies. This one runs a real virtual machine on real bytecode and
watches what happens.

**The first version guessed the internal storage layout of a standard wallet, and that was wrong.**
The standard specifies the **external interface and not the storage layout**. The guess produced a
false positive on a widely held real token. Replaced by using entirely real code and data of **real
holders**, obtained through the concentration module.

That exposed the second, narrower finding — §3.

The probe's outcomes are types rather than a boolean:

```rust
Reverted                          // the transfer was refused
ExecutedSilently                  // success, exit code 0, and ZERO output actions
PrecompiledContractUnsupported    // the bytecode cannot run under an emulator at all
```

Executing silently is the "accepted and swallowed" pattern the previous version could not catch at
all: formally a success, and no transfer happened.

---

## 3. Precompiled contracts: the finding that changed a verdict on the largest token

One widely held wallet is registered as a precompiled contract in the chain's configuration, and its
raw bytecode **cannot run under an emulated machine at all**, even on entirely real data.

**The first attempt to detect that was wrong.** The check matched text in the virtual machine's log —
and that line comes from the library's **global logger** rather than from the structured log returned
across the FFI. Caught by an integration test rather than by reasoning.

**Replaced by a structural check**: the relevant configuration parameter is decoded from the real
mainnet config through a generic dictionary decoder, and the probed wallet's cell hash is compared
against that set **before** the emulator is started.

The practical effect: the CLI stopped printing a false "reverted" on that token and prints an honest
"inconclusive".

---

## 4. The composite score

```rust
minting not renounced      → 1 point
the transfer probe refused → 3 points
top-holder share  > 0.5 → 2,  > 0.2 → 1
pool-owner share  > 0.5 → 2,  > 0.2 → 1
```

Zero is low, one or two is medium, three or more is high.

**The honeypot weighs triple**, because it is the only one that observes restricting *behaviour*
directly. The other three are indirect signs.

**The optional type in all four fields is substantive and never collapsed.** An absent value means
"could not check" and **never** becomes a false, which would read as clean. Concretely: if every tested
holder came back unsupported, the field stays absent.

**The high threshold is deliberately coarse**: several weak signs and one strong sign produce the same
verdict — stop treating this as routine. Finer discrimination is not yet justified by the data, and
that is recorded rather than imitated by a precision that does not exist.

---

## 5. What a smoke test over eight live tokens gave

A run against real mainnet, with no selection of favourable results. Five observations, each
substantive:

**The composite justified itself on a concrete example.** One token had minting **renounced** and its
transfer probe reverted anyway with a specific exit code. A single-factor check would have called it
clean.

**An original finding by the tool rather than a confirmation of somebody's label.** A group of tokens
produced a self-consistent, reproducible failure pattern pointing at a common malicious wallet
template from one deployer. No public scam label was found for that address, checked directly —
recorded honestly as an original finding rather than as agreement with an outside source.

**A token's name is not an identifier, and that was confirmed in practice.** An object carrying a
well-known name in the sample is almost certainly not that token: zero holders at the indexer and a
supply of the wrong order. The tool never relies on the name for identity, only on the address; this
case showed why — same-named doubles are common, and trusting a name would be a vulnerability in the
checker itself.

**A divergence from an external indexer is explained rather than passed over.** The indexer's own
mintable flag disagreed with our stricter check that the admin address is absent. That is not a
defect: theirs is evidently a heuristic of a different kind, probably examining the mint function
itself rather than the presence of an admin. Worth keeping in mind in any comparison with external
indexers.

**The honest limits of that run**, recorded beside its results.

The sample is small. There are no synthetic positive controls on a test network. No object was
checked against a history of value locked, so risk levels were compared against the **internal**
consistency of the signals rather than against confirmed external ground truth. The sort order
requested did not produce the expected one, so the sample is not the top of the market by volume.

---

## 6. The gift and NFT branch

Five modules: metadata decoding, a starter registry, authenticity, domain reputation, wallet
reputation. A separate binary drives them.

**The metadata decoder is written by hand** — no ready one exists in the crate that would have
supplied it, checked against its documentation rather than assumed. Four unit tests plus an
integration test against a real fixture passed on the first attempt, matching an independent ground
truth with no correction.

**A real finding from assembling the registry, and not one the task anticipated.** Every candidate was
checked by a live query rather than trusted from a search snippet. All nine confirmed gift collections
turned out to share an **identical** code hash — the official gift contract, different from a generic
marketplace collection. Recorded as a second, independent and cheaper signal: it strengthens the
address check without replacing it.

**The registry is honestly incomplete, and the wording reflects it.** The CLI prints "not in the
starter registry" rather than "fake". The difference is semantic: the tool says it does not know this
one, not that this one is a forgery.

**A live check on mainnet, five runs with no cherry-picking.** The item-to-collection path is confirmed
separately: the data call on an item resolves its collection address correctly, not only a direct call
against an already known collection.

**Domain reputation** computes an edit distance to known legitimate domains. Confirmed live: a
hyphenated imitation of the official host at distance one, on a real collection.

An honest caveat sits inside the module: one marketplace has **four** plausible domains and no
authoritative source for which is canonical. All four are recorded with a flag saying they are not
multi-source confirmed — the uncertainty is written into the data rather than resolved by an arbitrary
choice.

**Wallet reputation: a deliberate refusal rather than a gap.** A disputed trade as a signal is not
implemented, because **no observable on-chain event corresponds to it**. Documented in the module
itself as a decision rather than left as a silent hole.

---

## 7. What has not been done

- **Held-out validation.** Precision and recall against a set with confirmed outcomes is an open task.
  The smoke test over eight tokens **does not close it and does not replace it**.
- **Validating the domain heuristic against a real phishing dataset.** One documented precedent was
  used as a test — confirmation of the mechanism rather than a measurement of accuracy.
- **A public bot or API.** Deliberately left as a local command line: a question of monetisation and
  moderation rather than of engineering.
- **Synthetic positive controls on a test network.** Not done in any run.

## 8. Tests

Forty-eight in the crate: thirty-six unit and twelve integration, against a real emulator and captured
live HTTP responses rather than mocks. They need the library path and **skip rather than fail** without
it.

---

## 9. What this document does not cover

It does not describe the emulator these signals run on — that is [`emulator.md`](./emulator.md) — and
it does not decide who buys this: that is [`../POSITION.md`](../POSITION.md) §4, where the ceiling of a
feature against the ceiling of an oracle is argued. Here there are four signals, one that observes
behaviour, and the limits of the one run that has been made.
