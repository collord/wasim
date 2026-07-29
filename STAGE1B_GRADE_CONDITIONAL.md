# Stage-1b RAM Demo — Grade-Conditional Overloading

**Model:** [`schema_examples_manual/haul_truck_grade_conditional.json`](schema_examples_manual/haul_truck_grade_conditional.json)
**Regression test:** [`engine/tests/haul_truck_grade_conditional_smoke.rs`](engine/tests/haul_truck_grade_conditional_smoke.rs)
**Builds on:** [`STAGE1_RAM_DEMO.md`](STAGE1_RAM_DEMO.md) · **Spec:** [`HAUL_FLEET_MODEL_SPEC.md`](HAUL_FLEET_MODEL_SPEC.md) §1.2

Stage 1 asked *how much* a 25% overload costs in life (answer: it halves it). Stage 1b asks
the **policy** question: *when should you overload at all?* — and shows the answer is neither
"always" nor "never."

## The insight (spec §1.2)

> **Damage is paid in tonnes; revenue is earned in metal.**

A 300 t load of high-grade ore is worth far more than 300 t of waste, while the damage
inflicted is *identical* — damage depends on mass, not on what the mass is worth. So the
rational policy is **grade-conditional**: overload only when the value of the material
(grade × price) clears the crossover where the extra throughput outweighs the super-linear
life cost. The asymmetry is structural:

- **Revenue scales linearly** with payload (`payload × value`).
- **Failure cost scales super-linearly** with payload (`base_rate · (payload/240)^β · replacement_cost`).

So there's a crossover value `g*·p*`: below it, the wear isn't worth it; above it, overloading
prints money. Overload uniformly and you eat the wear on low-grade tonnes for nothing.

## What the model does

Three policies run on the **same** per-realization draws of load exponent
`β ~ Triangular(2.5, 3, 4)`, grade `~ Triangular(0.5, 1.0, 1.5)`, and price
`~ Lognormal(mean 320, sd 70)`:

- **A — always nominal** (240 t)
- **B — always overload** (300 t)
- **C — grade-conditional**: `payload = if (grade·price > crossover) then 300 else 240`

NPV integrates the discounted steady-state net rate `payload·value − failure_rate·replacement_cost`
over a 10-year horizon. (Steady-state with replacement — unlike Stage 1's run-to-failure — so
the extra throughput is realized over the full horizon; this is what lets overloading ever pay.)

## Result (4000 realizations, seed 20260728)

| Policy | E[NPV] | Note |
|---|---:|---|
| A — always nominal | 393,298 | the conservative baseline |
| B — always overload | **383,490** | **worse than nominal** — blind overloading destroys value |
| C — grade-conditional | **404,917** | **beats both** |

- **Overloads 45.7% of the time** under C — a genuinely mixed policy, not a disguised fixed one.
- **Value of grade-conditioning = C − best fixed = ~11,600 per truck** (~3% of NPV), created
  purely by *when* you overload.
- Life ratio 0.494 (overload still ~halves component life — continuity with Stage 1).

The headline for a maintenance manager: **"always overload" is a *worse* policy than "never
overload" on these economics — but the money is in doing it selectively.** That's a
counter-intuitive, defensible finding of exactly the kind that justifies a study.

## Reproduce

```bash
cd engine
cargo test --test haul_truck_grade_conditional_smoke -- --nocapture
```

## Honest edges (Stage-1b scope)

- **Campaign-level grade, not per-run.** Grade is one draw per realization — read it as "this
  bench/campaign has grade G; set the loading policy for it." Per-*run* grade sequencing (and
  the dispatch that responds to it) is the fleet problem — [`STAGE3_FLEET_SCOPE.md`](STAGE3_FLEET_SCOPE.md),
  where `overload_on = grade > g* AND price > p*` is already wired per truck.
- **Steady-state economics.** Failure frequency is `base_rate·(payload/240)^β` (life consumed
  per year = failures per year under replacement). The fully dynamic replace-cycle version
  (reset the damage stock on each failure, count discounted replacements) is a refinement; the
  policy ordering (C > A, C > B) is robust to it because both effects are monotone in payload.
- **Threshold is a heuristic.** `crossover` is fixed at the β = 3 value; because β is itself
  uncertain, C occasionally overloads a realization it shouldn't. It still dominates — a
  price/β-aware threshold would only widen the gap (a value-of-information angle for later).
