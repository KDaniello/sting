"""The constant-product scenario, written again in cadCAD.

An independent implementation of the same model on a different engine, so the question "does
this simulator say what a simulator should say" has an answer that is not this workspace's own
word. What is shared is the model — the curve, the rounding direction, the population, the
sizes — and what is not shared is everything that computes it.

The two engines cannot produce identical runs and are not asked to. Their random streams are
different by construction: one draws from a seeded ChaCha stream and the other from Python's
Mersenne Twister, so a run here is not a run there. What must agree is the distribution, and
`compare.py` is where that is tested.

The arithmetic is the part that must be copied exactly rather than reimplemented freely. Python
integers are arbitrary precision, so `(reserve_out * amount_in) // new_in` is the same floor the
Rust mechanism takes, and a reference that rounded the other way would be measuring its own
disagreement rather than the engine's.
"""

from __future__ import annotations

import random
from typing import Any

TRADE_CAP = 1_000_000
TRADERS = 5
OPENING_TOKEN = 1_000_000_000
OPENING_QUOTE = 1_000_000_000


def amount_out(reserve_in: int, reserve_out: int, amount_in: int) -> int:
    """The constant-product output, floored.

    Flooring leaves the truncated remainder in the pool, which is the direction the mechanism
    under test rounds and the direction a real venue rounds. Copying it is the point: the two
    engines are being compared, not two rounding policies.
    """
    return (reserve_out * amount_in) // (reserve_in + amount_in)


def swap(token: int, quote: int, buying: bool, size: int) -> tuple[int, int]:
    """One swap against the pool, returning the reserves it leaves behind.

    A swap too small to pay anything out is refused, exactly as the mechanism refuses it: taking
    the input while paying nothing would be a donation, and a reference that accepted it would
    drift away from the engine for a reason that is not the engine.
    """
    if size <= 0:
        return token, quote
    if buying:
        out = amount_out(quote, token, size)
        if out == 0:
            return token, quote
        return token - out, quote + size
    out = amount_out(token, quote, size)
    if out == 0:
        return token, quote
    return token + size, quote - out


def decisions(rng: random.Random, traders: int = TRADERS) -> list[tuple[bool, int]]:
    """What the population does this tick.

    The draws happen in the order the engine's agents act, and the size is drawn before the
    side, because that is the order the behaviour draws them in. Neither engine can see the
    other's stream, but a reference that drew in a different order would be a different model.
    """
    made = []
    for _ in range(traders):
        size = rng.randrange(TRADE_CAP) + 1
        buying = rng.randrange(2) == 0
        made.append((buying, size))
    return made


def policy_flow(_params, _step, _history, state, **_kwargs) -> dict[str, Any]:
    """The population's orders for this tick."""
    return {"orders": decisions(state["rng"], state.get("traders", TRADERS))}


def update_token(_params, _step, _history, state, signal, **_kwargs) -> tuple[str, int]:
    token, _ = apply_all(state, signal)
    return "token_reserve", token


def update_quote(_params, _step, _history, state, signal, **_kwargs) -> tuple[str, int]:
    _, quote = apply_all(state, signal)
    return "quote_reserve", quote


def apply_all(state, signal) -> tuple[int, int]:
    """Every order of the tick, applied in the order the population acted.

    Sequential rather than netted: each swap moves the price the next one trades against, and a
    reference that applied them together would be modelling a batch auction rather than a pool.
    """
    token, quote = state["token_reserve"], state["quote_reserve"]
    for buying, size in signal["orders"]:
        token, quote = swap(token, quote, buying, size)
    return token, quote


def run_one(seed: int, ticks: int, traders: int | None = None) -> tuple[int, int]:
    """One run of the reference, driven directly rather than through the engine's executor.

    cadCAD's executor is used by `run_via_cadcad` for the comparison that has to go through it.
    This path exists because a distribution needs thousands of runs, and driving the same state
    update functions without the executor's bookkeeping is the same model at a hundredth of the
    cost — which is a statement about the reference, not about the engine under test.
    """
    state = {
        "token_reserve": OPENING_TOKEN,
        "quote_reserve": OPENING_QUOTE,
        "rng": random.Random(seed),
        "traders": TRADERS if traders is None else traders,
    }
    for _ in range(ticks):
        signal = policy_flow(None, None, None, state)
        state["token_reserve"], state["quote_reserve"] = apply_all(state, signal)
    return state["token_reserve"], state["quote_reserve"]


def run_via_cadcad(seed: int, ticks: int) -> tuple[int, int]:
    """The same run, through cadCAD's own executor.

    Driving it this way proves the model above is the model cadCAD would run, so the cheap path
    used for the distribution is not a third implementation quietly diverging from both.
    """
    from cadCAD import configs
    from cadCAD.configuration import Experiment
    from cadCAD.configuration.utils import config_sim
    from cadCAD.engine import ExecutionContext, ExecutionMode, Executor

    configs.clear()
    initial = {
        "token_reserve": OPENING_TOKEN,
        "quote_reserve": OPENING_QUOTE,
        "rng": random.Random(seed),
    }
    experiment = Experiment()
    experiment.append_model(
        model_id="cpamm",
        initial_state=initial,
        partial_state_update_blocks=[
            {
                "policies": {"flow": policy_flow},
                "variables": {
                    "token_reserve": update_token,
                    "quote_reserve": update_quote,
                    "rng": lambda _p, _s, _h, state, _sig, **_k: ("rng", state["rng"]),
                },
            }
        ],
        sim_configs=config_sim({"N": 1, "T": range(ticks), "M": {"unused": [0]}}),
    )
    executor = Executor(
        exec_context=ExecutionContext(ExecutionMode().single_mode),
        configs=experiment.configs,
        supress_print=True,
    )
    records, *_ = executor.execute()
    last = records[-1]
    return last["token_reserve"], last["quote_reserve"]
