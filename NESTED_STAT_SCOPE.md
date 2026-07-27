# `nested_stat` — Conditional Nested-Submodel Statistics (Nested Conditional Expectation)

**Status:** **Built** — terminal binding ([`NESTED_STAT_EXAMPLE.md`](NESTED_STAT_EXAMPLE.md)) **and**
per-timestep binding (`each_step`, [`EXPOSURE_PROFILE_EXAMPLE.md`](EXPOSURE_PROFILE_EXAMPLE.md)).
Additive, backward-compatible.

`nested_stat` is the **conditional** twin of the marginal `submodel_stat`: for each outer realization it
re-runs an inner submodel with its input constants **bound to that realization's outer state**, and
reduces an inner output to a statistic. It evaluates the **nested conditional expectation**

```
value(outer realization i) = stat_inner( inner output | inner inputs ← outer state at i )
```

so `E_outer[ f(nested_stat) ]` is a genuine nested Monte-Carlo estimator. This is the one construct the
[American-option scope §8](AMERICAN_OPTION_SCOPE.md) identified as missing — the "conditional + fresh
randomness" quadrant — generalized so it serves many domains, not just an option dual.

---

## 1. Where it sits — the four "reach beyond one realization" constructs

Two orthogonal axes separate WASiM's cross-realization constructs. `nested_stat` fills the empty cell.

|  | **No fresh randomness** (reduce existing samples) | **Fresh randomness** (a real sub-simulation) |
|---|---|---|
| **Marginal** (independent of outer state) | `run_stat` / `run_stat2` / `run_regress` / `run_split_beta` | **`submodel_stat`** — inner runs once; a marginal statistic |
| **Conditional** (on the outer realization's state) | `lsm` — backward recursion over the run's own panel | **`nested_stat`** ← *this construct* |

- **vs. `submodel_stat`:** identical call shape, but `submodel_stat` runs the inner **once** (a single
  marginal draw, the same constant for every outer path); `nested_stat` runs it **once per outer
  realization**, conditioned on that path's state. Picking the wrong one is a real hazard — see §5.
- **vs. `lsm`:** `lsm` is conditional too, but reuses the run's **own** realized future as its sample (no
  new randomness); `nested_stat` draws **fresh** inner samples, so it estimates a true conditional
  expectation rather than reusing path outcomes.

## 2. The construct

```jsonc
{ "op": "nested_stat",
  "submodel_id": "innerBS",          // the submodel container to nest
  "output": "ipay",                  // interior element whose inner finals are reduced
  "statistic": "mean",               // mean | percentile | sd | cumulative_prob | exceedance | cte | sum | min | max
  "arg": { "op": "literal", "value": 95 },   // for the arg-taking statistics
  "bindings": [ { "input": "iS0", "from": "S_H" } ] }  // inner constant ← outer element (per realization)
```

Each `{input, from}` overrides the inner **constant** `input` with the outer element `from`'s realized
per-realization value. That binding is what makes the inner run conditional on the outer state.

## 3. How it runs (reuses the existing machinery)

`nested_stat` is a **post-run per-realization reduction**, like `lsm` / `run_split_beta`:

1. **Pass 1 (outer):** the normal MC run, force-saving each binding's `from` element finals.
2. **Reduce:** extract the inner submodel once (`extract_submodel`, shared with `submodel_stat`). For each
   outer realization `i`: override the bound inner constants with `finals(from)[i]`, run the inner
   submodel (its own `n_realizations`, an independent per-`i` seed), and reduce the inner `output` with
   the same reducer set as `run_stat` / `submodel_stat`. This yields an `[N_outer]` vector.
3. **Pass 2 (outer):** inject that vector through the per-realization channel; the `nested_stat` node
   reads its realization's entry (0.0 in pass 1).

In **`each_step`** mode the same three steps run per **timestep**: pass 1 force-saves the `from`
bindings' `time_history`; the reduce step produces a `[steps × N]` panel (an inner run at every
`(realization, step)` node, the graph built once and reused); pass 2 injects it through a per-`(step,
realization)` channel (`run_step_vecs`) that the eval arm reads by `step_index`, so the node's own
`time_history` is the profile over time.

- **Additive / backward-compatible:** a model with no `nested_stat` is byte-identical (the collector
  returns empty and the two-pass early-outs exactly as before). The inner submodel's marginal
  `submodel_stat` path is untouched.
- **Determinism:** each outer realization's inner run is seeded from `(root, submodel_id, i)`, so the
  inner draws are reproducible and conditionally independent across outer paths.
- **Binding target:** `input` must name a `constant` (fixed-scalar) element inside the submodel — its
  value is what gets overridden. Non-constant `input`s are ignored (documented, not an error).

## 4. Domains it unlocks (one construct, many uses)

All are instances of the nested conditional expectation `E_outer[ g( E_inner[ h | state ] ) ]`:

| Domain | Outer | Inner (bound to outer state) | Statistic |
|---|---|---|---|
| **Nested VaR / market risk** | risk factors to a horizon | portfolio revaluation at the horizon price | `percentile` (VaR), `cte` (expected shortfall) |
| **CVA / counterparty exposure profile** (`each_step`) | market factors along the path | derivative repricing at **every date** conditional on the state | `mean` → EE(t); read `time_history.p95` → PFE(t) |
| **Double-loop UQ** | epistemic parameters | aleatory variability given the parameters | `mean` / `percentile` |
| **Bayesian experimental design (EIG)** | design + parameter draw | data likelihood given the draw | `mean` (marginal likelihood), composed in outer expressions |
| **Pseudo-marginal / simulation-based inference** | parameter θ | likelihood estimator given θ | `mean` |

The worked [`NESTED_STAT_EXAMPLE.md`](NESTED_STAT_EXAMPLE.md) is the nested-VaR case, validated against
Black–Scholes.

## 5. The communication hazard (documented on purpose)

`nested_stat` and `submodel_stat` look **identical at the call site** but differ by orders of magnitude in
cost and in *meaning*:

- Reaching for **`submodel_stat`** when you needed the conditional one gives a **silently wrong** answer
  (a single marginal constant instead of a per-scenario value).
- Reaching for **`nested_stat`** when the marginal suffices gives a **runtime blow-up** (`N_outer` inner
  runs instead of one).

Rule of thumb: **does the inner computation depend on the outer path's state?** If yes → `nested_stat`
with `bindings`. If no → `submodel_stat`. The names encode the distinction (`nested_` = per-outer-path
nesting; a plain `submodel_` = one marginal sub-run).

## 6. Cost & the honest limits

- **Cost is the double loop:** `N_outer × N_inner` inner realizations. Keep both modest; there is no free
  lunch — this is the intrinsic cost of a conditional expectation with fresh inner samples.
- **Nested-MC bias:** when the outer functional `g` is nonlinear (a `max`, a `log`, an indicator), inner
  sampling noise biases the outer estimate `O(1/N_inner)` (Rainforth et al.). For a **linear** outer use
  (mean of a conditional mean) there is no such bias — the nested-VaR example is effectively linear in the
  inner mean, which is why it matches the closed form. Document `N_inner` when the outer use is nonlinear.
- **Two binding scopes, both built.**
  - **Terminal (`each_step: false`, default):** the inner is conditioned on the outer's **final** state —
    one inner run per outer realization; the node's value is a single scalar per realization. Covers
    double-loop UQ, EIG, nested risk at a single horizon, and pseudo-marginal likelihoods.
    ([`NESTED_STAT_EXAMPLE.md`](NESTED_STAT_EXAMPLE.md).)
  - **Per-timestep (`each_step: true`):** the inner is conditioned on the outer state at **every**
    timestep — one inner run per `(realization, step)` node, so the node evaluates to a per-step value and
    its `time_history` is a **profile over time**. This is what exposure profiles (EE(t)/PFE(t)), per-date
    conditional expectations, and MCTS-style rollouts need. Cost is `N_outer × steps × N_inner`; the
    `from` bindings must have `time_history` saved (the engine force-saves them).
    ([`EXPOSURE_PROFILE_EXAMPLE.md`](EXPOSURE_PROFILE_EXAMPLE.md).)
- **Binding to stochastic-process / stock state** beyond a fixed-scalar constant (both modes override
  constants only) is a natural future extension.

## 7. Takeaway

`nested_stat` gives WASiM a single, general **conditional nested simulation** primitive — the nested
conditional expectation that underlies nested VaR/CVA, double-loop UQ, Bayesian experimental design, and
pseudo-marginal inference — built additively on the existing submodel-extraction and per-realization
injection machinery, with the marginal-vs-conditional distinction made explicit in the name and docs so
users reach for the right one.
