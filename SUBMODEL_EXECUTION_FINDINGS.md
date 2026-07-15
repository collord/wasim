# Findings: submodel/optimization execution — two emit-side gaps

**For:** the re-gsm decode/emit side. **From:** the WASiM engine side.
**Re:** the engine now *executes* submodels + optimization (schema 0.8.1–0.8.3 features).
Two data gaps stop the marquee cases from producing verifiably-correct non-zero results.
Measured against the corpus at `~/openvsim/wasim/schema_examples/` on 2026-07-14 — re-measure
before acting, counts move each regeneration.

## What now works in the engine

The engine executes all four schema features end-to-end (fixture-verified):
- **Array comprehensions** (`vector_map`/`index_ref`/`index` over `dimensions[]`) — corpus-wide
  (101/101 `vector_map.over` resolve).
- **SubModels**: a `kind:"submodel"` container's interior is extracted into a nested run; a
  `submodel_stat` reduces the referenced output's per-realization samples
  (`mean`/`percentile`/`sd`/`cumulative_prob`).
- **Optimization**: Box's complex method over the `optimization.variables`, reducing the
  objective by its statistic.
- **Interface-input driving**: a parent value drives a submodel interface-input (see Gap 2).

The reduction math and the solver are proven on hand-authored fixtures. What's missing is
*runnable data* for the corpus optimization models — the two gaps below.

## Gap 1 — submodel_stat outputs are stubbed / not real expressions

> **Re-check 2026-07-14 (fresh regeneration): largely resolved.** `designoptimization.total_cost`
> is now a **REAL AST** (`sum_array([construction_cost, discharge_fee])`), and all three
> optimization objectives now have real ASTs. End-to-end, **2 of 3 optimization loops are live**
> — the objective moves with the search: `probabilisticoptimization` (105.8→120.2),
> `dynamicoptimization` (0.02→147.6). `designoptimization` still evaluates to 0 — **traced to
> three emit-side stubs in the `Prob_StormwaterModel` interior** (not the engine, not the driving):
> `total_cost = construction_cost + discharge_fee`, where (a) `Unit_Const_Cost` is
> `normal(mean=0, stddev=0)` → 0, so `construction_cost = pond_capacity × 0 = 0` regardless of the
> driven `pond_capacity`; and (b) `Orifice_Outflow`/`Overflow` are `literal 0.0` → `discharge = 0`
> → `discharge_fee = if(0 > limit, fee, 0) = 0`. So `total_cost ≡ 0`. Emit needs real
> ASTs/dist-params for those interior elements to close this third loop; independent of the engine.
> The remaining 8 stubbed `submodel_stat`
> outputs are all **`non-expr`**: 5 are `sample` nodes (LEGITIMATE — a distribution-sample output
> the engine reduces fine, NOT a gap) and 3 are `None`-rule (`Failed3`, `Monthly_Totals`,
> `Annual_Totals` — genuinely need a real output expression). The original table below is the
> pre-regeneration snapshot, retained for history.

A `submodel_stat` reduces a submodel *output* element's per-realization values. For that to be
meaningful, the output element must actually *compute* something. Today, of the outputs
referenced by a `submodel_stat`, these do not:

| model | output | problem |
|---|---|---|
| `designoptimization` | `total_cost` | **STUB AST** — `sum_array([0.0, 0.0])`; always 0 |
| `probabilisticoptimization` | `TheSystem` | output is a `sample` node, not an expression |
| `montecarlostatistics` | `TheSystem` | output is a `sample` node |
| `uncertaintyvariability` | `Failed3` | output element has no expression (rule = none) |
| `wgen_par` | `Monthly_Totals`, `Annual_Totals` | no expression (rule = none) |
| `bayesianupdating`, `customimportance` | `Distribution` | **output element MISSING** entirely |

**Impact:** `designoptimization`'s objective is `pdf_mean(Prob_StormwaterModel.total_cost)`, and
`total_cost` is a literal-0 stub — so the objective is a constant 0 no matter what the optimizer
does. This is the single biggest blocker to demonstrating probabilistic optimization end-to-end.

**Fixes, by sub-type:**
- **STUB AST** (`total_cost`): emit the real output expression instead of `sum_array([0,0])`.
  (This is the same class as the earlier pdf-stat stubbing — the output computation was decoded
  but dropped.)
- **`sample`-node outputs** (`TheSystem`): this may be *correct* — a submodel output that is a
  distribution sample. If so, no change needed (the engine reduces its per-realization draws
  fine). Please confirm these are intended as sampled outputs, not stubbed expressions.
- **no-expression / MISSING outputs**: the output-producing element wasn't emitted, or the
  interface names an id that doesn't resolve. Emit the producing element, or fix the interface
  output id to match a real element.

## Gap 2 — the interface-input → parent binding is not encoded (engine infers by leaf name)

The schema declares `container.interface.inputs` (element ids) but has **no explicit binding**
from an interface input to the parent element that drives it. The engine currently *infers* it:
when an interface input names an *interior* placeholder element, it's driven by the parent
(non-interior) fixed element with the **same leaf name** (e.g. parent `Model/orifice_area`
drives interior `Model/Prob_StormwaterModel/orifice_area`).

That inference only fits some submodels. Measured coverage of `interface.inputs`:

| model | submodel | inputs | interior | leaf-bound |
|---|---|---|---|---|
| `designoptimization` | Prob_StormwaterModel | 2 | 2 | **2** ✓ |
| `oil_sands_production` | Oil_Sands_Model | 10 | 3 | 3 |
| `dynamicoptimization` | SubModel1 | 1 | 0 | 0 |
| `probabilisticoptimization` | SubModel1 | 1 | 0 | 0 |
| `wgen_par` | Empirical | 6 | 0 | 0 |
| (+7 more) | | | 0 | 0 |

For most submodels `interface.inputs` names elements that are **not interior** (`interior=0`) —
so the input isn't an interior placeholder to override; it appears to name a parent element
directly. **The convention is inconsistent**: sometimes an interior placeholder (drive it),
sometimes a direct parent reference (different semantics).

**Two ways forward — your / the schema owner's call:**
1. **Standardize the convention** so emit always uses one shape (e.g. interface inputs are always
   interior placeholders, driven by a leaf-matching parent element — the `designoptimization`
   pattern). Then the engine's leaf-name inference is correct everywhere and no schema change is
   needed.
2. **Encode the binding explicitly** in the schema — e.g. `interface.inputs` becomes
   `[{ input: <interior id>, from: <parent id> }]` — removing the leaf-name guess. This is a
   small schema round (like the earlier submodel_stat one); the engine would consume the explicit
   `from` instead of inferring. Recommended if leaf names can collide or the direct-parent-
   reference cases are legitimate and need distinguishing.

The engine's current leaf-name inference is marked in `submodel_v2.rs` as replaceable, so
switching to an explicit binding later is a localized change.

## Priority

Gap 1 (`designoptimization.total_cost` real AST) is the highest-value single fix: it's the one
change that lets a real probabilistic-optimization objective be non-constant, which is the whole
point of the feature. Gap 2 is a correctness/consistency decision that can follow.

## Self-check for the next regeneration

- No `submodel_stat`-referenced output is a `literal 0.0` / `sum_array([0,0])` stub (Gap 1).
- Every `submodel_stat` `output` id resolves to a real element.
- `interface.inputs` follows one consistent convention (Gap 2) — flag which one you land on.
