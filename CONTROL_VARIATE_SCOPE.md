# Scope: Cross-Realization Bivariate Reducers (Control-Variate Support)

**Status:** Phases 1–4 **all implemented**. Phase 3 delivered multiple-control regression
(`run_regress`) *and* split-sample `b` (`run_split_beta`) — the latter on a new **per-realization
injection** channel (`EvalCtx.run_vecs`) that the earlier draft had flagged as the missing mechanism.
Phase 4 added `submodel_stat2` (bivariate reduction inside a submodel). Per-phase "what landed"
summaries at the end of this doc.
**Motivation:** [`OPTIONS_PRICING_EFFICIENCY.md`](OPTIONS_PRICING_EFFICIENCY.md) §5 — control variates
could not be expressed because the optimal coefficient needs a **covariance between two
elements across realizations**, and `run_stat` reduces a **single** element.

---

## 1. The gap, precisely

A control-variate estimator replaces the plain Monte Carlo estimator `Y` (e.g. a discounted
payoff) with

```
Y_cv = Y − b · ( C − E[C] )
```

where `C` is a **control** with a *known* expectation `E[C]`. `Y_cv` is unbiased for any `b`
(because `E[C − E[C]] = 0`), and its variance is minimized at

```
b* = Cov(Y, C) / Var(C)          (single control)
b* = Σ_C⁻¹ · σ_YC                 (multiple controls; Σ_C = control covariance matrix)
```

giving `Var(Y_cv) = Var(Y)·(1 − ρ²_{Y,C})`. A control correlated with `Y` at ρ = 0.95 cuts
variance ~10×. This is Glasserman §4.1 — often the single highest-leverage variance-reduction
technique in derivative pricing.

**Why WASiM can't express it today.** `b*` requires `Cov(Y, C)` and `Var(C)` — statistics of
**two elements jointly**, reduced over the Monte Carlo Run axis. The only cross-realization
primitive is the `run_stat` AST node:

```rust
// engine/src/model.rs
RunStat { element_id: String, statistic: SubmodelStatKind, arg: Option<Box<AstNode>> }
```

and `SubmodelStatKind` is entirely **univariate** — `Mean | Percentile | Sd | CumulativeProb
| Exceedance | Cte | Sum | Min | Max`. There is no way to reduce a *pair* of elements to a
covariance, so `b*` is not computable inside the model. The current workaround
(`OPTIONS_PRICING_EFFICIENCY.md` §5) is to hard-code `b` as a `constant`, which is
non-adaptive and wrong whenever the user re-parameterizes the contract.

---

## 2. What exists today (implementer's map)

The across-realization reduction already has a working two-pass engine; this feature *extends*
it rather than inventing a new mechanism.

| Concern | Location | Notes |
|---|---|---|
| AST node | `engine/src/model.rs` `AstNode::RunStat` (~L643) | single `element_id` |
| Reducer kinds | `engine/src/model.rs` `SubmodelStatKind` (~L667) | univariate only |
| Injection key | `engine/src/eval.rs` `run_stat_key()` (L140) | `"{elem}\u{1}{stat:?}\u{1}{arg}"` |
| Eval (node → scalar) | `engine/src/eval.rs` `AstNode::RunStat` (L760) | reads `ctx.run_stats[key]`, else 0 |
| Two-pass driver | `engine/src/engine_v2.rs` `run()` (L60–108) | pass 1 captures target finals; reduce; pass 2 injects |
| Collect targets | `engine/src/engine_v2.rs` `collect_run_stats` / `collect_run_stat_nodes` (L122, L159) | AST walk |
| Reduce | `engine/src/engine_v2.rs` `reduce_run_stat` (L208) | `samples: &[f64] → f64` |
| Force-save targets | `engine/src/engine_v2.rs` `RunState::force_save_targets` | pass-1 capture of per-realization finals |
| Array-lane mirror | `engine/src/array_lane.rs` (L615, L776, L961, L989–1022) | duplicate collect/reduce + columnar two-pass + fused single-pass |
| Graph ordering | `engine/src/graph_v2.rs` | run_stat target must precede consumer |
| Streaming moments | `engine/src/stream_accum.rs` `RunningMoments` (Welford M2) | basis for a fused covariance co-moment |

Key structural fact that makes this cheap: **pass 1 already captures each target's
per-realization final values, index-aligned by realization** (`res1.elements[id].final_values`).
Covariance of two elements is just those two aligned vectors fed to a bivariate reducer — no
new sampling, no new pass.

---

## 3. Proposed design

### 3.1 A dedicated bivariate node

Add a sibling to `RunStat` rather than overloading it (keeps the univariate hot path and its
single-element assumptions untouched):

```rust
// model.rs
RunStat2 {
    x: String,                 // first element id
    y: String,                 // second element id
    statistic: RunPairStat,
}

#[serde(rename_all = "snake_case")]
pub enum RunPairStat {
    Cov,             // sample Cov(x, y)
    Corr,            // Pearson correlation(x, y)
    Beta,            // Cov(x, y) / Var(x)  — slope of regressing y on x
}
```

`Beta` is the control-variate coefficient primitive: `b* = beta(x = control, y = target)`.

**JSON shape:**

```jsonc
{ "op": "run_stat2", "x": "control", "y": "est_plain", "statistic": "beta" }
```

### 3.2 The estimator becomes fully in-model

With `E[C]` a known `constant` (or `run_stat mean` of the control), the whole
control-variate estimator is ordinary expressions:

```
b_star  = run_stat2(beta, x = control, y = est_plain)
cv_est  = est_plain − b_star · ( control − control_mean )
```

Then the existing `results_spec.final_stats` on `cv_est` reports the reduced `std` /
`ci_half_width` — the efficiency read-out already documented in `OPTIONS_PRICING_EFFICIENCY.md`.

### 3.3 Reducer math (`engine/src/engine.rs`)

Add numerically-stable helpers alongside the univariate ones:

```rust
pub(crate) fn covariance(xs: &[f64], ys: &[f64]) -> f64;   // sample cov, denom (n−1)
pub(crate) fn correlation(xs: &[f64], ys: &[f64]) -> f64;  // cov / (sd_x·sd_y)
pub(crate) fn beta(xs: &[f64], ys: &[f64]) -> f64;         // cov(x,y)/var(x)
```

Degenerate guards: `Var(x) = 0 ⇒ beta = 0` (control contributes nothing — safe, unbiased),
`corr` with a zero-variance side ⇒ 0. Return a diagnosable value, never NaN into the model.

### 3.4 Key + injection (reuse the existing channel)

Reuse the single `ctx.run_stats: HashMap<String, f64>` map — no new `EvalCtx` field — with a
distinct key so it can't collide with univariate keys:

```rust
// eval.rs
pub(crate) fn run_stat2_key(x: &str, y: &str, stat: &RunPairStat) -> String {
    format!("{x}\u{1}{y}\u{1}{stat:?}\u{2}")   // \u{2} suffix distinguishes from run_stat_key
}
```

Eval mirrors `RunStat`: look up the key, default 0.0 in pass 1.

### 3.5 Two-pass driver changes

- `collect_run_stats` (both `engine_v2.rs` and `array_lane.rs`) also walks `RunStat2` and emits
  a target whose pass-1 capture set is **both** `x` and `y`.
- Reduce step: pull the two aligned `final_values` vectors, assert equal length, dispatch to
  `covariance` / `correlation` / `beta`.
- `force_save_targets` receives the union of all `x` and `y` ids.
- Graph ordering (`graph_v2.rs`): add edges so both `x` and `y` precede the consumer; forbid a
  `RunStat2` on a dependency cycle (same rule as `run_stat`).
- `collect_run_stat_nodes` AST-walk arms must recurse into `RunStat2` (it has no child AST, so
  it's a leaf — just add the match arm).

### 3.6 Array lane

MVP: when any `RunStat2` is present, take the **existing columnar two-pass fallback**
(`array_lane.rs` already has one for `run_stat`) — pass 1 materializes the `x`/`y` columns,
reduce computes the bivariate stat. Defer the fused single-pass covariance to Phase 2 (needs a
bivariate co-moment accumulator, a natural extension of `RunningMoments`: track `C2 = Σ(xᵢ−x̄)(yᵢ−ȳ)`
with a Chan-style parallel merge, exactly paralleling the existing `m2`).

---

## 4. Correctness notes

- **In-sample bias.** Estimating `b` from the same realizations used for the estimate makes
  `Y_cv` biased at **O(1/N)** (Glasserman §4.1.3). The two-pass structure computes `b̂` from all
  realizations in pass 1 and applies it in pass 2 — the standard "same-sample" estimator. The
  bias is negligible at MC realization counts but must be *documented*, with split-sample /
  pilot-run `b` as a Phase 3 refinement for the bias-sensitive.
- **Alignment.** `x` and `y` finals are per-realization and index-aligned by construction
  (identical per-realization seeds across passes); reducers assert equal length.
- **Reduces finals, not paths.** Like `run_stat`, `RunStat2` reduces end-of-run values.
  Capture-time / per-step covariance is out of scope (Phase 3+).
- **Determinism.** Pass 1 and pass 2 draw identical samples (content-seeded RNG), so `b̂` is
  reproducible; the two-pass parity test already guarding `run_stat` extends to `RunStat2`.

---

## 5. Phasing

| Phase | Deliverable | Touches |
|---|---|---|
| **1 (MVP)** ✅ | `Cov`/`Corr`/`Beta` node; scalar `engine_v2` two-pass; array-lane made ineligible → scalar fallback; reducers; graph ordering; tests. **Single control variate fully expressible.** | model.rs, eval.rs, engine.rs, engine_v2.rs, array_lane.rs, graph_v2.rs, summary.rs, submodel_v2.rs |
| **2** ✅ | `Corr` (landed in P1); `run_stat2` made eligible on the fused array lane, reducing over the two materialized columns (bit-identical to the scalar lane) | array_lane.rs |
| **3** ✅ | Multi-control regression (`run_regress`, indexed OLS via a linear solve) **and** split-sample `b` (`run_split_beta`, K-fold jackknife) on a new per-realization injection channel (`EvalCtx.run_vecs`) | model.rs, eval.rs, engine.rs, engine_v2.rs, graph_v2.rs, summary.rs, submodel_v2.rs, array_lane.rs |
| **4** ✅ | `submodel_stat2` — bivariate reduction (cov/corr/beta) of two submodel outputs across the submodel's realizations, evaluated on-demand (no two-pass) | model.rs, eval.rs, submodel_v2.rs, graph_v2.rs, summary.rs, array_lane.rs |

Phase 1 alone closes the documented gap.

## 6. Test plan

- **Unit:** `covariance`/`beta`/`correlation` vs hand-computed values; degenerate `Var(x)=0 ⇒ 0`.
- **Integration (extends the options example):** add a control `disc_S_T = exp(−rT)·S_T` with
  known mean `S0`, and `cv_est = est_plain − beta(disc_S_T, est_plain)·(disc_S_T − S0)`. Assert:
  1. `beta` matches the closed-form regression slope within MC error;
  2. `std(cv_est) < std(est_plain)` (meaningful variance reduction — the terminal price is
     highly correlated with a call payoff);
  3. `mean(cv_est)` within a few CI half-widths of `bs_price` (still unbiased);
  4. combined with antithetic, variance drops further (techniques compose).
- **Two-pass parity / determinism:** same seed ⇒ identical `b̂` and `cv_est`.

## 7. Effort & risk

- **Effort:** Phase 1 is ~1 focused change — one AST variant + enum, three reducers, and
  mirrored collect/reduce/order in the two run_stat sites. The mechanism (two-pass capture →
  reduce → inject) is reused wholesale; the only genuinely new code is the bivariate reducers.
- **Risk:** low for the scalar path (additive AST node; default output byte-identical when no
  `RunStat2` is present). The one real design decision is the array-lane fused single-pass
  (Phase 2) — deferring it to the existing two-pass fallback de-risks the MVP.
- **Back-compat:** purely additive — no schema field changes to existing nodes, no change to
  runs that don't use `run_stat2`.

## 8. Alternatives considered

1. **Hard-coded `b` constant** (today's workaround) — non-adaptive; breaks on re-parameterization.
2. **Offline `b`, injected via model params** — not self-contained; defeats the "diffable model
   is the artifact" principle.
3. **A high-level `control_variate` element type** — rigid; hides the math. The bivariate
   reducer is more composable and *also* unlocks regression betas, hedge ratios, and CAPM-style
   analyses for free.
4. **Overload `RunStat` with an optional partner** — muddies the univariate hot path and its
   single-element key/collection assumptions. A sibling node is cleaner.

---

**Bottom line:** add one bivariate cross-realization node (`run_stat2` with `cov`/`beta`/`corr`),
reusing the existing two-pass capture-reduce-inject machinery. That single primitive makes the
optimal control-variate coefficient computable inside a `model.json`, closing the gap and
generalizing to regression/beta analyses.

---

## Phase 1 — implemented

Landed exactly as designed above; deviations from the plan are noted.

- **AST / schema** (`model.rs`): `AstNode::RunStat2 { x, y, statistic }` + `RunPairStat { Cov, Corr, Beta }`.
  JSON: `{ "op": "run_stat2", "x": "control", "y": "target", "statistic": "beta" }`.
- **Reducers** (`engine.rs`): `covariance` (n−1), `correlation`, `beta` = Cov(x,y)/Var(x), with the
  degenerate `Var(x)=0 ⇒ 0` guard. Unit-tested (`reducer_tests::bivariate_reducers_hand_checks`).
- **Key + eval** (`eval.rs`): `run_stat2_key` (a `\u{2}`-suffixed key sharing the one `run_stats`
  map); `RunStat2` eval arm reads it, 0.0 in pass 1.
- **Two-pass driver** (`engine_v2.rs`): `collect_run_stats` now returns `(uni, pair)` from one AST
  walk; pass 1 force-saves every `x`/`y`; `reduce_run_stat2` computes the bivariate scalar from the
  two index-aligned final-value vectors; pass 2 injects it.
- **Graph** (`graph_v2.rs`): `RunStat2` adds no topo dependency (pre-computed), mirroring `RunStat`.
- **Array lane** (`array_lane.rs`): **deviation from the scope's "two-pass fallback"** — simpler and
  equivalent: a model containing `run_stat2` is marked lane-*ineligible*, so `run()` falls through to
  the scalar two-pass that handles it. The fused single-pass co-moment accumulator remains Phase 2.
- **Round-trip / labels** (`summary.rs`, `submodel_v2.rs`): render + AST-walk arms added.

**Demonstrated** in `schema_examples_manual/options_pricing_efficiency.json` (`disc_S_T`, `b_star`,
`cv_est`) and guarded by `engine/tests/options_efficiency_smoke.rs`: the control variate cuts the
estimator std from 14.76 → 5.61 (variance ↓ ~6.8×) at ~1× the work, unbiased vs Black–Scholes.

**Back-compat:** additive node; runs without `run_stat2` are byte-identical (single-pass early return
unchanged). Full engine test suite green (the one failing test, `seldm_example_project::emit_model_json`,
writes to a hardcoded absolute path and fails identically on a clean tree — unrelated).

## Phase 2 — implemented

The scope proposed a streaming co-moment accumulator; the actual array lane **materializes** each
element's Run column, so the simpler and equivalent implementation reduces the bivariate stat over
the two finalized columns — genuinely single-pass, since the columns are built once.

- **Eligibility** (`array_lane.rs`): `run_stat2` now passes `expr_allowed` (flat lane). The
  dimensioned lane keeps it ineligible → scalar fallback (a later phase if needed).
- **Ordering** (`collect_expr_deps`): a `run_stat2` consumer now depends on both `x` and `y`, so
  `augmented_order` finalizes both columns before folding the reduction in — mirroring how a
  univariate `run_stat` target is ordered before its consumer.
- **Reduction**: `collect_run_stat2s` gathers pair targets; both uni- and bivariate reductions share
  one `rstats` slot table (`Op::RunStat(slot)` reads either); `reduce_ready_pairs` computes each
  pair's scalar once both its columns finalize (single-pass path) or after the first `eval_pass`
  (cyclic two-pass fallback). `covariance`/`correlation`/`beta` are reused from `engine.rs`.
- **Verified** (`engine/tests/run_stat2_v2.rs`): X~N(0,1), W~N(0,1), Y=2X+W ⇒ beta≈2, cov≈2,
  corr≈2/√5. The **scalar and array lanes agree bit-identically**, and a downstream node consumes
  the reduction. `run_stat2` remains ineligible only on the (rarer) dimensioned lane.

## Phase 3 — multiple-control regression + split-sample `b` (per-realization injection)

### Multiple-control regression (`run_regress`) — implemented

Rather than return a coefficient *vector* (which would need vector-valued injection — a new
`EvalCtx` channel threaded through every context construction), each coefficient is a **scalar**,
reusing the Phase 1/2 scalar `run_stats` injection unchanged.

- **AST** (`model.rs`): `RunRegress { y, controls: Vec<String>, index }` →
  `{ "op": "run_regress", "y": "…", "controls": ["c0","c1",…], "index": k }`, the k-th OLS slope of
  regressing `y` on the controls. The model forms the CV estimator as
  `y − Σ_k b_k·(c_k − E[c_k])` with `b_k = run_regress(…, index=k)`.
- **Solver** (`engine.rs`): `regression_coefficients(y, controls)` builds Σ_C (control covariance
  matrix) and σ_Cy, then `solve_linear_or_zero` (Gaussian elimination, partial pivoting). A singular
  system — collinear or zero-variance controls — returns all-zero coefficients, so the control
  adjustment vanishes and the estimator stays unbiased (never NaN into the model).
- **Driver** (`engine_v2.rs`): `collect_run_stats` now returns a third target list; pass 1 force-saves
  `y` and every control; the reduce step solves and selects `index`; pass 2 injects the scalar.
- **Lanes**: `run_regress` reduces *many* columns via a linear solve (not a per-column fold), so it is
  array-lane-ineligible → scalar two-pass (like `erf`). Exhaustiveness/label arms added in
  `graph_v2.rs`, `summary.rs`, `submodel_v2.rs`, `array_lane.rs`.
- **Verified** (`engine/tests/run_regress_v2.rs` + `regression_coefficients_hand_checks`):
  correlated controls c0=X, c1=X+U, Y=3X+2U+W ⇒ recovered b0≈1, b1≈2 (exercising the non-diagonal
  solve); a single-control regress equals `beta`; collinear controls → zero; index out of range is a
  model error.

### Split-sample `b` (`run_split_beta`) — implemented via per-realization injection

The earlier draft flagged this as blocked: the in-sample bias fix needs the coefficient a realization
sees to depend on **which fold it is in**, but the two-pass mechanism injected **one scalar constant
across all realizations**. Phase 3 added the missing mechanism — a **per-realization injection
channel** — and built split-sample `b` on it.

- **Per-realization channel** (`eval.rs`, `engine_v2.rs`): `EvalCtx.run_vecs: Option<(&HashMap<String,
  Vec<f64>>, usize)>` — a key→`[N]` map plus the current realization index. `ArrayEnv` carries the
  map and a `Cell<usize>` current-realization; the scalar lane's realization loop sets the cell each
  iteration, and every `EvalCtx` reads `(map, cell)` so a node can index its own realization's value.
  `RunState` gains a `run_vecs` field the two-pass driver fills in pass 2 (parallel to `run_stats`).
- **Node** (`model.rs`): `RunSplitBeta { x, y, folds }` → `{ "op": "run_split_beta", "x": …, "y": …,
  "folds": K }`. For realization i it reads `beta(x, y)` estimated over every realization **not** in
  fold `i mod K` — so the coefficient excludes its own `(xᵢ, yᵢ)`.
- **Reducer** (`engine.rs`): `jackknife_beta(x, y, folds)` returns the `[N]` vector of leave-fold-out
  betas (folds clamped to `[2, n]`; degenerate → zeros).
- **Lane**: scalar-only (the array lane is columnar and has no single "current realization"); marked
  ineligible like `run_regress`.
- **Verified**: `jackknife_beta_leaves_out_the_right_fold` (hand-computed fold exclusion) and
  `engine/tests/run_splitbeta_v2.rs` — X~N(0,1),W~N(0,1),Y=2X+W: the injected coefficient is **not a
  constant** (exactly K=4 distinct fold values, cycling with `i mod 4`, each ≈ true beta 2), and a
  downstream CV estimator consumes the per-realization value and stays unbiased.

This channel is general: any future reduction that needs a per-realization value (not just
split-sample beta) can inject an `[N]` vector the same way.

**Back-compat:** all additive. Models without any run-stat still hit the single-pass early return
(now gated on all four target lists being empty); `run_vecs` is `None`/empty everywhere except pass 2
of a model that uses `run_split_beta`, so every other path is unchanged.

## Phase 4 — submodel_stat2 implemented

The bivariate analog of `submodel_stat`, for the control-variate math *inside* a nested submodel
simulation. The simplest addition of the arc: `submodel_stat` (and thus `submodel_stat2`) is
evaluated **on-demand in the evaluator** from the pre-computed submodel output vectors — no two-pass
driver, no injection channel, no `EvalCtx` change.

- **Node** (`model.rs`): `SubmodelStat2 { submodel_id, output_x, output_y, statistic }` →
  `{ "op": "submodel_stat2", "submodel_id": …, "output_x": …, "output_y": …, "statistic": "beta" }`.
  Both outputs come from the one submodel run, so they are index-aligned by realization.
- **Eval** (`eval.rs`): read both output vectors from `ctx.submodel_outputs`, reduce with the reused
  `covariance`/`correlation`/`beta`. Missing output → 0.0 (matching `submodel_stat`).
- **Collection** (`submodel_v2.rs`): the one functional wire — `collect_ast` registers **both**
  `(submodel_id, output_x)` and `(submodel_id, output_y)` so the submodel pre-pass materializes them.
- **Lanes / labels**: `graph_v2` adds the submodel dep, `summary` renders it, `array_lane` marks it
  ineligible (like `submodel_stat`).
- **Verified** (`engine/tests/submodel_stat2_v2.rs`): submodel draws X~N(0,1), W~N(0,1), exposes X and
  Y=2X+W; the parent reduces via `submodel_stat2` → beta≈2, cov≈2, corr≈2/√5.

**Back-compat:** additive node evaluated only when present; no change to any existing execution path.
