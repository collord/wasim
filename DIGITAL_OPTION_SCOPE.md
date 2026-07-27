# Digital (Binary) Option + Monte Carlo Greeks

**Status:** **Phases 1–3 built** — the payoff ([`DIGITAL_OPTION_EXAMPLE.md`](DIGITAL_OPTION_EXAMPLE.md))
and the **Greeks** ([`DIGITAL_GREEKS_EXAMPLE.md`](DIGITAL_GREEKS_EXAMPLE.md): likelihood-ratio and
common-random-numbers bump deltas, both matching the analytic `disc·n(d₂)/(S₀σ√T)`, LRM ≈ 3.3× lower
variance). A key finding: **LRM needed no engine feature** — the driving normal is an explicit
`random_variable`, so the score is a plain expression and the Greek is a `run_stat` mean, superseding the
"score node" originally proposed below. The remaining Greeks gap is **path-dependent** sensitivities,
where the driver lives inside a `stochastic_process` and isn't exposed (Phase 4). The digital *payoff*
also needed zero engine work; the enduring point of this scope is the sensitivity story it exposed.
**Motivation:** rounds out the payoff family (European / Asian / spread / barrier / lookback) with the
canonical *discontinuous* payoff, and gives a concrete reason to add MC sensitivities.

---

## 1. The payoff (already expressible)

- **Cash-or-nothing call:** pays `1` if `S(T) > K`, else `0`. Price `= disc·N(d₂)`.
- **Asset-or-nothing call:** pays `S(T)` if `S(T) > K`. Price `= S₀·N(d₁)`.

with `d₁ = [ln(S₀/K) + (r + σ²/2)T]/(σ√T)`, `d₂ = d₁ − σ√T`. Both closed forms are live-computable with
the existing `erf` builtin (as every other example does).

**No new engine feature is required.** The payoff is a one-liner on constructs that already exist:

```jsonc
{ "type": "terminal_expression", "id": "digital",
  "expression": { "ast": "disc * (S_T > K ? 1 : 0)" } }   // gt + if, reading S(T) via gap #3
```

So Phase 1 is purely an **example** (`digital_option.json` + smoke test): MC price matches `disc·N(d₂)`
within Monte Carlo error, and — the teaching point — a plain expectation converges fine even though the
payoff is discontinuous, because the *price* is an integral (smoothing), unlike its *derivatives*.

---

## 2. The real gap: Monte Carlo Greeks

The digital exists to motivate **sensitivities**, which WASiM cannot compute today (the only
derivative-adjacent machinery is importance-sampling likelihood ratios, used for variance reduction,
not for Greeks). Three standard estimators, with sharply different behavior on a digital:

| Method | How | Digital Δ |
|---|---|---|
| **Bump-and-revalue** (finite difference) | run at `S₀±ε`, `Δ ≈ (V₊−V₋)/2ε` | **noisy/biased**: variance ∝ `1/(N ε²)`; the discontinuity forces a bias/variance trade-off in `ε` |
| **Pathwise derivative** | differentiate the payoff along the path, `E[∂payoff/∂S₀]` | **fails**: the indicator isn't differentiable (derivative is a Dirac at `K`) |
| **Likelihood-ratio method (LRM)** | `E[payoff · ∂ log f(x;θ)/∂θ]` — differentiate the *density*, not the payoff | **works**: payoff untouched, so discontinuity is irrelevant; the natural estimator here |

The digital is exactly where the ranking is most dramatic: pathwise is inapplicable, bump is ugly, LRM
is clean. That is the case for building at least LRM.

## 3. Design options (Greeks)

1. **Bump-and-revalue harness (cheapest).** No new node — reuse the existing `RunConfigOverride` /
   params-file machinery (`params.rs`) to run the model at perturbed inputs and difference the results,
   sharing the seed (common random numbers) so the difference variance collapses. Deliverable: a small
   driver + a doc pattern, not an engine change. Covers Δ/Γ/vega for smooth payoffs; degrades on the
   digital (documented — that's the motivation for #3).
2. **Pathwise estimator.** A `pathwise_greek` construct differentiating the payoff AST w.r.t. a chosen
   input via the chain rule along the simulated path. Clean for smooth payoffs (vanilla Δ = `disc·1{S>K}`),
   **not applicable** to the digital — include only to show its limitation.
3. **Likelihood-ratio / score-function weights.** A per-realization *score* `∂ log f/∂θ` (for GBM w.r.t.
   `S₀`: `Z/(S₀ σ√T)`), so `Δ = E[payoff · score]`. Expressible as a new AST/node that exposes the
   driving normal `Z` and forms the score; the Greek is then an ordinary `run_stat(mean)` of
   `payoff·score`. This is the surgical, high-value piece — and it reuses the run-statistic reducers.

## 4. Phasing

| Phase | Deliverable | Engine work |
|---|---|---|
| **1** | `digital_option.json` + smoke test vs `disc·N(d₂)` / `S₀·N(d₁)` | **none** (example only) |
| **2** | Bump-and-revalue Greeks (common-random-numbers) as a documented driver pattern | small (harness) |
| **3** | LRM score node → digital Δ/vega as a `run_stat` of `payoff·score`; contrast with bump | moderate (new node) |
| **4** | Pathwise estimator for smooth payoffs (and its digital failure, side-by-side) | moderate |

## 5. Test plan

- **Price:** MC digital == `disc·N(d₂)` (cash), `S₀·N(d₁)` (asset), within MC error (Phase 1).
- **Greeks:** LRM Δ == the analytic digital delta (a Gaussian density term) within MC error; bump-Δ
  variance blows up as `ε→0` while LRM stays flat (the headline plot); pathwise Δ matches analytic on a
  *vanilla* call but is undefined for the digital.
- **CRN:** bump with shared seed has far lower variance than independent-seed bump.

## 6. Risk

Phase 1 is trivial. Phase 2 is a driver, low risk. Phase 3 (LRM) is the real addition — moderate: it
needs the driving normal exposed per realization and a score expression, but it composes with the
existing run-statistic layer rather than changing execution. **Bottom line:** ship the example now; use
it to justify LRM Greeks, the one genuinely missing capability it points at.
