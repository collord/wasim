# Correlated Assets: a Spread Option with an Exchange-Option Control Variate

**Model:** [`schema_examples_manual/correlated_assets_spread_option.json`](schema_examples_manual/correlated_assets_spread_option.json)
**Test:** [`engine/tests/correlated_assets_smoke.rs`](engine/tests/correlated_assets_smoke.rs)

The third example in the set (after the European efficiency example,
[`OPTIONS_PRICING_EFFICIENCY.md`](OPTIONS_PRICING_EFFICIENCY.md), and the path-dependent Asian,
[`ASIAN_OPTION_EXAMPLE.md`](ASIAN_OPTION_EXAMPLE.md)). It exercises the one core technique the other
two never touched: **cross-asset correlation**. Two correlated GBM assets are simulated, a
**spread option** `max(S₁−S₂−K, 0)` is priced by Monte Carlo, and the **exchange option**
(Margrabe, `K=0`) serves as a control variate — with its exact closed form doubling as the control's
known mean.

Grounded in Glasserman §2.3 ("Normal Random Variables and Vectors"), which develops the **Cholesky
factorization** of a covariance matrix to generate correlated normal vectors — done here explicitly
and in-model.

---

## 1. The idea

Correlated standard normals from the 2×2 Cholesky factor of `[[1, ρ], [ρ, 1]]`, which is
`[[1, 0], [ρ, √(1−ρ²)]]`:

```
Z₁ ~ N(0,1),  Zᵢ ~ N(0,1) independent
Z₂ = ρ·Z₁ + √(1−ρ²)·Zᵢ        ⇒  Var(Z₂)=1,  Corr(Z₁,Z₂)=ρ
```

Both assets are then exact risk-neutral lognormals driven by `Z₁` and `Z₂`, so their terminal
prices carry correlation ρ. The **spread option** `max(S₁−S₂−K, 0)` has no closed form for `K>0`
(Monte Carlo required); the **exchange option** `max(S₁−S₂, 0)` (`K=0`) does — **Margrabe (1978)** —
and is ≈ 0.99-correlated with the spread payoff, making it an excellent control.

Doing the Cholesky by hand (rather than with WASiM's native `correlations` field) keeps **ρ a live,
editable input** that flows into *both* the simulation and the closed form, and it mirrors the text.

---

## 2. How it maps onto WASiM

| Concept (Glasserman §2.3 / Margrabe) | WASiM element(s) |
|---|---|
| Independent normals | `Z1`, `Zi` — `random_variable` N(0,1) |
| Cholesky-correlated normal | `Z2 = rho*Z1 + sqrt(1-rho^2)*Zi` (`expression`) |
| Exact terminal prices | `ST1`, `ST2` = `Sᵢ₀·exp((r−σᵢ²/2)T + σᵢ√T·Zₖ)` |
| Spread / exchange payoffs | `spread_payoff`, `exchange_payoff` = `disc·(…)⁺` |
| Margrabe closed form | `exchange_price = S₁₀·N(d₁) − S₂₀·N(d₂)`, `σₓ = √(σ₁²−2ρσ₁σ₂+σ₂²)` (live via `erf`) |
| **Correlation self-check** | `realized_rho = run_stat2(corr, Z1, Z2)` |
| CV coefficient / estimator | `beta = run_stat2(beta, exchange_payoff, spread_payoff)`, `cv_spread = spread_payoff − beta·(exchange_payoff − exchange_price)` |

`run_stat2` (the Phase-1 bivariate reducer) appears twice: once as `corr` to *verify* the induced
correlation live inside the model, once as `beta` for the control variate.

---

## 3. Results (verified)

`S₁₀ = S₂₀ = 100`, `σ₁ = 0.20`, `σ₂ = 0.25`, `ρ = 0.5`, `r = 0.05`, `T = 1`, `K = 5`,
`seed = 20240707`, `N = 40 000` (from the smoke test):

| quantity | value |
|---|---|
| **Realized correlation** (`run_stat2 corr`) | **0.5025** (target 0.5) |
| **Exchange option — Margrabe exact** | **9.121** |
| Exchange option — MC | 9.114 ± 0.127 (95% CI) — **matches Margrabe** |
| Spread option — plain MC | 6.849, std **11.35** |
| Spread option — control variate | 6.855, std **1.53** |
| **Variance reduction (std_plain / std_cv)** | **≈ 7.4×** (≈ 55× in variance) |

1. **Correlation is induced correctly.** The realized `corr(Z₁,Z₂) = 0.5025` reproduces the input
   ρ = 0.5 — the Cholesky construction works, checked live in-model.
2. **The correlated simulation is right.** The exchange-option MC price matches the live Margrabe
   closed form within one CI half-width. Because Margrabe depends on ρ (through σₓ), this is a
   stringent joint check of the correlation *and* the pricing.
3. **The control variate works.** The ≈ 0.99-correlated exchange option cuts the spread estimator's
   standard deviation ~7.4× at essentially no extra cost, and stays unbiased. (Smaller than the
   Asian example's ~38× because the `K=5` offset decorrelates the two payoffs somewhat — an honest,
   representative number.)

---

## 4. Engine notes

This example was clean to build — the main friction is a familiar recurring one:

1. **Live vs static correlation parameters.** WASiM's native `correlations` field (Gaussian copula,
   Iman–Conover) handles N-asset correlation, but its coefficient is a **static** number, so ρ
   couldn't be an editable constant flowing into the closed form. The explicit 2×2 Cholesky here
   sidesteps that and keeps ρ live — but for `N > 2` assets, hand-writing the Cholesky factor in
   expressions is impractical. The same **expression-valued distribution/process parameters** fix
   noted in `ASIAN_OPTION_EXAMPLE.md` §4 (gap #1) applies here: allowing the `correlations`
   coefficient (or a correlation-matrix entry) to be an expression would give live, N-asset
   correlation without hand-rolled Cholesky.
2. **No native correlated-normal-vector primitive with live parameters.** A node that draws a
   correlated normal vector from an (expression-valued) correlation matrix would make multi-asset
   models first-class; today it is either the static `correlations` field or manual Cholesky.

Otherwise the exact-terminal construction, the `erf`-based Margrabe reference, and the `run_stat2`
correlation check + control variate all compose cleanly.

---

## 5. Takeaway

WASiM simulates correlated assets (Cholesky, §2.3), prices a spread option that genuinely needs
Monte Carlo, verifies the induced correlation *live* with `run_stat2 corr`, and reduces variance
with the Margrabe exchange option as a control — a ~7.4× reduction, validated against the exact
closed form. Together with the European and Asian examples, the three cover exact-terminal
efficiency, path dependence, and cross-asset correlation.
