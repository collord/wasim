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
| American put — LSM (`american_put`) | **6.024 ± 0.069** — matches the tree (small in-sample low bias) |
| European put — MC (`euro_put`) | 5.534 ± 0.084 |
| European put — Black–Scholes (`bs_euro`) | 5.5735 |
| **Early-exercise premium** | **≈ 0.49** |

Three things fall out:

1. **LSM prices early exercise.** The American put lands just below the binomial value (6.024 vs 6.0449
   at the effective maturity) — the expected **in-sample low bias** of LSM: the fitted policy is slightly
   sub-optimal, and pricing on the same paths that fit it biases downward. That's a feature to be aware
   of, not a bug; the fix (an out-of-sample pass and the Andersen–Broadie **dual upper bound** for a true
   confidence interval) is the Phase-2/3 work in the scope. (An earlier build mis-aligned the state and
   payoff panels by one step and over-priced at 7.55 — see §2; aligning them gives the correct 6.02.)
2. **The early-exercise premium is real.** American (6.04) clearly exceeds European (5.57) — the right to
   exercise early is worth ≈ 0.5 for an at-the-money put under positive rates.
3. **The path is validated.** The European put on the same simulated path matches Black–Scholes,
   confirming the GBM path feeding the LSM regression is correct.

## 4. Engine status

**Landed (Phase 1).** The `lsm` node: backward induction over the stored panel, ITM-filtered covariance
regression, per-realization cashflow injection. Additive — a new AST node handled like the run-stat
family; models without it are unchanged. Reuses `hist_store` and `regression_coefficients`.

**Still open** (from `AMERICAN_OPTION_SCOPE.md`), and honestly flagged:

- **In-sample bias / a true interval.** This is a low-biased point estimate. Out-of-sample pricing
  (fit on one path set, price on another) removes the bias; the **Andersen–Broadie dual** adds a
  matching *upper* bound, bracketing the true price. Phase 2/3.
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
