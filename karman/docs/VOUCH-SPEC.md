# The guarantee layer — specification of the second layer

The second layer of the dead-capital work as a state machine: state, transitions, rounding,
invariants, and the world it runs in. Written from
[`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §7.3 and §11 У8, and implemented by `sting-karman-mech-vouch`.
Where this document and the crate disagree, the crate is the bug.

The rules of the workspace apply here unchanged:
[`ARCHITECTURE.md`](./ARCHITECTURE.md) §4 for the port, §5 for the invariants, §8 for what will
get a change rejected; [`ARITHMETIC.md`](./ARITHMETIC.md) for widths and rounding.

---

## 1. What the mechanism is

Credit that stands on somebody else's stake.

One participant **writes a guarantee** for another and locks money behind it. The one guaranteed
may then **draw** credit against every guarantee standing for them — more than the stake behind
it, which is the whole point — and pays a **premium** for it. **Repaying** on time buys the
borrower a lower collateral requirement next time. **Failing to repay** takes the stake behind the
guarantee and hands it to the fund the credit came from.

Nothing here prices an asset and nothing here is secured by one. What stands behind a loan is a
person who was willing to lose money over somebody else, and the question the layer exists to
answer is how much credit that is worth.

## 2. What it is not, and what therefore cannot be concluded from it

- **There is no path.** Credit comes out of one common fund, not along a chain of personal lines
  between people who trust each other. This is a departure from Fugger (§4.8 of the manifest) and
  it is the single modelling choice that most decides what a run measures — see §8.
- **A defaulter keeps what it drew.** Nothing pursues an unspent balance. What a venue able to
  claw one back would recover is outside what is wired.
- **The graph is fixed.** Nobody withdraws a guarantee on suspicion and nobody declines to stand
  behind somebody who already looks overextended.
- **A seat is free.** One identity costs nothing to open beyond the money it must stake. What an
  identity check or a joining fee would do to the arithmetic of a ring is not measured.
- **Nothing is fitted.** No market of this shape has been measured, so there is nothing to fit to.
  Every parameter is a term a venue could post, and a result is a result about a market run on
  those terms.

## 3. Units and widths

| Unit | What it counts |
|---|---|
| `Cash` | the one asset: what is staked, drawn, repaid and seized |
| `Bps` | a share, in ten-thousandths; `WHOLE` is 10 000 |
| `Seconds` | elapsed time, the denominator a premium is quoted against |

`SECONDS_PER_YEAR` is 365 days exactly, whatever the calendar does: a rate needs a denominator
that never moves, or two runs of one scenario disagree about what a year was.

## 4. State

### 4.1 `Member`

`balance` free · `staked` locked behind guarantees it wrote · `owed` principal drawn and not
repaid · `settled` loans repaid on time · `failed` loans that fell due unpaid · `earned`,
`premium_paid`, `seized` as the record of what the layer did to it.

`net_worth()` is `balance + staked − owed`, floored at nothing. A participant that owes more than
it holds is worth nothing to stand behind anybody, and there is no weaker statement to make.

### 4.2 `Line`

One written guarantee: `by`, `of`, `stake`, `committed` (how much of that stake stands behind
loans still open), `written` (what a seasoning period counts from), `releasing` (when a withdrawal
binds), `live`.

### 4.3 `Loan`

`to`, `principal`, `due`, `backing` (which guarantees stand behind it and how much of each, in the
order they were taken), `open`.

### 4.4 What is deliberately absent

- **No interest accrual.** The premium is charged once, up front, for the whole term. A loan that
  accrued would put a second rate between the floor and the result the floor exists to produce.
- **No partial repayment.** A loan is settled or it is not.
- **No reputation beyond `settled`.** Standing is a count of settlements and nothing else, because
  anything richer would be a scoring rule to defend on its own.

## 5. Parameters

| Parameter | What it says |
|---|---|
| `venue` | who takes the venue's share of every premium; it never acts |
| `seconds_per_tick` | what turns an annual premium into one |
| `opening` | what share of a draw somebody with no history must have standing behind it |
| `step` | what each settled loan takes off that share |
| **`floor`** | **the least that share may ever fall to — the parameter the design turns on** |
| `backing` | `Stake` or `NetWorth`; what a guarantee counts for (§6.2) |
| `premium` | the annual premium on drawn principal |
| `venue_share` | what share of a premium the venue keeps; the rest reaches the guarantors |
| `seasoning` | how long a guarantee must stand before it backs anything |
| `release_lag` | how long a withdrawal waits before it binds |
| `execution` | what one action costs whoever takes it; it leaves the system |

`floor` is the most leverage the system will ever extend against a person rather than against
money. It is therefore also the prize an attacker is paying premiums to reach, and the whole of
§8 is about what happens at each value of it.

## 6. Transitions

### 6.1 The collateral schedule

`collateral(settled) = max(floor, opening − step · settled)`.

A step subtracted, not a curve: what is modelled is a schedule a venue could publish — settle one,
and the next asks for this much less. A curve would put a second fitted thing between the floor
and the result.

### 6.2 What a guarantee is worth

Under `Backing::Stake`, its stake. Under `Backing::NetWorth`, its stake scaled down whenever its
writer has staked more than it is worth net of its own debts, so that what a guarantor stands
behind can never exceed what it would actually lose:

```
worth = stake · min(1, net_worth / staked)
```

All of one writer's guarantees are scaled together rather than exhausted in order, which keeps the
answer independent of which guarantee is asked about, and therefore independent of the order draws
happen to arrive in.

A guarantee counts for nothing at all when it is not live, when a withdrawal of it has been asked
for, when it has not yet seasoned, or when its writer has itself failed to pay.

### 6.3 `Write { of, stake }`

Locks `stake`. Refused for oneself, for a participant that does not exist, for nothing, and from
anybody who has already failed to pay. Balance must cover the stake and the cost of acting.

### 6.4 `Release { line }`

Schedules a withdrawal for `now + release_lag`, and only the writer may ask. **A withdrawal always
waits.** Without the lag a guarantor could watch a borrower approach a due date it will not meet
and step out of the way, which is the same as there having been no guarantee at all. Only stake
that nothing stands on comes back, however long ago it was asked for.

### 6.5 `Draw { amount, term }`

1. `need = amount · collateral(settled) / WHOLE`, **rounded down** — less stake locked per unit
   drawn is more credit extended per unit staked, which is the direction that does not flatter the
   mechanism.
2. Guarantees are taken in the order they were written until `need` is covered. Short of it, the
   draw is refused with nothing moved.
3. `premium = amount · premium / WHOLE · term_seconds / SECONDS_PER_YEAR`, **rounded down**,
   against the venue.
4. The principal leaves the fund and reaches the borrower; the premium comes back out of it. A
   mechanism that asked for the premium beforehand would extend credit only to those who already
   had money, which is the condition this layer exists to relax.
5. The premium is split at `venue_share`, and the guarantors' part is shared in proportion to what
   each stands behind. **What the division cannot place goes to the venue rather than being
   invented into existence.**

### 6.6 `Repay { loan }`

The principal goes back to the fund, the backing is released, and `settled` rises by one. Only the
borrower repays, and only a loan that is still open.

### 6.7 `tick`

**Defaults before releases**, and the order is a rule rather than an accident: a withdrawal
maturing on the tick a loan it stands behind falls due must not be the way out of paying for it.

A loan that falls due unpaid: every guarantee behind it gives up what it had committed, the stake
reaches the fund, the borrower's debt is cleared, its `failed` rises by one, and whatever the
stakes did not cover is added to `written_off`. **The borrower keeps the principal** — see §2.

## 7. Invariants

**Per action.** Cash is conserved: what the members hold between them, free and locked, plus the
fund, plus what execution consumed, is what the world opened with. Debt is not cash and does not
appear — a draw moves money out of the fund and a repayment moves it back, and a default moves
stake rather than principal.

**Per tick** (`audit`, which costs a walk of the state):

- no guarantee stands behind more than is locked in it;
- what every member says it has locked is what the guarantees it wrote hold;
- what every member owes is the principal of its open loans and nothing else;
- the running total of what members hold is what they hold.

## 8. The world, and what it measured

### 8.1 The population

A thousand participants and a venue. **Nine hundred honest**, ninety of those bad borrowers that
stop paying at a point in their history given by their seat number — so that failures fall at
every stage of it rather than all at one, which would decide the answer by itself. **A hundred
confederates** whose circle is the ring and nobody else.

Every seat opens with the same money, posts the same stake, pays the same premium, and is wired
into a circle of the same size and shape — a stride around its own population. **Nothing separates
the ring from the honest but that it is closed and that it stops repaying at the floor.** The
mechanism is told none of it and cannot be: a hundred confederates look exactly like a hundred
ordinary participants who happen to know each other.

The ring stops at exactly the settlement that buys the floor: earlier it attacks before it has
bought anything, later it pays for standing it will never spend.

### 8.2 What a sweep of the floor found

Registered before the run as
[`findings/2026-08-20-vouch-a-ring-loses-only-where-credit-buys-nothing.json`](../findings/2026-08-20-vouch-a-ring-loses-only-where-credit-buys-nothing.json),
graded against `vouch-ring` at seed 7, 100 runs of 1460 ticks. **Verdict: refuted.**

| floor | leverage | ring's net result |
|---:|---:|---:|
| 10 000 | 1.00× | **−16 bps** |
| 9 000 | 1.11× | +850 bps |
| 8 000 | 1.25× | +1 917 bps |
| 7 000 | 1.43× | +3 285 bps |
| 6 000 | 1.67× | +5 076 bps |
| 5 000 | 2.00× | +7 676 bps |
| 4 000 | 2.50× | +11 487 bps |
| 3 333 | 3.00× | +15 171 bps |
| 2 500 | 4.00× | +22 684 bps |
| 2 000 | 5.00× | +30 345 bps |

The curve is monotone and has exactly one crossing, and it sits where the arithmetic puts it: a
ring that both posts the stake and takes the loan gains `leverage − 1` times what it staked, minus
what the premiums cost on the way down. **The only collateral a ring cannot beat is the collateral
at which credit buys nothing money would not have bought.**

Neither of the other two levers moves it:

- **Premium.** Raising it from 2 000 to 20 000 bps a year does turn the ring's net negative — by
  shrinking the drawn principal of the whole market by 83% and stopping honest borrowers from
  reaching the floor as well. It defeats the ring by closing the market.
- **The venue's share.** Across its whole range, from nothing to all of it, the ring's net moves
  from +15 440 to +14 543 bps. The guarantors' share of a premium a ring pays never leaves the
  ring, so only the venue's share costs an attacker anything, and it is not enough to matter.

`Backing::Stake` and `Backing::NetWorth` produce identical results here, to the basis point.
Netting a guarantee against its writer's debts bites only when free money falls below what is
owed, and nothing in this crate lets a borrower part with a principal — so the rule is pinned by a
test rather than exercised by the population. That is a gap and not a result.

### 8.3 What this does and does not settle

It settles that **a collateral floor is not an answer to a closed ring drawing on a common fund.**
The leverage the floor extends is the attacker's profit multiple, exactly and by construction, and
no premium reachable without closing the market changes it.

It does not settle the layer. The common fund of §2 is what makes the arithmetic work out this
way: a ring drains a pot that everybody supplied. A design in which credit travels along personal
lines, where an obligation is only ever accepted by somebody who chose to accept it, bounds a
ring's take by what its own members extended to it — and that is a different mechanism, not a
parameter of this one.

## 9. Conformance and the ladder

Determinism and invariant preservation via `sting_karman_core::conformance`, exercised by a population
that writes, draws and repays. The rules of §6 are pinned one at a time in
`crates/sting-karman-mech-vouch/tests/rules.rs`.

Level 0 of the ladder is unreachable rather than unattempted: there is no deployed reference to
difference this against. Level 1 likewise — no episode of it has ever been recorded.
