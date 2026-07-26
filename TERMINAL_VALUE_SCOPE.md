# Terminal-Value Accessor (engine gap #3)

**Status:** **prototype landed** — Option A (a `terminal_expression` element evaluated once, after the
run, against terminal stock levels). Option B (a closing boundary tick that also lets stateful
monitors fold in the terminal point) is scoped below as the follow-on.
**Motivation:** every path-dependent example so far ([`ASIAN_OPTION_EXAMPLE.md`](ASIAN_OPTION_EXAMPLE.md) §4 item 3,
[`BARRIER_OPTION_EXAMPLE.md`](BARRIER_OPTION_EXAMPLE.md) §5 item 3) has had to carry an **effective-maturity
workaround** — the option matures at `T_eff = T − Δt` instead of `T` — because an expression cannot read a
stock's terminal value.
**Test:** [`engine/tests/terminal_expression_v2.rs`](engine/tests/terminal_expression_v2.rs).

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

**Prototype limitations (deliberate, documented):**

- **Final value only.** A terminal expression contributes a `final_value`, not a per-step `time_history`
  (it has no per-step value by construction). Requesting `time_history` on one is a no-op.
- **Terminal reads of stateful *monitors* are unchanged.** A `filter(min)` still folds `S_0…S_{n-1}` and
  never sees `S_n` — so a **barrier** monitor still misses the terminal point. Reading a stock's terminal
  *level* is fixed; folding the terminal point into a running statistic is Option B.
- **Sink discipline is by convention.** Nothing yet *errors* if an ordinary element lists a terminal
  expression as an input; it would read a stale/absent value. A validation pass should reject that.

---

## 3. Option B — a closing boundary tick (the follow-on)

Make the run **symmetric**: it opens at `t_0` with initial levels and would **close at `t_n = T`** with
final levels — one extra **read-only** evaluation pass (Δt→0, no integration, no RNG draws) against
`stock_state = S_n`, from which `final_value`s are harvested.

- **Buys:** payoffs read `S(T)` **and** monitoring filters fold in the terminal point in one stroke — fixes
  barrier/lookback too (the residual Option A leaves).
- **Cost:** it shifts every stepped model's `final_value` forward one readable tick, so it must be
  **opt-in** (`simulation_settings.close_at_terminal: false` by default). The spec has one real subtlety —
  the closing tick may re-fold pure functions and monitoring filters, but must **not** re-run
  RNG-consuming or integrating rules (markov/pid), or it breaks their invariants. Pin the convention with a
  deterministic (σ=0) probe, exactly as the Asian averaging window was pinned.

Option A is the low-risk 80 %: it removes the `T_eff` tax for the common case (terminal payoffs). Option B
is the completeness pass for stateful monitors, gated behind a setting.

---

## 4. Test plan (Option B, when built)

- **Convention probe:** σ=0 GBM; confirm the closing tick exposes `S_n` to expressions and adds exactly one
  observation to a `filter(mean/min/max)`; confirm markov/pid draw/integrate **zero** extra times.
- **Barrier integration:** re-point `barrier_option_down_and_out.json` at the true `T` (drop `T_eff`) with
  `close_at_terminal`, and confirm the running min now includes the terminal date and the discrete price
  still matches the BGK-corrected reference.
- **Backward-compat:** every existing model with `close_at_terminal` off is byte-for-byte unchanged.

---

**Bottom line:** a `terminal_expression` node lets a payoff read the true `S(T)` as an exact per-realization
identity, retiring the `T_eff = T − Δt` bookkeeping for terminal payoffs — additive, backward-compatible,
and validated. Folding the terminal point into running *monitors* (barriers/lookbacks) is the natural
Option B follow-on, gated behind an opt-in closing-tick setting.
