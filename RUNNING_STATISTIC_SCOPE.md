# Scope: A Native Running-Statistic / Time-Average Node (engine gap #2)

**Status:** proposal / scoping
**Motivation:** [`ASIAN_OPTION_EXAMPLE.md`](ASIAN_OPTION_EXAMPLE.md) §4 (gap #2) — averaging a signal over
the run has no first-class support, and the accumulator idiom used to fake it is a footgun.

---

## 1. The gap

To compute an in-model **time average / running mean** of a signal `X(t)` today, the author must:

1. add an accumulator `sumX` with `rate = X / timestep` (so each Euler step adds one `X`);
2. add a second accumulator `cnt` with `rate = 1/timestep` (a step counter);
3. divide: `avg = sumX / cnt`.

and — critically — **know the one-step read lag**: an expression reading an accumulator sees its
value at the *start* of the current step (one step behind its end-of-run value). Dividing `sumX` by
a *constant* `n` is therefore off by one; only `sumX / cnt` (same lag on both) is correct. The Asian
example documents and works around exactly this. Two related shortcomings:

- **No first-class running statistic.** Three elements + a lag trick to express "the average of `X`
  so far". Running min/max (for barrier/lookback options), running sum, running variance, and the
  time-weighted average are all similarly absent as elements. (`results_spec`'s reporting-period
  `average` reduction is *post-run analysis*, not a value a payoff can consume.)
- **The read lag is implicit.** Nothing warns that `sumX / n` is wrong; the model just prices
  slightly off.

GoldSim, which WASiM's semantics track in places, has this as a primitive (a "time-weighted /
running average" of a signal).

---

## 2. What NOT to change: the stock read lag

The lag itself is a **deliberate discrete-time-integration semantic** (a stock exposes its
start-of-step value; the increment is applied at step end — the same reason a stock referencing
*itself* in its rate reads the previous step). Existing models rely on it. **This proposal does not
change stock read timing.** It adds a node whose *internal* update is lag-consistent, so the author
never has to reason about the lag.

---

## 3. Proposed design — a `running_stat` node rule

Add a stateful per-step node rule, alongside the existing `filter` / `hysteresis` / `pid` /
`markov` grid-only rules (v2 `NodeRule`):

```rust
// model_v2 NodeRule
RunningStat {
    input: String,          // the signal to reduce over time
    statistic: RunningKind, // Mean | Sum | Min | Max | Variance | Last
    #[serde(default)]
    window: Option<f64>,    // optional trailing window (timestep units); None = since t0
}

enum RunningKind { Mean, Sum, Min, Max, Variance, Last }
```

**Semantics.** At each step the node folds `input`'s current value into per-step state and exposes
the running statistic *including the current step* (or over the trailing `window`). `Mean` is the
time average `(1/k)Σ X(tⱼ)`; the node carries `(count, sum[, sumsq, min, max])` internally, so the
count is exact by construction — no `cnt` trick, no lag exposure. JSON:

```jsonc
{ "op"/"value_rule": "running_stat", "input": "S", "statistic": "mean" }
```

The Asian example's `sumS + sumLnS + cnt + avg_arith + avg_geo` (five elements) collapses to two
`running_stat` nodes (`mean` of `S`, and `mean` of an `ln(S)` expression) — and removes the lag
footgun entirely.

**Update timing.** The node reads `input` consistently with the other grid-only rules (which
already advance per-step state against the current step's context), so `input` at step `k` is the
value at `tₖ`. Because the reduction is internal, the count and the summed points always agree — the
`sumS/cnt` self-consistency, made native.

---

## 4. Implementation touch points

Mirrors the existing per-step stateful node rules — the machinery is all there:

| Concern | Location |
|---|---|
| Node rule + kind enum | `model_v2.rs` `NodeRule`, `v2_parse.rs` lowering, `v1_import.rs` if a v1 spelling is wanted |
| Per-step state + update | `engine_v2.rs` step loop (next to `markov_state` / filter / hysteresis handling) |
| Dependency (`input`) | `graph_v2.rs` `element_deps` (a current-step dep on `input`) |
| Label | `summary.rs` |
| Array lane | `array_lane.rs` — mark ineligible (per-step state), like the other grid-only rules |
| Exhaustiveness | wherever `NodeRule` is matched |

No changes to stock semantics, the two-pass driver, or `EvalCtx`. Purely additive: models without
`running_stat` are unaffected.

**Risk:** low–moderate. It is a new node rule (more surface than gap #1's parameter change), but it
composes with, rather than alters, existing execution. The one design decision is the update-timing
convention — pin it with a deterministic (constant-input) probe exactly as the Asian averaging
convention was pinned.

## 5. Test plan

- **Unit-ish:** `running_stat(mean)` of a deterministic ramp / constant equals the closed-form time
  average; `min`/`max`/`sum`/`variance` on known series.
- **Convention probe:** σ=0 GBM, confirm `mean` covers `t₁…tₙ` (or whichever window is chosen) and
  matches a hand sum — the same probe that pinned the Asian `sumS/cnt` window.
- **Integration:** re-express the Asian example's averages with `running_stat` and confirm the
  geometric price still matches the Kemna–Vorst closed form and the control variate still fires — a
  drop-in simplification of a validated model.

## 6. Phasing

| Phase | Deliverable |
|---|---|
| **1** | `RunningStat { input, statistic }` with `Mean`/`Sum`/`Min`/`Max`; step-loop state; graph/label/lane arms; tests; simplify the Asian example. Covers time-average and barrier/lookback extremes. |
| **2** | `Variance` (Welford, reuse `stream_accum::RunningMoments`); the optional trailing `window`. |

Phase 1 removes the averaging footgun and unlocks running min/max (barrier / lookback options) as a
bonus.

---

**Bottom line:** add one stateful `running_stat` node whose internal reduction is lag-consistent by
construction. It turns the five-element, lag-sensitive averaging idiom into a single node, unlocks
running extremes for barrier/lookback options, and leaves stock semantics and every existing model
untouched.
