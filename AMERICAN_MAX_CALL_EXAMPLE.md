# American Max-Call on Two Correlated Assets — Multi-Asset Longstaff–Schwartz

**Model:** [`schema_examples_manual/american_max_call_lsm.json`](schema_examples_manual/american_max_call_lsm.json)
**Test:** [`engine/tests/american_max_call_lsm_smoke.rs`](engine/tests/american_max_call_lsm_smoke.rs)
**Scope:** [`AMERICAN_OPTION_SCOPE.md`](AMERICAN_OPTION_SCOPE.md) (Phase 4) · **Companions:** [`AMERICAN_OPTION_EXAMPLE.md`](AMERICAN_OPTION_EXAMPLE.md) (single-asset LSM), [`CORRELATED_PROCESSES_EXAMPLE.md`](CORRELATED_PROCESSES_EXAMPLE.md) (correlated paths).

The single-asset [American put](AMERICAN_OPTION_EXAMPLE.md) regressed the continuation value on a cubic
basis of one underlying. A **max-call on two assets** — payoff `max(max(S₁, S₂) − K, 0)`, exercisable at
any date — needs the continuation value as a function of **both** underlyings at once. This example is the
first **multi-asset early-exercise** option in the corpus: it extends the `lsm` node to a **multivariate
basis** over several state panels, running on the **correlated per-step paths** from the basket work.

---

## 1. Why a max-call, and why dividends

A max-call is the classic Broadie–Glasserman / Andersen–Broadie multi-asset American benchmark. One
subtlety matters for the example to be meaningful:

> **Without dividends, an American max-call is never exercised early** — its price equals the European
> one. The discounted `max(S₁, S₂)` is a submartingale (a convex function of martingales), so holding is
> always at least as good as exercising, exactly as for a single-asset call.

So we give each asset a **continuous dividend yield `q = 0.10`** (risk-neutral drift `r − q`). The
dividend is what makes early exercise optimal and produces a genuine early-exercise premium for the LSM
machinery to capture.

## 2. The engine feature (`lsm` multi-asset, Phase 4)

The `lsm` node gains an optional **`states`** list. With it, the continuation regression uses a
**multivariate monomial basis** over the primary `state` and every element in `states`, up to total
degree `basis`:

```jsonc
{ "op": "lsm", "state": "S1_lag", "states": ["S2_lag"], "payoff": "h",
  "basis": 3, "rate": 0.05, "folds": 2 }
```

- For **degree 3 over `(S₁, S₂)`** the basis is `{S₁, S₂, S₁², S₁S₂, S₂², S₁³, S₁²S₂, S₁S₂², S₂³}` (9
  monomials, constant excluded — the covariance regression carries the intercept via the means).
- The backward pass reads **each** state's `[dates × paths]` panel from `hist_store`, scales each by its
  own `S₀`, builds the monomial row per path, and fits the same ITM covariance regression as before.
- **Byte-identical fallback:** with `states` empty (the single-asset case), the monomial basis reduces to
  the univariate power basis `{S, S², …, Sᵈ}` — exactly the old code path. The single-asset put test is
  unchanged.

The two assets co-move via **correlated per-step process shocks** (`correlations` on the GBM process,
`ρ = 0.30`; see [`CORRELATED_PROCESSES_EXAMPLE.md`](CORRELATED_PROCESSES_EXAMPLE.md)), so the paths
themselves are correlated, not just their terminals — required for a path-dependent early-exercise
decision.

The same **state/payoff timing alignment** as the single-asset case applies: each state panel is
`Sᵢ_lag = ref(Sᵢ)` (start-of-step read), matching the payoff, so no one-step-future foresight leaks in.
The option effectively matures at `T_eff = T − Δt`.

## 3. Validation — a Boyle–Evnine–Gibbs lattice

There is no closed form for an American max-call, so we validate against a **Boyle–Evnine–Gibbs (BEG)
2-asset binomial lattice** — a four-branch recombining tree that carries the correlation exactly. At the
LSM effective maturity `T_eff = 0.98` the lattice gives `9.56` (250 steps; converged). The European
max-call on the same lattice is `8.92`, so the early-exercise premium is real (`≈ 0.64`).

## 4. Results (verified)

`S₁,₀ = S₂,₀ = K = 100`, `r = 0.05`, `q = 0.10`, `σ = 0.20`, `ρ = 0.30`, `T = 1`, `Δt = 0.02` (50
exercise dates), degree-3 bivariate basis, `N = 40 000`, `seed = 20250727`:

| quantity | value |
|---|---|
| **American max-call — BEG lattice at `T_eff = 0.98`** | **9.56** |
| American max-call — multi-asset LSM **out-of-sample** (`american_maxcall`, 2-fold) | **9.35 ± 0.09** — valid lower bound |
| American max-call — multi-asset LSM in-sample (`american_maxcall_is`) | 9.35 ± 0.09 |
| European max-call — MC on the same paths (`euro_maxcall`) | 8.91 ± 0.13 |
| **Early-exercise premium** | **≈ 0.44** |

Three things fall out:

1. **The multi-asset LSM tracks the lattice as a lower bound.** The out-of-sample price (9.35) sits just
   below the lattice value (9.56). LSM prices under a *finite-basis* exercise policy, which is sub-optimal,
   so it is a **lower bound** — being ~0.2 below a converged lattice (a ~2% basis-suboptimality gap) is the
   correct direction, not an error. A degree-3 monomial basis in `(S₁, S₂)` cannot perfectly represent the
   continuation value's dependence on `max(S₁, S₂)`; the gap is the price of that approximation.
2. **The early-exercise premium is real.** American (9.35) exceeds European on the same paths (8.91) — the
   dividend makes the right to exercise early worth ≈ 0.44.
3. **The correlated paths are validated.** The European max-call MC (8.91) matches the European BEG lattice
   (8.92) to within Monte Carlo error — confirming the per-step `ρ = 0.30` correlation is applied correctly
   through the whole path.

## 5. Engine status

**Landed** — multi-asset LSM (American scope Phase 4): a `states` list on the `lsm` node, a multivariate
monomial basis (`monomial_exponents` / `basis_row`) over any number of state panels, the backward pass
reading and scaling each panel. Additive and backward-compatible — a single-asset model (empty `states`)
is byte-identical, and the single-asset put test passes unchanged.

**Still open** (future):

- **Richer bases for the max feature.** A monomial basis in `(S₁, S₂)` is generic; adding `max(S₁, S₂)` or
  the payoff itself as an explicit basis function would tighten the lower bound (standard for max-calls).
- **The tight dual** is still gated on nested simulation (`AMERICAN_OPTION_SCOPE.md` §8) — the single-asset
  hedged-martingale dual does not generalize cleanly to the max payoff without an inner sampler.
- **Combinatorial basis growth.** The monomial count grows as `C(k + d, d) − 1` in `k` assets and degree
  `d`; beyond a handful of assets an orthogonal or payoff-adapted basis is preferable.

## 6. Takeaway

WASiM prices a genuinely multi-asset American option: the `lsm` node's new `states` list turns the
continuation regression into a multivariate least-squares fit over several stored path panels, running on
the correlated per-step paths from the basket work. The out-of-sample price is a valid lower bound that
tracks a Boyle–Evnine–Gibbs 2-asset lattice, the dividend-driven early-exercise premium is captured, and
the correlated paths are validated against the European lattice — all while leaving the single-asset code
path byte-identical.
