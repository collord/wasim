# Options Pricing Efficiency Analysis in WASiM

**Model:** [`schema_examples_manual/options_pricing_efficiency.json`](schema_examples_manual/options_pricing_efficiency.json)
**Regression test:** [`engine/tests/options_efficiency_smoke.rs`](engine/tests/options_efficiency_smoke.rs)

This note shows how WASiM represents and solves a *pricing-efficiency* analysis in the
sense of Glasserman, *Monte Carlo Methods in Financial Engineering* (Springer, 2003),
Chapter 1. The running example is a European call priced by Monte Carlo, but the point is
the **efficiency machinery**: how to express a Monte Carlo estimator, read its standard
error, compare a variance-reduced estimator against a plain one, and check the estimate
against an exact benchmark — all inside a single `model.json`.

---

## 1. What "efficiency analysis" means

A Monte Carlo price is an *estimator*: the sample mean of a discounted payoff over `N`
independent replications. Two numbers govern its quality (Glasserman §1.1.3):

- **Variance** `σ²` of the per-replication estimator. The standard error of the price is
  `SE = σ/√N`, so the error shrinks only as `1/√N` — quadrupling the work halves the error.
- **Work** `τ` — the compute cost of one replication.

The **efficiency** of an estimator is `∝ 1 / (σ² · τ)`. A variance-reduction technique is
worthwhile only if it lowers `σ²` by more than it raises `τ`. So an efficiency analysis is
three measurements:

1. the **estimate** (sample mean) and its **standard error** (`σ/√N`);
2. the **bias** — does the estimate converge to the true price, or to something off by a
   discretization error? (Glasserman Example 1.1.1);
3. the **variance ratio** between competing estimators at equal work.

WASiM produces all three from one run.

---

## 2. How the model maps onto WASiM

Terminal asset price is simulated **exactly** from the lognormal law (no time-stepping, so
**no discretization bias**):

```
S_T = S0 · exp( (r − σ²/2)·T + σ·√T·Z ),   Z ~ N(0,1)
```

One `Z` is drawn per realization (`random_variable`, family `normal`). Everything else is a
deterministic `expression` of that draw, so each realization is one cheap static evaluation.

| Concept (Glasserman) | WASiM element(s) | Role |
|---|---|---|
| Risk-neutral shock | `Z` (`random_variable`, N(0,1)) | one draw per replication |
| Exact terminal price | `ST_plus` = `S0·exp(drift + diffusion·Z)` | exact GBM law ⇒ unbiased |
| Antithetic partner | `ST_minus` = `S0·exp(drift − diffusion·Z)` | reuses `−Z` |
| Discounted payoff | `payoff_plus`, `discount`, `est_plain` | the plain estimator |
| Antithetic estimator | `est_antithetic` = `discount·½(payoff₊ + payoff₋)` | variance-reduced |
| Control | `disc_S_T` = `discount·S_T`, `E[·]=S0` | correlated, known mean |
| CV coefficient | `b_star` = `run_stat2(beta, disc_S_T, est_plain)` | b* = Cov/Var, cross-realization |
| Control-variate estimator | `cv_est` = `est_plain − b*·(disc_S_T − S0)` | variance-reduced |
| Closed-form benchmark | `bs_price` via `d1`,`d2`,`Nd1`,`Nd2` | exact reference |

The Black–Scholes reference is computed **live** from the same inputs, using the engine's
`erf` builtin: `Φ(x) = ½·(1 + erf(x/√2))`. Because `bs_price` depends only on constants it is
identical in every realization, so its reported mean *is* the exact price — change `S0`, `K`,
`r`, `σ`, or `T` and the benchmark tracks automatically.

### Getting the efficiency numbers out

The per-replication standard error is **not** in the default summary (which is only
`mean + p05…p95`). You opt specific elements into richer stats with a `ResultsSpec` on the
run config — the engine then attaches a `final_stats` block per element:

```jsonc
// run config passed to the engine (WasmEngine.run_json / RunConfig)
{
  "n_realizations": 100000,
  "seed": 12345,
  "results_spec": {
    "final_stats": true,
    "confidence": 0.95,
    "elements": ["est_plain", "est_antithetic", "bs_price"]
  }
}
```

`final_stats` returns, per element:

| field | meaning for efficiency |
|---|---|
| `mean` | the Monte Carlo **price estimate** (= exact price for `bs_price`) |
| `std` | the per-replication estimator standard deviation `σ` |
| `ci_half_width` | half-width of the 95% t-interval on the mean = `z · σ/√N` — the **Monte Carlo error** |
| `ci_lower` / `ci_upper` | the confidence band around the estimate |
| `skewness`, `excess_kurtosis` | shape of the payoff distribution (why the CI is what it is) |

> **Engine note.** `final_stats` is computed by the **v2** engine (`run_v2`), which is what
> the WASM bridge / frontend runs. A v1 `model.json` like this one is auto-normalized to v2
> on load (`normalize_v1`), so the `.json` is authored in the readable v1 schema but the
> analysis layer is fully available. The native v1 reference engine (`engine::run`) returns
> `analysis: None` — use `run_v2` (as the smoke test does) if you drive the engine directly.

---

## 3. Results (verified)

Parameters `S0 = K = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, `seed = 12345`,
`N = 100 000`. Reproduced by the smoke test (which runs a faster `N = 20 000`).

| quantity | value |
|---|---|
| **Black–Scholes exact price** | **10.4506** |
| Plain MC estimate | 10.4875 |
| Plain per-replication `std` (σ) | 14.76 |
| Plain 95% CI half-width | **0.0915** |
| Antithetic MC estimate | 10.4478 |
| Antithetic per-replication `std` | 7.34 |
| Antithetic 95% CI half-width | **0.0455** |
| Antithetic variance-reduction (σ²ₚ/σ²ₐ) | **≈ 4.0** (std ratio 2.0) |
| Control-variate estimate (`cv_est`) | 10.4236 |
| Control-variate per-replication `std` | 5.61 |
| Control-variate variance-reduction (σ²ₚ/σ²_cv) | **≈ 6.8** (std ratio 2.6) |
| `CI(25k) / CI(100k)` | **1.99** (theory: 2.0) |

(Control-variate figures at `N = 20 000` from the smoke test; the `std` ratio to plain is
scale-free so it is directly comparable.)

Four facts fall out — the three efficiency questions of §1, plus the control variate:

**(1) Standard error and the 1/√N law.** The plain CI half-width is `0.0915` at 100k and
`0.182` at 25k — the ratio `1.99` confirms error `∝ 1/√N`. To *halve* the error you must
*quadruple* the sample. This is the fundamental cost of Monte Carlo and the reason variance
reduction matters.

**(2) Zero discretization bias.** Both estimates sit within one CI half-width of `10.4506`.
Because `S_T` is sampled from its exact law rather than an Euler-stepped path, the estimator
converges to the true price, not to a discretized approximation — the "no bias" leg of
Example 1.1.1. (Contrast in §5.)

**(3) Efficiency gain, work-normalized.** Antithetic halves the standard deviation
(variance ↓ ~4×) but each replication does *two* payoff evaluations, so its work `τ` is ~2×.
The honest efficiency ratio is

```
efficiency_anti / efficiency_plain = (σ²_plain · τ_plain) / (σ²_anti · τ_anti)
                                   = (14.76² · 1) / (7.34² · 2)  ≈  2.0
```

So antithetic is about **2× more efficient**: to reach the antithetic CI half-width of
`0.0455`, plain MC would need ~400k payoff evaluations, versus ~200k for antithetic. The raw
4× variance drop overstates the win by exactly the 2× extra work — precisely the accounting
Glasserman insists on.

**(4) Control variate (Glasserman §4.1).** `cv_est` uses the discounted terminal price as a
control (known mean `E[e^{−rT}S_T] = S0`) and estimates the optimal coefficient
`b* = Cov(control, payoff)/Var(control)` *inside the model* via the bivariate
`run_stat2(beta, …)` reducer. It cuts the standard deviation to `5.61` (variance ↓ ~6.8×) at
essentially the plain estimator's cost (one extra element evaluation, no second payoff), so its
work-normalized efficiency gain is close to its raw variance gain — and it beats antithetic
here while remaining unbiased (`10.4236`, within one CI half-width of `10.4506`). This is the
gap that [`CONTROL_VARIATE_SCOPE.md`](CONTROL_VARIATE_SCOPE.md) Phase 1 closed.

---

## 4. Try it / extend it

- **Move the contract.** Every input (`S0`, `K`, `r`, `sigma`, `T`) is an editable
  `constant`; the live `bs_price` benchmark and both estimators update together. Push the
  call deep out-of-the-money (`K = 160`) and watch the payoff skew explode and the CI widen —
  the case where plain MC is worst and importance sampling (Glasserman §4.6) pays off most.
- **Scale N.** Lower `n_realizations` for interactive runs; raise it to tighten the CI. Plot
  `ci_half_width` vs `N` on log–log axes to see the `−½` slope.
- **Interpret the estimator, not just the mean.** Turn on `"distribution": true` in the
  `results_spec` to get the PDF/CDF of the discounted payoff — the long right tail is *why*
  the variance is large and antithetic helps.

---

## 5. What this model deliberately does *not* do (and how WASiM would)

- **Discretization-bias study (Example 1.1.1).** This model uses the exact terminal law, so
  bias is zero by construction. To *reproduce* discretization error you would simulate the
  path with an Euler step — a `stochastic_process` (GBM resampled each timestep) or an
  `accumulator` integrating `dS = S(r·dt + σ·√dt·Z)` — over `m` steps, then compare the
  estimate against `bs_price` as `m` grows. WASiM's timestep grid is exactly the knob for
  that sweep; it is left out here to keep the efficiency story clean.
- **Greeks / sensitivities.** Delta, vega, etc. (Glasserman Ch. 7) would use the engine's
  `sensitivity_v2` layer rather than the payoff estimator here.

> **Note — control variates are now supported.** Earlier drafts listed control variates as a
> gap: the optimal coefficient `b* = Cov(payoff, control)/Var(control)` needs a covariance
> between two elements across realizations, which the univariate `run_stat` couldn't express.
> [`CONTROL_VARIATE_SCOPE.md`](CONTROL_VARIATE_SCOPE.md) Phase 1 added the bivariate
> `run_stat2` reducer (`cov`/`corr`/`beta`), and this model now uses it (`b_star`, `cv_est`,
> §2–§3). Split-sample `b` (to remove the O(1/N) in-sample bias) and multi-control regression
> remain future phases.

---

## 6. One-line takeaway

WASiM expresses a Monte Carlo option price as an ordinary `expression` over one `N(0,1)`
draw, and its `results_spec.final_stats` turns that into a full efficiency read-out —
estimate, standard error, confidence interval, and (against the exact `erf`-based
Black–Scholes benchmark) bias — letting you compare estimators on Glasserman's
`variance × work` footing directly in the model.
