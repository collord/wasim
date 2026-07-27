# American Put via Longstaff–Schwartz Least-Squares Monte Carlo

**Model:** [`schema_examples_manual/american_put_lsm.json`](schema_examples_manual/american_put_lsm.json)
**Test:** [`engine/tests/american_put_lsm_smoke.rs`](engine/tests/american_put_lsm_smoke.rs)
**Scope:** [`AMERICAN_OPTION_SCOPE.md`](AMERICAN_OPTION_SCOPE.md)

The first **early-exercise** option in the corpus, and the first use of a **backward** execution mode.
An American option can be exercised at any date before `T`; its price is a *supremum over stopping
times*, which a forward simulation can't evaluate directly (it would need to know the future). The
**Longstaff–Schwartz** algorithm solves it by backward induction with regression, and this example
adds it to the engine as the **`lsm`** construct.

---

## 1. What LSM does

```
at maturity:            V = intrinsic(S_T)                         on every path
for each earlier date t (BACKWARD):
    over IN-THE-MONEY paths, regress the discounted future value on a basis of S_t
    continuation(t) = fitted;  exercise where intrinsic(t) > continuation, else carry V forward
price = mean over paths of the discounted cashflow at each path's stopping time
```

The regression estimates the **conditional continuation value** `E[future | S_t]` from the cross-section
of paths — that is the trick that makes optimal stopping tractable by Monte Carlo.

## 2. The engine feature (`lsm`)

Before this, the engine's run loop was strictly **forward and per-realization**; cross-path information
was available only in the terminal two-pass reduction. LSM needs cross-path regression at *every* date,
run *backward*. The enabling insight (from `AMERICAN_OPTION_SCOPE.md`): **`time_history` already stores
the full `[dates × paths]` panel** in `hist_store`, so LSM is a **post-run backward analogue of the
existing terminal two-pass** — no new forward machinery.

An `lsm` AST node:

```jsonc
{ "op": "lsm", "state": "S", "payoff": "h", "basis": 3, "rate": 0.05 }
```

- `state` — element whose history is the regression state (the underlying `S`).
- `payoff` — element whose history is the immediate-exercise value (`h = max(K − S, 0)`).
- `basis` — polynomial degree in the (scaled) state; `rate` — the per-step discount `exp(−r·Δt)`.

**How it runs** (reusing existing pieces): pass 1 force-saves the `time_history` of `state` and `payoff`;
the backward pass walks `hist_store` from maturity, at each date fitting the shared **covariance
regression** (`regression_coefficients`, also used by `run_regress`) over in-the-money paths and applying
the exercise rule; each path's discounted-to-t0 cashflow is injected per realization through the same
channel as `run_split_beta`. The `lsm` element's mean is then the price (with a Monte Carlo CI).

### One correctness subtlety: align the state and payoff timing

A stock records its **end-of-step** value (`S_{k+1}`) in `time_history`, but an ordinary expression
reads a stock's **start-of-step** value (`S_k`). If the regression *state* used the stock's own history
(`S_{k+1}`) while the *payoff* `h = max(K − S, 0)` read `S_k`, the two panels would be misaligned by one
step — regressing on a one-step-**future** state leaks foresight into the exercise decision and
**over-prices** (an early build did exactly this: 7.55 vs a true ≈ 6.0). The fix is to read both at the
same timing: the state panel is `S_lag = ref(S)` (start-of-step, matching `h`). A consequence is that the
last readable date is `S(T − Δt)`, so the option effectively matures at `T_eff = T − Δt` — the same
one-step convention the barrier/lookback examples use.

## 3. Results (verified)

`S₀ = K = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, `Δt = 0.02` (50 exercise dates), cubic basis,
`N = 40 000`, `seed = 20250727`:

| quantity | value |
|---|---|
| **American put — binomial tree at `T_eff = 0.98`** | **6.0449** |
| American put — LSM **out-of-sample** (`american_put`, 2-fold) | **6.015 ± 0.069** — nearly unbiased lower bound |
| American put — LSM in-sample (`american_put_is`) | 6.024 ± 0.069 |
| American put — **dual upper bound** (`american_dual`) | **9.704 ± 0.046** (valid, loose) |
| European put — MC / Black–Scholes | 5.534 / 5.5735 |
| **Early-exercise premium** | **≈ 0.48** |

Four things fall out:

1. **LSM prices early exercise, and the estimate is bracketed.** The out-of-sample price (6.015) sits
   just below the binomial value (6.0449), and the **dual is a valid upper bound** (9.704 ≥ 6.045), so
   the true price is provably in **[6.015, 9.704]** — a primal–dual sandwich. The lower bound is tight
   (within ~0.4% of the tree); the upper bound is *loose* because a single hedging instrument can't
   replicate an American option (see §4). (An earlier build mis-aligned the state and payoff panels by
   one step and over-priced at 7.55 — see §2; aligning them gives the correct 6.02.)
2. **Out-of-sample removes the in-sample bias.** In-sample LSM fits the policy and prices on the *same*
   paths, so the policy overfits the noise. The 2-fold cross-fit (`folds = 2`) fits the policy on one
   fold and prices the other under it, so every path is priced out-of-sample — a nearly unbiased lower
   bound. Here the in-sample (6.024) and OOS (6.015) agree within Monte Carlo error (the in-sample bias
   is small at `N = 40 000`); the machinery is what matters, and it scales with the bias.
3. **The early-exercise premium is real.** American (6.02) clearly exceeds European (5.57) — the right
   to exercise early is worth ≈ 0.48 for an at-the-money put under positive rates.
4. **The path is validated.** The European put on the same simulated path matches Black–Scholes.

## 4. Engine status

**Landed.**

- **Phase 1 — `lsm` node.** Backward induction over the stored panel, ITM-filtered covariance
  regression, per-realization cashflow injection. Reuses `hist_store` and `regression_coefficients`.
- **Phase 2 — out-of-sample cross-fit** (`folds` on the `lsm` node). `folds ≥ 2` fits the policy on the
  complementary folds and prices each fold under it, so every path is priced out-of-sample — a nearly
  unbiased **lower** bound. `folds = 1` (default) is the in-sample estimate.
- **Phase 3 — dual upper bound** (`lsm_dual` node). The Rogers/Andersen–Broadie dual with the
  **underlying as the hedging martingale**, `M_t = θ·(discᵗ·S_t − S_0)`. Since `discᵗ·S_t` is a true
  martingale under the risk-neutral measure, `E[maxₜ(Zₜ − Mₜ)] ≥ price` for every `θ`, so minimizing
  over `θ` gives a **valid** upper bound. Together with the OOS lower bound it brackets the true price.

All additive — new AST nodes handled like the run-stat family; models without them are unchanged.

**Still open**, and honestly flagged:

- **A *tight* upper bound needs nested simulation.** The hedged-martingale dual is rigorous but loose
  (9.7 vs a true 6.0) — one instrument can't replicate the option. The tight Andersen–Broadie dual
  builds the Doob martingale of the *value* process via inner sub-simulations at each date; that needs
  a nested-simulation capability the engine doesn't have (a non-nested regression martingale was tried
  and **collapses** — it reproduces the primal instead of bounding it, so it was not shipped).
- **Basis / conditioning.** A cubic monomial basis in `S/S₀`, ITM-only, is the classic choice; richer
  or orthogonalized bases (Laguerre/Chebyshev) and multi-factor state (for max/min-of-assets Americans,
  reusing the correlation work) are natural extensions.
- **Exercise-date granularity.** Every grid step is an exercise date (the American limit for a fine
  grid); a Bermudan subset would be a small addition.

## 5. Takeaway

WASiM prices a genuinely American option: the `lsm` construct runs Longstaff–Schwartz as a **post-run
backward pass** over the `time_history` panel, reusing the covariance regression and the per-realization
injection channel that already existed. The price matches a binomial tree (up to the known in-sample
bias) and shows the early-exercise premium over the European value on the same path. Removing the bias
with an out-of-sample pass and a dual upper bound is the clear next step.
