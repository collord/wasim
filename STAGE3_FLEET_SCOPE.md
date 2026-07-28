# Stage-3 Fleet Model — Scope & Status

**Model:** [`parameters_examples/haul_fleet_overload.json`](parameters_examples/haul_fleet_overload.json)
**Regression test:** [`engine/tests/haul_fleet_overload_smoke.rs`](engine/tests/haul_fleet_overload_smoke.rs)
**Spec:** [`HAUL_FLEET_MODEL_SPEC.md`](HAUL_FLEET_MODEL_SPEC.md) §2–3 · **Verification:** [`MARKET_RESEARCH_REHYDRATION_2026-07.md`](MARKET_RESEARCH_REHYDRATION_2026-07.md) §1

## Headline

**Stage 3 is not a future build — the N-truck fleet model already exists and runs today.** The
rehydration prompt (and the original spec §3.2) called the array/dimension executor a "Large,
hard prerequisite" that *blocked* the fleet. The source-verification pass found that executor
**already landed**; this doc confirms the fleet model that rides on it **parses, runs, and is
bit-identical** — and adds the smoke test that pins it.

## What the model does (multi-truck, on today's engine)

A fleet of N trucks (a `Fleet` dimension, `size` = 5, trivially raised to 40) with **per-truck
state**, built entirely from verified primitives:

| Capability (spec) | How it's expressed | Verified |
|---|---|---|
| Per-truck cumulative damage | `damage` = `vector_map` over `Fleet`; `damage[i] = lag(damage)[i] + rate[i]` recurrence | ✅ runs |
| Power-law damage rate per truck | `vector_map`: `base·(payload[i]/240)^β`, β a scalar sample broadcast over the vector | ✅ |
| Per-truck failure latch | `failed` = `status` node over `Fleet` (fails at damage ≥ 1) | ✅ |
| **Wear-levelling dispatch** | `overload_target = argmin_array(damage + BIG·failed)` — assign the overload to the least-damaged *available* truck | ✅ (see gap below) |
| Grade+price-conditional overload | `overload_on` = `grade > g* AND price > p*` | ✅ |
| Fleet damage spread (the diagnostic) | `damage_spread = max_array(damage) − min_array(damage)` | ✅ |
| Production / revenue / maintenance NPV | `stock`s over the fleet aggregate | ✅ |
| Per-truck result surfacing | dimensioned outputs expand to `damage#1..#N`, each with its own history | ✅ |

**Run result (5 trucks, 500 realizations, seed 42):** all five `damage#k` series surface;
under wear-levelling the fleet stays almost perfectly balanced — `damage_spread ≈ 0.0045`
against `damage_mean ≈ 1.01` (the wear-levelling diagnostic doing exactly its job: no truck
races ahead of the pack). Bit-identical on repeat.

## The one real engine gap — now CLOSED ✅

Wear-levelling needs "the least-damaged truck **among those available**" — a *masked* argmin.
The engine had `argmin_array`/`argmax_array` with a stable lowest-index tie-break, but **no
masked/filtered reduction builtin**, so the model used the **penalty idiom**
`argmin_array(damage + BIG·failed)` — a workaround with a magic constant that couldn't express
richer availability predicates.

**This gap is now closed.** A masked reducer family was added to the engine
([`engine/src/eval.rs`](engine/src/eval.rs), [`model.rs`](engine/src/model.rs) `BuiltinFn`):

| Builtin | Meaning |
|---|---|
| `argmin_where(values, mask)` / `argmax_where` | 1-based index of the min/max among *selected* members; ties → lowest index; none selected → 0 |
| `masked_sum` / `masked_mean` / `masked_min` / `masked_max` `(values, mask)` | reduce over selected members only; none selected → 0 |
| `masked_count(mask)` | number of selected (non-zero) members |

A member is *selected* when its mask value is non-zero — same boolean convention as the rest of
the engine, same lowest-index tie-break as `argmin_array` (the bit-identity requirement), and
in-member-order accumulation (matching `reduce_data`) so results stay deterministic. Tests:
[`engine/tests/masked_reductions_v2.rs`](engine/tests/masked_reductions_v2.rs) — including an
equivalence test proving `argmin_where(damage, available)` matches the penalty idiom it replaces.

**The fleet model now uses it:** `overload_target = argmin_where(damage_prev, available)` (with
`available = 1 − failed`), and the old `damage_penalized`/`BIG` element is gone. The refactor is
behavior-preserving — the fleet smoke test produces bit-identical numbers (`damage_mean 1.0117`,
`damage_spread 0.0045`, `npv 4.257e10`) before and after — because `argmin_where` equals the
penalty idiom on every step where any truck is available (and where none is, no truck operates
anyway). Beyond dispatch, `masked_mean`/`masked_count` now express availability-filtered fleet
metrics (mean damage among *operating* trucks, live truck count) declaratively.

With this, **every item from the original build order is landed** — the array executor, per-member
`#k` surfacing, vector-preserving `lag` (spec §3.6 items 1–2), and now the masked dispatch
reducer (spec §3.5 item 2 / §3.6 item 3).

## What's still off the table (unchanged non-goals)

- **Array-valued stocks** are not supported — and not needed: per-member accumulation uses the
  `expression + lag` recurrence, which is the sanctioned pattern (verified in the spike).
- **Runtime-dynamic fleet size** — the `Fleet` dimension `size` is static (set in the model,
  not computed at runtime). Changing fleet size = editing one number and re-running, not a
  runtime variable. Fine for scenario sweeps; a hard limit for endogenous fleet growth.
- **Per-entity queuing DES** (individual truck trips through shovel queues) stays ceded — model
  haulage as aggregate cycle rates, per the ontology argument.

## Next steps to make it a deliverable

1. ~~Add the masked-reduction builtin so wear-levelling drops the penalty idiom.~~ **Done** (above).
2. **Dispatch-policy comparison** — the model implements wear-levelling; to *prove its value*,
   run it against naive (nearest-truck) and deliberate-cohorting dispatch and compare the
   damage-spread and clustered-downtime distributions (spec §1.3). This is a policy switch on
   `overload_target`, not new engine work.
3. **Availability feedback loop** (spec §1.4) — gate the per-truck damage rate on fleet
   availability so survivors work harder as trucks drop. The reinforcing loop is the single
   strongest argument for simulating rather than calculating; the fleet substrate now supports
   wiring it.
4. **Scale to N=40** and calibrate so the fleet doesn't uniformly saturate within the horizon
   (the current 5-truck demo runs to near-total wear by 5 yr — a calibration choice, not a
   structural limit).
5. **Value-of-information** on β via `submodel_stat` (verified working) — the output that tells
   a mine whether to fund a strain-gauge campaign instead of guessing the exponent.
