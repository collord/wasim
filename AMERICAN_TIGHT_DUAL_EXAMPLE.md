# A Tight Primal–Dual Bracket for an American Put — the Nested-Simulation Dual

**Model:** [`schema_examples_manual/american_put_tight_dual.json`](schema_examples_manual/american_put_tight_dual.json)
**Test:** [`engine/tests/american_tight_dual_smoke.rs`](engine/tests/american_tight_dual_smoke.rs)
**Scope:** [`AMERICAN_OPTION_SCOPE.md`](AMERICAN_OPTION_SCOPE.md) §8 · **Builds on:** [`EXPOSURE_PROFILE_EXAMPLE.md`](EXPOSURE_PROFILE_EXAMPLE.md) (`nested_stat` `each_step`), [`AMERICAN_OPTION_EXAMPLE.md`](AMERICAN_OPTION_EXAMPLE.md) (the primal)

This closes the American-option arc. The [primal LSM](AMERICAN_OPTION_EXAMPLE.md) gives a **lower** bound
on the price; the original [dual](AMERICAN_OPTION_EXAMPLE.md) gives a rigorous but **loose** upper bound
(one hedging instrument can't replicate the option). Here the `lsm_dual` node gains a **tight** upper
bound, built on the nested-simulation capability (`nested_stat` `each_step`) — turning a wide, only-just-
useful bracket into a genuinely tight one.

---

## 1. The duality, and why the first dual was loose

Optimal-stopping duality (Rogers / Haugh–Kogan / Andersen–Broadie): for **any** martingale `M` with
`M₀ = 0`,

```
price ≤ E[ maxₜ (Zₜ − Mₜ) ]        Zₜ = discᵗ · payoffₜ
```

so every martingale yields a valid upper bound; the **tightness** depends on how close `M` is to the
option's own value process. The shipped dual uses the single martingale `Mₜ = θ·(discᵗ·Sₜ − S₀)` — one
hedge — and is far from the value process, so the bound is loose (~9.7 vs a true ~6.05).

The tight dual needs the **Doob martingale of the value process**, whose increments are its one-step
innovations `Vₖ − E_{k−1}[Vₖ]`. Building `E_{k−1}[·]` is a **one-step conditional expectation at every
`(path, date)` node** — nested simulation, i.e. exactly `nested_stat` in `each_step` mode.

## 2. Which value process — a prototyped choice

Four candidate martingales were prototyped before building (see `AMERICAN_OPTION_SCOPE.md` §8/§8a). The
result was decisive:

| Martingale | Dual | Verdict |
|---|---|---|
| Single hedge `θ·(discᵗ·Sₜ − S₀)` | 9.70 | valid, **loose** |
| Doob of the **fitted LSM value** `max(intrinsic, Ĉ)` | 9.51 | valid but **loose** — the cubic continuation **extrapolates wildly** at fresh inner states |
| Doob of the **discounted intrinsic** `Yₜ = discᵗ·payoffₜ` | **6.49** | valid and **tight** ✓ |

The intrinsic-process martingale wins: it needs **no fit, no extrapolation, no closed form**, so it is
both tight and fully general. That is what the engine builds.

## 3. The engine feature (`lsm_dual` with `inner > 0`)

```jsonc
{ "op": "lsm_dual", "state": "S_lag", "payoff": "h", "rate": 0.05, "inner": 128 }
```

- `inner: 0` (default) → the loose single-hedge dual (unchanged).
- `inner > 0` → the **tight** dual: `Mₜ = Σ_{k≤t}(Yₖ − E_{k−1}[Yₖ])`, `Yₖ = discᵏ·payoffₖ`.

`E_{k−1}[Yₖ | S_{k−1}]` is estimated by nested simulation, done natively for speed: at each date the
engine draws `inner` next-step states by **resampling the panel's own one-step log-returns** (so **no σ
parameter is needed** — the empirical one-step law comes from the paths themselves), re-evaluates the
payoff there, and averages — on a **grid** over the conditioning state, interpolated per path (a handful
of payoff evaluations per date instead of one per node). The resampled draws are independent of each
path's own transition, so `M` is a genuine martingale ⇒ the bound stays **valid**; finite `inner` biases
it slightly **upward** (safe for an upper bound).

One authoring requirement: because the dual re-evaluates the **payoff at fresh states**, the payoff
element must reference the dual's `state` element (here both use `S_lag`). If it can't be evaluated as a
plain function of state + constants, the engine logs a warning and falls back to the loose dual.

## 4. Results (verified)

`S₀ = K = 100`, `r = 0.05`, `σ = 0.20`, `T = 1`, `Δt = 0.02` (50 dates), `N = 40 000`, `inner = 128`:

| quantity | value |
|---|---|
| **binomial American put** (`T_eff = 0.98`) | **6.045** |
| primal — LSM out-of-sample (`american_put`) | 6.015 ± 0.069 — lower bound |
| dual — single-hedge (`dual_loose`, `inner: 0`) | 9.704 ± 0.046 — valid, **loose** |
| dual — **nested Doob, tight** (`dual_tight`, `inner: 128`) | **6.358 ± 0.041** — valid, **tight** |
| **bracket** | **[6.015, 6.358]**  (was [6.015, 9.704]) |

The bracket around the true price **6.045** narrows from a width of **3.69 (≈ 61%)** to **0.34 (≈ 5.7%)**
— the tight dual is a valid upper bound (6.358 ≥ 6.045) sitting just above the price, while the loose
dual sat far above it. The primal and dual now genuinely *pin* the price.

## 5. Engine status

**Landed** — the tight LSM dual (`lsm_dual` `inner > 0`): a Doob-of-intrinsic martingale whose one-step
conditional expectation is a native nested simulation (resampled panel returns + payoff re-evaluation +
grid interpolation), reusing the same nested-simulation idea as `nested_stat` `each_step`. Additive —
`inner: 0` is the original loose dual, byte-identical; every model without `inner` is unchanged.

**Open:** the payoff must be a plain function of the dual's `state` element + constants (multi-asset /
exotic payoffs fall back to the loose dual); and it prices a single-factor state — a multi-asset tight
dual would reuse the multivariate LSM machinery.

## 6. Takeaway

WASiM now brackets an American option tightly from both sides: the LSM primal is a nearly unbiased lower
bound, and the `lsm_dual` node — with `inner > 0` — is an Andersen–Broadie-style upper bound built as the
Doob martingale of the discounted-intrinsic process, its one-step conditional expectations supplied by
nested simulation. The bracket tightens from ~60% to ~6% of the price, using only the paths' own returns
(no volatility input) and re-evaluating the payoff at fresh states — the capstone of the optimal-stopping
work, resting on the conditional-nested-simulation primitive built earlier in the arc.
