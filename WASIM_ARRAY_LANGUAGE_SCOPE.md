# WASiM array-language primitives — scope

*Scopes the **relational array/index operations** that deterministic Analytica models
lean on but WASiM's engine doesn't yet have. Motivated by a concrete case study — the
Lumina "Marginal Abatement (home heating)" model — whose economic core translates and
runs today, but whose ranking/cumulative/graph layer stubs out. Companion to
[DIMENSIONED_ARRAY_LANE_SCOPE.md](DIMENSIONED_ARRAY_LANE_SCOPE.md) (which added the
`Run × named-axis` execution) and [WASIM_ARRAY_COMPREHENSION_GAP.md](WASIM_ARRAY_COMPREHENSION_GAP.md)
(which added `vector_map`/dimensions). This is the layer above those.*

## Motivation — the case study

`Marginal_abatement_home_heating.ana` is **deterministic** (no distributions), 10
nodes. The `ana_to_wasim` converter + engine handle **5 of 10 cleanly**, and a native
build reproduces Analytica's economics to the cent:

- `Action` (index/dimension, 8 members), `Amount_of_reduction` / `Gross_Cost_of_action`
  (`Table(Action)(…)` → fixed arrays), `Net_cost_of_action`, `Marginal_cost_of_red` —
  all dimensioned elementwise arithmetic, which the engine + dimensioned array lane run.
- `NPV_per_MBTU` is a constant (`Round(Σₜ₌₀³⁰ 5/1.1ᵗ, 10) = 50`).

The **other 5 nodes are the "abatement graph" layer**, and each stubs on a missing
primitive:

| stubbed node | Analytica | missing primitive |
|---|---|---|
| `Sorted_action` | `SortIndex(marginal_cost)` | **sort → permutation index** |
| `Cum_reduction` | `cumulate(amount[Action=Sorted_action], …)` | **cumulate (prefix scan)** + **gather / reindex-by-index** |
| `Marginal_Abatement_c` | `if @Cumulative=@Sorted+1 … then marg[Action=Sorted] else null` | **ordinal `@`**, **null**, gather |
| `Cumulative_reduction` | `Concat(0, CopyIndex(Cum_reduction))` | **dynamic/computed index** |

WASiM today has dimensioned **elementwise** arithmetic, **collapsing reducers**
(`sum/mean/min/max/argmin/argmax/get_element/dot_product/…`), the **comprehension**
(`vector_map`/`index_ref`), **label subscript** (`array[dim=label]`), and array
literals. What it lacks is the **relational array-language layer**: sort, scan, gather,
ordinal position, and runtime-computed indexes. That layer is *what this class of model
is made of*, so closing it unlocks far more than one model.

## The primitives, scoped

Each is a deterministic, pure array→array (or array→scalar) op — the same shape as the
reducers that already exist, so they slot into `BuiltinFn` + an `eval.rs` arm, and the
**correctness-first dimensioned lane inherits them for free** (it reuses `eval_ast`; no
separate lane implementation). Bit-identity is by construction: sorts use `total_cmp`
with a stable index tie-break (the engine's existing determinism rule), scans run in
fixed axis order.

| # | primitive | proposed surface | semantics | shape | tractability |
|---|---|---|---|---|---|
| 1 | **sort / rank** | `sort_array(x)`, `rank_array(x)`, `sort_index(x)` | ascending sort of a 1-axis array; `rank` = 1-based rank per element; `sort_index` = the permutation (1-based source positions) | array→array (same axis) | **Low** — a `total_cmp` sort + stable tie-break; pure, deterministic |
| 2 | **cumulate / scan** | `cumulate(x)`, `cumproduct(x)` | running sum/product along the axis, `out[i]=Σ_{j≤i} x[j]` | array→array (same axis) | **Low** — fixed-order prefix fold |
| 3 | **gather / reindex** | `gather(x, idx)` for `x[Dim=perm]` | `out[i] = x[idx[i]-1]` (1-based); result rides the `idx` axis | (array, index-array)→array | **Med** — vector generalization of `get_element`; axis of result is the index's |
| 4 | **ordinal `@`** | `positions(Dim)` / `index_ref` | 1-based position along an axis; already available inside `vector_map` via `index_ref` — needs a standalone form for the implicit-array `@I` idiom | →array-over-axis | **Low–Med** — surfaces existing coordinate machinery |
| 5 | **null** | `NaN` sentinel + skip | Analytica `null`/empty-cell → `f64::NaN`; propagates; results/plot layer skips NaN cells | value-level | **Low–Med** — representation + reducer/plot NaN policy |
| 6 | **dynamic / computed index** | *(structural — see below)* | build an index from runtime values (`SortIndex`, `Concat`, `CopyIndex`) | — | **High** — breaks the static-dimension model |

### The structural gap: computed indexes

Primitives 1–5 are additive builtins. The real depth is #6: WASiM **dimensions are
declared and static** (fixed id, size, labels), but Analytica routinely builds indexes
at runtime — `SortIndex` *is* an index (a permutation), `Concat(0, CopyIndex(x))`
splices a new one. Three ways to absorb this, increasing cost:

- **(A) Anonymous-axis materialization.** Treat a computed index's result as an array on
  an *unnamed* axis of runtime length. Enough when the index is only consumed
  positionally (most `cumulate`/`gather` chains). Cheapest; no dimension-model change.
- **(B) Derived index.** A first-class but *restricted* computed index: a permutation,
  subset, or relabeling **of an existing declared index** (covers `SortIndex(x)` exactly
  — it's a permutation of `Action`). A bounded dimension-model extension.
- **(C) General computed indexes.** Arbitrary runtime index construction (`Concat`,
  `CopyIndex`, index arithmetic). Large; a genuine array-language feature.

**Recommendation:** do **(A)+(B)**. `SortIndex → sort_index` as a derived permutation
(B) plus positional gather/cumulate on an anonymous axis (A) express the entire
Marginal-Abatement graph *except* the cosmetic `Concat(0, …)` zero-origin, which is
plot framing. Defer (C).

## How the case study maps once 1–5 (+A/B) land

```
sorted      = sort_index(marginal_cost_of_red)          # #1 (B) permutation of Action
amt_sorted  = gather(amount_of_reduction, sorted)       # #3 reorder amount by rank
cum_reduce  = cumulate(amt_sorted)                       # #2 prefix sum
marg_sorted = gather(marginal_cost_of_red, sorted)       # #3
# staircase: vector_map over (step, rank) with index_ref for the @-ordinals, NaN for the gaps (#4,#5)
```

That is the whole model minus the `Concat(0,…)` x-origin — i.e. **9.x of 10 nodes**,
with only a plotting nicety left to (C).

## Phasing

| Phase | Deliverable | Size | Risk |
|---|---|---|---|
| 1 ✅ | **`sort_array`/`rank_array`/`sort_index` + `cumulate`/`cumproduct`** — self-contained `BuiltinFn`s + shape-preserving `eval.rs` arms; deterministic (`total_cmp` with stable lowest-source-index tie-break, fixed-order scan). *Shipped: engine builtins + `map_shape`/`argsort` helpers; converter maps `SortIndex`/`Sort`/`Rank`/`cumulate`/`cumproduct` (dropping the index arg); `array_language_v2.rs` checks all five on the scalar **and** dimensioned array lanes bit-identically, and `sort_index` reproduces the Marginal-Abatement `Sorted_action` exactly (`[1,3,6,8,2,7,4,5]`).* | M | Low |
| 2 | **`gather`/reindex-by-index** (the `x[Dim=perm]` form) + **standalone ordinal** (`positions`/`@`) — axis-aware; extends `Subscript`/adds an AST node. | M | Med |
| 3 | **`null`→NaN** value policy: propagation + reducer/results NaN-skip + plot gaps. | M | Low–Med |
| 4 | **Derived permutation index** (B): `sort_index` result usable as an index for gather/label alignment, not just positionally. | M–L | Med |
| — | General computed indexes (C): `Concat`/`CopyIndex`/index arithmetic. | L | later |

Phase 1 alone unblocks ranking + cumulative (the bulk of these models); 2–3 finish the
graph shape; 4 is polish; C is deferred. Each phase is validated the established way:
golden-diff the scalar lane, and (for dimensioned models) assert the array lane
bit-identical — the ops live in `eval_ast`, so both lanes share one implementation.

## Determinism & fit

- **Sorts:** `total_cmp` with a stable **lowest-source-index** tie-break — the same rule
  the engine already uses for percentile/`argmin` ties, so results are reproducible and
  order-independent of input permutation.
- **Scans:** fixed axis order (row-major), no reassociation — consistent with the
  bit-identity ethos of the array lane.
- **One implementation, both lanes:** because these are `eval_ast` ops, the scalar lane
  and the correctness-first dimensioned lane get them from the same code; the fused
  coordinate kernel (deferred, DIMENSIONED_ARRAY_LANE_SCOPE §3) would later need its own
  lowering only if it's ever built.

## Bottom line

The Marginal Abatement case study cleanly separates what WASiM *is* (a dimensioned
Monte-Carlo simulator with elementwise arithmetic + reducers) from what this class of
Analytica model *needs* (a relational array language: sort, scan, gather, ordinal,
computed indexes). Five of the six missing pieces are low/medium-risk additive builtins
that both execution lanes would share; only fully-general computed indexes are a large,
separable effort. **Phase 1 (sort + cumulate) is the highest-leverage single step** — it
turns "5 of 10 nodes" into most of the graph and generalizes to every rank/cumulative
Analytica model, not just this one.
