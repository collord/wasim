# Basket Option (N correlated assets)

**Status:** **Phases 1–2 built.** Phase 1: a European `N=3` basket
([`BASKET_OPTION_EXAMPLE.md`](BASKET_OPTION_EXAMPLE.md)) via the native `correlations` field,
geometric-basket control, ~12.5× variance cut. **Phase 2: a `correlation_matrices` model field** — an
ergonomic block that names the drivers once and gives the matrix, expanded into pairwise correlations
during normalization so it lowers to the existing Cholesky (proven bit-identical to the pairwise form in
`engine/tests/correlation_matrix_v2.rs`; v1 schema for now, v2-native lowering is a follow-on). Phase 3
(correlated *process* shocks, for path-dependent baskets) remains **proposed** — it touches the
determinism-critical per-step sampling path (GBM draws at both an init and a per-step site under the B1
sub-interval invariant) and warrants its own careful change.
**Motivation:** generalizes the two-asset [spread example](CORRELATED_ASSETS_EXAMPLE.md) to N assets and
exercises the geometric-basket control variate — the multi-asset analogue of the Asian control.

---

## 1. The payoff

A basket call pays `(Σᵢ wᵢ Sᵢ(T) − K)⁺` on a weighted sum of `N` correlated assets. The arithmetic
basket has **no closed form** (a sum of lognormals isn't lognormal — same intractability as the Asian
average), so Monte Carlo is genuinely required. The **geometric** basket `∏ᵢ Sᵢ(T)^{wᵢ}` *is* lognormal,
so it has a Black–Scholes-style closed form and `≈` the arithmetic basket — the textbook control variate
(exactly mirroring the Asian geometric control, but across assets instead of across time).

---

## 2. What the engine already does (verified)

Correlation is **not** a gap for a European basket:

- `build_corr_groups` (`engine.rs`) parses the pairwise `correlations` on `random_variable` elements,
  finds **connected components**, builds an **N×N** correlation matrix per component, and
  **Cholesky-decomposes** it (`cholesky` / `cholesky_matvec`). Nothing is hard-wired to 2 assets — the
  spread example just happens to use `N=2`.
- The spread example correlates **terminal draws**: correlated `random_variable` `Z`s → terminal
  lognormal prices `Sᵢ(T) = Sᵢ,₀·exp((r−σᵢ²/2)T + σᵢ√T·Zᵢ)` via a Gaussian copula. For a European basket
  (terminal payoff only) this is exactly right and generalizes to any `N` by adding assets and pairwise
  `correlations`.

So a European basket is a **drop-in extension** of the spread example: `N` correlated `random_variable`
draws, `N` terminal prices, a weighted-sum payoff, and a geometric-basket control with a live closed
form. **Phase 1 needs no engine change.**

## 3. The two gaps it does surface

1. **Correlation ergonomics (minor).** Correlations are declared **pairwise** on each
   `random_variable` — `O(N²)` hand-entered entries, and the matrix is implicit. A basket makes this
   painful. A native **`correlation_matrix`** input (name the assets once, give the matrix; the engine
   already builds the Cholesky) would replace `N(N−1)/2` scattered `CorrelationPair`s with one block, and
   let the model validate positive-definiteness up front.
2. **Correlated *process* shocks (real gap).** `correlations` live only on `random_variable` (a
   per-realization draw), **not** on `stochastic_process`. So a European basket (terminal draws) is fine,
   but a **path-dependent basket** — Asian-basket, worst-of/barrier-basket — needs correlated *per-step*
   shocks across the asset paths, which cannot be expressed today. Closing this means letting a
   correlation group span `stochastic_process` elements and applying the Cholesky factor to their
   per-step normal draws (the same `cholesky_matvec`, applied each step instead of once).

## 4. Design

- **Phase 1 — European basket example.** `basket_option.json`: `N=3`–`5` correlated `random_variable`
  draws → terminal prices → arithmetic-basket payoff; geometric-basket closed form (weighted geometric
  average of correlated lognormals is lognormal with `μ_g = Σwᵢ(ln Sᵢ,₀+(r−σᵢ²/2)T)`,
  `σ_g² = Σᵢ Σⱼ wᵢwⱼ σᵢσⱼ ρᵢⱼ T`) as a `run_stat2` control. Validation: geometric MC == closed form;
  arithmetic ≥ geometric (AM–GM); control cuts variance sharply (as in the Asian, ≈ correlation → 1).
- **Phase 2 — `correlation_matrix` input.** A single element/block naming `k` drivers and giving their
  matrix; lowers to the existing corr-group machinery. Backward compatible with pairwise `correlations`.
- **Phase 3 — correlated process shocks.** Extend corr groups to `stochastic_process` members; apply the
  group's Cholesky factor to the stacked per-step `Z`s each timestep. Unlocks Asian-basket, worst-of, and
  barrier-basket (which then also reuse `include_terminal`).

## 5. Phasing

| Phase | Deliverable | Engine work |
|---|---|---|
| **1** | European basket example + geometric-basket control | **none** |
| **2** | `correlation_matrix` input (ergonomics) | small–moderate (lower to existing Cholesky) |
| **3** | Correlated per-step process shocks | moderate (per-step `cholesky_matvec` over process draws) |

## 6. Test plan

- **Phase 1:** geometric-basket MC == live closed form within MC error; arithmetic ≥ geometric;
  realized sample correlation ≈ target (a σ=0-style probe on the `Z`s); geometric control cuts variance.
- **Phase 2:** a `correlation_matrix` model reproduces the pairwise-`correlations` model bit-for-bit.
- **Phase 3:** two correlated GBM *paths* reproduce the spread example's terminal correlation *and* carry
  it through the whole path (check per-step realized correlation); an Asian-basket prices sensibly.

## 7. Risk

Phase 1 low (example on proven correlation code). Phase 2 low–moderate (a lowering pass). Phase 3
moderate (touches the per-step draw path and determinism — pin with a constant-shock probe, as the
Cholesky ordering must be deterministic). **Bottom line:** the European basket is essentially free today;
the durable value is the `correlation_matrix` ergonomics and, for path-dependent multi-asset exotics,
correlated process shocks.
