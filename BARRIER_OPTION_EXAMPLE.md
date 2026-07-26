# Down-and-Out Barrier Call: Discrete Monitoring, Running Extremes, and a Continuity Correction

**Model:** [`schema_examples_manual/barrier_option_down_and_out.json`](schema_examples_manual/barrier_option_down_and_out.json)
**Test:** [`engine/tests/barrier_option_smoke.rs`](engine/tests/barrier_option_smoke.rs)
**Companions:** [`OPTIONS_PRICING_EFFICIENCY.md`](OPTIONS_PRICING_EFFICIENCY.md) (European call, exact terminal) · [`ASIAN_OPTION_EXAMPLE.md`](ASIAN_OPTION_EXAMPLE.md) (path-dependent average) · [`CORRELATED_ASSETS_EXAMPLE.md`](CORRELATED_ASSETS_EXAMPLE.md) (cross-asset correlation).

A **discretely monitored down-and-out call**: a European call that is voided ("knocked out") if the
underlying ever falls to a barrier `B < S₀` on a monitoring date. It is the richest of the four
examples because it exercises four engine features *at once* (expression-valued process params, a
native running statistic, the terminal-value accessor, and a control variate) and puts a genuine
**discretization bias** under the microscope — the one Glasserman singles out in §3.2.2: *"barrier options … do not
in general admit pricing formulas [under discrete monitoring]"*, and discrete monitoring
*"underestimates the maximum"* of the continuously-monitored path, so it **over**prices a down-and-out
relative to the continuous closed form.

Grounded in Glasserman, *Monte Carlo Methods in Financial Engineering*: barrier mechanics and the
discrete-vs-continuous monitoring bias are §3.2.2; the continuous closed form is the
Reiner–Rubinstein image (reflection) formula; the fix is the **Broadie–Glasserman–Kou (BGK)
continuity correction**, which shifts the barrier by `exp(±0.5826·σ·√Δt)`.

---

## 1. The problem

A down-and-out call pays `(S(T) − K)⁺` **only if** `min_j S(t_j) > B` over the monitoring dates
`t_j`; otherwise it pays nothing. Two things make it interesting:

1. **It needs a running extreme.** The payoff depends on the *path minimum*, not just the terminal —
   so the model must track `min S` as the path evolves.
2. **Discrete monitoring has no closed form.** With `m` monitoring dates the knock-out probability is
   strictly lower than under continuous monitoring (the path can dip below `B` *between* dates and
   recover unseen), so the discrete price sits **above** the continuous Reiner–Rubinstein price. That
   gap is a real, quantifiable Monte Carlo target — and the BGK correction closes most of it.

Contract: `S₀ = 100`, `K = 100`, `B = 90`, `r = 0.05`, `σ = 0.20`, `T = 1`, monitored on a
`Δt = 0.02` grid (`m = 50` dates).

---

## 2. How it maps onto WASiM

Everything lives on the timestep grid, and the model leans on **three** recently-added engine
features (see `ASIAN_OPTION_EXAMPLE.md` §4, `RUNNING_STATISTIC_SCOPE.md`, `TERMINAL_VALUE_SCOPE.md`):

| Concept | WASiM element |
|---|---|
| Per-step GBM shock, drift `r`, vol `σ` | `P` — `stochastic_process` (gbm); **drift/vol reference `r`/`σ` live** (gap #1) |
| Exact GBM price path | `S` — `accumulator`, `rate = S*P` ⇒ `S_{k+1} = S_k·exp(logret)` |
| **Running path minimum** (all dates, incl. terminal) | `run_min` — `filter`, `statistic = min`, **no window**, **`include_terminal`** ⇒ folds S(T) (gap #2 + #3) |
| **True terminal price** `S(T)` | `S_T` — `terminal_expression` `ref(S)` ⇒ end-of-run level (gap #3) |
| Vanilla call payoff (the control) | `vanilla = disc·(S_T − K)⁺` — a `terminal_expression`, matures at true `T` |
| Survival indicator | `survives = run_min > B ? 1 : 0` |
| **Down-and-out payoff** (the estimator) | `barrier = vanilla · survives` |
| Vanilla closed form (control mean) | `bs_price` — Black–Scholes at the true `T`, live via `erf` |
| Continuous down-out reference | `cdo_cont` — Reiner–Rubinstein image formula |
| BGK-corrected reference | `cdo_bgk` — image formula at the shifted barrier `B_adj` |
| CV coefficient / estimator | `beta = run_stat2(beta, vanilla, barrier)`, `barrier_cv = barrier − beta·(vanilla − bs_price)` (terminal) |

The running minimum — the whole reason a barrier option is path-dependent — is **a single node**:
`{"type":"filter","input":"S","statistic":"min"}` with no window is a cumulative minimum over every
monitoring date. Before gap #2 this needed a hand-built accumulator with a `min` rate and the
one-step-lag trick; now it is native, and running `max` would give an up-barrier or lookback for free.

### The maturity convention (true `T`, via `terminal_expression`)

An ordinary expression that reads a stock sees its **start-of-step** value — one step stale (the same
integration semantic the Asian example documents), so it can only reach `S(T − Δt)`. A
**`terminal_expression`** (gap #3) is instead evaluated once *after* the run, against terminal state,
where a `ref` to a stock resolves to its true end-of-run level `S(T)`. So the option matures at the
**true `T`**: `S_T = terminal_expression ref(S)` reads `S(T)` exactly (a per-realization identity with
the stock's own saved final), and every closed form (`bs_price`, `cdo_cont`, `cdo_bgk`) is evaluated
at `T` — no `T_eff = T − Δt` fudge. A `filter` reads its input one step stale too, so the running
minimum would otherwise stop at `t_{m−1}` and miss the terminal date; `run_min` sets
**`include_terminal`**, which folds `S(T)` into the running statistic after the run so the knock-out
monitor natively covers `t₀…t_m` — no hand-built `min(run_min, S_T)`. (Terminal *payoff* reads are the
Option-A prototype in `TERMINAL_VALUE_SCOPE.md`; `include_terminal` on running *monitors* is the
Option-B follow-on, done per-filter.)

---

## 3. The references (why three of them)

**`bs_price` — vanilla Black–Scholes at the true `T`.** The plain call on the same path. Its MC mean
must equal this closed form; that single check validates the GBM path, the drift/vol wiring, and the
terminal accessor all at once. It is also the control variate's known mean.

**`cdo_cont` — continuous-monitoring down-and-out (Reiner–Rubinstein).** For a *continuously*
monitored barrier the down-and-out call has the image-formula closed form

```
C_do(S₀,K) = C(S₀,K) − (B/S₀)^α · C(B²/S₀, K),   α = 2r/σ² − 1
```

(both `C(·)` are Black–Scholes calls at the true `T`). This is the price the discrete MC would
converge to only if the barrier were watched continuously.

**`cdo_bgk` — BGK continuity correction.** Broadie–Glasserman–Kou showed a discretely monitored
barrier behaves, to O(√Δt), like a *continuous* barrier shifted **away from the spot**:

```
B_adj = B · exp(−0.5826·σ·√Δt)      (down barrier: shift downward)
```

Evaluating the same image formula at `B_adj` gives a reference the **discrete** MC should match. It is
the practical punchline of §3.2.2: don't re-derive a discrete formula — correct the barrier and reuse
the continuous one.

---

## 4. Results (verified)

`S₀ = K = 100`, `B = 90`, `r = 0.05`, `σ = 0.20`, `T = 1` (true maturity), `Δt = 0.02` (`m = 50`),
`seed = 90909`. Closed forms (live, at the true `T`): **`bs_price = 10.4506`**,
**`cdo_cont = 8.6655`**, `B_adj = 88.53`, **`cdo_bgk = 9.1822`**. From the smoke test (`N = 30 000`):

| quantity | value |
|---|---|
| **Vanilla call — Black–Scholes (`bs_price`)** | **10.4506** |
| Vanilla call — MC (`vanilla`) | 10.31 ± 0.17 (95% CI) — **matches Black–Scholes at true `T`** (validates the path) |
| Continuous down-out — closed form (`cdo_cont`) | 8.67 |
| **BGK-corrected down-out — closed form (`cdo_bgk`)** | **9.18** |
| Down-and-out — plain MC (`barrier`) | 9.06 ± 0.17, std **14.62** |
| Down-and-out — control variate (`barrier_cv`) | 9.19 ± 0.05, std **4.71** |
| **Variance reduction (std_plain / std_cv)** | **≈ 3.10×** (≈ 10× in variance) |

Four things fall out:

1. **The path is correct — and matures at the true `T`.** The vanilla MC price lands within one CI
   half-width of the live Black–Scholes value at the **true `T`** (10.45, not the `T_eff = 0.98` value
   10.32 an ordinary expression would give) — a joint check of the GBM path, the `r`/`σ`-referencing
   process, and the `terminal_expression` accessor.
2. **Knock-out destroys value.** The down-and-out (≈ 9.2) is worth clearly less than the vanilla
   (≈ 10.3): some paths breach `B` and pay nothing.
3. **The §3.2.2 discretization bias is visible.** The discrete MC price (≈ 9.06) sits **above** the
   continuous Reiner–Rubinstein price (`cdo_cont = 8.67`) — discrete monitoring misses between-date
   dips, so it knocks out less often and over-prices relative to continuous. The gap (≈ 0.4) is exactly
   the effect Glasserman flags.
4. **The BGK correction closes the gap.** The continuous formula evaluated at the shifted barrier
   `B_adj` (`cdo_bgk = 9.18`) lands right on the discrete MC price (9.06 ± 0.17) — the continuity
   correction working as advertised, turning a ≈ 0.4 bias into a match within Monte Carlo error. The
   vanilla control variate then cuts the estimator's standard deviation ≈ 3.10× (≈ 10× in variance;
   unbiased by construction) and recenters `barrier_cv` to **9.19**, essentially on top of the BGK
   reference. The reduction is milder than the Asian example's 38× because `vanilla` and `barrier`
   agree only on *surviving* paths — on a knocked-out path `barrier = 0` while `vanilla > 0`, capping
   the correlation — but it is still a free, exact-mean variance cut.

---

## 5. Engine features this exercises (and what's still rough)

This example is a **payoff** for earlier engine work — it uses gaps #1, #2, and #3 together — so §5 is
about what each buys and the one seam that remains:

1. **Expression-valued process parameters — ✅ used.** The `P` node's drift and vol reference the
   editable `r` and `σ` constants directly (gap #1), so the path, the discount, and all three closed
   forms share one source of truth. Change `σ` and the path, the BGK shift, and the references all move
   together.

2. **Native running statistic — ✅ used, terminal-inclusive.** The knock-out monitor is a single
   expanding-window `filter(min)` (gap #2). It sets **`include_terminal`**, which folds the terminal
   fixing `S(T)` into the running statistic after the run (gap #3, Option B) — so `run_min` natively
   covers `t₀…t_m` and `survives` reads it directly, with no hand-built `min(run_min, S_T)`. Running
   `max` with `include_terminal` gives up-barriers / lookbacks the same way, no model change.

3. **Terminal-value accessor — ✅ used (retires the `T_eff` workaround).** The earlier version of this
   example matured at `T_eff = T − Δt` because an ordinary expression cannot read a stock's terminal
   value (the stock read-lag, `ASIAN_OPTION_EXAMPLE.md` §4 item 3). A `terminal_expression` (gap #3,
   `TERMINAL_VALUE_SCOPE.md`) reads `S(T)` directly — a per-realization identity with the stock's own
   final — so the option now matures at the true `T`, `bs_price` is Black–Scholes at `T`, and
   `barrier_cv` (a terminal expression) even reads the `run_stat2` control coefficient. Both sides —
   the terminal payoff *and* the terminal monitor fold — are now native.

4. **The one remaining axis: monitoring frequency is the timestep.** Barrier monitoring is locked to
   the integration grid, so the monitoring calendar and the timestep are the same knob. Decoupling
   them (monitor on a coarser calendar than you integrate) would need a resampling filter or a
   monitoring-date list — not needed here, but the natural next axis for exotic-barrier work.

---

## 6. Takeaway

WASiM prices a genuinely path-dependent, discretely monitored exotic — **maturing at the true `T`** —
and puts a **named discretization bias** on screen: an exact GBM path (gap #1 process params), a
running minimum as one native `filter(min)` node (gap #2), a true terminal read `S(T)` via
`terminal_expression` that retires the `T_eff` workaround for both the payoff and the monitor (gap #3),
a live Black–Scholes / Reiner–Rubinstein / BGK reference stack (via `erf`), and a vanilla control
variate (`run_stat2`, itself consumed by a terminal expression). The discrete MC price sits above the
continuous closed form exactly as Glasserman §3.2.2 predicts, and the Broadie–Glasserman–Kou
continuity correction recovers a reference the simulation matches — all inside one validated
`model.json`.
