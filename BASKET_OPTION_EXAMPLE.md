# Basket Option: N Correlated Assets, Native Correlation + Geometric Control

**Model:** [`schema_examples_manual/basket_option.json`](schema_examples_manual/basket_option.json)
**Test:** [`engine/tests/basket_option_smoke.rs`](engine/tests/basket_option_smoke.rs)
**Scope:** [`BASKET_OPTION_SCOPE.md`](BASKET_OPTION_SCOPE.md)
**Companions:** [`CORRELATED_ASSETS_EXAMPLE.md`](CORRELATED_ASSETS_EXAMPLE.md) (two-asset spread) · [`ASIAN_OPTION_EXAMPLE.md`](ASIAN_OPTION_EXAMPLE.md) (geometric control over *time*).

A **European arithmetic-basket call** pays `(Σᵢ wᵢ Sᵢ(T) − K)⁺` on a weighted sum of `N` correlated
assets. It generalizes the two-asset [spread example](CORRELATED_ASSETS_EXAMPLE.md) to `N = 3` and
demonstrates two things: WASiM's **native `correlations` field scales to N assets** (no manual Cholesky),
and the **geometric-basket control variate** — the cross-asset analogue of the Asian's geometric control
over time.

Priced by **exact terminal** simulation (correlated `N(0,1)` draws → terminal lognormal prices), so no
time-stepping and no discretization bias. **No new engine feature** is required.

---

## 1. The problem

The **arithmetic** basket has no closed form (a sum of lognormals isn't lognormal — the same
intractability as the Asian average), so Monte Carlo is genuinely needed. The **geometric** basket
`∏ᵢ Sᵢ(T)^{wᵢ}` *is* lognormal, so it has a Black–Scholes-style closed form and `≈` the arithmetic
basket — a control correlated at ≈ 0.997 with a known mean.

Contract: `N = 3` equal assets, `Sᵢ,₀ = 100`, `σ = 0.20`, pairwise correlation `ρ = 0.30`
(equicorrelation), weights `wᵢ = 1/3`, `K = 100`, `r = 0.05`, `T = 1`.

---

## 2. How it maps onto WASiM

| Concept | WASiM element |
|---|---|
| Correlated shocks | `Z1, Z2, Z3` — `random_variable` `N(0,1)`, each with a **`correlations`** entry (`ρ`) to the others |
| Exact terminal prices | `STᵢ = Sᵢ,₀·exp((r − σ²/2)T + σ√T·Zᵢ)` |
| Arithmetic basket call | `basket_arith = disc·(Σ wᵢ STᵢ − K)⁺` |
| Geometric basket call | `basket_geo = disc·(∏ STᵢ^{wᵢ} − K)⁺` = `disc·(exp(w·Σ ln STᵢ) − K)⁺` |
| Geometric closed form | `geo_price` — lognormal `ln G ~ N(μ_g, σ_g²)`, live via `erf` |
| CV coefficient / estimator | `beta = run_stat2(beta, basket_geo, basket_arith)`, `basket_cv = basket_arith − beta·(basket_geo − geo_price)` |

### Native correlation (the point)

Each shock declares `"correlations": [{ "partner": "Zⱼ", "coefficient": 0.30 }, …]`. The engine's
`build_corr_groups` finds the connected component `{Z1, Z2, Z3}`, assembles the `3×3` correlation matrix,
and **Cholesky-factors it** — nothing is hard-wired to two assets. Because the marginals are normal, the
Gaussian copula (`x = Φ⁻¹(Φ(L·z)) = L·z`) reproduces **exactly** the linear correlation on the shocks, so
the geometric closed form (which uses those `ρᵢⱼ`) matches the MC to Monte Carlo error. The
[spread example](CORRELATED_ASSETS_EXAMPLE.md) built its `2×2` correlation by hand; here the engine does
the `N×N` factorization.

### The geometric-basket closed form

`ln G = Σ wᵢ ln Sᵢ(T)` is normal with `μ_g = Σ wᵢ(ln Sᵢ,₀ + (r − σᵢ²/2)T)` and
`σ_g² = T·Σᵢ Σⱼ wᵢ wⱼ σᵢ σⱼ ρᵢⱼ` (for equal assets, `σ_g² = σ²T·(Σwᵢ² + ρ(1 − Σwᵢ²))`). Then
`geo_price = disc·[e^{μ_g + σ_g²/2}·N(d₁) − K·N(d₂)]` — a Black-76 on the geometric basket, built live
from `erf`.

---

## 3. Results (verified)

`seed = 31313`, `N = 60 000` (smoke test):

| quantity | value |
|---|---|
| **Geometric basket — closed form (`geo_price`)** | **7.844** |
| Geometric basket — MC (`basket_geo`) | 7.79 ± 0.08 — **matches** |
| Arithmetic basket — plain MC (`basket_arith`) | 8.40, std **10.83** |
| Arithmetic basket — control variate (`basket_cv`) | 8.46 ± 0.007, std **0.87** |
| **Variance reduction (std_plain / std_cv)** | **≈ 12.5×** (≈ 155× in variance) |

Three things fall out:

1. **The correlated draws are correct.** The geometric-basket MC lands within one CI half-width of the
   live closed form — a joint check of the native `N×N` Cholesky correlation and the geometric formula.
2. **AM–GM holds across assets.** Arithmetic (8.40) > geometric (7.79): the arithmetic mean dominates
   the geometric, so the arithmetic basket is worth more — the cross-asset analogue of the Asian's AM–GM.
3. **The control variate is excellent.** At ≈ 0.997 correlation the geometric basket cuts the estimator's
   standard deviation ≈ 12.5× (≈ 155× in variance) at no extra cost — even stronger than the Asian's
   time-average control, because diversification makes the two baskets track very tightly.

---

## 4. Engine features this exercises (and what's still open)

A **payoff on existing machinery** — no engine changes:

1. **Native N-asset correlation.** The `correlations` field + `build_corr_groups` + Cholesky, exercised
   at `N = 3` (validating that it is not two-asset-specific). Diversification (`σ_g < σ`) falls out.
2. **Geometric-basket control variate** via `run_stat2` — the cross-asset counterpart of the Asian
   geometric control.

Open items (see [`BASKET_OPTION_SCOPE.md`](BASKET_OPTION_SCOPE.md)), neither needed here:

- **Correlation ergonomics.** Correlations are declared **pairwise** (`O(N²)` entries by hand). A native
  `correlation_matrix` input — name the drivers once, give the matrix — would be far cleaner at larger
  `N` and could validate positive-definiteness up front.
- **Correlated *process* shocks.** This European basket uses terminal draws; a **path-dependent** basket
  (Asian-basket, worst-of, barrier-basket) needs correlated *per-step* shocks, which the engine does not
  support today (`correlations` live only on `random_variable`, not `stochastic_process`).

---

## 5. Takeaway

A European basket on `N` correlated assets is a drop-in extension of the spread example: the engine's own
`correlations` field builds the `N×N` Cholesky, the geometric basket gives a live closed-form control at
≈ 0.997 correlation (≈ 12.5× variance cut), and the geometric MC validates the whole correlated draw
against the closed form — all inside one `model.json`, no engine changes. The natural next steps are
correlation-matrix ergonomics and correlated process shocks for path-dependent multi-asset exotics.
