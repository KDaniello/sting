# The second language: reading FunC

The detectors read Tolk. The hunt measured what that costs: of eighteen entries in the published bug
bounty scopes of the TON ecosystem, one carried code the static contour could read — 49 files of
2122. The chain does not run on Tolk yet.

**An analyser is not narrow because it has few rules — it is narrow because it reads one of the two
languages its subject is written in.** This is the second one.

## 1. What moved

| | Before | After |
|---|---|---|
| scope entries the contour reads | 1 of 18 | **12 of 18** |
| files it reads | 49 | **170** |
| `FunC` files parsing with no `ERROR` node | 189 of 199 | **199 of 199** |
| detectors that read both languages | 0 | 1 |

Twelve times the entries and three and a half times the files, from one language rather than from
fourteen more rules. No fifteenth detector was worth a tenth of that, and the arithmetic said so
before any of this was written.

## 2. The pattern crosses, the vocabulary does not

A detector is a pattern plus a vocabulary. `LEP` asks whether a `slice` parameter is read and its end
never asserted; that question is about what a program does and it survives the change of language.
What does not survive is every word in it — the call that asserts a slice is spent, the field holding
a taken branch, the way the language says a call consumed something.

So the words live in one value, `Dialect`, and the implementation does not fork. **A detector that
forks into two implementations agrees with itself only by accident**, and the day one of them is
fixed is the day they stop agreeing at all.

`DEPR` does not cross and will not: whether a name is deprecated is a fact about one compiler.

### 2.1. Where FunC turns out to be the easier language

Tolk decides whether a call moves a slice's read cursor in the callee's declaration — `mutate self` —
which a call site cannot see. So the answer had to be transcribed from the standard library: every
`fun slice.*` declared without it, listed by hand, checked against a commit.

FunC decides at the call site, in one character. `s~load_uint(32)` writes the result back into `s`;
`s.preload_uint(32)` does not. The grammar already knows, and the list is not needed.

The same asymmetry runs the other way for the cooperating sub-parser. Tolk declares it — `mutate s:
slice` — and the detector skips it. FunC has no such keyword: it *returns* the slice instead, and
`(slice, (int, int)) ~load_op_and_query_id(slice s)` is the shape the standard library beside every
wallet in the ecosystem is written in. Reading that took a rule rather than a keyword.

## 3. What the grammar could not read

The vendored `FunC` grammar read 189 of 199 files in the scope. Three edits took it to 199, each
with a file behind it rather than a guess.

| Change | The construct | Files |
|---|---|---|
| `catch` accepts `underscore` beside `identifier` | `try { … } catch (_, _) { … }` | 4 |
| `!` added to `identifier` | `storage.upgrade_storage:get!("meta"H)` | 5 |
| `!` added to `function_name` | `slice upgrade_storage:get!(cell storage, int h)` | 1 |

The first is not a missing rule but a rule naming the wrong node. `try_statement` exists upstream and
its catch clause accepts `commaSep($.identifier)`; a bare `_` is not an identifier there, because
that rule needs a second character once it starts with an underscore, and `_` has a node of its own.
Every contract in the sample catches into `_` at least once, so every one of them lost a function to
an `ERROR` node.

The other two are one change stated twice: a declaration and a call site have to agree on what a name
may contain, or a function the grammar reads cannot be defined in it.

One thing had to be vendored before any of that: `grammar/`, which `grammar.js` requires and which
the original vendoring did not copy. **A fork nobody can rebuild is a binary with a story attached.**

## 4. Three tree shapes that decided the design

A grammar-neutral surface that renamed node kinds would have answered none of these, which is why it
answers what a construct *is* instead. Each was read off a real parse rather than assumed —
`crates/sting-abi-func/examples/func_shapes.rs` prints the trees.

- **There is no assignment node.** `a = 2;` is an `expression_statement` holding two named children
  and an unnamed `=`.
- **A method call does not contain its receiver.** The receiver is the immediately preceding named
  sibling, which is also what makes a chain work: in `begin_cell().store_uint(…).end_cell()`, the
  receiver of the last call is the one before it.
- **The `alternative` field is on the `else` keyword too.** `child_by_field_name` answers with the
  keyword, so a branch has to be taken by kind. Reading the keyword instead would have concluded
  that a slice ended on one path only — silently, and in the safe-looking direction.

## 5. What the live code taught, and what the review caught after it

`LEP` fired 96 times on the FunC half of the scope. It fires 39 times now. None of the fixes was
anticipated: two came from reading code nobody here wrote, and three more from the review that
followed.

**Forwarding did not work at all.** The check looked for Tolk's `call_argument` node, which the FunC
grammar does not have, so a slice handed to a helper was never recognised as handed on — and
`elector-code.fc` and `config-code.fc` route their whole message body that way, opcode by opcode.
54 of the 96.

**A sub-parser was being asked to end a parse its caller continues** — the returned-slice shape of
§2.1. 10 more.

The second fix was wrong before it was right, and the measurement is what caught it. Its first
version also silenced `extractBouncedBody`, which returns a number read *out of* the slice rather
than the slice itself. The tell was Tolk's precision moving from 34.1% to 35.7% when it had no
business moving at all. **A receiver inside a return is not a returned value**, and both shapes are
tests now.

Three more came from the review rather than from the run, and one of them moved the number that had
already been published. A receiver sits inside the argument group of whatever encloses it, because
`FunC` splices a call and its receiver into the surrounding expression — so `throw_unless(701,
body~load_uint(32) == 5)`, the most common opcode-dispatch line the language has, was read as
handing the slice to `throw_unless`. Seven findings were being suppressed that way. The first
published figure for this round was 32 of 32, measured with that suppression in place; it is 39 of
39 now, and the earlier number is restated rather than kept.

The other two were silent in the other direction. An `elseif` chain is one flattened node carrying
several `consequent` fields — the grammar declares it `multiple` — and reading only the first walked
past the middle branch and called it covered. And a plain reassignment has no node of its own in
`FunC`, so the rule that breaks an alias matched nothing at all, and an `end_parse` on a re-pointed
name read as closing the parameter. Both are tests now, and both would have been false negatives:
the direction that looks like a clean file.


## 6. The numbers, apart rather than averaged

A detector that reads two languages does not read them equally well, and one row averaging them
describes neither. So the benchmark keeps a tally per language and prints them side by side.

| Set | Precision | Over what |
|---|---|---|
| fixtures, Tolk | 34.1% | 15 true of 44 |
| fixtures, `FunC` | 100.0% | 2 of 2 |
| live `FunC`, before the fixes | 40.6% | 39 of 96 |
| live `FunC` | **100.0%** | 39 of 39 |

**The last row is the kind of number to distrust on sight, and here is what it is worth.** It says
every one of the detector's 39 claims about live code is true of that code — the call is absent on
some path — and it says nothing else. `LEP` reports a fact and not a defect: an unchecked tail is not
a class `docs.ton.org` names, which is why the detector is `Info` and outside the default set. Two
fixtures written here cannot carry a claim either; the fixture row is a smoke test and the live row
is the measurement.

Tolk did not move: `LEP` 15/29/0 at 34.1%, aggregate 82.4%, default set 96.4%, the same figures as
before a line of this was written.

## 7. Saying which detectors did not look

Thirteen detectors have no rule for `FunC`. They are not run against it, and the run hands back their
ids rather than their silence.

**"This detector has no rule here" and "this detector found nothing" are different claims**, and a
report that renders them alike credits the analyser for code no rule of its own ever examined. That
is the same distinction the reach table draws between a language the contour cannot read and a file
it read and found clean, one level down.

## 8. What this does not do

It does not read Tact, and it does not port every detector — `LEP` is one of fourteen, and the other
thirteen say so in their own row rather than by keeping quiet.

It does not claim that reading a language is understanding it. The 39 surviving findings are facts
about parse cursors, not defects; nothing was reported to any programme and nothing here says a
target is safe.

And it does not make the fetch cheap. The run clones 45 MB to read 170 files, because a scope naming
four files inside a repository of 2702 still costs the repository. Fetching only the named paths was
tried and measured — the cache falls to 1.4 MB — and abandoned: the deferred fetch a partial clone
relies on does not complete against this remote, and **a cheap fetch that sometimes yields an empty
checkout is worse than an expensive one**, because from the outside the run cannot tell the two
apart.
