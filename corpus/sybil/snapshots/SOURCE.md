# The snapshot, and what it is a snapshot of

`mc-87210000-87210003.json` is what this tree's ingestion produced from masterchain blocks
87 210 000 to 87 210 003 of TON mainnet, read over toncenter's v3 REST interface on 2026-08-23:
**46 transactions and 60 messages**, carrying 19 distinct opcodes, in 50 543 bytes.

The range is fixed and finalized, which is the whole point of committing the file. A finalized block
is a fact that does not move, so a replay of the same range has to produce the same rows — and when
it does not, either the chain answered differently or this ingestion is not deterministic. Both are
things whoever depends on it needs told, and neither is visible from a count.

Rebuilt and checked by one command:

```sh
sting-sybil-ingestion snapshot --from 87210000 --to 87210003 --live \
  --verify corpus/sybil/snapshots/mc-87210000-87210003.json
```

It was run three times on the day it was written: once to produce the file and twice against it,
byte for byte.

## What it is not

It is not a fixture for the acceptance tests. Those run against synthetic blocks and a real column
store, and they check what ingestion does with rows rather than which rows the chain had. This file
answers one question the tests cannot: whether reading the same range twice gives the same answer.

It is not a sample of mainnet. Four blocks of one minute of one day are four blocks; nothing here
supports a statement about what the chain carries in general.

It is not the column store's own state. What is written down is what the ingestion produced, read
back out of the in-memory store, so a store that transformed a row on the way in would not be caught
by it.
