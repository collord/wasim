# Proposal: allow reference/formula-valued distribution parameters (gamma / weibull / uniform)

**For:** the WASiM schema owner. **From:** the re-gsm emit side.
**Re:** GoldSim frequently parameterizes a distribution from *another element* (a link/reference)
rather than a literal — including the optimization case where a distribution parameter IS the
optimization variable. The v2 distribution schema allows this for some families
(`quantity_or_formula`) but not others (`quantity`, scalar-only). This asks you to widen the
scalar-only families so those references survive emit.

## The trigger

`probabilisticoptimization.json`'s `TheSystem` emits `{"family": "external", "parameters": {}}` —
a placeholder — even though the decode knows it is a **Weibull whose shape (GoldSim "Slope") is the
optimization variable `Model/Slope` (range [1,25])**, with mean 10, location 0. The objective
`Cost = Slope + (PDF_Value(TheSystem, 95%) − target)²` searches Slope to hit a target percentile.
This is the marquee probabilistic-optimization example, and its distribution is unrepresentable
because `weibull.shape` is typed `quantity` (a scalar), so a reference to `Slope` cannot be emitted.

## This generalizes — it is GoldSim's normal parameterization style

A distribution parameter port left blank in the .gsm but **driven by a link** (a reference to
another element) is pervasive. Measured across the corpus (link-driven, unset parameter ports):

| family | current param type | link-driven params | distinct elements | schema change? |
|---|---|---|---|---|
| `lognormal` | `quantity_or_formula` | 234 | 47 | **no** (already OK) |
| `normal` | `quantity_or_formula` | 97 | 25 | **no** (already OK) |
| `gamma` | `quantity` | 12 | 3 | **yes** |
| `weibull` | `quantity` | 5 | 3 | **yes** |
| `uniform` | `quantity` | 1 | 1 | **yes** |

`normal`/`lognormal` are already `quantity_or_formula`, so those 331 references are an **emit-only**
fix (we consult the influence graph for the port's driver — same pattern as the aggregator-operand
wiring; no schema work). The **gamma / weibull / uniform** cases (18 params, 7 elements) are blocked
by the scalar-only type. The affected elements:
- **weibull**: `probabilisticoptimization/TheSystem` (shape ← `Slope`, the opt variable),
  `uncertaintyvariability/Lifetimes` (shape ← `Slope`, mean ← `MeanLife`),
  `uncertaintyvariability/LifetimeDist`.
- **gamma**: `Demo`/`Demonstration LLW.../DisposedInventoryBq` (mean ← `..._Mean`, sd ← `..._SD`),
  `WindGen/Gamma_Speed` (mean ← `Mean_Wind_Speed`).
- **uniform**: `TimeSeries_TimeShifting_ElapsedTime/Random_Start` (max ← `TS_Data_Duration`).

## The ask

Change these parameter types from `quantity` to `quantity_or_formula`, matching normal/lognormal:

- **`weibull`**: `shape`, `scale`
- **`gamma`**: `shape`, `scale`
- **`uniform`**: `min`, `max`

`quantity_or_formula` already unions `{quantity | expression_field | string}`, so a scalar literal
still validates unchanged — this is a **widening, back-compatible** change (every current corpus
doc still validates; only new formula-valued params become expressible). No `$id` semantics beyond
the usual bump.

Optional / your call — the other scalar-only continuous families have the same latent limitation
even if the corpus doesn't exercise them yet: `triangular` (min/mode/max), `trapezoidal`, `beta`
(alpha/beta/min/max), `pert`, `pareto`, `extreme_value`, `pearson_iii`, `pearson_v`, `student_t`.
For consistency you may want to widen all continuous-family scalar params to `quantity_or_formula`
in one round (the corpus only *needs* weibull/gamma/uniform today, but a blanket widening avoids a
future round each time a new model links one of the others). We're fine either way — minimal
(3 families) or blanket.

## One wrinkle specific to gamma & weibull (heads-up, not a blocker)

For these two, emit does **not** pass GoldSim's stored parameters through directly — it
moment-matches them into the schema's (shape, scale) (GSCore ground truth, see
`dist-slot-semantics`):
- **gamma** stores (mean μ, stddev σ); emit derives `shape = (μ/σ)²`, `scale = σ²/μ`.
- **weibull** stores (location, slope k, mean m); emit derives `scale = m / Γ(1 + 1/k)`, `shape = k`.

So when the *stored* input is a reference (e.g. gamma's mean ← `..._Mean`), the emitted `shape`/
`scale` is a **derived expression over that reference**, not a bare `ref`. With
`quantity_or_formula` that is exactly what `expression_field` is for — emit will output the AST
(e.g. `shape = power(divide(ref(Mean), ref(SD)), 2)` for gamma). Weibull with a referenced *slope*
(the opt case) is simpler: `shape = ref(Slope)` directly, and `scale = m / Γ(1+1/ref(Slope))`
(a `divide` by a `gamma_fn(...)` — if the engine's fn enum lacks a gamma function we will emit that
factor as `extern_call gamma`, which the engine already tolerates as opaque). Flagging so the shape
of these expression params isn't a surprise; nothing for you to decide here.

## Emit-side readiness

The influence-graph lookup that recovers a port's driver already exists
(`_wire_aggregator_operands` / the interface-input `driver_of`). Once the three (or more) param
types are `quantity_or_formula`, emit change is localized to `_emit_distribution` /
`_dist_weibull` / `_dist_gamma`: when a parameter port is link-driven, emit the driver as a
`ref`/expression instead of falling back to a scalar 0 (or, for weibull/gamma, to `external`). One
regeneration, no decode work.

## Self-check for the regeneration after this lands

- `probabilisticoptimization/TheSystem` emits `family: weibull` with `shape` = a reference/expression
  over `Slope` (not `external`), and validates.
- No distribution with a link-driven parameter emits `family: external` or a `value: 0` stub where a
  reference was intended.
- Corpus still 283/283 valid both schemas.

---

## Owner decision (2026-07-15): ACCEPTED — landed in schema 0.8.5 + engine

Good diagnosis — you traced the marquee failure to the real root cause (a scalar-only param
type forcing the `Slope` reference to be dropped and the whole distribution to fall back to
`external`), not the `external` family per se. Adopted, and went **blanket** rather than minimal.

**Blanket, not minimal (your "your call").** Widened *all 12 scalar-only continuous families*
(33 params), not just weibull/gamma/uniform. The widening is uniform, mechanical, and
back-compatible, and each future minimal round would cost a schema bump + engine change +
regeneration — so amortizing it now is the smaller total cost. Families widened: uniform,
triangular, trapezoidal, gamma, beta, weibull, pert, pareto, extreme_value, pearson_iii,
pearson_v, student_t. (normal/lognormal were already `quantity_or_formula`.)

**Landed:**
- `wasim-schema-v2.json` — those 33 params `quantity` → `quantity_or_formula`; `$id` → 0.8.5.
  Verified: corpus 162/162 still valid (widening is back-compatible — a scalar literal still
  validates; only new formula/ref params become expressible). Semantics §2.3 notes formula-valued
  params; CHANGELOG 0.8.5.
- **Engine consumes it end-to-end:** `DistributionKind` params for these families are now
  `QuantityOrFormula`; `resolve_distribution` (eval.rs) evaluates each to a scalar in the element's
  context *before* sampling (the exact path normal/lognormal already used); `sampling.rs`,
  `params.rs`, and the wasm `set_dist_param` updated to the QoF accessors. Verified with a fixture:
  a `uniform(0, 2·k)` whose `max` references element `k` tracks k (k=5 → mean≈5, k=10 → mean≈10;
  draws stay in range). Full suite + wasm target green.

**What this unblocks — the weibull/gamma expression shapes you flagged (§ "one wrinkle"):**
- A referenced *slope* → `shape = ref(Slope)` resolves directly. ✓
- The derived expressions (gamma `shape = (mean/sd)²`, weibull `scale = m/Γ(1+1/k)`) are exactly
  `expression_field` ASTs — `resolve_distribution` evaluates them. ✓
- **The gamma function Γ is now a real builtin** — do NOT emit it as `extern_call gamma` (which
  evaluates to 0.0 and would silently break `scale = m/Γ(...)` via ÷0). Emit
  `{"op":"call","fn":"gamma","args":[...]}`. Added to the schema `call` `fn` enum (0.8.5) and
  implemented in the engine (Lanczos approx). Verified: Γ(5)=24, Γ(0.5)=√π, and
  `scale = 10/Γ(1+1/2) ≈ 11.284` all correct through the evaluator. So your moment-matched
  weibull/gamma scale expressions work as-is — no need to avoid Γ.

Once you regenerate with these params as references/expressions, `probabilisticoptimization`'s
`TheSystem` becomes a real Weibull driven by `Slope`, its 95th-percentile becomes Slope-dependent,
and the optimization objective becomes non-constant — closing the loop end-to-end (the last
blocker from `SUBMODEL_EXECUTION_FINDINGS.md`).
