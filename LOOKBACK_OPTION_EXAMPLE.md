# Floating-Strike Lookback Call: A Running Extreme as the Strike

**Model:** [`schema_examples_manual/lookback_option_floating_strike.json`](schema_examples_manual/lookback_option_floating_strike.json)
**Test:** [`engine/tests/lookback_option_smoke.rs`](engine/tests/lookback_option_smoke.rs)
**Companions:** [`OPTIONS_PRICING_EFFICIENCY.md`](OPTIONS_PRICING_EFFICIENCY.md) · [`ASIAN_OPTION_EXAMPLE.md`](ASIAN_OPTION_EXAMPLE.md) · [`CORRELATED_ASSETS_EXAMPLE.md`](CORRELATED_ASSETS_EXAMPLE.md) · [`BARRIER_OPTION_EXAMPLE.md`](BARRIER_OPTION_EXAMPLE.md).

A **floating-strike lookback call** pays `S(T) − min_t S(t)` — you buy at the realized minimum and
sell at maturity, so the payoff is always ≥ 0. Where the [barrier](BARRIER_OPTION_EXAMPLE.md) used a
running extreme to *gate* a vanilla payoff, the lookback makes the running extreme **the strike
itself**. It rounds out the running-extreme family and shows the terminal-value accessor
(`TERMINAL_VALUE_SCOPE.md`) on a fresh payoff — plus a distinctive, exact **in-model identity** that
cross-checks the price without any closed form.

Grounded in Glasserman, *Monte Carlo Methods in Financial Engineering* (lookbacks / path extrema,
§3.2.2 on discrete monitoring). The continuous-monitoring closed form is Goldman–Sosin–Gatto (1979).

---

## 1. The problem

The floating-strike lookback call pays `(S(T) − m)` where `m = min_j S(t_j)` over the monitoring
dates. Two features:

1. **The strike is a running minimum** — a path extreme, exactly what `filter(min)` computes.
2. **Discrete monitoring biases it the *opposite* way from a barrier.** Discrete monitoring misses
   between-date dips, so the discrete minimum is **higher** (less extreme) than the continuous one —
   which makes the floating lookback worth **less**. The discrete MC price therefore sits **below**
   the continuous closed form, the mirror image of the barrier's over-pricing.

Contract: `S₀ = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, monitored on a `Δt = 0.02` grid (`m = 50`).

---

## 2. How it maps onto WASiM

| Concept | WASiM element |
|---|---|
| Per-step GBM shock, drift `r`, vol `σ` | `P` — `stochastic_process` (gbm); drift/vol reference `r`/`σ` (gap #1) |
| Exact GBM price path | `S` — `accumulator`, `rate = S*P` |
| **Floating strike = running minimum** | `run_min` — `filter`, `statistic = min`, expanding window, **`include_terminal`** (gaps #2 + #3) |
| True terminal price `S(T)` | `S_T` — `terminal_expression` `ref(S)` (gap #3) |
| **Lookback payoff** | `lookback = disc·(S_T − run_min)` — a `terminal_expression` |
| Continuous closed form | `lb_cont` — Goldman–Sosin–Gatto, live via `erf` |
| **In-model identity** | `min_mean = run_stat(mean, run_min)`, `lb_check = S₀ − disc·min_mean` |
| Control variate | `ctrl = disc·S_T` (mean `S₀`), `beta = run_stat2(beta, ctrl, lookback)`, `lookback_cv = lookback − beta·(ctrl − S₀)` |

The floating strike is literally one node — `filter(min, include_terminal)` — the same monitor as the
barrier, now consumed as the strike. `include_terminal` folds the terminal fixing `S(T)` so the
minimum covers `t₀…t_m`; `S_T` reads the true terminal for the sell leg. Both are the gap-#3
accessor.

### The exact identity (why no closed form is needed to trust the price)

For *any* monitoring scheme, discounting is linear:

```
price = disc·E[S(T) − m] = disc·E[S(T)] − disc·E[m] = S₀ − disc·E[m]
```

because `disc·E[S(T)] = disc·S₀·e^{rT} = S₀` under the risk-neutral GBM. So the lookback price equals
`S₀ − disc·E[min S]` **exactly** — no distributional assumption about the minimum. The model computes
`E[min S]` in-model with a `run_stat(mean)` over `run_min` and forms `lb_check = S₀ − disc·E[min]`;
the MC payoff mean must equal it. That is a self-consistency check the *engine* performs, independent
of any external formula — and it exercises the univariate `run_stat` reducer alongside the bivariate
`run_stat2` used for the control variate.

---

## 3. Results (verified)

`S₀ = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, `Δt = 0.02` (`m = 50`), `seed = 70707`, `N = 40 000`.
Continuous closed form (live, GSG): **`lb_cont = 17.22`**.

| quantity | value |
|---|---|
| **GSG continuous lookback (`lb_cont`)** | **17.22** |
| Identity `S₀ − disc·E[min S]` (`lb_check`) | **15.92** |
| Lookback — plain MC (`lookback`) | 15.87 ± 0.14 (95% CI), std **14.45** — **matches the identity** |
| Lookback — control variate (`lookback_cv`) | 15.90 ± 0.06, std **5.81** (~**2.5×** cut) |
| Control mean `disc·E[S(T)]` (`ctrl`) | 99.95 ≈ `S₀` |

Four things fall out:

1. **The price is self-consistent.** The MC lookback mean equals the in-model identity
   `S₀ − disc·E[min S]` within Monte Carlo error — a closed-form-free validation of the payoff, the
   terminal read, and the running-min monitor, driven by a `run_stat`.
2. **The §3.2.2 bias runs the other way.** The discrete MC price (≈ 15.9) sits **below** the
   continuous GSG value (17.22): discrete monitoring gives a *higher* minimum, so the floating
   lookback is worth **less** — the mirror image of the barrier, where discrete monitoring *over*-priced.
   The gap (≈ 1.3) is the O(√Δt) monitoring bias and shrinks as the grid refines.
3. **The control mean is exact.** `disc·E[S(T)] = S₀ = 100` by construction, so `disc·S(T)` is a
   valid control with a known mean.
4. **The control variate works.** `disc·S(T)` is ≈ 0.92-correlated with the lookback (both rise with
   `S(T)`), cutting the estimator's standard deviation ~2.5× (~6× in variance), unbiased.

---

## 4. Engine features this exercises

A pure **payoff** for the gap work — no new engine changes:

1. **Expression-valued process params (gap #1)** — `P` references `r`/`σ`.
2. **Native running statistic (gap #2)** — the floating strike is one `filter(min)`.
3. **Terminal-value accessor (gap #3)** — `include_terminal` folds `S(T)` into the minimum, and
   `terminal_expression` reads the terminal for the sell leg and the control; `lookback_cv` (a
   terminal expression) reads a `run_stat2` coefficient.
4. **`run_stat` + `run_stat2` together** — the univariate mean (for the identity) and the bivariate
   beta (for the control variate) in one model.

The one thing it does *not* implement is a discrete-monitoring **continuity correction** for the
lookback (the Broadie–Glasserman–Kou shift, analogous to the barrier's BGK step). The example instead
validates via the exact identity and brackets against the continuous form; adding a lookback BGK
correction (and showing the corrected reference matches the discrete MC) is a natural follow-on.

---

## 5. Takeaway

A running extreme is a first-class citizen: the same `filter(min, include_terminal)` that gated the
barrier is here the **strike** of a lookback, with `terminal_expression` supplying the terminal leg.
The price is validated two ways — an exact, closed-form-free `run_stat` identity
(`price = S₀ − disc·E[min]`) and the Goldman–Sosin–Gatto continuous form, which the discrete MC sits
below exactly as the §3.2.2 discretization bias predicts — and a `disc·S(T)` control variate cuts the
variance ~2.5×, all inside one validated `model.json`.
