# The showcase — specification of the first layer

The mechanism [`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §7.2 states in prose, written as the
deterministic state machine this workspace can run. It is the executable half of the manifesto's
§12.5–12.6, and it is a specification rather than a description: where the manifesto leaves a rule
open, this document closes it and says why.

Nothing below concerns credit or a unit of account. Those are the second and third layers, and the
order is not a preference — a currency without a drain is [`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md)
§4.7, and it died.

---

## 1. What the mechanism is

A venue where an asset that exists in one copy is held under two joined obligations:

- its holder names a price, and
- anyone may take it at that price, without the holder's consent,

and where holding under those obligations is charged for, continuously, as a share of the named
price. Naming a low price is punished by losing the asset cheaply; naming a high one is punished by
the charge. Between the two lies the truth, which is the whole of
[`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §5.2–5.3.

Entry is voluntary and exit is possible. An asset outside the showcase is charged nothing and
cannot be taken; it is also invisible, uncomparable and unbuyable, which is the whole of what the
showcase sells.

## 2. What it is not, and what therefore cannot be concluded from it

- **It does not know what anything is worth.** Value is a property of the agents acting on it
  (§8), never of the state. This is not a simplification: an item carrying its own worth would let
  an agent read the answer the simulation exists to measure, and every result would be circular.
- **It does not model message delivery.** A chain that settles asynchronously can deliver a
  reprice and a buyout in either order; the lag of §6.3 is the mechanism's defence against that,
  and whether the defence holds is decided by enumerating interleavings against a deployed
  contract ([`DEAD-CAPITAL-ROADMAP.md`](./DEAD-CAPITAL-ROADMAP.md) gate Г4), not here.
- **It does not model credit, reputation or a unit of account.** Every participant pays from a
  balance it already has.
- **It does not price the charge.** The rate is an input. Finding the rate at which the market
  starts telling rare from ordinary is the entire experiment (gate Г1).
- **It pays nothing for being on the showcase.** What the showcase gives is exactly one thing, and
  the mechanism already gives it: **an asset outside it cannot be bought.** A holder lists because
  listing is the only way a buyer can reach them, and withdraws when the charge outgrows being
  reachable. [`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §12.9 asks for a separate configurable benefit
  instead; there is none here on purpose. A benefit that is dialled rather than derived would let
  the rate at which holders start leaving — claim У9, the whole upper bound of the mechanism — be
  set by whoever chose the number. Here it is a property of the population and has to be measured.

## 3. Units and widths

Integer base units throughout, checked, with a stated rounding direction at every lossy operation
— [`ARITHMETIC.md`](./ARITHMETIC.md). No float reaches a transition.

| Unit | What it counts | Width |
|---|---|---|
| `Cash` | the settlement asset, in its own base units | 128-bit |
| `Bps` | a share, in ten-thousandths | 32-bit |
| `Seconds` | elapsed time, from the scenario's seconds per tick | 64-bit |

There is no fixed-point scale in the mechanism. A share is basis points because every rate this
family speaks — the charge, the commission, a distribution split — is quoted that way in the
sources it is calibrated against, and a scale of `1e18` over a market whose median item is worth a
few units of `Cash` would be precision spent on nothing.

**One asset, one unit.** The mechanism does not hold two kinds of money. What an item is bought
with is what the charge is paid in and what the pool distributes.

**A year is 31 536 000 seconds** — 365 days, exactly, always. Not because the calendar says so but
because a rate needs a denominator that never moves: a leap year that lengthened the divisor would
make two runs of the same scenario disagree, and the reference this workspace already models
divides its own annual rates the same way.

## 4. State

```
Showcase
├─ items:        Item      indexed by ItemId, in index order
├─ participants: Purse     indexed by AgentId, in index order
├─ venue:        AgentId            one of them, and the only one that never acts
├─ pool:         Amount<Cash>       what the charge has collected and not yet shared out
├─ spent:        Amount<Cash>       what execution consumed, which leaves the system for good
└─ distributed_through: Tick        the last period boundary the pool was shared out at
```

**The venue is a participant.** It takes the commission and the part of the pool that is not
shared out, and a forced sale (§6.5) hands it an item and takes the price out of its purse. Giving
it a purse rather than a hole in the accounts is what lets §9.1 be checked at all — and it makes
the one interesting failure visible: a venue that has spent what it took cannot afford its own
forced sale, and the mechanism has to say what happens then rather than assume it never happens.

`spent` therefore holds one thing: what execution cost. That money is the chain's, nobody's here,
and it does not come back.

### 4.1 `Item`

| Field | Type | Meaning |
|---|---|---|
| `collection` | `CollectionId` | which family it belongs to; the charge is a property of a collection, never of the system ([`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §7.2) |
| `holder` | `AgentId` | who holds it. Every item has exactly one holder at every tick |
| `listing` | `Option<Listing>` | present exactly when the item is on the showcase |

### 4.2 `Listing`

| Field | Type | Meaning |
|---|---|---|
| `declared` | `Amount<Cash>` | the price that binds: what anyone may take the item at |
| `assessed_to` | `Tick` | the moment the charge has been settled up to |
| `pending` | `Option<Pending>` | a raise or a withdrawal that has been asked for and has not taken effect |

`Pending` is `{ price: Option<Amount<Cash>>, effective: Tick }`. A `None` price is a withdrawal
from the showcase; a `Some` is a raise.

### 4.3 `Purse`

| Field | Type | Meaning |
|---|---|---|
| `balance` | `Amount<Cash>` | what the participant can spend |
| `traded` | `u32` | how many buyouts it has been a party to inside the current period |
| `paid` | `Amount<Cash>` | what it has paid in charge inside the current period |

`traded` and `paid` exist because the distribution rule is one of the things the experiment varies
(§7), and two of the three candidate rules are functions of them. They are reset at each
distribution and are therefore state, not statistics.

### 4.4 What is deliberately absent

- **No true worth.** See §2.
- **No order book, no offers, no auction.** The declared price is the only quote, and a buyout is
  the only trade. An offer mechanism would give a holder a way to discover demand without
  committing to a price, which is precisely the freedom the mechanism exists to remove.
- **No per-item timer.** The charge is assessed when an item is touched and at no other moment
  (§6.1), so the state carries no schedule and a tick costs nothing per item.

## 5. Parameters

Fixed for a run, and named in the scenario. Every one of them is a knob a sweep may turn.

| Parameter | Type | Range swept | What it decides |
|---|---|---|---|
| `charge` | `Bps` per year, per collection | 0 … 5000 | the rate. The curve over it is the experiment |
| `lag` | ticks | 0 … 8 | how long a raise or a withdrawal waits before it binds (§6.3) |
| `commission` | `Bps` | 0 … 1000 | what the venue takes from a buyout |
| `execution` | `Amount<Cash>` | 0 … | what one action costs the participant that takes it, and leaves the system |
| `arrears` | `Forfeit` \| `ForcedSale` | both | what happens when the charge cannot be paid (§6.5) |
| `distribution` | `Even` \| `ByTrades` \| `ByCharge` | all three | how the pool is shared out (§6.7) |
| `share` | `Bps` | 0 … 10000 | how much of the pool is shared out; the rest is the venue's |
| `period` | ticks | — | how often the pool is shared out |

`charge` is per collection because the measured turnover it is calibrated against differs between
collections by a factor of nine ([`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §6.1), and a rate
that is right for one is wrong for the other by construction — this is claim У4.

## 6. Transitions

Five actions and one time-driven rule. The order rules are applied in is part of this
specification: a mechanism whose outcome depends on the order two of its own rules ran in is a
mechanism with two answers.

Every transition resolves against copies and commits once. A rejection returns `Err` and leaves the
state exactly as it found it, including the pool and the event buffer.

### 6.1 The charge, and why it is lazy

The charge owed on a listing is a share of the declared price over elapsed time:

```
elapsed_seconds = (now − assessed_to) · seconds_per_tick
a_year          = declared · charge / 10 000                    rounded down
owed            = a_year · elapsed_seconds / SECONDS_PER_YEAR   rounded down
```

Two steps rather than one, and the reason is arithmetic rather than taste. In one step the product
is a 128-bit price by a 32-bit share by a 64-bit span — up to 2²²⁴, which no width in this
workspace holds. Each step here is one `mul_div` of the kind [`ARITHMETIC.md`](./ARITHMETIC.md) §2
describes: 128-bit operands, a 256-bit intermediate that provably cannot overflow, a 128-bit result
or an error. Two roundings down instead of one make the figure slightly smaller again, which is the
side §6.1 wants to be wrong on.

It is computed **when the item is touched** — listed, repriced, bought, withdrawn, settled — and at
no other moment. Nothing walks the items on a tick.

This is not an optimisation, it is the mechanism. A venue that walked its items would send one
message per item per period, and on the measured market that is about a hundred thousand messages a
quarter against a state rent of less than a thousandth of a unit per item per year
([`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §12.14) — the cost of collecting the charge would exceed
the charge on everything cheap, and the mechanism would apply only to the top of the market. Lazy
assessment moves the floor of applicability down by more than a factor of four, and a simulation
that assessed eagerly would be measuring a mechanism nobody can deploy.

Two consequences follow, and both are part of the mechanism rather than of its implementation:

- What a seller receives at a buyout is net of what the item owes.
- An item can owe more than anyone intended, because nothing forced the holder to pay. §6.5 bounds
  it.

**Rounding down**, and this is a deliberate inversion of the usual direction. Everywhere this
workspace models an existing venue, rounding favours the venue, because that is what the reference
does and level 0 demands agreement with it ([`VERIFICATION.md`](./VERIFICATION.md) §5). Here there
is no reference: the venue is the design under test. Rounding the charge up would let the mechanism
collect dust it may not collect in practice, and every result would be flattered by exactly the
amount the design most needs to be doubted about — the cheap end of the market, where the charge is
small enough for a rounding direction to be most of it. Rounding down makes every measured effect a
lower bound, and makes the price below which the charge is nothing **visible in the results instead
of hidden in them**. The same rule fixes every other direction in this document: `commission` and
`execution` round **up**, against the participant, because they are frictions and overstating a
friction understates the mechanism.

### 6.2 `List { item, price }`

Puts an item on the showcase at a price.

Rejected when: the actor does not hold the item · the item is already listed · `price` is zero ·
the actor cannot pay `execution`.

A price of zero is refused rather than accepted, because it is a declaration that the item may be
taken for nothing, and it is indistinguishable in the state from an item nobody has priced. A
holder who wants to give an item away has an action for it that this mechanism does not need to
provide.

Effect: `execution` leaves the actor and joins `spent`; the listing opens with `assessed_to = now`
and no pending change.

### 6.3 `Reprice { item, price }`

Names a different price.

Rejected when: the actor does not hold the item · the item is not listed · `price` is zero · the
actor cannot pay `execution` or the charge owed (§6.5).

Effect: the charge owed is settled first. Then:

- **A lower price binds at once.**
- **A higher price binds after `lag` ticks**, held in `pending` until then. The old price binds
  meanwhile.

**Why the asymmetry.** The lag exists for exactly one attack: a holder who sees a buyout coming
raises the price and defeats a trade the mechanism promised was available. On a chain that does not
order messages from different senders, that race is winnable, so the promise has to be defended by
the mechanism rather than by the ordering. A *lower* price cannot defeat a buyout — it can only
make one easier — so delaying it buys nothing and costs price discovery in the one direction a
market of dead assets most needs to move. This is narrower than
[`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §12.6, which lags every reprice; the difference is
deliberate, and the minimum workable `lag` is a gate Г4 result rather than a number chosen here.

A second pending change replaces the first, and its clock starts again. Otherwise a holder could
queue a raise, and on seeing a buyout queue a second one that lands sooner.

### 6.4 `Withdraw { item }`

Takes an item off the showcase. It stops being chargeable and stops being buyable.

Rejected for the reasons of §6.3. Takes effect after `lag` ticks, held in `pending` with no price,
for the reason a raise does: an instant withdrawal defeats a buyout exactly as well as an instant
raise. The charge accrues over the lag and is settled when the withdrawal lands.

### 6.5 `Settle { item }` and arrears

Pays what an item owes, from the holder's balance into the pool.

**The debt is capped at the declared price, between one settlement and the next.** No stretch of
being on the showcase can cost more than the price declared over it. This is a property of the mechanism and not a convenience: without it, entering the showcase
is an unbounded liability against an asset the holder may value for reasons the market never sees,
and no risk-averse holder enters at all. Entry is voluntary, so a mechanism nobody enters is a
mechanism that does not exist. With the cap, the worst that being on the showcase can cost is
exactly what the holder said the thing was worth, and that sentence is what makes the offer
credible.

Arrears arise only where the charge is paid out of a purse — a reprice, a withdrawal, a settle.
**A buyout can never fall into them**, because there the charge comes out of the price the buyer
paid and is capped by it (§6.6). That is a property worth having: the one transition the mechanism
exists to produce is the one transition that cannot fail for want of money.

When the holder's balance does not cover what is owed, the parameter `arrears` decides, and both
settings are simulated because they are not obviously ranked:

- **`Forfeit`** — as much as the balance holds is taken into the pool, the item leaves the
  showcase, and the rest of the debt is written off and never charged again. Gentle, and it hands
  a holder a way to sit on an asset for nothing by keeping an empty purse.
- **`ForcedSale`** — the venue buys the item at its declared price out of its own purse. The debt
  comes out of that price into the pool, what is left reaches the holder, and the item leaves the
  showcase in the venue's hands. Harsh, and it makes an empty purse the most expensive way to hold
  anything.

  Where the venue's purse does not cover the declared price the sale cannot happen, and `Forfeit`
  applies instead. How often that happens is a measurement rather than a footnote: a venue that
  cannot afford its own rule has a rule only on paper, and a run in which that is common has found
  something about the parameters rather than about the code.

Both are honest about what they cost, and which one a market survives is a result rather than a
preference.

### 6.6 `Buy { item }`

Takes a listed item at its declared price.

Rejected when: the item is not listed · the actor already holds it · the actor's balance does not
cover `declared + execution`.

The order of the effect is fixed and every step of it is stated, because a different order produces
a different number:

1. `declared` leaves the buyer's purse.
2. What the listing owes is assessed and taken out of it into the pool — at most the declared
   price, by the cap of §6.5. The seller's own purse is never touched.
3. `commission` of the declared price, rounded up and never more than what step 2 left, goes to the
   venue's purse.
4. What remains reaches the seller.
5. The item's holder becomes the buyer, and **the item stays on the showcase at the same declared
   price**, assessed from now.
6. `execution` leaves the buyer and joins `spent`.

Steps 2 and 3 are in that order and not the other, and the difference is real: a debt of the whole
declared price leaves the venue nothing, where a commission taken first would leave the pool short
of a charge that was properly owed. The charge is a debt and the commission is a fee, and a debt is
paid before a fee.

Step 5 is a decision. The alternative — the item leaves the showcase and the buyer chooses whether
to re-enter — is closer to a voluntary regime, and it means every purchase is followed by a window
in which the asset is neither chargeable nor buyable. On a market whose whole disease is that
assets stop moving, handing every buyer a free pause is handing the disease a way back in. The item
stays listed at the price its new holder just proved it was worth to them, and they may reprice it
at once downward or after the lag upward. A buyer who wants out uses `Withdraw` and waits, in the
open, like everybody else.

A pending change **does not survive a buyout**: it was the previous holder's statement about an
asset they no longer hold.

### 6.7 `tick`

The only time-driven rule is distribution, and it fires at period boundaries.

```
if now − distributed_through ≥ period:
    shared   = pool · share / 10 000                        rounded down
    the venue takes pool − shared into its purse
    per rule = Even | ByTrades | ByCharge
    every participant but the venue receives its portion of shared         rounded down
    what the division could not place stays in the pool
    traded and paid are reset to zero for every participant
    distributed_through = now
```

Rounding down with what is left over staying in the pool, because the alternative is inventing the
last unit. `ByTrades` and `ByCharge` over a period in which nobody traded or nobody was charged
share out nothing and leave that part of the pool where it is; that is not a failure and the report
says so.

`tick` cannot fail. It touches no listing, reads no exogenous input, and every operation in it is
bounded by the pool it divides — which is the second reason assessment is lazy: a tick that had to
assess every item would be a tick that could abort, and an abort in a sweep is a run that produced
no evidence at all.

## 7. Events

One per thing that happened, and enough of it to reconstruct the ledger: `Listed`, `Repriced`
(carrying whether it bound at once or was queued), `Withdrawn`, `Bought` (carrying the price, the
charge settled, the commission, and both parties), `Settled`, `Forfeited`, `ForceSold`,
`Distributed`. Refusals leave through the observer's own channel, as everywhere in this workspace,
because the reason an action was refused exists nowhere else.

## 8. What acts on it

Agents, not the mechanism, carry valuations. Each participant `i` values item `x` at

```
value(i, x) = intrinsic(x) + taste(i, x)
```

where `intrinsic` is drawn when the world is generated and `taste` is drawn per pair — the model of
[`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §12.5, after Weyl and Zhang.

**`intrinsic` is never in the mechanism's state.** It is generated with the world, handed to each
agent as its own private parameter, and handed to measurement so convergence can be scored against
it. An agent that could read it out of the state would be an agent that knows the answer, and
convergence would measure nothing but the simulator telling itself what it already stored.

The populations — honest holder, collector, moneyless newcomer, careless recipient, and the seven
predators of [`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §12.7 — are behaviours in `sting-karman-agents`, and
each is a claim about behaviour that the manifesto's §11 numbers a prediction for.

## 9. Invariants

Checked after every transition. A violation is not a market outcome; it aborts the run.

Four of them are properties of the state alone, and the engine checks them after every transition:

1. **Nothing is created or destroyed.** The sum of every balance — the venue's among them — plus
   the pool, plus `spent`, equals what the world opened with.
2. **One holder.** Every item has exactly one, and that one is somebody in the population.
3. **A price exists exactly on the showcase, and it is not zero.** An item has a declared price if
   and only if it is listed, and a pending price is a price too.
4. **A change never takes effect before the listing was last assessed.** A change whose moment has
   already come and gone is the ordinary state of a mechanism that charges lazily — nobody has
   touched the item since — so what is checked is the only thing that would be wrong: a change
   dated before the assessment it is supposed to follow.

Three more are properties of a transition rather than of a state, because they are about the clock
and a state does not carry one. They are held by tests that inject their own defect and watch it
caught, which is the same standard by a different instrument:

5. **A debt never exceeds its declaration.** What a listing owes since it was last settled is at
   most the price declared over that stretch.
6. **Assessment does not run backwards.** `assessed_to` never decreases, and never passes `now`.
7. **The venue never acts.** It holds a purse and it may hold items, and it is the actor of no
   transition. Everything that reaches it was sent there by a rule.

Health is not among them. An item that owes more than its holder can pay is exactly the state
§6.5 exists for, in the same way that an unhealthy position is what liquidation is for.

## 10. Conformance and the ladder

The mechanism passes the suite in `sting_karman_core::conformance` — determinism, invariant preservation,
rejection leaving state untouched — and pins a golden run in `benches/baseline.toml`, like every
other family here.

Its position on the ladder ([`VERIFICATION.md`](./VERIFICATION.md)) is stated rather than assumed,
and it is unlike the others:

| Level | Status | Why |
|---|---|---|
| 0 | **unreachable for now** | there is no deployed reference to diff against; there will be one after phase 2, and this crate becomes its specification |
| 1 | **not applicable** | no real episode of this mechanism has ever been recorded, because it has never run |
| 2 | **reachable immediately, and the first gate** | the synthetic market must reproduce the measured turnover, its spread across collections, the transfers per moved item and the share of items carrying a price, before the charge is switched on. Until it does, no result about the mechanism means anything — this is gate Г0 |
| 3 | **the point of the exercise** | the claims of [`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §11 are registered in `findings/` before the sweeps that grade them |

A mechanism at level 2 makes level-2 claims and no others, and its reports say so in the gaps
section that [`REPORTING.md`](./REPORTING.md) §2.9 requires.

## 11. Where it will live

A new crate, `sting-karman-mech-showcase`, beside the other families. Nothing in `sting-karman-core` learns a
word of this: a family that needed the core to change would be a design failure to escalate
([`ARCHITECTURE.md`](./ARCHITECTURE.md) §5, invariant 4), and this one needs the four ports exactly
as they stand — its state, its actions and its events are its own, its charge reads nothing outside
itself, and it consumes no exogenous path at all.

That last point is worth stating plainly, because it is unusual here. A credit market needs an
oracle and a rate; a perpetual venue needs funding. This mechanism needs **nothing from outside the
run**. Every price in it is declared by a participant. That is the claim the whole manifesto rests
on: the machine makes prices where there were none, rather than reading prices from somewhere else.

## 12. The world it runs in

A mechanism is only as good as the market it is run against, and a market that was fitted to the
answer proves nothing. This section fixes how the synthetic market is built, what in it is measured
and what is fitted, and what would show the fit to be worthless. It is the specification gate Г0
grades, and it is written before the fit rather than after it, for the reason every threshold in
this project is.

### 12.1 What is taken and never fitted

Straight out of the census (`testdata/fixtures/ton-census.json`), per collection:

| Taken | Why it is not a free hand |
|---|---|
| how many items the collection has | a count |
| what share of them sits at the issuer | a count |
| how many holders the rest is spread over | a count |
| what share of the float the largest holder has | a count |
| the median price the collection's sales actually paid | a measurement, and it sets the scale, not the shape |
| how many participants the whole float is spread over | a count: 2.68 float items per holder |

These are the shape of the market. Nothing about them is chosen, and a run that changed one would be
a run about a different market.

The last of them was found by getting it wrong. A population is not a free number beside a stock:
the census fixes how thinly a float is held, and a world seating one holder for every eighteen items
where the market seats one for every two and a half is a world where a single decision to sell puts
seven times as much on a showcase. Everything downstream moves with it.

**What a participant opens with is not in this table, and is not fitted either.** The census reports
no wealth, so a purse is set high enough that nothing is refused for want of money, and that it is
high enough is shown rather than assumed: raised from forty tons to a thousand, every number the
gate is stated in stops moving — turnover within half a per cent, the band between collections
within one. Below about two hundred tons it does bind, and the collections it binds are the dear
ones, which stop trading entirely. A number that changes nothing above a threshold is a threshold
and not a knob, and this is where the threshold was measured.

### 12.2 What is drawn

**True worth.** Each item `x` gets an `intrinsic(x)` drawn from a lognormal law, discretised to
integer base units, with the collection's measured median as its scale and one shape `σ` shared by
the whole world. Lognormal because value here is the product of many small independent factors —
rarity, aesthetics, the moment it was issued — and a product of such factors is lognormal whatever
they are individually. Nothing in the data argues for a different family, and a family chosen to fit
would be a family that cannot fail.

**Taste.** Each pair of a participant `i` and an item `x` gets a `taste(i, x)`, drawn uniformly from
`[−d · intrinsic(x), +d · intrinsic(x)]` for one dispersion `d` shared by the whole world. Uniform
and bounded rather than normal and tailed, because a tail here would be a second shape parameter
doing the first one's work, and because a valuation must stay positive without a truncation rule
nobody can justify.

**Taste is never stored.** A world of a million items and ten thousand participants has ten billion
pairs. `taste(i, x)` is a pure function of the world's seed, the participant and the item, computed
where it is needed and thrown away — the same value on every machine and every re-run, at no memory
at all. It is not drawn from the run's stream and it is not agent randomness
([`ARCHITECTURE.md`](./ARCHITECTURE.md) §5, invariant 8): it is a fact of the world, fixed before the
first tick, and an agent reading it is reading its own opinion rather than tossing a coin.

**What an agent may see.** Its own `value(i, x) = intrinsic(x) + taste(i, x)`, and nothing else.
`intrinsic` alone is never readable — not by an agent, not through the mechanism's state (§4.4).
Convergence is scored against it from outside the run, which is the only place it can be scored from
without the simulation grading its own homework.

### 12.3 What is fitted

Five numbers are varied, and this section said three before the fit was attempted. Both of the
numbers it did not name were forced by measurements that could not be reached without them, and
saying so is cheaper than pretending the first count held.

| Varied | What it controls |
|---|---|
| `σ` — the shape of `intrinsic` | how far apart the things in a collection really are |
| `d` — the dispersion of `taste` | how much two people disagree about the same thing |
| `willing` — the share of holders who would sell at some price | how much of the market is reachable at all |
| `opening` — what a seller first asks above its own valuation | how far above a market a standing offer starts |
| `step` — what it takes off that at each look | how long a listing stands before it clears |

**Why the last two.** A market that sells 4.5% of its float in a quarter while 6.7% of it stands
priced, and whose standing asks are 3.7 times what its sales settle at, is a market whose listings
come down. A population with one fixed markup per holder can produce the turnover or the gap and
never both: a markup low enough to clear is a markup too low to leave a gap, and one high enough to
leave the gap never clears. Asking high and selling often are the same sellers at two points of one
walk. The mechanism has had `Reprice` since §5 — lower binds at once, higher waits out the lag, and
that asymmetry is the whole of its design — and a population that never used it was not modelling
this mechanism at all.

**And they are not five independent handles.** `σ` and `d` are not separately identified: both widen
the prices paid inside a collection, and only their combination is pinned by anything measured here.
Say four, then, and say that the fit is a generous one. That is the honest count, and the point of
§12.4 does not rest on it being small.

### 12.4 The gate

Four measured numbers ([`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §12.8), and the split between them is
the whole point:

| # | Number | Measured | Role |
|---|---|---:|---|
| 1 | share of the float **sold** over a quarter | 4.51% | fitted against |
| 2 | items carrying a price | 6.71% | fitted against |
| 3 | dispersion of the prices paid **inside a collection**, `p75/p25`, median over collections | 1.407 | fitted against |
| 4 | standing ask over price paid, inside a collection, median over collections | 3.7× | fitted against |
| 5 | **the spread of turnover between collections** | **1.30% … 7.70%** | **nothing is fitted to it** |

**Number 1 is not the 38.5% the manifesto quotes, and the difference is the largest correction this
stage made.** That figure counts every item that changed hands. Only 11.7% of those left a price on
the chain; the rest are gifts, wallet moves and the legs of an escrow. This mechanism produces
nothing but priced buyouts, so grading it against transfers would grade a market of sales against a
market that is seven-eighths not sales at all. What is comparable is what left a price: 31 827 of
706 056 float items, or 4.51%. The true sale rate lies between that and 38.47% and cannot be
narrowed further from the chain. **The same substitution is owed to `τ*`**, which
[`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) §11 calibrates against the transfer rate where Weyl and
Zhang's theorem asks for the chance of a *sale* — about eight times the charge, and a decision for
the manifesto rather than for this file.

**Numbers 3, 4 and 5 are read one collection at a time and never pooled**, because that is how the
census reports them and because pooled they answer a different question. The collections span from
2.5 tons to 5 355; a quartile pooled across them measures which collections exist, a gap pooled
across them compares the asks of dear collections against the sales of cheap ones, and — the same
error, on the number this project lives on — a **convergence** pooled across them rewards a market
for knowing that one collection is dearer than another, which nobody in the run discovered and
which §12.1 handed it. Measured inside a collection, `ρ` at no charge falls from 0.81 to 0.18.

Four aggregates are fitted. The fifth number is what 117 collections do *differently from each
other*, and the only thing that varies between them in the model is what §12.1 took out of the
census without choosing it — every behavioural number is the same for every collection. If the
spread comes out right anyway, the world has structure that the fit did not put there. If it does
not, the fit reproduced four averages and understands nothing about why one collection trades and
another does not, and that is a failure worth having found before anything was built on it.

**Pass:** all five within 10% — the spread compared at both its edges.

The report states three more and gates none of them: purchases per item sold (measured 1.147),
items per holder of the float (2.26), and the largest holder's share of it (9.6%). They are stated
because a world that misses them badly is a world worth doubting, and ungated because this project
does not fit what it does not test.

### 12.5 Size

**The gate is graded at the size the market has**: 1 295 768 items and 263 459 participants, item
for item with the census. Smaller worlds were tried first and every number the gate is stated in
moved with the size — turnover from 4.55% at sixty thousand items to 3.77% at six hundred thousand,
and the band between collections from 1.29–10.00% to 1.85–9.01%. None of that drift is the market;
it is what a per-collection rate does when a collection holding four thousand items is represented
by two hundred. A world generated at one twentieth of the market is a sample of it, and the spread
between collections in a sample carries the sampling as well as the market. At full size there is no
sample and the question does not arise.

It is affordable because it was made so: a run of the full market takes about three seconds, down
from thirty-six, after conservation stopped being recounted over a quarter of a million purses after
every applied action and started being carried (`Mechanism::audit`, [`ARCHITECTURE.md`](./ARCHITECTURE.md)
§5), and after a population stopped holding a copy of the market each.

Smaller worlds remain the right thing for sweeping a parameter across many values, where what is
wanted is the shape of a curve rather than a number compared against a measurement. What must not
happen is grading a per-collection statistic on one.

