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

## The one real engine gap (everything else is present)

Wear-levelling needs "the least-damaged truck **among those available**" — a *masked* argmin.
The engine has `argmin_array`/`argmax_array` with a stable lowest-index tie-break (verified),
but **no masked/filtered reduction builtin**. Today the model expresses the mask with the
**penalty idiom**: `argmin_array(damage + BIG·failed)` — add a huge constant to failed trucks
so they can never win. This is correct and deterministic, but it is a workaround:

- it relies on `BIG` being larger than any real damage spread (a magic constant), and
- it can't express richer availability predicates (in-shop, tire-constrained, route-locked)
  without stacking more penalty terms.

**Recommended engine addition (small):** a masked reducer family —
`argmin_where(values, mask)` / `masked_min` / `masked_mean` — with the same lowest-index
tie-break as `argmin_array`, so dispatch and criticality queries read declaratively instead of
via penalty arithmetic. This is the *only* item from the original build order (spec §3.5 item
2 / §3.6 item 3) still outstanding for the fleet vertical; items 1–2 (the array executor,
per-member `#k` surfacing, vector-preserving `lag`) all landed.

## What's still off the table (unchanged non-goals)

- **Array-valued stocks** are not supported — and not needed: per-member accumulation uses the
  `expression + lag` recurrence, which is the sanctioned pattern (verified in the spike).
- **Runtime-dynamic fleet size** — the `Fleet` dimension `size` is static (set in the model,
  not computed at runtime). Changing fleet size = editing one number and re-running, not a
  runtime variable. Fine for scenario sweeps; a hard limit for endogenous fleet growth.
- **Per-entity queuing DES** (individual truck trips through shovel queues) stays ceded — model
  haulage as aggregate cycle rates, per the ontology argument.

## Next steps to make it a deliverable

1. **Add the masked-reduction builtin** (above) so wear-levelling drops the penalty idiom.
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
