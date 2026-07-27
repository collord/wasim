# Monte Carlo Greeks: Likelihood Ratio vs Bump-and-Revalue (Digital Delta)

**Model:** [`schema_examples_manual/digital_option_greeks.json`](schema_examples_manual/digital_option_greeks.json)
**Test:** [`engine/tests/digital_greeks_smoke.rs`](engine/tests/digital_greeks_smoke.rs)
**Scope:** [`DIGITAL_OPTION_SCOPE.md`](DIGITAL_OPTION_SCOPE.md) · **Payoff:** [`DIGITAL_OPTION_EXAMPLE.md`](DIGITAL_OPTION_EXAMPLE.md)

The [digital example](DIGITAL_OPTION_EXAMPLE.md) showed the *price* of a discontinuous payoff converges
fine. This one computes its **delta** — where naive Monte Carlo struggles — and contrasts the two
standard estimators, validated against the analytic digital delta.

**It needs no new engine feature.** The likelihood-ratio delta turned out to be a plain expression,
because the driving normal `Z` is already an explicit `random_variable`. (The `DIGITAL_OPTION_SCOPE.md`
Phase-3 "score node" is therefore unnecessary for exact-terminal models; the remaining engine work is
only for *path-dependent* Greeks, where the driver lives inside a `stochastic_process` — see below.)

---

## 1. The two estimators

For a cash-or-nothing call `V = disc·1{S(T) > K}`:

- **Likelihood-ratio method (LRM).** Differentiate the *density*, not the payoff:
  `Δ = E[V · ∂ log f/∂S₀]`. For GBM the score is `∂ log f/∂S₀ = Z/(S₀·σ√T)`. The payoff is left
  untouched, so its discontinuity is irrelevant and the estimator's variance is **low and independent of
  any bump size**.
- **Bump-and-revalue (central, common random numbers).** `Δ ≈ (V(S₀+ε) − V(S₀−ε))/2ε`, reusing the same
  `Z` on both bumped spots. For a digital, `V(S₀+ε) − V(S₀−ε)` is nonzero only on the rare paths where
  `K` falls between the two bumped terminals, so the estimator is a **spike** — same mean, much higher
  variance, and it worsens as `ε → 0`.

Both are unbiased for the analytic delta `Δ = disc·n(d₂)/(S₀·σ√T)` (`n` = standard normal pdf).

## 2. How it maps onto WASiM

| Concept | WASiM element |
|---|---|
| Driver (exposed) | `Z` — `random_variable` `N(0,1)` |
| Shared lognormal multiplier (CRN) | `mult = exp((r−σ²/2)T + σ√T·Z)`; `ST = S₀·mult` |
| Price | `cash_call = disc·1{S(T) > K}` |
| **LRM score / delta** | `score = Z/(S₀·σ√T)`, `delta_lrm = cash_call·score` (its mean is Δ) |
| **Bump delta (CRN)** | `ST_up=(S₀+ε)·mult`, `ST_dn=(S₀−ε)·mult`, `delta_bump = (cash_up−cash_dn)/2ε` |
| Analytic delta | `delta_analytic = disc·n(d₂)/(S₀·σ√T)` |

Because both bumped spots reuse the same `mult` (the same `Z`), the bump uses **common random numbers** —
without that its variance would be hopeless. A Greek is then just the `final_stats.mean` of an estimator
element.

## 3. Results (verified)

`S₀ = K = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, `ε = 1`, `seed = 13131`, `N = 200 000`:

| quantity | value |
|---|---|
| **Analytic digital delta** | **0.018762** |
| LRM delta | 0.01870 ± 0.00012, std **0.028** |
| Bump delta (CRN, ε=1) | 0.01871 ± 0.00041, std **0.093** |
| **std ratio (bump / LRM)** | **≈ 3.3×** |

Both estimators hit the analytic delta; the **LRM estimator has ≈ 3.3× lower standard deviation**, and the
gap *widens as ε shrinks* (LRM variance is ε-free, bump variance ∝ 1/ε). On a discontinuous payoff the
likelihood-ratio method is the right tool — the pathwise estimator, which differentiates the payoff, is
inapplicable here (the derivative is a Dirac at `K`).

## 4. Engine status

- **Delta / vega by LRM or bump: expressible today** for exact-terminal (European-style) payoffs — the
  driver is an explicit element, so the score is an expression and the Greek is a `run_stat` mean. This
  example is the demonstration; `DIGITAL_OPTION_SCOPE.md` Phase 1/3 are effectively covered.
- **Still open — path-dependent Greeks.** When the driver lives *inside* a `stochastic_process` (a
  stepped path), its per-step normals aren't exposed, so LRM/pathwise can't reference them. Exposing a
  process's driving normals (or a first-class `greek` construct) is the remaining engine work, and is the
  natural companion to the correlated-process-shocks feature (both are about the process's normal draws).

## 5. Takeaway

A digital's delta — the canonical hard case for MC Greeks — is computed two ways and validated against
the closed form, showing the likelihood-ratio method's variance advantage on a discontinuous payoff, with
**no engine change**: the driver is already exposed, so a Greek is a mean of `payoff·score`. Sensitivities
for path-dependent payoffs remain the one piece that needs the engine to expose process drivers.
