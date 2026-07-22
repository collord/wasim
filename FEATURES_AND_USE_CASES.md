# WASiM Engine — Features and Use Cases

*What the WASiM simulation engine can model, and the classes of problem it is built to solve.*

Companion to `schema/wasim-engine-semantics.md` (the behavioral contract) and
`schema/wasim-schema-v2.json` (the model format). This document distills the
engine's capabilities into problem classes and the primitives that serve them.
It does not reference any model from the example corpus.

Status conventions used below:
- **Implemented** — evaluated end-to-end by the engine.
- **Provenance-only / placeholder** — parsed, graph-connected, and round-tripped,
  but evaluated to a documented placeholder (usually `0.0`) pending a future round.
  These are called out explicitly so the feature list is not read as more than it is.

---

## 1. What kind of tool this is

WASiM is a **probabilistic, dynamic systems-simulation engine**. At its core it is a
fixed-step explicit-Euler evaluator that advances a graph of typed elements over
a timeline, wrapped in a Monte-Carlo realization loop. On top of that core it
carries primitives for stochastic sampling, mass balance and transport, reliability
logic, discrete events, feedback control, and study-level workflows (optimization,
sensitivity, results analysis).

It is designed as an **open, diffable substitute for GoldSim-class** probabilistic
simulation: the model is a single transparent JSON document that version-controls,
diffs, and reviews like source code, and the engine runs entirely client-side
(native Rust or WASM in the browser) so model data never leaves the user's machine.

The engine targets domains with mature simulation practice but heavy platform
lock-in: **environmental consulting, water and contaminant modeling, infrastructure
and engineered-systems reliability, and quantitative risk/financial analysis.**

---

## 2. Problem classes it can solve

Each class below is expressed in terms of the model constructs that implement it.

### 2.1 Dynamic systems modeling (stocks, flows, feedback)
Model any system describable as state variables integrating rates over time.
- **Stocks** integrate a net rate via explicit Euler; rates may be given directly or
  as sums of inflow/outflow element references.
- **Links** move quantity between elements as an absolute rate or a fraction of the
  source.
- **Feedback loops** close through a stock (integrator) or an explicit one-step `lag`
  node, which breaks algebraic cycles.
- **Compound / exponential growth** (self-referential rate) via the `compound_growth`
  stock trait — used for interest, population, or any multiplicative process, with the
  correct multiplicative discretization to avoid systematic bias.

Typical problems: reservoir and pond storage dynamics, population/biomass models,
inventory and material accumulation, financial balances that compound.

### 2.2 Monte-Carlo uncertainty and risk analysis
Propagate input uncertainty through the model and characterize output distributions.
- **Sampled inputs** drawn once per realization from a large distribution roster
  (§3.2), constant within a realization unless a resampling trigger re-draws them.
- **Rank correlation** between inputs (Iman-Conover) with achieved-vs-target
  correlation diagnostics.
- **Lag-1 autocorrelation** (AR(1)) applied across timesteps within a realization.
- **Latin Hypercube sampling** stratifies once-per-realization draws for lower-variance
  estimates; composes with Iman-Conover.
- **Importance sampling** for rare-event estimation: draw from a biased distribution and
  weight by the likelihood ratio `f/g`, feeding weighted statistical reductions so the
  true expectation is recovered with far lower tail variance.
- **Realization weights** make every analysis-layer statistic weighted (the hook that
  importance sampling and post-hoc reweighting use).

Typical problems: probabilistic cost/schedule risk, exceedance-probability estimation,
rare-event tail quantification, uncertainty propagation for any deterministic model.

### 2.3 Mass balance and contaminant transport
A genuine slice of contaminant-transport modeling in the engine core.
- **Cells** are well-mixed compartments tracking mass per species per medium, with
  inflows from transport links and sources.
- **Equilibrium partitioning** redistributes a species across media by partition
  coefficients (Kd) — solved algebraically for two phases, as a small linear system for
  three or more.
- **Radioactive/first-order decay chains** apply exponential decay per species and route
  the decayed mass into daughter species by branching fraction, processed parent-first.
- **Derived concentration** outputs (`mass / (volume·fraction·porosity)`) whenever a cell
  declares a positive volume.
- **Transport on links**: plug-flow transit delay (FIFO slug delivery), first-order
  transit decay, and dispersion parameterized by a Péclet number (Ogata-Banks analytical
  RTD or tanks-in-series — *the dispersed-RTD transfer function is a deferred math spec*).
- **Finite-inventory source release** at a rate that drops to zero on exhaustion,
  optionally schedule-gated.

Typical problems: groundwater/vadose-zone contaminant fate, multi-compartment mass
balance, radionuclide decay-chain ingrowth, source-term depletion, pond/cell mixing.

*Emit caveat: the mass-delivery input side (source/transport-link body decode) and
per-nuclide half-lives are gated upstream in the emitter for real source models; the
engine paths are in place.*

### 2.4 Reliability, availability, and fault analysis
- **Boolean logic gates** — AND / OR / NOT / k-of-n voting over a recursive tree, with
  reference/condition/input leaves. Serves both **fault trees** (true = failed) and
  **reliability block diagrams** (true = working); `semantics` is interpretive metadata.
- **Failure/repair state machines** — a two-state (working/failed) automaton with
  stochastic transitions on multiple failure bases: `exposure_time`, `operating_time`,
  `demand` (per-demand probability), `event`/`on_event`, and `condition`. Repair policies:
  none, repair, replace (as-good-as-new), and scheduled preventive maintenance.
- **Markov chains** — discrete-state degradation with per-step transitions and
  state→value mapping; transition rates may be expression-valued (state/time-dependent).
- **Milestones and latching status flags** for time-to-event and achievement tracking.

Typical problems: system availability, mean time to failure/repair, fault-tree top-event
probability, maintenance-policy comparison, multi-state degradation.

*Deferred: the `capacity_demand` failure basis (needs demand/capacity fields) is currently
a no-op that never fails.*

### 2.5 Discrete events and event-driven dynamics
- **Events** fire on triggers (always / on-condition / periodic / on-schedule / on-event)
  and apply additive, multiplicative, or replacement effects to targets.
- **Poisson rate generation** — number of occurrences per step drawn from Poisson(rate·dt),
  optionally carrying an event value.
- **Interrupt effect** — ends the realization at the step it fires (last values held).
- **Event predicates** in expressions — `occurs(event)` and `changed(ref)` let node logic
  react to firings and value changes.
- **Queues** — discrete-change / conveyor delay of an arrival signal with capacity blocking,
  exposing both throughput and current queue level.
- **Resources** — a scalar balance with `spend` (supply-limited), `deposit` (capacity-clamped),
  and `borrow`/return event effects.

Typical problems: intermittent releases, scheduled operations, arrival/service delays,
resource contention, occurrence-count processes, run-terminating conditions.

### 2.6 Feedback control
- **PID / proportional controllers** — Euler-discretized control law with integral and
  derivative terms, output clamps, and an anti-chatter deadband; loops close through a
  stock plant.
- **On/off (bang-bang) controllers** — a stateful hysteresis latch that holds state inside
  the deadband so it does not chatter at the setpoint.
- **Hysteresis nodes** — two-threshold latching on an input signal (Schmitt-trigger style).

Typical problems: level/flow regulation, thermostat-style switching, any measured-value →
actuator feedback with or without integral action.

### 2.7 Time-series, delays, and signal processing
- **Time series** playback with interpolation and constant extrapolation at the bounds.
- **One-step `lag`** and chained lags for exact N-step delays.
- **Convolution** of an input against a response function — table-valued or an
  expression over the lag variable (keeping e.g. a unit-hydrograph shape parameter live
  and calibratable). Cumulative (S-curve) responses convolve their successive differences.
- **Rolling-window filters** — mean / min / max / sum / EMA over a trailing window; a window
  ≥ the run length becomes a running extremum/cumulative statistic since t=0.
- **Stochastic processes** — geometric Brownian motion (arithmetic / geometric / log-drift
  mean types, with a lower-bound clamp) and **Ornstein-Uhlenbeck mean reversion**.

Typical problems: routing/response-function hydrology, moving averages and running peaks,
price/rate processes, arbitrary signal delays and smoothing.

### 2.8 Table lookups and functional relationships
1-D, 2-D, and 3-D table interpolation invoked from expressions (`lookup_call`):
- Interpolation: `linear`, `step`, monotone-cubic `spline` (Fritsch-Carlson, no overshoot),
  and `log_linear` (log-result).
- Multilinear (bilinear / trilinear) N-D interpolation.
- **Table modes** as reserved lookup arguments: `TBL_Integral` (cumulative trapezoid),
  `TBL_Inverse`, `TBL_Inv_Integral` (the stage-storage inversion), `TBL_Derivative` (slope).

Typical problems: stage-storage-discharge curves, rating curves, empirical response
surfaces, any tabulated `y = f(x)` or `f(x, y[, z])` relationship, and their integrals/
inverses/derivatives.

### 2.9 Optimization studies
- **Static optimization** — search input variables (with bounds, optional integer
  restriction) to maximize/minimize an objective, subject to **enforced constraints**
  (infeasible candidates costed to +∞; Box's-complex implicit-constraint handling).
- **Deterministic or probabilistic objectives** — a single run's value, or a statistic
  (mean / percentile / peak / valley / sum) reduced across realizations, typically at a
  submodel boundary.
- **Dynamic (per-timestep) optimization** — a submodel-scoped optimization re-solved each
  outer step, producing an optimized variable **time series** that tracks a moving driver.

Typical problems: design sizing, policy/parameter tuning, probabilistic (chance-constrained)
optimization, tracking control that re-optimizes as conditions change.

### 2.10 Nested Monte-Carlo (submodels)
- **Submodels** are nested simulations with their own time-stepping and realization
  settings, driven through a typed interface (input bindings + readable outputs).
- **`submodel_stat`** reads a reduced statistic (mean / percentile / sd / cumulative
  probability) of a submodel output back into a parent expression — the mechanism behind
  probabilistic objectives and "run N realizations, reduce to one number" drivers.

Typical problems: two-level uncertainty (inner distribution feeding an outer decision),
statistics-driver models, single-time-point Monte-Carlo generators (`duration = 0`).

### 2.11 Financial and payoff modeling
- **Compound growth** stocks for interest/returns; **PV / annuity factor** builtins for
  discounting.
- **Option / insurance payoffs** (`payoff_spec` on events) express threshold-conditional
  payouts (strike crossing; claims above a deductible up to a cap) that plain effects
  cannot. *Provenance-only today — parsed and round-tripped, not yet executed.*

Typical problems: discounted cash flow, compound-return projections, option/insurance
payout structures (once the payoff executor lands).

### 2.12 Instant / driver models
A `duration = 0` (or sub-half-timestep) run evaluates the model **once** at t = start.
This supports optimization/statistics drivers, single-period calculations, and
sequence/parameter generators whose real timeline is a nested submodel run.

---

## 3. Capability reference

### 3.1 Element primitives and node value-rules
Six primitives — **node, stock, link, event, gate, cell** — plus definition types
(**species, medium, container**). A `node` is discriminated by its `value_rule`:

| Value rule | What it does |
|---|---|
| `fixed` | Constant scalar or array (editable inputs are optimization variables) |
| `expression` | Evaluate an AST (subsumes selectors, aggregators, controllers) |
| `sample` | Draw from a distribution once per realization (or on resampling) |
| `process` | GBM / mean-reverting (Ornstein-Uhlenbeck) stochastic process |
| `lookup` | Table interpolation target (1-D/2-D/3-D, invoked via `lookup_call`) |
| `series` | Time-series playback with interpolation |
| `lag` | One-step delay (cycle breaker) |
| `convolution` | Rolling convolution against a table or expression response |
| `markov` | Discrete-state Markov chain |
| `hysteresis` | Two-threshold latch on an input signal |
| `filter` | Rolling window mean/min/max/sum/EMA |
| `gate_logic` | Boolean gate tree as a node |
| `status` | Trigger-set / trigger-reset latching flag |
| `milestone` | Elapsed time of first trigger fire (NaN until achieved) |
| `pid` | PID / proportional / on-off controller |
| `queue` | Discrete-change / conveyor delay |

Plus the **`resource`** primitive (spend/deposit/borrow balance).

### 3.2 Distribution roster
Continuous & discrete families supported by `sample`:
uniform, normal, lognormal, lognormal_moments, triangular, trapezoidal, exponential,
gamma, beta, weibull, pearson_iii, pearson_v, pert, pareto, extreme_value, student_t,
discrete_uniform, bernoulli, discrete, cumulative, sampled;
log_uniform, log_triangular, log_cumulative (log-space sampling);
triangular1090 / log_triangular1090 (10th/90th-percentile + mode parameterization);
binomial, negative_binomial, poisson;
extreme_probability (order-statistic min/max of n nested draws);
beta_success_failure (Beta from success/failure counts, optionally affine-scaled).

All support **optional truncation** (`min`/`max`). Distribution **parameters may be
formulas** (references/expressions over other elements) — resolved to a scalar before
drawing, so a distribution's parameters can be optimization variables or derived from the
rest of the model.

*The `external` family cannot be sampled: it degrades to 0.0 with a warning unless an
inline empirical `{samples, weights}` fallback is supplied.*

### 3.3 Stock traits (activated by field presence)
- `capacity_clamp` (cap the level, expose the excess)
- `overflow_routing` (route excess to another element)
- `compound_growth` (self-referential multiplicative rate)
- `priority_withdrawal` (priority-ordered demand allocation, floor-limited)

Stock **output ports** publish, per declared flow role — `addition`, `withdrawal`,
`overflow`, `net_change` — as either a per-step `rate`, a `cumulative` running total, or
the `level` itself.

### 3.4 Link traits
- `priority_allocation` (priority-ordered supply among sibling links)
- `transit_buffer` (plug-flow FIFO delay; dispersion via Péclet number)
- `transit_decay` (first-order decay in transit)
- `scheduled_flow` (transfer only when a schedule trigger fires)
- `species_transport` (carry species/medium with flux mechanisms)

### 3.5 Cell traits
- `partitioning_equilibrium` (Kd redistribution across media)
- `decay_chain_propagation` (first-order decay + daughter ingrowth)
- `source_release` (finite-inventory release)

### 3.6 Event traits
- `rate_generation` (Poisson occurrences per step)
- `failure_state_machine` (working/failed automaton with repair policies)

### 3.7 Expression language (AST)
- **Arithmetic & comparison:** `+ − × ÷`, unary negate, `power`, and `= ≠ < ≤ > ≥`.
- **Boolean:** `and`, `or`, `not`; `if/then/else` selection.
- **Builtins:** min, max, abs, sqrt, exp, ln, log, log2, sin/cos/tan, sinh/cosh/tanh,
  asin/acos/atan/atan2, floor, ceil, round, int, sign, mod, gamma (Γ), erf, erfc, step.
- **Array/vector builtins:** sum_array, mean_array, min_array, max_array, size_array,
  dot_product, interp_array, get_element, column_count.
- **Table builtins:** table_min, table_max; `lookup_call` with the `TBL_*` modes.
- **Event predicates:** occurs, changed.
- **Financial:** pv_factor, annuity_factor.
- **Time reference:** `time_ref` calendar/clock properties (below).
- **Structured nodes:** `ref` (optionally output-port-qualified), `lookup_call`,
  `submodel_stat`, and the array nodes `vector_map` / `index_ref` / `index` / `extern_call`.
- **`extern_call`** preserves unimplemented source functions verbatim (evaluates to 0.0)
  for round-trip fidelity.

### 3.8 Units and dimensional checking
- All quantities normalized to SI at load; `display_unit` is frontend-only.
- Broad unit support: time, length, mass, volume, concentration, rate, dimensionless.
- **Reserved globals** resolve in expressions: `gee`, `TimestepLength`, `SimDuration`,
  `Realization`.
- **Optional static dimensional checker** (`warn` default / `strict`): infers each
  expression's dimension vector and can hard-fail inconsistent models before running;
  unresolvable/exempt cases never produce false positives.

### 3.9 Calendar and time
- `time_ref` properties: year, month, day_of_month, day_of_year, days_in_month, and
  (anchor-gated) hour, minute, second, start, elapsed_months, elapsed_years.
- Default **fixed 365-day** calendar; a `calendar_start` anchor switches to a real
  proleptic-Gregorian calendar with leap years.
- `get_year`/`get_month`/… builtins operate on explicit date arguments using the real
  calendar regardless of anchor.

### 3.10 Timebase / integration modes (runtime-configured)
- `fixed` — bit-identical fixed-grid explicit Euler.
- `event_accurate` — inserts unscheduled sub-steps to refine integration at known instants
  and at **stock bound crossings** (closed-form crossing time, re-run to land on the bound,
  coupled re-evaluation of dependents). Sub-steps consume no randomness, so seed/realization
  streams match `fixed`. Grid remains the statistical, state-machine, and reporting lattice.

### 3.11 Results and analysis layer (runtime-configured)
Default surface is a mean + p05/p25/p50/p75/p95 time-history band plus per-realization
final values. An opt-in `results_spec` unlocks per element:
- custom percentile bands over the time history;
- final-value **PDF / CDF / CCDF** distribution objects;
- **capture-time snapshots** (cross-realization distribution at chosen elapsed times);
- final-value **summary stats** — mean confidence interval, skewness, excess kurtosis,
  conditional tail expectation (CTE);
- **reporting-period aggregation** — accumulated / average / change / rate-of-change over
  consecutive periods.
- All reductions become **weighted** when realization weights are supplied.

### 3.12 Sensitivity analysis (runtime-configured)
A sensitivity sweep over model inputs is available as a run option, producing the
sensitivity of results to varied inputs (a study layer around the realization loop).

### 3.13 Arrays and dimensions
Named ordinal dimensions with comprehension nodes (`vector_map`, `index_ref`, `index`) —
**implemented**. An element is array-valued when its primary output declares `dimensions`; its
vector flows through the graph, is read per-member by `index`, and is expanded at the results
boundary into per-member series `<id>#1..#N`. Per-member state without array stocks: `lag`
preserves vector shape, so an array `expression`+`lag` recurrence integrates each member.
Array-valued **`status`** latches per member (other stateful rules stay scalar this round).
Reductions include `sum/mean/min/max/size_array`, `dot_product`, `interp_array`, and
`argmin_array`/`argmax_array` (lowest-index tie-break) for wear-levelling-style dispatch.
Only `extern_call` remains an opaque `0.0` node.

---

## 4. Deliberate boundaries and current placeholders

**Provenance-only / placeholder (parsed and round-tripped, not yet executed):**
- Financial payoff execution (§2.11) — effects stay empty.
- Linked-Excel `spreadsheet` elements — the workbook is external; fixed-0 placeholder.
- The `external` distribution family (no inline fallback) → 0.0.
- Dispersed-flow residence-time transfer function — deferred math spec.
- `capacity_demand` failure basis — no-op until schema fields land.
- `submodel_stat` on engines without nested execution → 0.0 (degrades like a dangling ref).

**Documented non-goals (by design, not gaps):**
- Adaptive/error-controlled timestepping — Euler-only, matching the reference design point.
- DLL / ODBC / external-process coupling and live spreadsheet evaluation.
- Localized-container name scoping (references are globally-unique ids, resolved by exact
  string equality — no relative or scope-aware lookup).
- Per-container internal clocks and distributed processing.
- Deep same-step event cascades (the event pre-pass is a single declaration-order pass,
  not a fixpoint).

**Robustness posture:** dangling references, absent optional inputs, and partially-emitted
models **load and run** (unresolved references evaluate to 0.0 with a warning) rather than
being rejected — so incomplete models degrade gracefully instead of failing hard.
