# Nested VaR / Horizon Revaluation — the Conditional Nested-Submodel Statistic

**Model:** [`schema_examples_manual/nested_var_horizon.json`](schema_examples_manual/nested_var_horizon.json)
**Test:** [`engine/tests/nested_var_smoke.rs`](engine/tests/nested_var_smoke.rs)
**Scope:** [`NESTED_STAT_SCOPE.md`](NESTED_STAT_SCOPE.md) · **Related:** [`AMERICAN_OPTION_SCOPE.md`](AMERICAN_OPTION_SCOPE.md) §8 (the empty quadrant this fills)

This is the first use of **`nested_stat`** — the **conditional** nested-submodel statistic. It prices a
portfolio at a future **risk horizon** by, for each simulated horizon scenario, re-running an inner
valuation Monte Carlo **conditioned on that scenario**. The distribution of the result across scenarios
is the horizon P&L distribution — the exact nested-simulation structure of **VaR and CVA**. Because the
inner conditional value here has a **closed form (Black–Scholes)**, the whole construct is checkable.

---

## 1. The problem shape (why it needs conditional nesting)

A horizon risk measure asks: *simulate the world forward to a horizon `H`; in each scenario, what is my
portfolio worth; what does that distribution of values look like in the tail?* The middle step —
"what is the portfolio worth **in this scenario**" — is itself an expectation conditional on the
scenario's state. When the portfolio holds anything without a closed form, that inner value must be
**simulated**, from the scenario's state. Outer simulation of the world, inner simulation of the value
conditioned on each outer state: a nested conditional expectation.

## 2. How it maps onto WASiM

| Concept | WASiM element |
|---|---|
| Market factor to the horizon | `S_H` — outer risk-neutral GBM horizon price (driven by `Zc`) |
| **Inner valuation submodel** | `innerBS` — a `kind: submodel` container: a call-payoff MC in remaining maturity `Trem` |
| Inner start price (bound) | `iS0` — inner **constant**, bound to `S_H` per outer realization |
| Inner value estimate | `ipay` — inner discounted payoff; its **mean** ≈ Black–Scholes at `iS0` |
| **Conditional portfolio value** | `port_value = nested_stat(innerBS.ipay, mean; iS0 ← S_H)` |
| Closed-form check (per path) | `bs_ref = BS(S_H, K, r, σ, Trem)` |
| **Marginal** contrast | `marginal_bs = submodel_stat(innerBS.ipay, mean)` — one constant `BS(100)` |

The single line that does the nesting:

```jsonc
{ "op": "nested_stat", "submodel_id": "innerBS", "output": "ipay", "statistic": "mean",
  "bindings": [ { "input": "iS0", "from": "S_H" } ] }
```

For each outer realization the engine overrides the inner constant `iS0` with that path's `S_H`, runs the
inner call-valuation MC (its own 4 000 realizations, an independent seed), and takes the mean discounted
payoff — the portfolio's value in that horizon scenario.

## 3. Results (verified)

`S₀ = K = 100`, `r = 0.05`, `σ = 0.20`, horizon `H = 0.25`, remaining maturity `Trem = 0.75`,
`N_outer = 2 000`, `N_inner = 4 000`:

| quantity | value |
|---|---|
| mean `port_value` (nested MC) | **10.6354** |
| mean `bs_ref` (closed form at `S_H`) | **10.6378** |
| **per-path corr(`port_value`, `bs_ref`)** | **0.99948** |
| horizon value 5th percentile — nested / closed form | **2.409 / 2.404** |
| horizon value range (5th → 95th pctile) | ≈ 2.4 → ~24 |
| `marginal_bs` (submodel_stat, constant) | ≈ 8.5 (one `BS(100)` MC estimate; closed form 8.77) |

Four things fall out:

1. **The nested MC reproduces the conditional expectation — per path.** `port_value` matches the
   closed-form `bs_ref` in the mean (10.6354 vs 10.6378) **and** tracks it scenario-by-scenario at a
   correlation of **0.99948**. That per-path agreement — not just the average — is the signature of a
   genuinely *conditional* statistic: each outer path gets its own inner expectation.
2. **It produces a distribution, and hence a risk measure.** The horizon value spans ≈ 2.4 → 24 across
   scenarios; its **5th percentile (2.409)** — a VaR-style tail read — matches the closed-form tail
   (2.404). This is the whole point of nested simulation: a *distribution* of conditional values, not a
   point.
3. **The marginal contrast is stark.** `submodel_stat` on the same inner runs it **once** with the
   default start price, so `marginal_bs` is a **single constant** (~8.5, an MC estimate of `BS(100)`)
   shared by every outer path — it neither varies with `S_H` nor equals the conditional mean (10.6). Same
   call shape, completely different object. This is exactly the marginal-vs-conditional hazard the scope
   doc warns about.
4. **It's validated end to end** because the inner conditional value is Black–Scholes — the nested MC has
   no closed form in general, but here we can check it exactly.

## 4. Engine status

**Landed** — `nested_stat` (conditional nested-submodel statistic): a new AST node with `bindings`, a
per-outer-realization inner-run driver (`run_nested_stat`) reusing `extract_submodel` and the
per-realization injection channel, seeded per outer path for reproducible conditional independence.
Additive and backward-compatible — a model without it is byte-identical, and the marginal `submodel_stat`
path is untouched.

**Also built:** per-**timestep** binding (`each_step`) for exposure profiles over time — see
[`EXPOSURE_PROFILE_EXAMPLE.md`](EXPOSURE_PROFILE_EXAMPLE.md). **Still open** (see
[`NESTED_STAT_SCOPE.md`](NESTED_STAT_SCOPE.md) §6): binding to stochastic-process / stock state (not just
fixed-scalar constants), and — for nonlinear outer functionals — documenting the `O(1/N_inner)`
nested-MC bias (absent here because the outer use is linear in the inner mean).

## 5. Takeaway

`nested_stat` turns the "simulate the world, then simulate the value inside each world" pattern into one
declarative construct. Here it computes a horizon VaR whose conditional values reproduce Black–Scholes
per scenario at correlation 0.9995, while the marginal `submodel_stat` on the same inner collapses to a
single constant — a clean demonstration that the engine now expresses **conditional** nested simulation,
the primitive under nested VaR/CVA, double-loop UQ, and Bayesian experimental design.
