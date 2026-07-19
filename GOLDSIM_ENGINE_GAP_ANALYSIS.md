# WASiM ↔ GoldSim Engine Gap Analysis

**Purpose.** Identify the gap between the WASiM simulation **engine** and the
**computational engine** of GoldSim (Dynamic Monte Carlo Simulation Software), as
inferred from the GoldSim User Guide. This document is deliberately scoped to
*engine / model-semantics* functionality. It says nothing about authoring UI,
graphical editing, dashboards, or the fact that WASiM has no model-authoring
front end yet — those are out of scope by request.

**Sources.**
- WASiM: direct survey of the Rust engine (`engine/src/*.rs`, `engine/tests/*`,
  and the design notes at repo root). The canonical path is the **v2** model
  (`model_v2.rs` `Primitive`, executed by `engine_v2.rs`); v1 JSON is normalized
  into v2 via `v1_import.rs`.
- GoldSim: the 7-part User Guide (≈1,280 pp.). Extraction reached the full table
  of contents, the Appendix D–F reference (units, database formats, **integration
  & timestepping algorithm**), the Glossary and Index (which enumerate every
  element type and built-in function), and detailed chapter slices for input
  elements, stocks, delays, the discrete-event engine, and the results/analysis
  engine. A few mid-document mechanical sections were not machine-extractable
  (noted as *low-confidence* where relevant), but the capability **surface** is
  fully pinned down by the TOC + Glossary + Index.

**How to read this.** Each section states what GoldSim's engine does, what WASiM's
engine does, and the delta. A consolidated, severity-ranked gap table appears at
the end, followed by "Where WASiM already matches or leads."

---

## 0. Executive summary — the five gaps that matter most

1. **The time engine itself.** GoldSim is a **variable-timestep, event-driven,
   causality-ordered hybrid simulator**. It inserts *unscheduled updates* at the
   exact instant of any between-timestep event (a scheduled event, a stock hitting
   a bound, a resource exhausting, an `At Date`/`At ETime`/`At Stock Test`
   trigger), supports scheduled timestep changes, dynamically-adjusted timesteps,
   per-container internal clocks, and reporting-period aggregation. **WASiM is a
   fixed-step explicit-Euler evaluator over a single global `dt`.** Everything
   snaps to the grid; there are no unscheduled updates and no multi-rate clocks.
   This is the deepest architectural gap and it colors everything below (event
   timing accuracy, mass conservation at bound crossings, stiff subsystems).

2. **The discrete-event layer.** GoldSim has a rich transactional layer —
   Timed/Triggered Events, Decision, Random Choice, Discrete Change (Add/Replace/
   Push), Status, Milestone, Interrupt, Event/Discrete-Change Delays with
   **queues and capacity**, and a **Resource** system (Spend/Borrow/Deposit) — all
   ordered by an explicit causality sequence. WASiM has a genuine but *thinner*
   event primitive (triggers + Poisson + effects) and no resources, no queue
   modeling, no Milestone/Interrupt, and a narrower trigger vocabulary.

3. **Procedural scripting.** GoldSim's Script element is a full mini-language
   (local variables, assignment, `if/else`, `for`/`do`/`while`/`repeat-until`,
   `break`/`continue`). WASiM has **no procedural execution** — a v1 `script`
   element evaluates only `expressions[0]`; the only iteration is the functional
   `vector_map` array comprehension.

4. **Probabilistic breadth.** GoldSim ships Latin Hypercube sampling, importance
   sampling (with user-defined realization weights), simulated Bayesian updating,
   a larger distribution roster, and a deep results/statistics layer
   (PDF/CDF/CCDF, arbitrary percentiles, capture times, tail extrapolation,
   confidence bounds, conditional tail expectation, correlation/regression/
   importance-measure sensitivity). WASiM implements Monte Carlo with
   Iman-Conover rank correlation and AR(1) autocorrelation, but **LHS is declared
   and not implemented**, there is no importance sampling / realization weighting
   / Bayesian updating, and results expose only `mean + p05/p25/p50/p75/p95` plus
   per-realization final values.

5. **Dimensional analysis & interop.** GoldSim is a **dimensionally-enforced**
   engine (mismatched units are a hard error, conversions are automatic) with
   first-class external coupling (Excel spreadsheet element, external **DLL**
   elements, ODBC database linkage). WASiM has a unit registry but performs **no
   runtime dimensional checking** (only warnings), and has **no external coupling**
   (JSON model in, JSON results out).

The good news, stated up front: WASiM already covers a surprising amount of
GoldSim's *core* — Euler stocks with bounds/overflow/priority-withdrawal, links
with transit/dispersion/decay, a multi-cell **mass-transport** layer with species
decay chains and Kd partitioning, nested-Monte-Carlo submodels, an optimizer, a
failure/repair state machine, Markov chains, and fault-tree gates. The gaps are
concentrated in (a) the time/event engine, (b) scripting, (c) probabilistic
tooling and results analysis, and (d) interop.

---

## 1. Simulation paradigm & the time engine

| Capability | GoldSim | WASiM | Gap |
|---|---|---|---|
| Core paradigm | Dynamic + probabilistic + discrete-event **hybrid** | Dynamic + probabilistic; discrete events as effects on a fixed grid | Partial |
| Integration method | **Euler only**, by deliberate design (rejects Runge-Kutta/variable-order because uncertainty dwarfs integration error & higher-order methods can't handle discontinuities/coupled transport) | **Euler only** (`engine_v2.rs`) | **None — WASiM matches GoldSim's stated philosophy exactly** |
| Timestep model | Basic Step **+ unscheduled updates + scheduled timestep changes + dynamic adaptive timestep + per-Container internal clocks** | Single fixed global `dt`; `n_steps = round(duration/dt)` | **Large** |
| Unscheduled updates | Auto-inserted at exact event/bound-crossing/resource-exhaustion times to preserve accuracy & mass conservation | None — events and crossings resolve on the next grid step | **Large** |
| Reporting periods | Accumulated / average / change / rate-of-change over monthly/annual periods, independent of Basic Step | None | Medium |
| Element evaluation order | Explicit **causality sequence** (state analysis, upstream-before-downstream), user-inspectable/adjustable, with defined feedback-loop semantics | Topological sort (`graph_v2.rs`, petgraph); cycles rejected (v2) / warn-skipped (v1); `lag` & stocks are back-edges | Small (functionally similar; GoldSim's is richer around discrete-change ordering) |
| Deterministic vs probabilistic runs | Yes | Yes (`n_realizations`) | None |
| Calendar vs elapsed-time basis | Both; calendar dates referenceable | Elapsed-time; a fixed 365-day non-leap calendar for `year/month/day` time functions | Medium (**no leap years**, no true calendar/date arithmetic) |

**The central point.** GoldSim treats time as a continuum onto which it projects a
*dynamically chosen* set of update points; WASiM treats time as a fixed lattice.
Consequences that a WASiM modeler cannot currently reproduce:
- An event scheduled for t = 33.65 d executes *at* 33.65 d in GoldSim; in WASiM it
  is quantized to a grid step.
- A reservoir that overflows partway through a step has its overflow computed at
  the exact crossing time in GoldSim (mass-conserving); WASiM computes it at step
  end.
- Stiff subsystems can run on a finer internal clock in GoldSim without forcing the
  whole model to a small `dt`; WASiM must shrink the global `dt` (or push the fast
  dynamics into a duration-0 submodel).

---

## 2. Element / object library

GoldSim exposes ~50 element typeIDs. WASiM v2 reorganizes into **6 primitives
(Node, Stock, Link, Event, Gate, Cell) + 2 definitions (Species, Medium)**, where
"traits" activate by field presence. The mapping below is the heart of the gap.

### 2.1 Input elements

| GoldSim | WASiM equivalent | Delta |
|---|---|---|
| **Data** (scalar/vector/matrix constant) | `Node::Fixed` (`Scalar` or `Array`) | Match |
| **Stochastic** (distribution; vector; resamplable) | `Node::Sample` (distribution, `autocorrelation`, `resampling`, `correlations`) | Near-match; roster & resampling detail differ (see §6) |
| **Time Series** | `Node::Series` (timestamps/values, interpolation) | **Partial.** WASiM lacks: average-over-timestep output (flow conservation), `Rate_of_Change` output, discrete-change output mode, time-shifting (random start / align-years / periodic wrap), and **record-and-play-back** of another element's output |
| **Lookup Table** (1-D/2-D/**3-D**; derivative, integral, inverse lookup; log result interp) | `Node::Lookup` (1-D/2-D; linear/step; extrapolate/clamp) | **Partial.** No 3-D tables, no table derivative/integral/inverse-lookup, no log result interpolation; `cubic`/`spline` requested → **mapped to linear** at the engine boundary |
| **History Generator** (geometric growth, random walk, reversion to median/target, correlated stochastic histories) | Approximated by `Node::Process` (GBM) + expressions | **Gap** as a first-class element; some cases expressible |

### 2.2 Function elements

| GoldSim | WASiM equivalent | Delta |
|---|---|---|
| **Expression** | `Node::Expression` (AST) | Match |
| **Selector** (nested if/then) | nested `If` in an expression | Match (composable) |
| **Sum** | `+` / `sum_array` | Match |
| **Extrema** (running lifetime peak/valley) | `Node::Filter` is **windowed** (Min/Max over N), not lifetime | **Gap** — no running-since-start extrema without a hand-built stock |
| **Allocator** (allocate a signal by demands & priorities) | Link `priority` allocation / Stock `withdrawals` priority | Partial-match |
| **Splitter** (split by fractions/amounts; route discrete changes) | Link `fraction`; no discrete-change routing | Partial |
| **Controller** (Deadband / Proportional / **PID**) | none built-in (expressible w/ stock + expression, laboriously) | **Gap** — no built-in feedback controller |
| **Convolution** (solves convolution integral) | `Node::Convolution` (inline or ref response) | **Match** |
| **Previous Value** (prior-timestep output; closes recursive loops) | `Node::Lag` (strict 1-step) + accumulator self-reference reads previous step | Match |

### 2.3 Stock (integrator) elements

| GoldSim | WASiM `Stock` | Delta |
|---|---|---|
| **Integrator** (integrate a rate; up to 3 moving-average outputs; `Pushed_Out` for aging chains; "rate applies to previous step" option) | `rate`/`inflows`−`outflows`, Euler | Partial — **no built-in moving-average outputs, no aging-chain Push** |
| **Reservoir** (Additions vs Withdrawal Requests; Upper/Lower bounds; `Overflow_Rate`, `Withdrawal_Rate`, `Is_Full`; **`Is_Full` is a state variable usable to close feedback loops**) | `floor`, `capacity` + `overflow_target`, `withdrawals` (priority), `return_rate` (compound growth) | Near-match on mechanics; **no `Is_Full` state-variable feedback idiom**, no separate actual-withdrawal output semantics surfaced the same way |
| **Pool** (scalar; multiple named inflows/outflows each with priority; `Total_Inflow/Request/Outflow`) | `inflows`/`outflows` element lists + link priority | Partial-match |
| **Aging chains** (Integrator + discrete `Push`) | none | **Gap** |

### 2.4 Delay elements

| GoldSim | WASiM | Delta |
|---|---|---|
| **Material Delay** (conveyor/pipeline, conserves, dispersion) | Link `transit_time` (plug flow) + `dispersion` (Ogata-Banks RTD) + `decay_rate` | **Match / arguably richer** on transport physics |
| **Information Delay** (exponential smoothing/forecasting, no conservation) | `Node::Filter::Ema` approximates; v1 `Delay` (ring buffer) | Partial |
| **Event Delay / Discrete Change Delay** (queue, capacity, `Num_in_Queue`, `Mean_Time`, `Current_Service_Time`, conveyor-belt vs fixed-at-entry) | none | **Gap — no queue/service modeling** |

### 2.5 Discrete-event elements

| GoldSim | WASiM | Delta |
|---|---|---|
| **Timed Event** (Regular / Poisson / Stochastic-interval / cumulative-count / remaining-time types) | `Event` with `rate` (Poisson) or trigger; `Periodic` trigger | Partial — Poisson & periodic yes; stochastic-interval / remaining-time / cumulative-count types no |
| **Triggered Event** | `Event` + `TriggerSpec` | Match |
| **Discrete Change** (Add / Replace / Push) | `EffectSpec` modes `Additive` / `Multiplicative` / `Replace` | Near-match (**no Push**) |
| **Decision** (branch to one of N named outputs) | expressible via `If` | Composable |
| **Random Choice** (event tree; importance sampling) | expressible via `Sample`+`If`; no importance sampling | Partial |
| **Status** (latching condition, separate true/false triggers) | `Node::Hysteresis` (Schmitt) approximates | Partial |
| **Milestone** (records event time; achievement probability, mean lag) | none | **Gap** |
| **Interrupt** (terminate/skip realization on event) | none | **Gap** |

### 2.6 Logical elements

| GoldSim | WASiM | Delta |
|---|---|---|
| **And / Or / Not / Logic Tree (N-Vote)** | `Gate` (`And/Or/Not/NVote/Reference/Condition/Input`, Success/Failure semantics) + inline `GateLogic` node | **Match / arguably richer** (explicit fault-tree primitive) |

### 2.7 Structural / advanced elements

| GoldSim | WASiM | Delta |
|---|---|---|
| **Container** (hierarchy) | `container` field / `ContainerDef` | Match (semantic, not just visual) |
| **Conditional Container** (dormant when inactive — not recalculated) | none | **Gap** |
| **Looping Container** (iterate contained elements each step until convergence) | none | **Gap** (matters for simultaneous/implicit equations) |
| **Localized Container** (namespace scoping, aliases, globalizing) | flat IDs; no scoping | **Gap** |
| **SubModel** (embed a full model; nested Monte Carlo) | `ContainerDef{kind:Submodel}` + `SubmodelStat` (`submodel_v2.rs`) | **Match** (see §7) |
| **Script** (procedural language) | none (single-expression only) | **Gap** (see §3) |
| **Spreadsheet / External(DLL) / File** (interop) | none | **Gap** (see §9) |
| **Resource / Resource Store** (limited supply; Spend/Borrow/Deposit) | none | **Gap** |
| **Clone** (referenced copy) | none | Gap (authoring convenience; low engine impact) |

### 2.8 Extension-module elements

GoldSim's paid modules add engine capability that WASiM partially reimplements in
its core:

| GoldSim module | Capability | WASiM |
|---|---|---|
| **Contaminant Transport / Flow** | `Cell` elements: multi-media mass/heat transport, species, sources, Kd partitioning, decay chains, coupled stiff ODEs | **Substantial partial match**: `Cell` primitive with `media`/`species`, `partitioning_equilibrium` (Kd), `source_release`, radioactive **decay chains with daughter ingrowth**, advective/dispersive transport. **But**: cell output is **mass, not concentration** (no `C = mass/(volume·porosity)`); `Link.fluxes`/`geometry` are structurally defined but **not populated by the v2 parser**; no coupled-link stiff solver (WASiM is Euler on the global grid) |
| **Reliability** | Action & Function elements, failure modes, fault trees | **Partial match**: `Event.failure_process` FSM (bases ExposureTime/OperatingTime/Demand/Condition; repair policies None/Repair/Replace/PM) + `Gate` fault trees. **But** `CapacityDemand`/`Event` failure bases are **no-ops** |
| **Financial** | Fund elements, annuity/PV/FV functions, insurance/investment/option | **Gap** (expressible via stocks/expressions, but no financial primitives/functions) |

---

## 3. Procedural scripting & control flow

- **GoldSim Script element**: local variable declarations & assignment, `if/else`,
  `for`, `do`, `while`, `repeat-until`, `break`/`continue`, with debugging. This is
  a Turing-complete per-element computation.
- **WASiM**: **no procedural execution.** The v1 `script` element is imported and
  only `expressions[0]` is evaluated; `procedural` control flow emits a warning
  (README and `model.rs` confirm). The only iteration construct is the functional
  **array comprehension** (`VectorMap`/`IndexRef`/`Index`, fully implemented in
  `eval.rs`), and there are no user-defined functions. Raw
  `QuantityOrFormula::Formula` strings that were never parsed to an AST evaluate to
  **0.0**.

**Gap: large.** Any GoldSim model that relies on a Script element for imperative
logic (iterative solvers, bespoke allocation, string/array building loops,
stateful bookkeeping) has no direct WASiM expression. Note the transpiler notes
(`notes_to_transpiler.md`, `WASIM_ARRAY_COMPREHENSION_GAP.md`) show this is a known
Tier-1/Tier-2 boundary.

---

## 4. Dynamics, integration & feedback

- **Integration**: both are **explicit Euler, fixed-rate-over-step** — WASiM
  matches GoldSim's Appendix-F design decision exactly, including the "rate applies
  to previous step" subtlety (GoldSim's optional flag; WASiM's accumulator
  self-reference reads the previous step). **Not a gap.**
- **Discrete/continuous coupling**: GoldSim adds discrete changes *instantaneously*
  on top of the continuous Euler sum, inserting unscheduled updates so a
  mid-step discrete change is timed exactly. WASiM folds event effects into the
  per-step integration pass (order: link transfer → event pass → stock
  integration → overflow re-clamp → cell transport). Same *net* semantics on a
  step boundary; **different (coarser) timing** off the grid.
- **Feedback vs recursive loops**: GoldSim distinguishes *feedback loops* (contain
  ≥1 state variable, solved directly via causality ordering) from *recursive
  loops* (instantaneous circular logic, solved via **Previous Value** elements or
  **Looping Containers** that iterate to convergence). WASiM breaks cycles at
  `Lag` and stock back-edges and otherwise **rejects cyclic graphs** (v2). **Gap**:
  no iterate-to-convergence facility for simultaneous/implicit equations within a
  timestep (GoldSim's Looping Container).

---

## 5. Stocks, flows & mass balance

This is one of WASiM's strongest areas and close to parity:

- **Match/near-match**: bounded reservoirs (`floor`, `capacity`), overflow routing
  to a target, priority-ordered withdrawals/allocations, compound growth
  (`return_rate`), mass-conserving link transfers with plug-flow transit,
  dispersion (Ogata-Banks residence-time kernel), and first-order decay in transit.
  The `stock_traits_v2` tests confirm priority allocation and overflow routing.
- **Multi-cell mass transport** (GoldSim CT module territory): WASiM tracks mass
  per `(cell, species, medium)`, does Kd equilibrium partitioning across media, and
  runs **radioactive decay chains with daughter ingrowth** — a real subset of
  GoldSim's Contaminant Transport engine.
- **Gaps**: (a) cell state is exposed as **mass, not concentration** (V2_SCOPING
  M4 known limit); (b) `Link.fluxes`/`geometry` (advective/diffusive/settling/
  precipitation mechanisms; pipe/aquifer/conduit geometry) are **defined in the
  model types but not wired up by `v2_parse`**; (c) no bound-crossing unscheduled
  update, so overflow/withdrawal at a bound is resolved at step end rather than at
  the exact crossing; (d) no `Is_Full`-style state-variable output for closing
  control feedback.

---

## 6. Probability, distributions & sampling

### 6.1 Distribution roster

WASiM **has**: Uniform, Normal, Lognormal (log-params and moments), Triangular,
**Trapezoidal**, Exponential, Gamma, Beta (4-param), Weibull, Pearson V,
Pearson III, PERT, Pareto, Extreme Value (Gumbel), Student-t, Discrete Uniform,
Bernoulli (≙ GoldSim Boolean), Discrete, Cumulative, Sampled (weighted empirical).

GoldSim **additionally has** (WASiM gap):
- **Log-Uniform**, **Log-Triangular**, **Triangular/Log-Triangular 10-90**
  variants, **Log-Cumulative**
- **Binomial**, **Negative Binomial**
- **Poisson as a sampling distribution** (WASiM uses Poisson only for event rates)
- **Extreme Probability (min/max)** (distribution of the extreme of N samples)
- **Beta specified by (successes, failures)**
- **Externally-defined distribution** (via DLL/Spreadsheet). WASiM has an
  `External` distribution family but it **cannot be sampled — it returns 0.0**,
  which strands otherwise-valid models.

WASiM has one GoldSim doesn't surface directly: **Trapezoidal** (SELDM-derived).

### 6.2 Sampling & correlation

| Capability | GoldSim | WASiM |
|---|---|---|
| Monte Carlo | Yes | Yes (ChaCha8, per-realization stream) |
| **Latin Hypercube** | Yes (stratified) | **Declared (`SamplingMethod::Lhs`) but NOT implemented** — always independent MC |
| Rank correlation | Copulas / correlation algorithms (Iman et al.) | **Iman-Conover** (v2) / Gaussian copula (v1) — **match** |
| Autocorrelation | Correlate a Stochastic to its own previous value | **AR(1)** per-step in normal space — **match** |
| Truncation | Yes | Yes (rejection; clamp under AR(1)) |
| **Importance sampling** | Yes (rare-event biasing; on Stochastics & Timed Events) | **None** |
| **User-defined realization weights** | Yes | **None** |
| **Simulated Bayesian updating** | Yes (dynamically revise distributions) | **None** |
| Resampling of a stochastic | Trigger-based; correlated stochastics resample with driver | `resampling: TriggerSpec` — match |

**Gap: medium-large.** The correlation machinery is at parity, but LHS
(a headline GoldSim efficiency feature) is unimplemented, and importance sampling /
realization weights / Bayesian updating are entirely absent.

---

## 7. Submodels, nested Monte Carlo & containers

- **Match**: WASiM's `ContainerDef{kind:Submodel}` is a genuine **nested Monte
  Carlo** ("MC-in-MC") facility with its own `simulation_settings` and an
  interface (`inputs`/`outputs`); parent expressions pull submodel statistics via
  `SubmodelStat` (`Mean`, `Percentile`, `Sd`, `CumulativeProb`). Input binding via
  the `from` driver extracts the driver's dependency closure into the submodel
  (`SUBMODEL_INTERFACE_INPUT_BINDING.md`) — which is exactly what enables
  probabilistic optimization (parent search variable → submodel). This maps well to
  GoldSim's SubModel + Nested Monte Carlo and "separating uncertainty from
  variability."
- **Gaps** relative to GoldSim containers: **Conditional Containers** (dormant/
  not-recalculated when inactive), **Looping Containers** (iterate-to-convergence),
  **Localized Containers** (namespace scoping/aliases), and **per-container
  internal clocks**. WASiM containers are organizational + the submodel special
  case; they do not carry activation state, local iteration, local scope, or a
  local timestep.

---

## 8. Optimization, sensitivity & results analysis

### 8.1 Optimization

- WASiM `optimize_v2.rs` implements **Box's complex method** over editable
  `Fixed` scalar variables, objective `Maximize/Minimize` reduced by
  `Mean/Percentile/Peak/Valley/Sum`, with probabilistic objectives re-running the
  nested submodel per candidate. This is a real match for GoldSim's
  objective/optimization-variable/probabilistic-optimization capability.
- **Gaps**: GoldSim supports **constraints**; WASiM parses `OptConstraint` but
  **does not enforce it** (box projection only). GoldSim's calibration framing and
  "required condition" are not modeled.

### 8.2 Sensitivity analysis

- GoldSim has two engines: a **deterministic multi-run sweep → tornado / X-Y
  charts**, and a **statistical** analysis from a probabilistic run computing
  coefficient of determination, correlation coefficients, standardized regression
  coefficients, partial correlation coefficients, and importance measures.
- WASiM has **neither implemented** — there is a `SENSITIVITY_ANALYSIS_SPEC.md`
  design (one-at-a-time sweep + tornado reusing the optimize harness) but **no
  `sensitivity_v2.rs` module and no schema encoding**. **Gap: medium.**

### 8.3 Results / output analysis

This is a large, under-appreciated gap.

| GoldSim result capability | WASiM |
|---|---|
| Time-history **probability bands** with user-chosen percentile pairs (default 8 pairs + median; mean optional) | Fixed `mean + p05/p25/p50/p75/p95` only |
| **Arbitrary / custom percentiles**, per-output custom statistic | Not configurable |
| Distribution results as **PDF / CDF / CCDF** (exceedance) | Only `final_values` array is returned; no PDF/CDF/CCDF/exceedance objects |
| **Capture Times** (snapshot distributions at arbitrary times) | Only per-timestep bands + final values |
| **Tail extrapolation** for final-value/distribution stats at small N | None |
| **Realization classification & screening** (categories by condition, Net/Gross %, include/exclude) | None |
| **Reporting-period aggregation** (accumulated/average/change/rate) | None |
| Confidence bounds on the mean & on distributions; **Conditional Tail Expectation**; skewness/kurtosis | `mean`, `percentile`, `std`, `cumulative_prob` helpers exist internally but are not exposed as results |
| **Scenarios** (store/compare input sets) | None |
| Importance-weighted statistics | N/A (no importance sampling) |

**Gap: medium-large** — WASiM's result surface is a small fixed summary; GoldSim's
is a full probabilistic-analysis workbench at the engine level.

---

## 9. Units, expression language & interop

### 9.1 Dimensional analysis

- GoldSim is **dimensionally enforced**: every element has output dimensions, links
  are unit-checked, compatible units auto-convert (m + ft ok), incompatible units
  are a **hard error** (m + hr rejected). Rich units DB (`units.dat`), custom
  units, absolute-vs-difference unit handling (Cdeg/Fdeg, dates).
- WASiM has an SI unit registry (`units.rs`) with parse/convert/display (including
  affine temperature), but **`validate()` only emits warnings** — there is **no
  runtime dimensional analysis** and no hard rejection of unit mismatches.
- **Gap: medium.** Also: no true calendar/date type (fixed 365-day, **no leap
  years**); GoldSim supports date/datetime units and calendar-time simulation.

### 9.2 Built-in functions

WASiM's scalar/array/time builtins (52) largely cover GoldSim's math/trig set and
add `Log2, Sign, Int, atan2, InterpArray, DotProduct`. **GoldSim additionally
provides**: `cot`, **Bessel**, **beta function**, **error function (erf)**,
**standard-normal** and **Student-t distribution functions**, the event functions
**`occurs`** / **`changed`**, **importance-sampling functions**
(`ImpOld/ImpProb/ImpWeight/ImpNew`), and **financial functions** (annuity/PV/FV).
Also table-function operations (derivative/integral/inverse of a lookup) and
first-class matrix operations (solve systems of equations). **Gap: small-medium**,
mostly special functions, event-predicate functions, and matrix algebra.

### 9.3 Arrays & matrices

- WASiM: first-class scalar/vector values with broadcasting, array literals,
  **implemented array comprehensions** (`vector_map`), and array builtins. Good
  coverage of vectors.
- GoldSim: full **vectors and matrices** with label sets (Named / Indexed),
  constructor functions (`row`/`col`), whole-array arithmetic, matrix algebra
  (solve linear systems), vector-as-lookup-table, and arrays up to 60 columns from
  DB import. **Gap**: true 2-D **matrix** semantics and matrix algebra;
  label-set-based indexing.

### 9.4 External coupling / data exchange

- GoldSim: **Spreadsheet element** (bidirectional Excel per-timestep), **External
  (DLL) element** (link C/C++ at runtime; can define lookup tables, distributions,
  time series; run out-of-process), **ODBC database linkage** (Generic / Simple /
  Extended GoldSim DB, effective-dated, CRC-verified), text/CSV import/export, XML
  model inventory, GoldSim Player runtime.
- WASiM: **JSON model in, JSON results out.** `ExternCall` AST node exists but
  **evaluates to 0.0** (round-trip preservation only). No spreadsheet/DLL/DB.
- **Gap: large** for interop — though arguably *by design* (WASiM's thesis is a
  transparent, diffable open format, and a browser/WASM runtime cannot load DLLs).
  Worth calling out as a deliberate scope difference rather than a defect.

---

## 10. Consolidated, severity-ranked gap table

Severity = engine impact for reproducing GoldSim-class models (not authoring
convenience).

| # | Gap | Severity | Notes |
|---|---|---|---|
| 1 | **Unscheduled updates / variable & multi-rate timestepping** (scheduled changes, dynamic adaptive, per-container internal clocks, reporting periods) | **High** | Deepest architectural gap; affects event timing & mass conservation |
| 2 | **Procedural Script element** (loops, if/else, local vars) | **High** | Only functional `vector_map`; no imperative logic |
| 3 | **Results/analysis engine** (PDF/CDF/CCDF & exceedance, arbitrary percentiles, capture times, classification/screening, tail extrapolation, CTE/confidence bounds, reporting-period aggregation, scenarios) | **High** | WASiM returns a fixed 5-percentile+mean summary |
| 4 | **Discrete-event richness**: queues/capacity (Event & DC Delays), **Resources** (Spend/Borrow/Deposit), Milestone, Interrupt, Status, aging-chain Push, Random Choice/Decision as primitives | **High** | WASiM has triggers+Poisson+effects but none of these |
| 5 | **Sampling tooling**: LHS (declared, unimplemented), importance sampling, user realization weights, Bayesian updating | **Medium-High** | Correlation (Iman-Conover/copula) & AR(1) already at parity |
| 6 | **Runtime dimensional analysis / unit enforcement**; true calendar/dates (leap years) | **Medium** | WASiM only warns; fixed 365-day calendar |
| 7 | **Sensitivity analysis** (tornado + statistical measures) | **Medium** | Spec exists (`SENSITIVITY_ANALYSIS_SPEC.md`), no module |
| 8 | **Container semantics**: Conditional (dormant), Looping (iterate-to-convergence), Localized (scoping) | **Medium** | Submodel/nested-MC already covered |
| 9 | **Distribution roster** gaps (Log-Uniform/Log-Triangular/10-90, Binomial, Negative Binomial, Poisson-as-dist, Extreme Probability, Beta(succ/fail)); **External dist samples 0.0** | **Medium** | WASiM adds Trapezoidal |
| 10 | **Lookup tables**: 3-D, derivative/integral/inverse-lookup, log result interp; `cubic`→linear | **Medium** | |
| 11 | **Feedback controller** (PID/Proportional/Deadband); running lifetime **Extrema**; **History Generator** | **Medium** | Partly expressible |
| 12 | **Matrix algebra & label-set arrays** (solve systems, matrix ops) | **Medium** | Vectors covered; matrices not |
| 13 | **External coupling** (Excel/DLL/ODBC) | **Low-Medium** | Largely a deliberate scope difference (open JSON format, WASM sandbox) |
| 14 | **Cell = mass not concentration**; `Link.fluxes/geometry` not parsed; optimization constraints not enforced; `OnEvent`/`CapacityDemand`/`Event` bases no-op; `Formula` strings → 0.0 | **Varies** | WASiM's own known internal caveats (documented in-repo); each strands specific models |
| 15 | **Financial primitives/functions** (Fund, annuity/PV/FV, insurance/option) | **Low** | Expressible via stocks/expressions |
| 16 | Aging-chain Push; Clone; distributed processing; versioning | **Low** | Versioning provided externally by git; distributed processing is deployment, not engine semantics |

---

## 11. Where WASiM already matches or leads GoldSim's engine

To keep the picture honest, WASiM is **not** a strict subset:

- **Euler-only integration** is not a gap — it is exactly GoldSim's deliberate
  design choice (Appendix F), for the same reasons (uncertainty ≫ integration
  error; discontinuity handling).
- **Rank correlation** (Iman-Conover) and **AR(1) autocorrelation** are at parity
  with GoldSim's copula/correlation machinery.
- **Mass-transport core**: multi-cell, multi-species, multi-medium mass balance
  with **Kd partitioning** and **radioactive decay chains + daughter ingrowth** is
  a genuine slice of GoldSim's *paid* Contaminant Transport module, in WASiM's
  core.
- **Transport physics on links** (plug-flow transit, Ogata-Banks dispersion,
  first-order decay in transit) matches/exceeds the Material Delay element.
- **Fault-tree gates** (`And/Or/Not/N-Vote`, Success/Failure semantics) and a
  **failure/repair state machine** cover a real slice of the Reliability module.
- **Nested-Monte-Carlo submodels** with dependency-closure input binding and an
  **optimizer** (Box's complex) that drives them — a working analogue of GoldSim's
  SubModel + probabilistic optimization.
- **Markov chains, Convolution, Hysteresis, rolling-window Filter** are
  first-class node rules.
- **Open, diffable JSON model format** with a WASM runtime — a different (and in
  some respects stronger) design point than GoldSim's single binary `.gsm`.
- **SELDM parity** (USGS Stochastic Empirical Loading and Dilution Model):
  Pearson-III, Trapezoidal ICDF, zero-inflated mixtures, single-timestep storm
  models — a validated real-world modeling capability.

---

## 12. Suggested engine roadmap implied by the gaps

Ordered to maximize model-coverage per unit of engine work:

1. **Variable/event-accurate timestepping** (gap #1): introduce unscheduled
   updates at event/bound-crossing times and scheduled timestep changes. This is
   the highest-leverage, highest-effort item and unblocks faithful discrete-event
   and bound-crossing behavior.
2. **Results/analysis layer** (gap #3): configurable percentiles, CCDF/exceedance,
   capture times, reporting-period aggregation. Mostly additive; the stat helpers
   (`percentile`, `std`, `cumulative_prob`) already exist internally.
3. **Finish declared-but-unimplemented items** (low-hanging, credibility):
   implement **LHS**; enforce **optimization constraints**; make the **External
   distribution** samplable; wire `Link.fluxes`/`geometry`; land the
   **sensitivity** module per the existing spec.
4. **Discrete-event depth** (gap #4): Resources, queue/capacity delays, Milestone,
   Interrupt, Status, aging-chain Push.
5. **Procedural scripting** (gap #2): a Tier-2 executor for the transpiler's
   control-flow (the transpiler notes already anticipate this).
6. **Dimensional enforcement + real calendar** (gap #6); **matrix algebra** and
   **3-D/analytic lookup tables** (gaps #10, #12); **containers** (Conditional/
   Looping/Localized, gap #8) and a **feedback controller** (gap #11).

---

### Appendix — methodology & confidence

- WASiM claims are **high-confidence** (read from source: `model_v2.rs`,
  `engine_v2.rs`, `eval.rs`, `sampling.rs`, `optimize_v2.rs`, `submodel_v2.rs`,
  `graph_v2.rs`, `units.rs`, the `engine/tests/*` suite, and the repo design docs).
- GoldSim claims are **high-confidence for capability existence** (grounded in the
  User Guide's TOC, Glossary, Index, Appendix D–F, and detailed chapter slices for
  input/stock/delay/discrete-event/results elements). A few mechanical details in
  non-extractable page bands (pp. 265–384, 665–784, 800–1104, and the distribution-
  math / function-reference appendices) are inferred from the Index/Glossary plus
  the guide's cross-references; these are flagged as *partial/low-confidence* where
  they occur and do not affect the identification of gaps, only the depth of
  mechanical detail.
- "Gap" here means an engine capability present in GoldSim with **no faithful
  WASiM engine encoding**. "Composable/expressible" items (where a WASiM modeler
  could hand-build the behavior from existing primitives) are marked as such and
  rated lower severity.
