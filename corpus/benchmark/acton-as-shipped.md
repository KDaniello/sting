# One dataset, two analysers

Participant configuration: **acton 1.1.0 as shipped**, read from `out.json`.

Dataset units: 508 (real code 43, injections 368, fixtures 97).
No participant output for 0 units, 0 unparsed.

**93 of 508 units excluded**: the participant could not compile them (`C001` or a parse error). The comparison below is over the remaining 415.

| Class | sting TP/FP/FN | sting precision | sting recall | acton TP/FP/FN | acton precision | acton recall |
|---|---|---|---|---|---|---|
| AC | 10/19/0 | 34.5% | 100.0% | 0/0/10 | n/a | 0.0% |
| BR | 41/0/0 | 100.0% | 100.0% | no rule | — | — |
| EPS | 40/0/0 | 100.0% | 100.0% | 40/1/0 | 97.6% | 100.0% |
| GVR | 14/2/0 | 87.5% | 100.0% | no rule | — | — |
| ID | 7/0/0 | 100.0% | 100.0% | no rule | — | — |
| LEP | 15/19/0 | 44.1% | 100.0% | no rule | — | — |
| PL | 41/0/0 | 100.0% | 100.0% | 41/1/0 | 97.6% | 100.0% |
| SUA | 40/0/0 | 100.0% | 100.0% | no rule | — | — |
| TLB | 16/0/0 | 100.0% | 100.0% | no rule | — | — |
| UBM | 15/5/0 | 75.0% | 100.0% | 0/10/15 | 0.0% | 0.0% |
| UDA | 24/2/0 | 92.3% | 100.0% | no rule | — | — |
| UR | 37/1/0 | 97.4% | 100.0% | no rule | — | — |
| UTA | 37/0/0 | 100.0% | 100.0% | no rule | — | — |

| Over the 4 shared class(es) | TP | FP | FN | Precision | Recall |
|---|---|---|---|---|---|
| sting | 106 | 24 | 0 | 81.5% | 100.0% |
| acton | 81 | 12 | 25 | 87.1% | 76.4% |
