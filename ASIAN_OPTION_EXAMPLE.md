# Asian Option with a Geometric Control Variate

**Model:** [`schema_examples_manual/asian_option_control_variate.json`](schema_examples_manual/asian_option_control_variate.json)
**Test:** [`engine/tests/asian_option_smoke.rs`](engine/tests/asian_option_smoke.rs)

A companion to [`OPTIONS_PRICING_EFFICIENCY.md`](OPTIONS_PRICING_EFFICIENCY.md). Where that
example priced a European call by **exact** terminal simulation (no time-stepping), this one prices
an **arithmetic-average Asian call** — a *path-dependent* payoff that forces the engine's dynamical
core (a stepped GBM path plus running averages) — and reduces its variance with the classic
**geometric-average Asian control variate**, exercising the control-variate machinery
(`run_stat2`) end-to-end on the problem it was invented for.

Grounded in Glasserman, *Monte Carlo Methods in Financial Engineering*: the Asian payoff and its
discretization are §1.1.2 / Example 1.1.2 and Eq. 3.28 (`S̄ = (1/n)Σ S(tᵢ)`, *"no exact formulas …
because the distribution of S̄ is intractable"*); the geometric control is the textbook variance
reducer (§4.1).

---

## 1. The problem

The **arithmetic** Asian call pays `(S̄_A − K)⁺` on the average price `S̄_A = (1/m)Σ S(tⱼ)` over
monitoring dates. `S̄_A` has no closed form, so Monte Carlo is genuinely required. The **geometric**
average `S̄_G = exp((1/m)Σ ln S(tⱼ))` *does* have a closed form (Kemna–Vorst — the log-average of a
GBM is normal), and `S̄_G ≈ S̄_A` to second order in volatility, so the geometric payoff is an almost
perfect control: correlated with the arithmetic payoff at ≈ 0.99, with a known mean.

---

## 2. How it maps onto WASiM

Unlike the European example, everything here lives on the **timestep grid**:

| Concept | WASiM element |
|---|---|
| Per-step GBM shock | `P` — `stochastic_process` (gbm, drift `r`, vol `σ`); returns a per-step *rate* |
| Exact GBM price path | `S` — `accumulator`, `rate = S*P` ⇒ `S_{k+1} = S_k·exp(logret)` (exact, not Euler-biased) |
| `Σ S(tⱼ)`, `Σ ln S(tⱼ)` | `sumS`, `sumLnS` — accumulators, `rate = S/dt` and `ln(S)/dt` |
| Monitoring-point count `m` | `cnt` — accumulator, `rate = 1/dt` |
| Averages | `avg_arith = sumS/cnt`, `avg_geo = exp(sumLnS/cnt)` |
| Discounted payoffs | `payoff_arith`, `payoff_geo` = `disc·(avg − K)⁺` |
| Closed-form control mean | `geo_price` (Kemna–Vorst, live via `erf`) |
| CV coefficient / estimator | `beta = run_stat2(beta, payoff_geo, payoff_arith)`, `cv_arith = payoff_arith − beta·(payoff_geo − geo_price)` |

The GBM idiom is worth calling out: the process node returns a **per-step rate** `P` such that the
accumulator update `S += S·P·dt` equals `S·(exp(logret) − 1)`, so `S` follows *exact* geometric
Brownian motion at each monitoring date — no Euler discretization bias in the marginal.

### The averaging convention (and why `cnt`, not a fixed `n`)

An expression that reads an accumulator sees the accumulator's value at the **start of the current
step** — one step behind its end-of-run value. Read naively, `sumS/n` (with `n` a constant) is
therefore off by one step. The fix is to divide by the **same-lagged** step counter `cnt`: both
`sumS` and `cnt` are read one step stale, so `avg = sumS/cnt` is exactly the average over the `m`
points actually summed, whatever the grid. A deterministic (σ=0) probe confirmed the identity. The
average covers `t_j = j·dt, j = 0 … m−1` (includes the initial fixing `S₀`); the closed form uses
the same `m = cnt` and `dt` so it tracks the MC exactly.

---

## 3. Results (verified)

`S0 = K = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, monthly grid, `seed = 424242`. From the smoke test
(which runs a coarser 6-step grid; the live closed form self-adjusts). At the model's default 12-step
grid, `N = 30 000`:

| quantity | value |
|---|---|
| **Geometric Asian — closed form (`geo_price`)** | **4.855** |
| Geometric Asian — MC (`payoff_geo`) | 4.810 ± 0.076 (95% CI) — **matches the closed form** |
| Arithmetic Asian — plain MC (`payoff_arith`) | 5.002, std **7.01** |
| Arithmetic Asian — control variate (`cv_arith`) | 5.048, std **0.18** |
| **Variance reduction (std_plain / std_cv)** | **≈ 38×** (≈ **1450× in variance**) |

Three things fall out:

1. **The path model is correct.** The geometric MC price lands within one CI half-width of the live
   Kemna–Vorst closed form — a stringent joint check of the GBM path, the averaging convention, and
   the formula.
2. **AM–GM holds.** Arithmetic (5.00) > geometric (4.81): the arithmetic average dominates the
   geometric, so the arithmetic Asian is worth more.
3. **The control variate is spectacular.** A ≈ 0.99-correlated control with a known mean cuts the
   estimator's standard deviation ~38× at essentially no extra cost — plain MC would need ~1450× as
   many paths to match `cv_arith`'s precision. This is the geometric-Asian control (Glasserman §4.1)
   running on the `run_stat2` primitive from `CONTROL_VARIATE_SCOPE.md`.

---

## 4. Engine work that would better support this model configuration

Building this surfaced real friction. None of it blocked the example, but each would make
path-dependent / averaging models substantially cleaner. Roughly in priority order:

1. **Expression-valued process (and distribution) parameters.** `ProcessSpec.mean` / `stddev` are
   static `Quantity` values, so the path's drift and vol **cannot reference** the editable `r` /
   `sigma` constants — they are hard-coded in `P` and must be kept in sync by hand (see the model's
   "engine gap #1" notes). `reversion_rate` / `reference_value` / `initial_value` already accept
   `QuantityOrFormula`; extending `mean`/`stddev` (and the analogous distribution parameters) to the
   same would let one editable `sigma` drive both the path and the closed form. **Highest-value fix.**

2. **A first-class time-average / running-statistic node.** Averaging a signal over the run today
   needs a hand-built `sumS` accumulator *plus* a `cnt` accumulator *plus* a division, and the
   author must know the one-step-lag trick to get the count right. A native "running mean / time
   average of X" element (GoldSim has this) would collapse `sumS + cnt + avg_arith` into one node and
   remove the footgun. The `results_spec` reporting-period `average` reduction is close in spirit but
   is **post-run analysis**, not an in-model value a payoff can consume.

3. **Lag-free / end-of-step stock reads (or a terminal-value accessor).** An expression reads an
   accumulator's *start-of-step* value, so (a) the average is one step stale — worked around with
   `cnt` — and (b) trying to add the **terminal** `S(T)` to shift the average to the more standard
   `t_1 … t_n` window failed: referencing the stock's terminal value from an expression returned
   `0`. The example therefore averages `t_0 … t_{m-1}` (with the initial fixing). A way to read an
   accumulator's end-of-step / final value from an expression would allow the conventional window and
   remove the `cnt` workaround.

4. **A GBM (and OU) *level* process.** The path needs the `process`-drives-`accumulator` idiom
   (`rate = S*P`) because the GBM process node returns a per-step *rate*. A process family that
   returns the **level** directly (as the mean-reverting branch already does internally) would let
   `S` be a single node instead of a process + accumulator pair.

5. **Grid/parameter coupling guards.** `T` must equal `simulation_settings.duration`, and the process
   vol's time unit must match the timestep unit; nothing checks these. (The `cnt`/`dt` construction
   already removed the earlier `n = duration/timestep` coupling — a good pattern to make native.)

---

## 5. Takeaway

WASiM can price a genuinely path-dependent option and drive a high-leverage control variate on it:
an exact GBM path (process + accumulator), running averages (sums ÷ a step counter), a live
Kemna–Vorst reference (via `erf`), and the `run_stat2` control-variate coefficient — a ~38×
variance reduction, validated against closed form. The averaging ergonomics (§4) are where the
engine could most improve for this class of model.
