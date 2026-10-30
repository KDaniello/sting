# findings

A claim exists when its number lands here together with the method that produced it
([`docs/VERIFICATION.md`](../docs/VERIFICATION.md) §4). One file per claim, dated, immutable
once registered: thresholds and verdict zones are written before the run, and never touched
after.

A file here is JSON, because a report grades itself against it rather than a reader doing the
comparison by eye. Nothing in the code can prove a file was written before a run — that is what
this directory's history is for — so what the code does instead is refuse to grade a sweep the
claim was not registered against, and quote the registered zones into the report beside the
verdict, where the two sit side by side and nobody has to be believed.

## The file

| Field | What it holds |
|---|---|
| `name` | how the verdict is referred to afterwards |
| `registered` | the date the claim entered this directory |
| `statement` | what is being claimed, in a sentence a reader can disagree with |
| `basis` | where the thresholds came from. A zone with no basis is a number someone liked |
| `applies_to` | the scenario, seed, run count and tick count this grades — and no other sweep |
| `zones` | the graded bands, each naming a metric, a statistic, its edges, and whether landing there confirms or refutes |

A zone with neither edge holds every measurement and is refused at load; so is a zone whose
edges cross, and a claim with no zones at all. The first zone whose measurement lands decides
the verdict, so the order they are written in is part of the registration.

## Grading a sweep

```bash
karman run --scenario testdata/scenarios/cpamm-smoke.json --runs 1000 --seed 42 \
  --claim findings/2026-08-19-cpamm-smoke-symmetric-flow-leaves-no-drift.json
```

Without `--claim` the report states no verdict and says so. That is the honest outcome for a
sweep nobody registered anything against, and it is the usual one.
