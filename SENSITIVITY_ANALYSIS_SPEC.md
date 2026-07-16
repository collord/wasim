# Spec: Sensitivity Analysis (runtime-configured, not schema-encoded)

**Status:** design spec, for a fresh implementation session.
**Scope:** a runtime sensitivity-analysis capability — vary one or more model inputs
across a range, observe how a chosen result responds. Reuses the evaluation harness
built for optimization. **No schema change, no emit-side dependency.**

## Why NOT in the schema

Optimization *is* authored model content — a `COptimization` object lives in the
`.gsm` (variables, objective, bounds the modeler saved), so it belongs in the schema
and the transpiler populates it. **Sensitivity analysis is not model content** — in
GoldSim it is enabled and configured *live in the runtime* against an already-loaded
model; nothing about it is persisted in the `.gsm`. Encoding a `sensitivity` block in
the schema would invent persisted state for a runtime action: emit would have nothing
to populate it from (GoldSim's sensitivity/scenario managers were explicitly excluded
as "run infrastructure" in the 0.7.0 changelog), and the field would sit empty across
the entire corpus.

So the configuration flows **UI → engine call → results**, exactly like the existing
"Run Simulation" flow — never touching `model.json` or the transpiler. This is also
strictly smaller than the optimization round: an engine capability method + a frontend
panel, with no schema/parser/emit-spec work.

## What it does

Given a loaded model, let the user pick, at runtime:
- one or more **input variables** — any editable element (an editable `fixed` node or a
  `sample` node; these are exactly what the Dashboard already enumerates), each with a
  **range** (`lower`, `upper`) and a **step count**, around a **base** value;
- a **result** — any element output, optionally reduced by a Monte-Carlo **statistic**
  (mean / percentile / sd / cumulative_prob), the same statistic set optimization uses;
- a **method** — see below.

Run the model once per sweep point and report how the result responds.

### Methods (start with the first two)

- **One-at-a-time (line sweep):** vary each variable across its `steps` points holding
  the others at `base`; produce one response curve `(input value → result)` per variable.
  The primary view; directly answers "how does the result move as I change X?"
- **Tornado:** for each variable, evaluate the result at its `lower` and `upper`
  (others at base); the bar is the result swing `|result(hi) − result(lo)|`, sorted
  descending — ranks inputs by influence. A cheap special case of one-at-a-time
  (2 points/variable).
- **(Later) Monte-Carlo importance:** across an ensemble, rank inputs by correlation
  with the result. Defer — needs an ensemble sampler over the chosen inputs.

## Engine side (reuses the optimization harness verbatim)

New module `engine/src/sensitivity_v2.rs`. The evaluation of a single sweep point is
**identical to an optimization candidate** — lift/share, do not duplicate:

- **`set_variable(model, id, value)`** (`optimize_v2.rs:36`) — set an input for a point.
  (Currently `fn`-private in optimize_v2; make it `pub(crate)` or move to a shared
  `eval_harness` module both use.)
- **Evaluate:** clone model → set variables → `ModelGraphV2::build` → `engine_v2::run`
  → read `results.elements[result_id].final_values` → reduce. This is `optimize_v2.rs`
  `evaluate` (`:72`) minus the maximize/minimize flip — share the body.
- **Reduce:** `reduce_objective(samples, statistic)` (`optimize_v2.rs:56`) + the stats
  helpers `mean`/`percentile`/`std`/`cumulative_prob` (`engine.rs:787-811`).
- **SubModel pre-pass:** happens inside `engine_v2::run` automatically
  (`submodel_v2::run_submodels`), so a **probabilistic** sensitivity (result depends on
  a submodel statistic) re-runs the nested Monte-Carlo per sweep point for free — same
  as optimization candidates. No extra work.

Entry point + result type (mirror `optimize`/`StudyResults` in `optimize_v2.rs`):
```rust
pub struct SensitivitySpec {
    pub result: ResultRef,            // { element_id, statistic: Option<ObjectiveStatistic> } — reuse Objective's shape sans direction
    pub variables: Vec<SweepVar>,     // { element_id, lower, upper, base, steps }
    pub method: SensitivityMethod,    // OneAtATime | Tornado
}
pub struct SensitivityResults {
    pub base_result: f64,
    pub curves: Vec<VarCurve>,        // per variable: Vec<(input, result)> (one_at_a_time)
    pub tornado: Vec<TornadoBar>,     // per variable: { element_id, low, high, swing } (tornado)
}
pub fn sensitivity(model: &Model, spec: &SensitivitySpec, config: &RunConfig)
    -> Result<SensitivityResults, EngineError>
```
Deterministic given the seed (submodel runs re-seed per call, as today).

## wasm bridge

Add a method on `WasmEngine` mirroring `run_json` (`wasm.rs:65`):
```rust
pub fn sensitivity_json(&self, spec_json: &str) -> Result<String, JsError>
```
Parse the runtime-supplied spec, call `sensitivity(&self.model, &spec, &config)`,
serialize `SensitivityResults`. Reuses the display-unit boundary conversion pattern
already in `run_json` for the result values if desired.

## Frontend

- **New tab** `'sensitivity'`: add to the `Tab` union (`store.ts:35`), the `TABS`
  registry (`App.tsx:8`), and the content dispatch (`App.tsx`). A `SensitivityTab`
  component alongside the existing tabs.
- **Config panel** (transient UI state, NOT persisted to the model): pick variables
  from the editable elements the Dashboard already computes
  (`summary.elements.filter(e => e.editable)`, `DashboardTab.tsx:251`); each row gets
  lower/upper/steps inputs (default range from the element's `bounds` if present, else
  ±50% of current value; base = current value). Pick a result element + optional
  statistic. Pick method (one-at-a-time / tornado).
- **Run action + worker plumbing:** a `runSensitivity(spec)` store action mirroring
  `run` (`store.ts:167`); new `MainToWorker` variant `{ type: 'run_sensitivity', spec }`
  (`protocol.ts:9`) and a `WorkerToMain` result variant (`protocol.ts:14`); worker case
  in `sim.worker.ts` calling `engine.sensitivity_json(...)`. The engine `.d.ts`
  (`frontend/src/engine.d.ts`) gains the method signature.
- **Results view:** one-at-a-time → a line chart per variable (reuse the ResultsTab
  charting, `x = input value`, `y = result`); tornado → a horizontal bar chart sorted
  by swing. A base-case marker on each curve.

## Persistence (optional, later — and still not the schema)

If a user should save/reload a sensitivity configuration, that's a **frontend**
concern — a saved-view or the existing "Save parameters" mechanism (`SaveParamsButton`
in DashboardTab) — a sidecar file, never `model.json`.

## Probabilistic sensitivity works end-to-end (GoldSim "Probabilistic Optimization")

The `probabilisticoptimization.json` scenario — GoldSim's probabilistic-optimization
example (support.goldsim.com article 360047030994) — sweeps correctly and is the reference
case for a *probabilistic* result. Its `Cost = Slope + (P95(TheSystem) − 10)²` reduces a
nested Monte-Carlo distribution (the 95th percentile of a Weibull whose shape = `Slope`,
scale = `10/Γ(1 + 1/Slope)`) inside the expression via `submodel_stat`. The GoldSim article
runs its **Sensitivity Analysis** dialog with Result=`Cost`, one Independent Variable
`Slope` (Lower=5, Central=6, Upper=10), 11 points, and reports **Central Value = 14.66** and
an **X-Y curve bottoming at ≈12.69** — the optimum, which its optimization run then matches.

Reproducing that exact config in our apparatus (one-at-a-time, `Slope ∈ [5,10]`, base=6,
11 steps, seed 42) yields **Central (Slope=6) = 14.55** and a **U-curve minimum = 12.64 at
Slope≈8.5** — matching GoldSim's 14.66 / 12.69 to within Monte-Carlo sampling noise (stable
across 100→5000 realizations). Over the full optimization box `Slope ∈ [1,25]` the curve
runs ~390 (Slope=1) → ~12.6 (Slope≈9) → ~25 (Slope=25). The nested Weibull Monte-Carlo
re-runs per sweep point, so the P95 genuinely responds to the swept input. This depends on
the formula-valued distribution params + `gamma` builtin (schema 0.8.5).

GoldSim's dialog offers three outputs on this config; our mapping: **X-Y Function Chart** →
one-at-a-time line sweep (✓); **Tornado Chart** → tornado method (✓); **Result Data** → the
raw `curves`/`tornado` arrays behind the charts (✓). GoldSim's "Use quantiles for Stochastic
elements" option (sweep a *sample* node by quantile rather than value) has no analog — we
sweep fixed scalars only (see divergence #1). Not covered: GoldSim's multi-variable
response-surface view; our one-at-a-time is strictly one curve per variable.

**Known limitation (shared with optimization, not caused by this feature):** a probabilistic
sensitivity whose result depends on a submodel with an **`external` distribution** flatlines —
the engine samples `external` as 0.0, so the submodel statistic is constant regardless of the
swept input. This is the `external`-distribution / decode gap in `SUBMODEL_EXECUTION_FINDINGS.md`,
independent of sensitivity. (`probabilisticoptimization`'s `TheSystem` is Weibull, not
`external`, so it is NOT affected — earlier drafts of this spec misattributed it.)

## Verification

Engine + frontend, no schema:
1. **Engine unit tests** (`engine/tests/sensitivity_v2.rs`): a hand-authored model with
   a known response, e.g. `y = 2·x + 3` swept x∈[0,10] steps=5 → curve `[(0,3),(2.5,8),
   (5,13),(7.5,18),(10,23)]`; tornado on a 2-var model orders by the larger coefficient;
   a probabilistic result via a submodel-stat reduces per point.
2. **wasm**: `cargo check --target wasm32-unknown-unknown --features wasm`; rebuild
   `pkg/` (`./build-wasm.sh`) + `frontend` (`npm run build`) — the recurring wasm-drift
   gotcha; the browser loads the deployed artifact, not the source.
3. **Frontend**: `tsc --noEmit` + `npm run build`; a Playwright e2e (extend the existing
   `e2e/` harness) that loads a deterministic corpus model, configures a one-at-a-time
   sweep, runs it, and asserts a non-degenerate curve + no page error.

## Reuse checklist (so this stays a thin layer, not a parallel impl)

- Share `set_variable` + the candidate-evaluate body with `optimize_v2` (extract a
  small `eval_harness` if cleaner than cross-module `pub(crate)`).
- Reuse `reduce_objective` + `mean`/`percentile`/`std`/`cumulative_prob`.
- Reuse the submodel pre-pass (automatic via `engine_v2::run`).
- Reuse the editable-element enumeration + charting on the frontend.
The genuinely new code is: the sweep loop (vs. Box's-complex search), the
`SensitivitySpec`/`SensitivityResults` types, the wasm method, and the Sensitivity tab.

## Implementation notes (as-built — divergences from the design above)

Two corrections surfaced against the actual code and are what shipped:

1. **Sweepable inputs are fixed-scalar nodes, NOT `summary.elements.filter(e => e.editable)`.**
   `set_variable` (now in `eval_harness`) only sets a `Fixed { Scalar }`; `is_editable`
   also returns true for **sample nodes**, which cannot be swept by one number. More
   decisively, **0 of 162 corpus models mark a fixed scalar `editable: true`** (verified) —
   the `editable` flag is a curatorial "surface for hand-editing" marker that the corpus
   only ever sets via sample nodes. Filtering on `editable` would show "no sweepable inputs"
   on every real model. The frontend therefore filters on `value_rule === 'fixed'` with a
   finite scalar value (any `editable`) — the same class of node `optimize_v2` already
   sweeps as its variables. With this, **92/162 models are sweepable**.
2. **Shared harness returns `Result`, not `f64::INFINITY`.** `evaluate_point` in the new
   `eval_harness` module propagates model/run errors; the optimizer wraps it back to `∞`
   for its search, while the sweep surfaces a failed point as an error rather than a silent
   flat curve. `reduce_objective` moved to `eval_harness::reduce`. The result statistic set
   is the existing `ObjectiveStatKind` (mean/percentile/peak/valley/sum) — `sd` /
   `cumulative_prob` are NOT wired into the reducer and were not added.

Result values cross the same display-unit boundary as `run_json` (in the target element's
display unit); input values stay canonical (the UI supplies them canonically). Tornado
swing scales by `|factor|` only (the offset cancels in a difference).
