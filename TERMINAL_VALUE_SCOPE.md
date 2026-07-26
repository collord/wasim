# Terminal-Value Accessor (engine gap #3)

**Status:** **both parts landed.** Option A — a `terminal_expression` element evaluated once, after the
run, against terminal stock levels — reads a stock's true `S(T)` in a *payoff*. Option B — folding the
terminal observation into running *monitors* — landed as a per-filter **`include_terminal`** flag
(cleaner than the global closing tick originally scoped, since Option A already fixes payoffs; see §3).
**Motivation:** every path-dependent example so far ([`ASIAN_OPTION_EXAMPLE.md`](ASIAN_OPTION_EXAMPLE.md) §4 item 3,
[`BARRIER_OPTION_EXAMPLE.md`](BARRIER_OPTION_EXAMPLE.md)) has had to carry an **effective-maturity
workaround** — the option matures at `T_eff = T − Δt` instead of `T` — because an expression cannot read a
stock's terminal value.
**Tests:** [`engine/tests/terminal_expression_v2.rs`](engine/tests/terminal_expression_v2.rs) (Option A),
[`engine/tests/running_monitor_terminal_v2.rs`](engine/tests/running_monitor_terminal_v2.rs) (Option B).

---

## 1. The gap (grounded in the execution model)

Within a timestep `k`, the v2 step loop does two things **in order** (`engine_v2.rs`):

1. **Topo pass** — every expression/filter reads `outputs[stock]`, which still holds the
   **start-of-step** level `S_k`.
2. **End-of-step publish** — *after* the topo pass, integration runs and the stock's recorded value is
   overwritten with the **post-update** level `S_{k+1}` (`engine_v2.rs`, *"recorded value reflects
   post-update level"*).

So at the final step `k = n−1`:

- An ordinary payoff expression reads `S_{n-1} = S(T−Δt)` → the lag.
- The stock's **own saved `final_value` is `S_n = S(T)`** — harvested *after* the publish.

The key fact: **`S(T)` already exists in the engine** (as `stock_state` and the stock's own final). The
gap is only that **no expression can consume it**, because expressions run before the publish and there is
no evaluation tick after the last integration. The examples pay for this with the `T_eff` convention — exact,
but the author asked for a `T`-year option and got a `T−Δt`-year one.

---

## 2. Option A — a terminal-expression layer (what landed)

A new element kind, evaluated **exactly once, after the run**, against each realization's terminal state.
Inside it a `ref` to a stock resolves to that stock's end-of-run level `S(T)`. It is a **sink**: it may
read other elements' final values, but nothing may read it back during the run.

```jsonc
{ "type": "terminal_expression", "id": "payoff",
  "inputs": ["disc", "S", "K"],
  "expression": { "ast": { … disc * max(S − K, 0) … } },
  "save_results": { "final_value": true } }
```

**How it works.** No changes to stock timing (§ the lag is deliberate). The engine simply:

- **skips** terminal expressions in the per-step topo pass (they take no per-step value), and
- after the final step — once `outputs` already holds every stock's published `S(T)` — walks the terminal
  expressions in topo order, evaluating each against `outputs` and inserting the result back so a terminal
  expression can read an earlier one. The normal `save_final` harvest then picks them up.

Because the terminal read *is* the same value the stock harvests as its own final, the relationship is an
**exact per-realization identity**, not a statistical one.

**Touch points (all additive):** `model.rs` (`ElementKind::TerminalExpression`), `model_v2.rs`
(`NodeRule::TerminalExpression`), `v1_import.rs` (normalize + inputs + label), `v2_parse.rs`
(`"terminal_expression"` lowering), `engine_v2.rs` (per-step skip + post-run pass + `rule_name`),
`summary.rs` (label), `graph.rs`/`engine.rs` (v1 reference-engine arms → deps + `0.0`). The array lane
rejects it via its existing catch-all, so terminal-expression models run on the scalar lane. Models without
a terminal expression are byte-for-byte unaffected.

**Verified** (`terminal_expression_v2.rs`, a v1 model, `N = 80 000`):

- **Exact identity:** a `terminal_expression` `ref(S)` equals the stock's saved final `S(T)` at *every*
  realization (< 1e-9), while an ordinary expression `ref(S)` lags it by one step (`S(T−Δt)`); the two
  means match the two lognormal expectations `100·e^{rT}` and `100·e^{r(T−Δt)}`.
- **Maturity moves to the true `T`:** a European call written on the terminal read converges to
  Black–Scholes at `T` (**10.45**), not to BS at `T_eff = 0.98` (10.32) — the value the *same* payoff as an
  ordinary expression produces. The `T_eff` workaround is gone.

**Adopted in a real model.** [`BARRIER_OPTION_EXAMPLE.md`](BARRIER_OPTION_EXAMPLE.md) was rebuilt on the
accessor: the down-and-out call now matures at the true `T` (vanilla-MC = BS at `T`, 10.45), the
control-variate estimator `barrier_cv` is itself a `terminal_expression` reading a `run_stat2`
coefficient — proving the accessor composes with the two-pass control-variate machinery — and the
knock-out monitor's `run_min` filter sets **`include_terminal`** (Option B, below) so it natively
covers the terminal date.

**Prototype limitations (deliberate, documented):**

- **Final value only.** A terminal expression contributes a `final_value`, not a per-step `time_history`
  (it has no per-step value by construction). Requesting `time_history` on one is a no-op.
- **Sink discipline is by convention.** Nothing yet *errors* if an ordinary element lists a terminal
  expression as an input; it would read a stale/absent value. A validation pass should reject that.

---

## 3. Option B — folding the terminal point into running monitors (landed, per-filter)

Option A fixes terminal *payoffs*. Running *monitors* (a `filter` for a barrier/lookback) have the same
one-step lag: a filter reads its input's start-of-step value, so it folds `S_0…S_{m−1}` and never sees
`S_m = S(T)`. The original scope proposed a **global closing tick** — a symmetric extra read-only pass at
`t_n = T` that re-folds pure functions and monitors — but since Option A already covers payoffs, the
monitor gap is better closed **surgically**, without a model-wide `final_value` shift.

**What landed: a per-filter `include_terminal` flag.** `{"type":"filter", …, "include_terminal": true}`
folds the input's terminal value `S(T)` into the running statistic *once, after the run* — reusing the
exact same fold as the per-step loop (`fold_filter`), so the monitor now covers `t₀…t_m`. It runs in the
post-run terminal pass, before terminal expressions, so a `survives = terminal_expression run_min > B`
reads the terminal-inclusive minimum. Correct when the input resolves to a terminal level (a stock, or a
chain of terminal reads); an ordinary-expression input folds its one-step-stale value (documented).

- **Additive & opt-in per node:** default `false`; a filter without it is byte-for-byte unchanged, so no
  global `final_value` shift and no interaction with RNG/integrating rules — the closing-tick's one real
  subtlety simply doesn't arise.
- **Covers barriers *and* lookbacks:** `min` with `include_terminal` is a down/knock-out monitor; `max`
  is an up-barrier / lookback maximum; `mean` closes a running average at `S(T)`.
- **Verified** (`running_monitor_terminal_v2.rs`): an `include_terminal` filter's final equals the
  interior filter combined with a `terminal_expression` read of `S(T)`, as an **exact per-realization
  identity** (`rmin_t == min(interior, S(T))`, `rmax_t == max(interior, S(T))`), and the fold strictly
  changes the monitor on the fraction of paths where the terminal is a new extreme.

**Still open (a genuine global closing tick).** `include_terminal` folds a filter's *own* input. A filter
whose input is an ordinary *expression* of a stock (e.g. `filter(min, f(S))`) still folds the stale
`f(S_{m−1})`, because expressions aren't re-evaluated at `T`. Closing that fully is the original global
closing tick (re-evaluate expressions at `T`, then fold monitors), gated behind
`simulation_settings.close_at_terminal` — deferred, since terminal-read-of-a-stock (the common case) is
now covered by Option A + `include_terminal`.

---

## 4. Touch points

- **Option A (`terminal_expression`):** `model.rs` (`ElementKind::TerminalExpression`), `model_v2.rs`
  (`NodeRule::TerminalExpression`), `v1_import.rs` (normalize + inputs + label), `v2_parse.rs` lowering,
  `engine_v2.rs` (per-step skip + post-run pass), `summary.rs`/`graph.rs`/`engine.rs` arms. Array lane
  rejects via its catch-all → scalar lane.
- **Option B (`include_terminal`):** an `include_terminal: bool` field on the `filter` rule (`model.rs`,
  `model_v2.rs`, `v1_import.rs`, `v2_parse.rs`), a shared `fold_filter` helper (extracted so the per-step
  loop and the terminal fold advance identically), and a terminal-fold loop in `engine_v2.rs`'s post-run
  block (before the terminal-expression pass, so a monitor is terminal-inclusive when a terminal
  expression reads it).

Both are additive: a model with neither feature runs byte-for-byte as before.

## 5. Verified

- **Option A** (`terminal_expression_v2.rs`): terminal `ref(S)` == stock final `S(T)` per realization
  (exact); a European call on it converges to BS at the true `T` (10.45), not `T_eff` (10.32).
- **Option B** (`running_monitor_terminal_v2.rs`): `include_terminal` filter final ==
  `min`/`max`(interior filter, `S(T)`) per realization (exact), and strictly changes the monitor on paths
  where the terminal is a new extreme.
- **Integration** (`barrier_option_smoke.rs`): the down-and-out example uses both — matures at true `T`,
  `run_min` is terminal-inclusive, `barrier_cv` is a terminal expression reading a `run_stat2` — and still
  matches the BGK-corrected reference within Monte Carlo error.

---

**Bottom line:** a `terminal_expression` node reads a stock's true `S(T)` in a payoff (an exact
per-realization identity), and a per-filter `include_terminal` folds `S(T)` into a running monitor —
together retiring the `T_eff = T − Δt` bookkeeping for both terminal payoffs and barrier/lookback
monitors, additively and validated. A genuine global closing tick (re-evaluating *expression*-valued
filter inputs at `T`) remains the only deferred piece, gated behind an opt-in
`simulation_settings.close_at_terminal`.
