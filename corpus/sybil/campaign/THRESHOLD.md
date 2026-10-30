# The threshold, registered before the campaign

What counts as a pass, written down before anything is measured against it. A threshold named after
a run is not a threshold: it is a description of the result, and every result passes one of those.

## What is being measured

A retrospective campaign against an airdrop whose outcome is known: addresses the project itself
excluded as coordinated, against the addresses this platform's clustering flags at the score
threshold the model states. The comparison is one confusion matrix over the owners both sides name.

| Quantity | What it means here |
|---|---|
| positive | the campaign's own exclusion list names this owner |
| flagged | this platform's risk score is at or above the model's threshold |
| precision | of the owners flagged, the share the campaign also excluded |
| recall | of the owners the campaign excluded, the share flagged |

## The numbers

| Check | Threshold |
|---|---|
| precision | at or above **0.90** |
| recall | at or above **0.50** |
| owners in the comparison | at least **1 000** on each side |
| score threshold applied | the model file's own, `0.7692573575930904` for `fixture.v1` |

**Precision is set higher than recall on purpose.** A false positive here is an accusation against a
person who did nothing, and the cost of one is not the mirror image of the cost of a miss. A detector
that catches half of a farm and never names an innocent is worth more to whoever runs an airdrop than
one that catches nine tenths and names one honest participant in ten.

**Both numbers are published whichever side of these they land on**, together with the commit that
this file was registered in — which is what makes the registration mean anything.

## What this threshold does not claim

It does not claim the campaign's own exclusion list is correct. It is a label, produced by somebody
else's method, and the comparison measures agreement with it rather than truth: an owner this
platform flags and the campaign did not may be a farm the campaign missed. That is why the result is
reported as precision and recall against a named list, not as accuracy.

It says nothing about ownership. A cluster is coordination, and coordination is not a statement that
one person holds every address in it — [`ROADMAP.md`](../../../docs/ROADMAP.md) §11 puts that
outside this milestone deliberately.
