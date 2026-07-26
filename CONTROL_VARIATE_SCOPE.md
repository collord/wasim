# Scope: Cross-Realization Bivariate Reducers (Control-Variate Support)

**Status:** proposal / scoping
**Motivation:** [`OPTIONS_PRICING_EFFICIENCY.md`](OPTIONS_PRICING_EFFICIENCY.md) §5 — control variates
cannot yet be expressed because the optimal coefficient needs a **covariance between two
elements across realizations**, and today's `run_stat` reduces a **single** element.

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
| **1 (MVP)** | `Cov` + `Beta` node; scalar `engine_v2` two-pass; array-lane two-pass fallback; reducers; graph ordering; tests. **Single control variate fully expressible.** | model.rs, eval.rs, engine.rs, engine_v2.rs, array_lane.rs, graph_v2.rs |
| **2** | `Corr`; fused single-pass covariance accumulator in array lane (perf parity with `run_stat`) | array_lane.rs, stream_accum.rs |
| **3** | Multi-control regression (vector control → normal equations / `run_regress`); split-sample `b` to kill in-sample bias | new node/reducer |
| **4** | `submodel_stat` symmetry — bivariate reduction across a submodel's realizations | submodel_v2.rs |

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
