# Proposal: represent GoldSim Probabilistic Optimization in the WASiM schema

**For:** the WASiM schema owner. **From:** the re-gsm decoder/emit side.
**Status:** proposal — the schema *shape* is yours to decide; this documents the intent, the
decoded source data, and a concrete starting point. The emit side is ready to populate whatever
shape you land on.

---

## 1. What it is (the intent we're failing to capture)

GoldSim **Optimization** searches for the input values that make a model's result best. The
user configures:

- **Optimization variables** — scalar `Data` or `Stochastic` elements GoldSim is allowed to
  adjust. Each has a **lower bound**, **upper bound**, an **initial value** (starting guess),
  and an optional **integer-only** restriction. (Bounds/initial are numeric only.)
- **Objective function** — a model result to **maximize** or **minimize** (typically a
  cumulative, peak, or valley value). It must depend on all the optimization variables.
- **Constraint(s)** — optional condition(s) that a valid solution must satisfy.
- The solver is **Box's complex method** (iterative search; converges on the optimum).

**Probabilistic** optimization: when the system is stochastic, the objective can't be a single
deterministic output — it must be a **Monte Carlo statistic** of an output (e.g. mean, 50th
percentile). GoldSim does this by embedding the model in a **SubModel** and running many
realizations per candidate solution.

**Deliverable of an optimization run** = the optimal variable values + the objective's value.
It is a *study over the model*, not a single run of it.

Sources: [Overview of Optimization](https://help.goldsim.com/Modules/5/overviewofoptimization.htm),
[Optimization Variables](https://help.goldsim.com/Content/GS/specifyingtheoptimizationvariables.htm),
[Objective & Constraints](https://help.goldsim.com/Content/GS/specifyingtheobjectivefunctionandconstraints.htm),
[Probabilistic Optimization](https://support.goldsim.com/hc/en-us/articles/360047030994-Probabilistic-Optimization),
[SubModel example](https://help.goldsim.com/Content/GS/submodelexampleprobabilisticoptimization.htm).

## 2. The gap today

The emitted model.json describes **one baseline configuration run once**. `simulation_settings`
is only `{duration, timestep, n_realizations, seed}`. There is no objective, no direction, no
concept that certain nodes are search variables with ranges. The optimization variables emit as
ordinary `constant`/`random_variable` nodes at a single value; their bounds, the objective, the
direction, and the constraints are all dropped. A "which design maximizes revenue?" study reads
as "here's the model at defaults."

### 2a. Prerequisite gap: v2 has no SubModels (a regression from 0.7.0)

Probabilistic optimization **requires SubModels** — the objective must be a Monte-Carlo
*statistic* of an output, which GoldSim computes by embedding the model in a SubModel and running
N realizations per candidate solution. So optimization can't be fully represented unless the
schema can represent a SubModel. Today:

- **0.7.0** (`model.schema.json`) *does* model SubModels: `container_def.kind` includes
  `"submodel"`, plus a per-container **`simulation_settings`** (the nested run's clock/MC settings)
  and an **`interface`** (named boundary inputs/outputs). (Emit currently only sets
  `kind: "submodel"` and leaves those two `null` — marked but not filled.)
- **v2 / 0.8.0** (`wasim-schema-v2.json`) **dropped the concept entirely**: `container_def` is
  `additionalProperties:false` with only `{id, name, parent, children, elements}` — no `kind`, no
  nested `simulation_settings`, no `interface`. A SubModel emits as a plain container,
  indistinguishable from a grouping folder; the nested-run semantics are lost.

So for v2 this is really **two coupled gaps**: (a) no SubModel concept and (b) no optimization
concept — and (b) can't be fully modeled without (a). See design question #2.

## 3. What the .gsm actually gives us (decoded, available to emit)

The decoder already reads the optimization settings — the data is present, we just have nowhere
to put it. From `Hydropower_optimization.gsm` (a real 6-variable study):

**`COptimization`** (the document's optimization settings; hangs off the model clock):
- objective function — stored as a **reference** to a model element (exact decode slot to be
  pinned during emit; it is not an inline formula)
- direction / enabled flags (maximize vs minimize) — decoded as bytes; semantics to confirm
- the list of optimization variables

**`COptimizationVariable`** (one per variable) — 6 in this model:

| target element | lower | upper | initial (guess) | integer-only |
|---|---|---|---|---|
| `Target01` | 20 ft | 146 ft | 140 ft | no |
| `Target02` | 20 ft | 146 ft | 115 ft | no |
| `Target03` | 20 ft | 146 ft | 30 ft | no |
| `Target04` | 20 ft | 146 ft | 100 ft | no |
| `Div_Cap` | 20 cfs | 500 cfs | 200 cfs | no |
| `Outlet_Dia` | 2 in | 20 in | 72 in | no |

(The three numeric inputs per variable are the lower bound, upper bound, and initial value;
their exact source-slot order is an emit detail I'll confirm. Bounds come through with units,
so they'll be SI-normalized like every other quantity — value in SI, original in `display_unit`.)

**Not needed in the schema:** Box's complex method itself (solver mechanics, engine's job) and
per-trial iteration state. The schema should carry the **problem definition**, not the solver.

## 4. Proposed shape (a starting point — adapt to your conventions)

Optimization is a **document-level study config**, so a top-level `optimization` block reads more
naturally than a per-node trait (a variable's range only means something in the context of *this*
study). But the variables *are* references to existing elements, so an alternative is a node
trait `optimization_variable: {lower, upper, initial, integer}` + a top-level objective. Your
call. A top-level block:

```jsonc
"optimization": {
  "objective": {
    "element_id": "Total_Revenue",      // the result being optimized
    "direction": "maximize",            // maximize | minimize
    "statistic": { "kind": "mean" }     // [probabilistic] mean | { "kind":"percentile","p":50 } | null (deterministic)
  },
  "constraints": [
    { "condition": { /* quantity_or_formula */ }, "label": "..." }   // optional
  ],
  "variables": [
    {
      "element_id": "Target01",
      "lower":   { "value": 6.096,  "unit": "m", "display_unit": "ft" },   // 20 ft, SI-normalized
      "upper":   { "value": 44.50,  "unit": "m", "display_unit": "ft" },   // 146 ft
      "initial": { "value": 42.67,  "unit": "m", "display_unit": "ft" },   // 140 ft
      "integer": false
    }
    // ... Div_Cap, Outlet_Dia, ...
  ],
  "sampling": { "realizations_per_trial": 1000 }   // [probabilistic] N MC runs per candidate; null if deterministic
}
```

`optimization` is absent for non-optimization models (the overwhelming majority), so it's
optional at the top level.

## 5. Design questions for you

1. **Top-level block vs. node trait** for the variables (I lean top-level block; see above).
2. **SubModels + the probabilistic objective (the coupled decision — see §2a).** v2 has no
   SubModel concept, but probabilistic optimization needs one. Two paths:
   - **Restore SubModels in v2 first**, then let the objective's statistic hang off the SubModel's
     own Monte-Carlo settings (the natural, GoldSim-faithful model — a SubModel *is* the "run N
     realizations and reduce to a statistic" boundary). This is the bigger but more correct fix,
     and it re-closes a 0.7.0→v2 regression that affects more than just optimization. Likely
     mirrors 0.7.0's `container_def` submodel fields (`kind`, nested `simulation_settings`,
     `interface`) adapted to the v2 container shape.
   - **Or** put an inline `statistic` + `realizations_per_trial` on the `optimization` block (as
     drafted in §4) and treat the SubModel as an ordinary container — simpler, but it encodes the
     nested-run semantics in the optimization block instead of where GoldSim actually keeps them,
     and still leaves v2 unable to represent SubModels in general.
   My lean: restore SubModels in v2 and reference them from the objective; the inline-statistic
   form is the fallback if SubModel support is out of scope for this round.
3. **Constraints** — reuse `quantity_or_formula`/`trigger_spec`, or a dedicated shape?
4. **Units on bounds** — confirm you want SI value + `display_unit` (consistent with `quantity`),
   which is what emit will produce.
5. **Version gating** — this lands in 0.8.0 (primitives). OK to add there?
6. **Engine semantics** — `wasim-engine-semantics.md` will need a short section on how an engine
   interprets an `optimization` block (or whether it's provenance-only, like `connections`).

## 6. What I'll do on the emit side (once the shape is settled)

- Walk `MasterClockInfo → COptimization`, emit the `optimization` block: objective element +
  direction + statistic, constraints, and each variable's `{element_id, lower, upper, initial,
  integer}` (SI-normalized bounds).
- Resolve the two TBDs during implementation: the exact objective-reference decode slot, and the
  maximize/minimize flag semantics (I'll confirm against the corpus optimization models rather
  than guess).
- **If SubModels are restored in v2 (§2a / Q2):** emit can fill them — a SubModel is already
  decoded as an `SSubModel` container (emit tags it in 0.7.0), and the nested clock is reachable
  the same way as the top-level one, so I can populate a nested `simulation_settings` (I already
  extract duration/timestep to SI) and the interface. This is independent of optimization and
  fixes the plain 0.7.0→v2 SubModel regression too.
- Validate against the corpus optimization models (`Hydropower_optimization`,
  `CalibrationOptimization`) and keep the full corpus at 283/283.

If you'd rather I pin down the objective reference + flag semantics *before* you design (so the
spec is fully closed), say so and I'll do that RE pass first.
