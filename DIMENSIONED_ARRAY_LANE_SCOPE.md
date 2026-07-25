# Dimensioned array lane — scope

*Companion to [ARRAY_LANE_DESIGN.md](ARRAY_LANE_DESIGN.md). This is a **scoping**
document — the design and phasing for extending the opt-in array lane from the
flat-Monte-Carlo subset (Phases A–D, shipped) to models with **named dimension axes**.
It commits to a layout and a boundary, and phases the work.*

> **Status — the committed Analytica models now run on the lane, bit-identical.**
> `run_dim_lane` (in `array_lane.rs`) evaluates dimensioned models per realization via
> the shared `eval_ast`, so `vector_map`/`index`/`subscript`/`array`/reducers +
> elementwise + `run_stat` + **`submodel_stat`** + fixed arrays all work, reaching
> bit-identity the low-risk way (reuse the scalar evaluator, not a fused kernel —
> mirroring the flat lane's A→B sequencing). **`submodel_stat` is served by the existing
> `run_submodels` pre-pass** (a one-directional feed; submodels stay on the scalar
> engine). **Both `eviu_plane_catching_native` and `platform_decommissioning_native` are
> now eligible and produce results bit-identical to the scalar lane end to end**
> (`dim_lane_examples_v2.rs`: 234k / 176k values, 0 mismatches). The remaining item is
> the **fused coordinate kernel** — a pure *optimization*, the dimensioned analogue of
> Phase B.

## Why — the real next lever

The array lane (A–D) runs the flat-MC subset 8–27× faster than the scalar lane,
bit-identically. But the two committed Analytica re-solutions —
`tools/examples/eviu_plane_catching_native` and `platform_decommissioning_native` —
are **not** eligible, and the blocker is **not** the builtins gap Phase D closed. A
census of their expression ops:

| op | EVIU | Platform | meaning |
|---|---|---|---|
| `ref` | 22 | 32 | element reference |
| `call` | 6 | 18 | builtins (incl. array reducers) |
| `submodel_stat` | 4 | 9 | **reduce a submodel output** |
| `vector_map` | 3 | 1 | **comprehension over a dimension** |
| `index_ref` | 5 | 4 | current index inside a `vector_map` |
| `index` | 5 | 4 | **subscript into an array** |
| `array` | 0 | 3 | **array literal** |
| arithmetic / `if` / `gt` | 11 | 35 | already eligible |

Both models declare **dimensions** (EVIU: `Depart`, size 25, labelled) **and** contain
one **submodel** (`"kind": "submodel"`) read via `submodel_stat`. So unblocking them
needs three things the lane does not have yet: **dimensioned arrays**, **array
construction/indexing**, and **submodel-statistic reads**. This document scopes all
three; the dimensioned-array VM is the large piece.

## What already exists (reuse, don't rebuild)

The `NamedArray` substrate from the earlier engine work already encodes the exact
multi-axis semantics — the scalar lane just applies them **once per realization**:

- `Value::Array(Box<NamedArray>)`, `NamedArray { axes: Vec<Axis{id,len}>, data: Vec<f64> }`,
  **row-major** flat data (`eval.rs`).
- `broadcast_named(a, b, f)` — align-by-name multi-axis broadcast; result axes are the
  **union of input axes in canonical (sorted-id) order**; `operand_index` projects a
  result coordinate onto each operand's own axes (broadcasting over axes it lacks).
- `NamedArray::subscript(dim, pos)` — fix one axis at a position (label subscript).
- `reduce_data(data, init, fold)` — a **plain sequential fold** (`sum_array` = fold `+`,
  init 0.0, in member order). This is the determinism anchor.
- Scalar-lane eval arms already implemented for `VectorMap`, `IndexRef`, `Index`,
  `Subscript`, `Array`, `SubmodelStat`, and the array-reducer builtins.
- `submodel_v2::run_submodels(model, config)` — a **pre-pass** that runs each referenced
  submodel once and returns `HashMap<(submodel, output), Vec<f64>>` of per-realization
  samples. `submodel_stat` reduces these exactly like `run_stat` reduces a column.

The lane already carries `dim_labels` and `dimensions` maps in its `EvalCtx` (unused in
the flat subset). The reducers, broadcaster, and subscript are the same functions the
lane will call — so bit-identity is a matter of *reusing* them, not re-deriving them.

## Core design decision — Run-major columns + a coordinate-aware VM

**Layout.** A dimensioned element's column is one `NamedArray` whose axes are
`[Run, D₁, D₂, …]` (Run outermost), `data` row-major of length `n_real · ∏|Dᵢ|`. A
scalar element stays a `Run` column (or a broadcast scalar), i.e. the Phase-A–D
`ColData`. This generalizes "each value is a Run column" to "each value is a Run column
of cells."

**The fused kernel becomes coordinate-aware.** Today `Op::Col(slot)` reads `col[r]`.
For a dimensioned output, the loop iterates over the output element's **cells**
(`Run × its own axes`), and each operand read projects the output cell's coordinate
onto the operand's axes — precisely `operand_index`. Concretely, compilation computes,
per expression, the output axis signature = union of input signatures (align-by-name,
`broadcast_named`'s rule), and the VM iterates output cells with a mixed-radix
coordinate, reading each `Col` via the projection. Elementwise ops, `if`, and the
Phase-D builtins are unchanged per cell.

**Why this is bit-identical (validated).** With Run as an outer axis, the per-cell
elementwise values equal the scalar lane's per-realization per-cell values (same ops),
and an axis reduction reuses `reduce_data`'s same fixed-order fold. The spike
`dim_lane_spike_v2.rs` confirms it: a `vector_map`+elementwise+`sum_array` model,
recomputed Run-major from the scalar run's own draws, matches `to_bits()` across 2000
realizations × 4 cells, members included. Bit-identity here follows from *construction*
(reuse the reducers/broadcaster), the same argument that held for A–D.

## Per-construct plan

- **Elementwise over dimensions** (`+ - * / if` builtins, dimensioned operands): output
  axes = union of operand axes; VM projects each `Col` read. Reuses `broadcast_named`'s
  coordinate math, lifted into the fused loop.
- **Axis reductions** (`sum_array`/`mean_array`/`min_array`/`max_array`/…): collapse
  **all** axes of the argument to a scalar per realization via `reduce_data` (the scalar
  lane's current behaviour — axis-selective reduction is still deferred there too). In
  the lane: for each Run cell, fold that realization's `∏|Dᵢ|` slice. Fixed order,
  deterministic.
- **`vector_map` over `D`** synthesizes axis `D`: the result gains axis `D`, and
  `index_ref(row)` inside the body reads the **current `D` coordinate** (1-based) rather
  than a runtime stack. Nested `vector_map` (col/row) → two axes. This compiles to "add
  axis `D`; lower `index_ref` to the coordinate component."
- **`index` / `subscript`** fix an axis: `subscript(dim=label)` resolves the label via
  `dim_labels` to a position and drops that axis (reuse `NamedArray::subscript`);
  `index[array, i]` selects a 1-based member (runtime-constant index first; dynamic
  per-cell index is a later item — see non-goals).
- **`array` literal** builds a small anonymous-axis column from scalar elements.
- **`submodel_stat`** — the clean boundary. Run the existing `run_submodels` pre-pass
  (unchanged, scalar), then reduce `submodel_outputs[(sub,out)]` with the same reducers.
  This is a **one-directional feed from a pre-pass**, *not* the interleaved hybrid
  ARRAY_LANE_DESIGN.md deferred — it never splices into the scalar main loop, so it
  respects "never touch the scalar lane." (Submodels themselves stay on the scalar
  engine; only their statistics enter the lane.)

**Results surfacing.** A dimensioned element saves `final_values` as its scalar collapse
(`v.as_scalar()`) plus one `<id>#k` column per member (`engine_v2` array-member records).
The lane fills each `#k` from cell `k` of the element's column — the spike already checks
this reproduction.

## Eligibility expansion

`eligible` drops the blanket "no dimensions" / "no submodels" rejections and instead
accepts a model iff every element is: a scalar node rule (as today), **or** a
dimensioned expression whose ops are all lane-compilable (elementwise + `if` + Phase-D
builtins + `vector_map`/`index_ref`/`index`/`subscript`/`array` + array-reducer builtins
+ `run_stat`/`submodel_stat`), over declared dimensions of known size. Anything else
(stocks, state-machine rules, dynamic indices, unresolved axes) keeps the whole model on
the scalar lane — the same conservative gate, widened.

## Determinism & memory

- **Determinism:** all reductions stay fixed-order `reduce_data`; canonical (sorted-id)
  axis order from `broadcast_named` is preserved so `a⊕b` and `b⊕a` share a layout. No
  SIMD reassociation.
- **Memory:** a dimensioned column is `O(n_real · ∏|Dᵢ|)` live — for EVIU's `Depart=25`
  at 100k realizations, 2.5M f64 (20 MB) per such element. **Chunk the Run axis** (process
  R realizations at a time; the fused loop is chunk-friendly) to bound peak memory. This
  is the same chunking ARRAY_LANE_DESIGN.md notes for large flat N, now load-bearing.

## Phasing

| Phase | Deliverable | Size | Risk |
|---|---|---|---|
| 1 ✅ | **Correctness-first dimensioned lane** — `run_dim_lane` evaluates dimensioned elements per realization via the shared `eval_ast` (so `vector_map`/`index`/`subscript`/`array`/reducers/broadcast/fixed-arrays all work), mirrors the scalar draws, surfaces `<id>` + `<id>#k`, and reduces `run_stat` two-pass. *Shipped, bit-identical.* | L | Low–Med |
| 2 ✅ | **`submodel_stat`** via the `run_submodels` pre-pass boundary (a one-directional feed, not a loop splice; submodel interior elements are also evaluated in the parent context, as the scalar lane does). *Shipped — **EVIU + Platform run end-to-end, bit-identical** (`dim_lane_examples_v2.rs`).* | M | Med |
| 3 ◑ | **Speed.** *Shipped: the safe per-realization optimizations — reuse one `outputs` map across realizations (constants seeded once, no per-realization rebuild or array re-clone), hoist the topo-ordered expression list (no O(elements) scan per id) and the `#k` member keys. **~2.3× vs the scalar engine, bit-identical** (`dim_lane_examples_v2::bench_dim_lane_vs_scalar`).* **Deferred: the fused coordinate kernel** (Run-major `[Run, D…]` columns + `operand_index` projection) — measurement showed the residual cost is `eval_ast`'s tree-walk and per-realization `NamedArray` allocations, which only a parallel coordinate VM removes; see the note below. | L | Med |
| — | Dynamic (per-cell runtime) indices; axis-selective reduction; closure/bytecode compiler | L | later |

*(The original 0–4 plan folded together: reusing `eval_ast` collapsed the separate
"column type / coordinate kernel / axis-reductions / indexing" phases into one
correctness-first slice, deferring the coordinate kernel to a pure optimization pass.)*

### On the fused coordinate kernel (deferred, with evidence)

Phase 3's fusion was measured before it was built. The correctness-first dim lane is
**already ~2.3× faster** than the scalar engine (it skips the full engine's per-step /
importance / history / event machinery and runs `run_stat` single-structure), and the
cheap per-realization optimizations above are in. Profiling what's left points at
`eval_ast`'s tree-walk (per-node `HashMap<String,Value>` ref lookups) and the
per-realization `NamedArray`/`Vec` allocations that `vector_map`/broadcast/reducers
make. Removing those means **not** reusing `eval_ast` — a parallel, slot-indexed,
coordinate-aware VM — because `eval_ast`'s `&HashMap<String,Value>` context is shared
with the scalar lane and can't be swapped for a slot vector without touching it.

That VM is real work at real risk, and unlike the flat lane (pure scalar arithmetic →
8× from bytecode) the dimensioned models carry irreducible per-realization array
materialization (`vector_map` builds a 25-wide array each realization) plus gather/index
ops (`argmin_array`, `get_element`, `index`, `subscript`) that a coordinate kernel must
reproduce bit-for-bit — so the realistic ceiling is more like 3–5×, not 8×. Against a
banked, bit-identical 2.3× with the correctness goal already met, the marginal ~2× is a
weak return for a large, high-risk build. **Recommendation: stop here unless a specific
dimensioned model is a measured bottleneck**; if it is, build the coordinate VM for the
elementwise + reduction + `vector_map` subset first (the array hot path) and keep
`eval_ast` per-realization for the gather/index tail.

Each phase is validated the same way as A–D: **golden-diff the scalar lane byte-identical**
without the flag, and assert the lane **bit-identical** to the scalar lane with it — the
capstone being the EVIU and Platform native models run scalar-vs-lane. Ship phase-by-phase;
0–2 already deliver the dimensioned-arithmetic win for the (large) no-submodel class.

## Risk register

- **VM generalization (Med).** The coordinate projection is the substantive new code, but
  its semantics are `broadcast_named`/`operand_index` — already written and unit-tested.
  The lane lifts them into the fused loop; correctness is a reuse argument, not new math.
- **Bit-identity of reductions (Low).** `reduce_data` is a plain fold; the lane calls the
  same function. Validated by the spike.
- **Submodel boundary (Low–Med).** Distinct from the deferred interleaved hybrid: submodels
  are a pre-pass, consumed one-directionally, so no scalar-loop surgery. The risk is only in
  matching `submodel_stat`'s reducer/arg handling, which is a direct copy of the scalar arm.
- **Memory blowup (Med).** Real: `O(Run·∏dims)`. Mitigated by Run-axis chunking, which must
  land in Phase 1, not be deferred.
- **Scope creep into the scalar hybrid (watch).** The dimensioned lane must keep the same
  all-or-nothing model gate as A–D. Per-*subgraph* scalar/array interleaving stays deferred
  (ARRAY_LANE_DESIGN.md) — the submodel pre-pass is the *only* cross-lane feed, and only
  because it is not interleaved.

## Bottom line

Unblocking the committed Analytica models is a **4-phase** effort (Run-major dimensioned
columns → coordinate-aware kernel → reductions/`vector_map` → indexing → `submodel_stat`),
all inside `array_lane.rs`, all reusing the existing `NamedArray` semantics for
bit-identity, and none touching the scalar main loop. The core layout+reduction claim is
spike-validated. Phases 0–2 already pay off for dimensioned no-submodel models; phase 4 is
what carries EVIU and Platform across the line.
