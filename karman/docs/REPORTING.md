# Reporting — what every run must say about itself

A sweep that prints a number has not finished: the number without its context is an opinion.
Every sweep emits a run report, and the report is judged by one standard — **it answers the
reader's questions without its author in the room**. This document fixes what that requires.
The executable half lives in `sting-karman-report`; the composition roots are obliged to produce a
report for every sweep, not offered the option.

## 1. Why this is a contract, not a feature

Reporting is the part of a system most often deferred, and the deferral is invisible until the
first hard question — *how many runs actually completed? what data was excluded? can I
reproduce this number?* — has no answer. By then the runs are gone. So the report is produced
at run time, from the run itself, and a sweep whose report is missing a mandatory section is a
failed sweep, not a terse one.

## 2. The mandatory sections

Every report carries all nine. A section with nothing to say says so explicitly — "no runs
aborted" is information; an absent section is a question nobody can answer later.

1. **Identity.** Scenario name, seed, run and tick counts, and the exact command that
   reproduces the sweep. A report whose numbers cannot be reproduced is an opinion.
2. **Initial conditions.** The state the sweep started from — reserves, population, mechanism —
   so every drift in the report has a visible base.
3. **Coverage.** Planned versus completed versus aborted runs, with the first abort's reason
   verbatim. Losses are named, never netted out: a sweep that quietly lost a third of its runs
   measured a different reality than it reports.
4. **Outcomes.** What the sweep reached, counted rather than distributed, and what it never
   reached at all. A distribution answers how much; only a count answers whether it happened,
   and that second question is one no single run can answer. Below one run in ten it is stated
   as a count and never as a rate: a reader handed "0.03%" carries away an expectation, and a
   reader handed "three runs in ten thousand" carries away three runs.
5. **Distributions.** For every metric: its base where one exists, the median with its drift
   from base, and the percentile corridor (min · p05 · p25 · median · p75 · p95 · max) with
   the corridor's relative width, the spread about the mean and one whole minus that spread
   over the median — by nearest-rank over integer values, exact and platform-independent. Point estimates without corridors are forbidden: a median without its
   spread hides exactly the information a risk tool exists to show. Activity is a metric too:
   applied and rejected actions are distributions, not footnotes.
6. **By regime.** Where the sweep was a mixture of futures, every one of them summarised on its
   own — its corridor, its spread and its stability — because a mixture collapsed into a single
   distribution reports the average of futures that never happen together. A part with too few
   runs for its corridor's lower edge to be anything but its minimum states its middle and no
   corridor, rather than printing extremes under percentile names.
7. **Extreme runs.** For each headline metric, the run index of the lowest and highest final
   value. The run index is the RNG stream, so each extreme is replayable exactly — the report
   hands the reader the worst case, not just its existence.
8. **Verdict.** The claim this sweep was graded against, quoted with its registration date, its
   basis and its zones, beside the measurement that decided it. A sweep with nothing registered
   against it says exactly that, because "no verdict" is a finding and a silent section is not.
9. **Gaps — what this run cannot answer.** Unwired layers, uncalibrated populations,
   uncounted phenomena, excluded data — each stated with its consequence. The composition root
   fills this section, because it alone knows what it wired and what it left out. An empty
   gaps list is a claim of completeness, and it is almost always false.

## 3. Rules

- **The report is an artifact.** Written to disk with the crash-safe discipline of
  `sting-karman-data`, in Markdown for the reader and available as JSON for machines. The console
  summary names the report path.
- **Verdicts only against preregistered thresholds.** A report may state a verdict
  ("confirmed", "refuted", "no verdict") only when the thresholds were fixed before the run —
  the discipline of `findings/` (docs/VERIFICATION.md §4). A threshold invented after seeing
  the distribution is not a verdict, whatever it is called.
- **Conservatism is stated, not implied.** Where a reported number carries a known bias, the
  report names the direction (docs/VERIFICATION.md §5).
- **Gaps grow before they shrink.** Closing a gap means shipping the layer that closes it;
  deleting a gap line without shipping anything is falsification of the record.

## 4. Where reporting lives

| Piece | Home | Responsibility |
|---|---|---|
| `RunReport`, `MetricSummary`, `Coverage`, `Identity` | `sting-karman-report` | the structure and its Markdown rendering; pure values, no I/O |
| writing the artifact | `sting-karman-data::write_text_artifact` | crash-safe file output |
| filling the gaps section | composition roots (`sting-karman-cli`, SDK) | naming what was wired and what was not |
| verdict thresholds | `findings/` | preregistered, immutable once registered; JSON, so the report grades itself against them rather than a reader comparing by eye |
| grading a sweep against a claim | `sting-karman-report::grade` | refuses a sweep the claim was not registered against; quotes the zones into the report beside the outcome |
