# NamedArray — engine design & implementation plan

**Goal.** Give the runtime `Value` **axis identity** — a labeled n-dimensional
array — so that align-by-name broadcast, label subscript, and across-realization
reductions become one uniform algebra instead of three special-cased mechanisms.
This is the keystone the Analytica work converges on (see
[ANALYTICA_TRANSLATION_STATUS.md](ANALYTICA_TRANSLATION_STATUS.md) §9.3), and it
is independently the engine's biggest simplification.

This is a **plan**, not a diff. It scopes the change surface, sequences it into
individually-shippable phases that preserve bit-identity at each step, and names
the risks. No code here changes behavior.

---

## 1. What it unlocks (and what it is not)

**Unlocks** — each of these becomes a thin rider once the type exists:

- **Intelligent-array broadcast** — `a[Platform,Year] * b[Year,Scenario]` aligns
  on `Year`, outer-products the rest. Today `zip_with` truncates positionally.
- **Label subscript** — `x[Dim='label']` = index one axis by label. The blocker
  the translation work stalled on.
- **≥2-D tables** — Platform's `[Attribute,Option]` data, GoldSim vectors/matrices.
- **Mid-graph sample statistics** — `Mean(x)`, `Probability(x>t)` as `reduce(Run)`,
  the same op as reducing any axis (the §2 gap), no submodel wrapper.
- **Retiring `#k`** — array results carry their axes instead of being flattened to
  `<id>#1..#N` and stitched back together.

**Non-goals** (explicitly out of scope for this effort):

- **Runtime-*dynamic* index membership** — an index whose length is computed
  mid-run (`Subset`, `SortIndex` producing data-sized sets). Pre-declared
  dimensions cover most of the corpus; defer data-length-varying axes.
- **A ragged / sparse array** — dense row-major only.
- **Autodiff, GPU, an external tensor lib** — `ndarray` is an *optional* later
  substrate (§7), not part of the core.
- **v1 (`model.rs`/`engine.rs`) changes** — land in v2 only (§8).

---

## 2. Current state (measured)

| Fact | Detail | Consequence for the plan |
|---|---|---|
| `Value` | `enum { Scalar(f64), Vector(Vec<f64>) }` (`eval.rs:37`) | add a labeled case; keep `Scalar` as the 0-D fast path |
| Broadcast | `zip_with` (Vector,Vector) is a **positional** zip truncated to `min(len)` (`eval.rs:69`) | this is the one function whose *semantics* change |
| Axis identity | **none on the value** — which dim a `Vector` is over lives in `outputs[].dimensions` + the `#k` naming convention | axis id must move onto the value |
| Array results | element with declared `dimensions` → expanded to `<id>#1..#N`, member `k = vec[k-1]`, missing → 0 (`engine_v2.rs:384,2248`) | this mechanism retires (Phase 5) |
| Reductions | **three** paths: `submodel_stat` (across Run, mid-graph, `eval.rs:419`), `results_spec` (across Run, boundary), `sum_array`/`mean_array` (across the array axis, `eval.rs:768`) | unify onto `reduce(axis, stat)` |
| Determinism | explicit `total_cmp` sorts, fixed fold order (`engine.rs:854,900`); bit-identity is a guarantee | canonical axis order + stable folds are mandatory |
| WASM | pure-Rust build (`build-wasm.sh`), one dependency (serde), no BLAS | dense `Vec<f64>`, no SIMD/BLAS reductions |
| Footprint | `Value::Vector`/`into_vec`/`as_scalar`/`zip_with`: **~117 sites** (eval 54, engine_v2 63, engine 7) | most are mechanical; ~a dozen are semantic |

The AST already has the array vocabulary (`Array`, `VectorMap`, `Index`,
`IndexRef`, `SubmodelStat`) via the shared `model::AstNode`, so **no new AST is
required** for broadcast/reduction — only the value model and the eval of those
existing nodes change. Label subscript adds one node (Phase 3).

---

## 3. Target type

```rust
/// A dense, row-major array over named axes. 0-D (no axes) is a scalar.
pub struct NamedArray {
    axes: Vec<Axis>,   // canonical order: sorted by axis id (determinism, §6)
    data: Vec<f64>,    // len == axes.iter().map(|a| a.len).product()
}
pub struct Axis { pub id: AxisId, pub len: usize }   // AxisId = interned u32 (or String v1)

pub enum Value {
    Scalar(f64),          // keep: the common path pays nothing
    Array(NamedArray),    // labeled n-d; 1-D replaces today's Vector
}
```

Design decisions:

- **Axis id + len on the value; labels off it.** Align-by-name needs only id+len
  at runtime. Labels are model-static (`DimensionDef.labels`); a label→position
  map is threaded via `EvalCtx` and used only by label subscript. Keeps the value
  small and cheap to clone.
- **Canonical axis order = sorted by id.** So `a*b` and `b*a` yield identical
  layout, and reductions fold in an operand-order-independent sequence →
  bit-identity survives (§6).
- **`Scalar` stays a distinct variant**, not a 0-axis `Array`, so scalar-heavy
  models keep a zero-allocation path and the wasm stays small.
- **`Vector` is retired, not kept** — a 1-D `Array` with one axis *is* the old
  vector. During migration, `into_vec()`/`as_scalar()` keep working as the
  compatibility shims so the ~100 mechanical sites don't all change at once.

Semantics to implement on the type:

- `broadcast_zip(a, b, f)` — union axes; each operand is repeated (strided) along
  axes it lacks; `f` applied elementwise over the aligned layout. Reduces to
  today's behavior when axis sets are equal or one side is scalar.
- `reduce(axis_id, fold)` — collapse one axis with a stable fold; drops that axis.
- `subscript(axis_id, pos)` — fix one axis at a position; drops that axis.
- `get(coords)` / iteration — for `index`, results surfacing.

---

## 4. Phased plan (each phase ships green, bit-identical unless noted)

The ordering guarantees the corpus stays bit-identical until the one phase that
*intends* a semantic change (Phase 2), which is gated behind multi-axis inputs
that don't exist in today's models.

### Phase 0 — introduce the type, no behavior change  *(size: M)*
- Add `NamedArray` + `Value::Array`; make `into_vec`/`as_scalar`/`map` handle it.
- Internally represent every value that is *today* a `Vector` as a 1-D `Array`
  with a single **anonymous** axis (a reserved id). `zip_with` on two anonymous
  1-D arrays = today's positional zip. **Golden test: the whole corpus is
  bit-identical.** This is pure plumbing; the ~100 mechanical sites compile via
  the shims.
- **Exit:** `cargo test` fully green, byte-identical results snapshot.

### Phase 1 — axis-tagged producers  *(size: M)*
- Tag outputs with real axis ids where the dimension is known:
  `vector_map{over}` → axis `over`; `array` literal / fixed vector → the declared
  output dimension; `index`/`submodel_stat`-sweep → the swept axis.
- Thread the `over` id into `EvalCtx` (it already tracks the index *stack*; add the
  axis *id* alongside).
- Still single-axis everywhere, so still bit-identical. Now values *know* their
  axis.
- **Exit:** results carry axis ids; corpus bit-identical.

### Phase 2 — align-by-name broadcast  *(size: M, the first semantic change)*
- Replace `zip_with`'s (Array,Array) arm with `broadcast_zip` (union + align by
  id). Single-axis-matching and scalar cases are unchanged; the *new* behavior
  only triggers when two operands carry *different* axis sets — which no current
  model produces, so the corpus stays bit-identical, but multi-axis arithmetic now
  works.
- **Exit:** a new 2-D broadcast test (`a[A]*b[B]` → `[A,B]`) passes; corpus
  unchanged.

### Phase 3 — label subscript + unified reductions  *(size: M)*
- Add one AST node `Subscript{ array, dim, label }`; eval resolves `label`→pos via
  the `EvalCtx` label registry, then `subscript(dim,pos)`. (Static labels first;
  variable-label selection is the non-goal runtime-index case.)
- Reimplement `sum_array`/`mean_array`/`min_array`/`max_array` as
  `reduce(axis, …)` over the value's (single) axis — same numbers, one code path.
- **Exit:** label-subscript test; array-reducer tests bit-identical.

### Phase 4 — `Run` as a reducible axis  *(size: L, highest value + risk)*
- Represent the Monte-Carlo sample as a real axis (`Run`, len = n_realizations) on
  values that vary across realizations. `submodel_stat` and the `results_spec`
  boundary reductions become `reduce(Run, stat)` on that axis; mid-graph
  `Mean`/`Probability`/`percentile` fall out with no submodel wrapper — closing the
  §2 gap.
- This reorders how realizations flow through eval (today the engine loops
  realizations outside eval; here a value can hold the sample axis). Gate behind a
  feature flag; migrate `submodel_stat` first, then the boundary, then retire the
  duplicate machinery.
- **Exit:** mid-graph `reduce(Run)` test; `submodel_stat` corpus bit-identical
  (same reducer, same order); the two native examples reproduce their numbers.

### Phase 5 — retire `#k`  *(size: M, cleanup)*
- Results surface reads `NamedArray` axes directly; keep `<id>#k` as a
  **compatibility view** (frontends/tests depend on it) generated from the axes.
- **Exit:** results identical; `array_members` bookkeeping deleted from the hot loop.

**Optional later — `ndarray` substrate.** Only if profiling shows the dense
`Vec<f64>` kernels are the bottleneck. `NamedArray.data` becomes `ArrayD<f64>`;
pure-Rust, BLAS feature-gated off for wasm, reductions kept deterministic. No API
change above the type. Not scheduled.

Capability delivered by phase: broadcast **P2**, label subscript / ≥2-D **P3**,
mid-graph sample stats **P4**. The Analytica translation fidelity jump lands at
**P3–P4**.

---

## 5. Change surface by file

| File | Phase(s) | What changes |
|---|---|---|
| `eval.rs` | 0–4 | `Value`, `NamedArray`, `zip_with`→broadcast, `Index`/`VectorMap`/`Array`/`SubmodelStat` arms, `Subscript` arm, array reducers, `EvalCtx` axis+label fields |
| `engine_v2.rs` | 0,1,4,5 | value construction, axis tagging, Run-axis flow, `#k` expansion → axis-aware surfacing |
| `model.rs` | 3 | one `Subscript` AST variant (shared v1/v2) |
| schema (`*.json`) | 3 | `subscript` in `ast_node`; version bump |
| `summary.rs`, `results_spec.rs` | 4,5 | reductions routed through `reduce(axis)`; result naming from axes |
| `engine.rs` (v1) | — | untouched (v1 has no dimensions; keeps `Scalar`/`Vector` shims) |

---

## 6. Determinism (the sharpest constraint)

Bit-identity is a shipped guarantee; the array work must not weaken it.

- **Canonical axis order** = sorted by (interned) axis id, applied on every array
  construction, so layout is independent of operand order and expression shape.
- **Stable folds** — reductions iterate in canonical index order with a plain
  sequential fold. **No** `rayon`/SIMD/pairwise reordering, no BLAS. If a fast path
  is ever added, it must be behind a "deterministic mode" that the default uses.
- **Golden snapshot test** run at Phases 0/1/3/5 asserts the full corpus is
  byte-for-byte unchanged; Phase 2/4 assert *the parts that existed before* are
  unchanged and only genuinely-new shapes differ.

---

## 7. Performance & wasm

- **Scalar fast path preserved** — `Value::Scalar` stays a bare `f64`; the vast
  majority of nodes never allocate. This is the single most important perf/wasm
  decision.
- **Dense row-major `Vec<f64>`** — predictable, wasm-friendly, no deps.
- **Broadcast materializes** the result (no lazy views in v1) — simple and
  deterministic; revisit only if memory shows up in profiling.
- **wasm binary** grows by the `NamedArray` methods only; no new crate in the
  default build.

---

## 8. Migration & compatibility

- **v2-only.** The v1 path stays on `Scalar`/`Vector` via the shims; it has no
  dimensions to generalize and no reason to churn. NamedArray is the v2
  convergence point (as the status doc already frames it).
- **Schema is additive** — only Phase 3 adds `subscript`; bump the minor version,
  old models still validate.
- **`#k` kept as a view** through Phase 5 so the frontend and existing tests keep
  working during and after the change.
- **Converter** (`ana_to_wasim.py`) is unaffected until we choose to emit v2 +
  `subscript`/multi-dim — a separate, later decision. This plan does **not** raise
  the mechanical-conversion fidelity on its own; it builds the substrate that a
  future v2-targeting converter (or hand-authoring) needs.

---

## 9. Test strategy

1. **Golden corpus snapshot** — serialize all example/fixture results to a hash;
   assert unchanged at each bit-identical phase. The regression backbone.
2. **NamedArray unit tests** — broadcast (shared/disjoint/scalar axes), reduce
   (each stat, canonical order), subscript (hit/miss), get/iterate.
3. **New-capability integration** — a 2-D `[Attribute,Option]` MADA model
   (Platform-shaped) that today needs stubs; a mid-graph `reduce(Run)` model.
4. **The two native examples** (`eviu_native`, `platform_native`) as end-to-end
   guards — their numbers must survive Phase 4's reduction refactor.
5. **Determinism** — run twice, assert byte-identical; run with axes authored in
   swapped order, assert identical results (canonical-order proof).

---

## 10. Estimate, sequencing, open questions

**Effort** (relative; one engineer): P0 M, P1 M, P2 M, P3 M, P4 L, P5 M. P0–P3
deliver broadcast + label subscript + ≥2-D and are the natural first milestone;
**P4 (Run-as-axis) is the largest single step** and the one that closes the §2
gap — worth isolating as its own milestone with its own review.

**Recommended first milestone:** P0→P3 (type + broadcast + subscript + ≥2-D),
which makes multi-dimensional Analytica/GoldSim models representable and is fully
bit-identical on today's corpus. Decide on P4 separately once P0–P3 prove the type.

**Open questions to resolve before P4:**
- Does the sample (`Run`) axis live *inside* `NamedArray` for all uncertain
  values, or only where a reduction consumes it? (Perf: carrying `Run` on every
  value is n_realizations× memory. Likely: keep today's realization-loop for
  plain propagation, lift to a `Run` axis only under a reduction node.)
- Axis id interning — `String` (simple) vs `u32` (fast, stable order). Start
  `String`, intern if profiling demands.
- Label registry location — `EvalCtx` field vs a resolved-at-build side table.
  `EvalCtx` field is simplest and matches how `dimensions` (sizes) already flow.

---

*Companion to ANALYTICA_TRANSLATION_STATUS.md (§9). Grounded in a read of
`eval.rs`, `engine_v2.rs`, `model.rs`, `results_spec.rs`, `summary.rs` at this
commit. Nothing here is implemented yet.*
