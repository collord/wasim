# Counterparty Exposure Profile — `nested_stat` in `each_step` Mode

**Model:** [`schema_examples_manual/exposure_profile_each_step.json`](schema_examples_manual/exposure_profile_each_step.json)
**Test:** [`engine/tests/exposure_profile_smoke.rs`](engine/tests/exposure_profile_smoke.rs)
**Scope:** [`NESTED_STAT_SCOPE.md`](NESTED_STAT_SCOPE.md) · **Companion:** [`NESTED_STAT_EXAMPLE.md`](NESTED_STAT_EXAMPLE.md) (terminal-mode nested VaR)

The [nested-VaR example](NESTED_STAT_EXAMPLE.md) conditioned the inner run on the outer's **terminal**
state — one revaluation per scenario. A **counterparty exposure profile** needs the revaluation at
**every date along the path**: EE(t) and PFE(t) are functions *of time*. That is the **`each_step`** mode
of `nested_stat` — one inner run per `(realization, timestep)` node — and this example is its first use.

---

## 1. The problem shape

Counterparty credit exposure is a **profile over time**: at each future date `t`, on each simulated path
of the market factors, you reprice the portfolio conditional on that date's state to get the exposure,
then read the profile across paths — **Expected Exposure** `EE(t) = E[exposure(t)]` and **Potential
Future Exposure** `PFE(t) = percentile_t exposure(t)`. CVA integrates `EE(t)` against the counterparty's
default density. Every piece requires the inner revaluation at **every (path, date) node** — the
per-timestep nested simulation.

## 2. How it maps onto WASiM

| Concept | WASiM element |
|---|---|
| Market factor path | `S` — outer GBM accumulator; `S_t` — start-of-step read, `time_history` saved |
| **Inner revaluation submodel** | `innerRoll` — a `kind: submodel` call-valuation MC (rolling 6-month maturity) |
| Inner start price (bound per step) | `iS0` — inner constant, bound to `S_t` at **each timestep** |
| **Exposure profile** | `exposure_t = nested_stat(innerRoll.ipay, mean; iS0 ← S_t; each_step)` |
| EE(t) / PFE(t) | `exposure_t.time_history.mean` / `.p95` — read straight from the per-step summary |
| Closed-form check (per step) | `bs_t = BS(S_t, K, r, σ, Trem)` |

The one line that produces the whole profile:

```jsonc
{ "op": "nested_stat", "submodel_id": "innerRoll", "output": "ipay", "statistic": "mean",
  "bindings": [ { "input": "iS0", "from": "S_t" } ], "each_step": true }
```

With `each_step: true`, the engine force-saves the `from` binding's **`time_history`** panel, and at every
`(realization, step)` node it overrides `iS0` with `S_t[step][realization]`, runs the inner MC, and
reduces — producing a `[steps × N]` panel. The outer element reads its `(step, realization)` cell each
step, so **its `time_history` *is* the exposure profile**: mean → EE(t), p95 → PFE(t). No extra wiring.

## 3. Results (verified)

`S₀ = K = 100`, `r = 0.05`, `σ = 0.20`, rolling maturity `Trem = 0.5`, horizon `1.2` yr in `Δt = 0.2`
steps, `N_outer = 400`, `N_inner = 2 000`:

- **EE(t) reproduces the closed form at every step.** The each_step nested mean tracks `mean(BS(S_t))`
  step-by-step (within Monte-Carlo tolerance) — the conditional revaluation is correct at *each date*, not
  just the terminal.
- **PFE(t) reproduces the closed form at every step.** The p95 of the exposure across scenarios tracks the
  p95 of `BS(S_t)` — the tail profile is right.
- **The profile is genuinely time-varying.** Both EE(t) and PFE(t) **grow** over the horizon as the
  factor's dispersion widens — the classic exposure-profile shape, not a flat line.
- **Per-path fidelity.** At the terminal step, the exposure tracks the closed-form `BS(S_H)`
  **path-by-path** at correlation > 0.99.

(The test prints the full EE(t)/PFE(t) profile next to the closed form, step by step.)

## 4. Engine status

**Landed** — `each_step` mode on `nested_stat`: an `each_step` flag on the node, force-saving of the
`from` bindings' `time_history`, a `[steps × N]` reduction driver (`run_nested_stat_each_step`, graph
built once and reused across all `(step, realization)` nodes), and a new per-`(step, realization)`
injection channel (`run_step_vecs`) the eval arm reads by `step_index`. Additive — terminal-mode
`nested_stat` and every model without it are unchanged.

**Cost.** `each_step` is the full per-node nested loop: `N_outer × steps × N_inner` inner realizations.
Keep all three modest; subsample outer paths and inner draws for large books.

**Open:** binding to stochastic-process / stock state beyond fixed-scalar constants; and the tight
Andersen–Broadie American dual, which this per-timestep conditional expectation now unblocks (see
[`AMERICAN_OPTION_SCOPE.md`](AMERICAN_OPTION_SCOPE.md) §8).

## 5. Takeaway

`each_step` turns `nested_stat` into a **per-date** conditional simulation: the outer element's
`time_history` becomes an exposure profile, with EE(t) and PFE(t) falling straight out of the per-step
summary and matching Black–Scholes at every step. This is the per-`(path, step)` nested capability that
counterparty exposure / CVA analytics and the tight American dual require — now expressed in one
declarative flag.
