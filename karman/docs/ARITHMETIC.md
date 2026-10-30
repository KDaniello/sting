# The numeric contract

The widths, scales and rounding rules that mechanism arithmetic obeys, and the requirement each
one places on [`sting-karman-types`](../crates/sting-karman-types).

Every rule below is traceable to a reference implementation. That is not deference: level 0 of
the ladder ([`VERIFICATION.md`](./VERIFICATION.md)) demands bit-exact agreement with the
reference an implemented mechanism claims to model, and a numeric decision taken without
reading that reference is a divergence scheduled for later.

## 1. What the references compute in

| Reference | State width | Arithmetic width | Scales | Rounding | Time base |
|---|---|---|---|---|---|
| Morpho Blue | `uint128` — every field of `Market` | `uint256` | `WAD = 1e18` for rate, LLTV and fee · `ORACLE_PRICE_SCALE = 1e36` for price | down and up | seconds |
| Morpho AdaptiveCurveIrm | `int256` | `int256` | `WAD = 1e18` | down | seconds |
| Hyperliquid | per-asset `szDecimals`; price at 5 significant figures, at most `6 - szDecimals` decimals for perps and `8 - szDecimals` for spot | — | per asset | — | seconds |
| Constant product | integer base units | integer | none | down | — |

## 2. Widths

**State is 128-bit unsigned.** Morpho stores every market aggregate — supply and borrow assets,
supply and borrow shares, the fee, the last update — in `uint128`. A single position's shares
are bounded by the market total that holds them, so the aggregate width is the binding one.

**Intermediates are 256-bit, and 256 bits always suffice.** For 128-bit operands
`x · y ≤ (2¹²⁸ − 1)² = 2²⁵⁶ − 2¹²⁹ + 1`, and rounding up adds at most `d − 1 ≤ 2¹²⁸ − 2`, so the
largest intermediate is `2²⁵⁶ − 2¹²⁸ − 1 < 2²⁵⁶`. A 256-bit intermediate therefore cannot
overflow for any 128-bit operands, in either rounding direction — the bound is a property of the
widths, not an assumption about the values.

**Results are 128-bit.** A result that does not fit is an error, never a wrap and never a
saturation: a saturated number still looks like a number.

**Money is 128-bit; a price is not money.** Everything a market holds — what was supplied, what
is owed, the shares against both, the fee — is 128-bit, because the reference stores it that way.
A price is held by nobody and is a quantity of nothing: `IOracle.price()` returns `uint256`, and a
live market already needs it. cbBTC quoted in USDC is `6.48e38` against a scale of `1e36`, which
is 1.9 times what the width money is carried in can hold, so a 128-bit price would not model that
market badly — it could not open it at all.

`Wide` is therefore public, and it is the only value of that width that crosses a boundary. It
cannot become money by accident: no `Amount` is of it, no state field holds one, its halves are
private, and every operation on it that ends in a quantity returns 128 bits or an error. On the
wire it is a decimal string, for the same reason a 128-bit value is.

## 3. Scales belong to mechanism families, not to the core

Morpho alone uses two: `1e18` for rates and thresholds, `1e36` for oracle prices. Hyperliquid
uses neither — its precision is per asset, and its prices are constrained by significant figures
rather than by a fixed denominator.

A scale is therefore a property of the family that speaks it, declared in that family's crate.
`sting-karman-types` owns the **operation** and no scale: `mul_div` with an explicit rounding
direction is what every family needs, and it is all they share. A fixed-point type in the
foundation would be one family's convention imposed on the others.

## 4. Rounding

**Two directions, down and up.** Nothing in the references rounds to nearest, so nothing here
offers it.

**Every lossy operation takes its direction as an argument, and there is no default.** A
direction that can be omitted is a direction that will be omitted. Which direction is correct
follows from [`ARCHITECTURE.md`](./ARCHITECTURE.md) §5 invariant 6: the one that favours the
venue.

Morpho expresses rounding up as `(x · y + d − 1) / d`. For `d > 0` this equals `⌈x · y / d⌉`, so
a quotient-and-remainder formulation agrees bit for bit; §2 shows the addition cannot overflow.

## 5. Time

Seconds. Morpho's `lastUpdate` is a timestamp and its accrual consumes elapsed seconds, so a
mechanism that accrues needs to know what a tick is worth. That conversion is scenario
configuration and never engine knowledge ([`ARCHITECTURE.md`](./ARCHITECTURE.md) §8).

## 6. What lies outside these widths

The adaptive curve interest-rate model computes in `int256`, and its exponential is capped at a
value of roughly `5.77e40` in WAD units — about `5.77e58` as an integer, two orders of magnitude
beyond signed 128-bit. That arithmetic is not expressible in the widths above, and widening the
foundation to hold one contract's intermediate would be the tail wagging the dog.

It does not need to be. In the reference the interest-rate model is a separate contract reached
through an address, and that seam is reproduced as a port of the lending family:
[`BorrowRate`](../crates/sting-karman-mech-lending/src/rate.rs) takes the market it is pricing, because
a rate reads utilisation and an input that saw only the clock could not express the one feedback
loop a credit market exists to exhibit. Accrual itself — the compounding series, the interest and
the fee it is split by — is unsigned and inside the widths above, so level 0 covers it with the
rate supplied identically to both sides of the differential.

The oracle is a different kind of input and stays where it was: a price is a function of the clock
and of nothing the mechanism does, so it is an exogenous path behind
[`PathSource`](../crates/sting-karman-core/src/ports/data.rs).

The cost is stated rather than hidden: a run driven by a rate it was handed makes no claim about
the rate curve itself, and its report says so in the gaps section
([`REPORTING.md`](./REPORTING.md)).

## 7. Where each rule is enforced

| Rule | Enforced by |
|---|---|
| money is 128-bit unsigned, changes are 128-bit signed | the types themselves — a negative quantity is unrepresentable |
| units do not mix | the unit parameter of `Amount` and `Delta`; mixing fails to compile |
| every lossy operation states its direction | the signature — `Rounding` has no default |
| intermediates do not overflow | §2's bound, and a property test that reconstructs `q · d + r = x · y` in 256-bit space |
| a price is the only 256-bit value that crosses a boundary | `Wide` has no public halves and narrows only through operations that return money or an error; level 0 drives the reference at a price above 128 bits |
| results that do not fit are errors | `ArithError`, which mechanisms surface as a rejection |
| no scale in the foundation | review against §3; a fixed-point type in `sting-karman-types` is a rejected change |
| no floats in a transition | `ARCHITECTURE.md` §8, and level 0 of the ladder |

## Sources

- [`MathLib.sol`](https://github.com/morpho-org/morpho-blue/blob/main/src/libraries/MathLib.sol) — `WAD`, `mulDivDown`, `mulDivUp`, `wTaylorCompounded`
- [`SharesMathLib.sol`](https://github.com/morpho-org/morpho-blue/blob/main/src/libraries/SharesMathLib.sol) — `VIRTUAL_SHARES`, `VIRTUAL_ASSETS`, the four conversions
- [`ConstantsLib.sol`](https://github.com/morpho-org/morpho-blue/blob/main/src/libraries/ConstantsLib.sol) — `ORACLE_PRICE_SCALE`, `MAX_FEE`, `LIQUIDATION_CURSOR`
- [`IMorpho.sol`](https://github.com/morpho-org/morpho-blue/blob/main/src/interfaces/IMorpho.sol) — the width of every stored field
- [`ExpLib.sol`](https://github.com/morpho-org/morpho-blue-irm/blob/main/src/adaptive-curve-irm/libraries/ExpLib.sol) — the signed arithmetic and the exponential's cap
- [Hyperliquid tick and lot size](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/tick-and-lot-size) — per-asset precision
