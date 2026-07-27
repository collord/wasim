# Correlated Process Shocks: A Worst-of Asian Option

**Model:** [`schema_examples_manual/worst_of_asian_correlated.json`](schema_examples_manual/worst_of_asian_correlated.json)
**Tests:** [`engine/tests/worst_of_asian_smoke.rs`](engine/tests/worst_of_asian_smoke.rs) · [`engine/tests/correlated_processes_v2.rs`](engine/tests/correlated_processes_v2.rs)
**Scope:** [`BASKET_OPTION_SCOPE.md`](BASKET_OPTION_SCOPE.md) (Phase 3) · **Companions:** [`CORRELATED_ASSETS_EXAMPLE.md`](CORRELATED_ASSETS_EXAMPLE.md) (terminal correlation), [`ASIAN_OPTION_EXAMPLE.md`](ASIAN_OPTION_EXAMPLE.md).

The [spread example](CORRELATED_ASSETS_EXAMPLE.md) correlated two assets at their **terminals** (single
draws). A **path-dependent** multi-asset payoff can't do that — a running average is a function of the
whole path, so the *paths* themselves must co-move. This example prices a **worst-of Asian call**,
`disc·(min(Ā₁, Ā₂) − K)⁺` on the running averages `Āᵢ`, using the new **correlated process shocks**.

---

## 1. The engine feature (basket scope, Phase 3)

Correlations previously lived only on `random_variable` (a single per-realization draw). A
`stochastic_process` now takes a **`correlations`** field too:

```jsonc
{ "type": "stochastic_process", "id": "P1", "process": { "family": "gbm", … },
  "correlations": [ { "partner": "P2", "coefficient": 0.5 } ] }
```

The engine builds the connected group's `N×N` correlation matrix, Cholesky-factors it once, and at
**every timestep** draws a vector of i.i.d. normals, applies the Cholesky factor, and feeds each process
its correlated shock (`sample_gbm_with_z`). So the whole GBM *paths* co-move — not just their terminals.

- **Additive & opt-in:** a process with no `correlations` draws independently, exactly as before — the
  RNG stream is unchanged, so every existing model is byte-for-byte identical. The per-step group draw is
  a no-op when no process declares a correlation.
- **GBM only** for now (the options case). OU/mean-reverting processes keep drawing independently.
- **Marginals preserved:** the Cholesky couples only the shocks, so each asset's own law is unchanged
  (`E[Sᵢ(T)] = Sᵢ,₀·e^{rT}`) — verified in `correlated_processes_v2.rs`.

## 2. How it maps onto WASiM

| Concept | WASiM element |
|---|---|
| Two correlated per-step shocks | `P1`, `P2` — `stochastic_process` (gbm); `P1` declares `correlations` to `P2` |
| Exact GBM paths | `S1`, `S2` — `accumulator`, `rate = Sᵢ·Pᵢ` |
| Running averages | `avg1`, `avg2` — `filter`, `statistic = mean` (expanding, gap #2) |
| Worst-of underlying | `worst = min(avg1, avg2)` |
| **Worst-of Asian call** | `worstof = disc·(worst − K)⁺` |
| Single-asset Asians (bound) | `asian1`, `asian2 = disc·(avgᵢ − K)⁺` |

## 3. Results (verified)

`S₀ = K = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, `Δt = 0.02`, `ρ = 0.5`, `N = 60 000`:

Two checks (there is no closed form for a discretely-averaged worst-of):

1. **Structural bound.** `min(Ā₁, Ā₂) ≤ Āᵢ`, so the worst-of Asian is worth no more than either
   single-asset Asian — and it is a sizeable positive value.
2. **Correlation monotonicity** — the defining signature of the feature. Re-pricing at `ρ ∈ {−0.5, 0.5,
   0.95}` shows the worst-of price **rises sharply with correlation** — `0.94 → 2.94 → 4.79`: when the
   assets co-move they seldom fall apart, so the *minimum* average is higher and the option is worth
   more; when they're anti-correlated, one is usually low when the other is high, dragging the minimum
   down. (The worst-of 2.94 also sits well below either single-asset Asian, ~5.65.) The strict ordering
   confirms the correlation is genuinely applied *through the path*, not just at the terminal.

The direct proof that the shocks carry the target correlation is `correlated_processes_v2.rs`: two GBM
processes declared at `ρ = 0.6` produce paths whose terminal log-returns are correlated at **0.600**
(uncorrelated: −0.004), with marginals unchanged.

## 4. Engine status

**Landed** — correlated process shocks (basket scope Phase 3): a `correlations` field on
`stochastic_process`, a `build_process_corr_groups` companion to the sample-correlation builder, per-step
Cholesky injection via `sample_gbm_with_z`. Additive and backward-compatible.

**Still open** (future): a native `correlation_matrix` block *for processes* (the
`correlation_matrices` model field currently expands onto `random_variable` drivers only); correlated OU
processes; and correlation coefficients that reference editable constants (they are literals on the
process today).

## 5. Takeaway

Correlated *process* shocks complete the correlation story: terminal correlation (`random_variable` +
Cholesky) was already there for European multi-asset payoffs; this adds **per-step** correlation so the
asset *paths* co-move, unlocking path-dependent multi-asset exotics — worst-of / best-of, Asian baskets,
two-asset barriers. The worst-of Asian here is priced on genuinely correlated paths, its price moves the
right way with `ρ`, and the underlying shock correlation is validated to hit its target exactly.
