# WASiM ↔ GoldSim Engine Gap Analysis

**Purpose.** Identify the gap between the WASiM simulation **engine** and the
**computational engine** of GoldSim (Dynamic Monte Carlo Simulation Software), as
inferred from the GoldSim User Guide. This document is deliberately scoped to
*engine / model-semantics* functionality. It says nothing about authoring UI,
graphical editing, dashboards, or the fact that WASiM has no model-authoring
front end yet — those are out of scope by request.

> ## Revision 2 — re-run against the updated engine
>
> **This is a re-analysis.** Revision 1 (the original) drove a substantial round
> of engine work — `WORKPLAN_TIER_A.md`, `WORKPLAN_TIER_B.md`, `WORKPLAN_TIER_C.md`
> are the workplans it spawned, and **Tier A + Tier B have essentially all landed**
> (schema line advanced 0.9.1 → 0.9.6 per commit history; the engine parses
> `wasim_version` version-agnostically). Tier C (the "big bets") is per-item
> go/no-go gated and **not yet started**. This revision re-surveys the current
> Rust source (`engine/src/*.rs`, `engine/tests/*`) and marks every gap
> **CLOSED / PARTIAL / OPEN / BY-DESIGN**.
>
> Two claims in Revision 1 were already stale when written and are corrected here:
> **sensitivity analysis** (one-at-a-time + tornado) and **running-lifetime
> extrema** were already implemented.
>
> Net movement since Revision 1: of the five headline gaps, **two are largely
> closed** (results/analysis engine; discrete-event depth), **two are partially
> closed** (the time engine; probabilistic tooling), and **one remains open by
> design** (external interop). The single largest *remaining* engine gap is now
> **procedural scripting** (Tier C1).

**Sources.**
- WASiM: direct survey of the Rust engine (`engine/src/*.rs`, `engine/tests/*`,
  the workplans, and the design notes at repo root). The canonical path is the
  **v2** model (`model_v2.rs` `Primitive`, executed by `engine_v2.rs`); v1 JSON is
  normalized into v2 via `v1_import.rs`. Capability status below is stated as
  **FULL / PARTIAL / ABSENT** against the current code, with `file` citations.
- GoldSim: the 7-part User Guide (≈1,280 pp.). Extraction reached the full table
  of contents, the Appendix D–F reference (units, database formats, **integration
  & timestepping algorithm**), the Glossary and Index (which enumerate every
  element type and built-in function), and detailed chapter slices for input
  elements, stocks, delays, the discrete-event engine, and the results/analysis
  engine. The capability **surface** is fully pinned down by the TOC + Glossary +
  Index; a few mechanical sections were not machine-extractable and are flagged
  *low-confidence* where relevant.

**How to read this.** Each section states what GoldSim's engine does, what WASiM's
engine does *now*, and the delta. A consolidated, severity-ranked gap table with a
**Status** column appears at the end, followed by "Where WASiM already matches or
leads."

---

## 0. Executive summary

### 0.1 What Revision 1 flagged and where it stands now

| Rev 1 "top-5" gap | Rev 2 status | What landed |
|---|---|---|
| **1. The time engine** (unscheduled updates, variable/multi-rate timesteps) | **PARTIAL** | Event-accurate **sub-interval integration** at scheduled (`on_schedule`) instants **and stock floor/capacity crossings** landed, opt-in (`TimebaseMode::EventAccurate`); a mid-step bound crossing splits the step at the closed-form crossing instant so coupled downstream elements re-evaluate there (RNG-invariant; 64-split/step guard). **Remaining**: periodic-trigger sub-stepping absent; no variable/scheduled *global* timestep (Tier B2 deferred). The grid is still the fixed statistical/reporting lattice. |
| **2. Discrete-event layer** | **LARGELY CLOSED** | **Queues** (capacity, `num_in_queue`), **Resources** (Spend/Deposit/Borrow, priority, overdraw protection), **Milestone**, **Interrupt**, **Status** latch, **PID controller**, `OnEvent` trigger all FULL. Remaining: aging-chain **Push** primitive, richer Timed-Event types. |
| **3. Procedural scripting** | **OPEN** | Not started (Tier C1, gated). Still the largest remaining engine gap. |
| **4. Probabilistic breadth** | **PARTIAL** | **LHS now real**; distribution roster expanded (Log-Uniform/-Triangular/-Cumulative, 10-90 variants, Binomial, Negative Binomial, Poisson, Extreme Probability, Beta(succ/fail)); **results/analysis engine** now rich (custom percentiles, PDF/CDF/CCDF/exceedance, capture times, CI/skew/kurtosis/CTE); realization **weights** added (post-hoc); **importance sampling** landed (biased draws + likelihood-ratio weighting, S4, normal/lognormal/uniform/exponential PDFs). **Still absent**: Bayesian updating, realization classification/screening, scenarios. |
| **5. Dimensional analysis & interop** | **SPLIT** | Dimensional analysis **CLOSED** (static strict-units mode can hard-fail a run); true **leap-year calendar** + new time refs **CLOSED** (anchor-gated). External **interop** (DLL/Excel/ODBC) remains **OPEN but is an explicit non-goal** (contradicts the open-JSON/WASM thesis; `WORKPLAN_TIER_C.md` non-goals). |

### 0.2 The gaps that matter most now

1. **Procedural Script execution** (Tier C1) — GoldSim's Script element is a full
   imperative mini-language; WASiM still has no procedural executor. Highest corpus
   pressure (script-heavy models currently evaluate to 0.0).
2. **Finish the time engine** — wire `BoundCrossing` into the step loop (the code
   exists, unused), let periodic triggers sub-step, and land scheduled non-uniform
   *global* timesteps (Tier B2). Today's event-accuracy only covers explicit
   `on_schedule` instants.
3. **Results: realization classification/screening + scenarios** — the statistics
   layer is now strong, but there is no "categorize/screen realizations by
   condition" facility and no scenario comparison.
4. **Sampling depth** — importance sampling **now landed** (S4: biased draws +
   likelihood-ratio weighting, normal/lognormal/uniform/exponential PDFs); the
   External distribution still yields 0.0 unless an inline fallback table is
   supplied; LHS falls back to plain MC for non-closed-form-ICDF distributions,
   and an importance node draws plain MC (LHS/IC skipped for it, phase-1 limit).
5. **Statistical sensitivity measures** — OAT + tornado landed, but correlation /
   regression (SRC) / partial-correlation / importance measures did not.
6. **Known stubs to close** — `Link.fluxes`/`geometry` still not parsed;
   `CapacityDemand` failure basis is a no-op (needs schema fields). *(`OnEvent`
   triggers and the `Event` failure basis are now wired — S1,
   `failure_bases_v2.rs`. Cell **concentration** output is computed in the
   engine — S2, `cell_concentration_v2.rs`. Cell bodies **are** now decoded by
   the emitter (volume/species/media populated); the remaining cell gap is the
   **mass-delivery + decay layer** — see §10a.)*

The mass-transport core, transport physics on links, fault-tree gates,
failure/repair FSM, nested-Monte-Carlo submodels, the Box's-complex optimizer
(now with **enforced constraints**), Markov chains, convolution, and SELDM parity
remain WASiM strengths — several are genuine subsets of GoldSim's *paid* modules.

---

## 1. Simulation paradigm & the time engine

| Capability | GoldSim | WASiM (now) | Status |
|---|---|---|---|
| Core paradigm | Dynamic + probabilistic + discrete-event **hybrid** | Same, with discrete events resolved on the grid + opt-in sub-interval integration | PARTIAL |
| Integration method | **Euler only**, by deliberate design | **Euler only** (`engine_v2.rs`) | **Match (by shared philosophy)** |
| Sub-timestep event accuracy | Unscheduled updates at *any* between-step event (scheduled events, bound crossings, resource exhaustion, `At Date/ETime/Stock Test`) | **Sub-interval integration at `on_schedule` instants and stock bound crossings** (`TimebaseMode::EventAccurate`, opt-in; split points from `schedule[]` on Events/Links/resampling plus closed-form floor/capacity crossings). Grid stays the statistical/state/reporting lattice; sub-steps consume no RNG and keep results `n_steps`-shaped. (Resource-exhaustion and condition-`Stock Test` sub-stepping still grid-quantized.) | **PARTIAL** |
| Bound-crossing accuracy | Overflow/withdrawal + coupled downstream re-eval computed at exact crossing time | `BoundCrossing` provider **wired into the step loop** (`engine_v2.rs`): a mid-step floor/capacity crossing splits the step at `t_c`, pins the stock to the bound, and re-runs so downstream elements reading the stock re-evaluate at the crossing for the remainder of the step. RNG-invariant across the re-run; 64-split/step guard. Tested in `timebase_boundcrossing_v2.rs` | **CLOSED (EventAccurate)** |
| Periodic-trigger / condition-trigger sub-stepping | Yes (`At Stock Test`, periodic) | **Absent** — only explicit `schedule` vectors split steps; periodic and condition crossings stay grid-quantized | ABSENT |
| Variable / scheduled *global* timestep | Scheduled timestep changes; dynamic adaptive; per-container internal clocks | **Absent** — single fixed global `dt` (Tier B2 deferred; per-container clocks are a documented non-goal) | ABSENT |
| Reporting periods | Accumulated / average / change / rate over periods | **FULL for fixed-length periods** (`results_spec.rs` `ReportingReduction`); **not yet calendar-month/year aware** | PARTIAL |
| Element evaluation order | Explicit **causality sequence** | Topological sort (`graph_v2.rs`); grid-only stateful rules fenced to run once per grid step under the sub-interval loop | Small delta |
| Deterministic vs probabilistic | Yes | Yes | Match |
| Calendar vs elapsed-time; leap years | Both; real calendar | **True proleptic-Gregorian leap-year calendar** when `calendar_start` anchor set; legacy fixed 365-day when not (`eval.rs`) | **CLOSED (anchor-gated)** |

**Net:** the deepest architectural gap is *narrower* but not closed. WASiM can now
place event *effects* and integration at exact scheduled instants and conserve mass
across a partial step — but only for explicitly scheduled times, only as opt-in
integration refinement, and still on one fixed global grid. Wiring the existing
`BoundCrossing` provider and landing Tier B2 (scheduled non-uniform grid) are the
two concrete steps left.

---

## 2. Element / object library

WASiM v2 primitives are now **7 (Node, Stock, Link, Event, Gate, Cell, Resource)
+ 2 definitions (Species, Medium)** — `Resource` is new (`model_v2.rs`).

### 2.1 Input elements

| GoldSim | WASiM (now) | Status |
|---|---|---|
| **Data** (scalar/vector/matrix constant) | `Node::Fixed` | Match (scalar/vector; no true matrix — see §9.3) |
| **Stochastic** (distribution; resamplable) | `Node::Sample` (+`autocorrelation`, `resampling`, `correlations`) | Match |
| **Time Series** | `Node::Series` | **Partial** — still lacks average-over-timestep output, `Rate_of_Change` output, discrete-change output mode, time-shifting (random start / align-years / periodic), and record-and-play-back |
| **Lookup Table** (1-D/2-D/**3-D**; derivative/integral/inverse; log interp) | `Node::Lookup` — now with **`TBL_Derivative`** mode, **log-result interpolation**, monotone-cubic (spline no longer downgraded to linear at parse), and reserved `TBL_*` names | **Largely closed** — verify 3-D table + true cubic *evaluation* fidelity in `eval.rs`; inverse-lookup coverage to confirm |
| **History Generator** (growth, random walk, reversion) | `Node::Process` now includes **reversion** (`process_reversion_v2.rs` test) + expressions | Partial (closer; not a 1:1 element) |

### 2.2 Function elements

| GoldSim | WASiM (now) | Status |
|---|---|---|
| Expression / Selector / Sum | `Expression` / nested `If` / `+`,`sum_array` | Match |
| **Extrema** (running lifetime peak/valley) | Running-extremum via the `Filter` whole-run-window encoding | **Match** (corrects Rev 1) |
| Allocator / Splitter | Link `priority` / `fraction`; Stock priority `withdrawals` | Partial |
| **Controller** (Deadband / Proportional / **PID**) | **`Node::PidController`** — all three modes: `pid` (setpoint, kp/ki/kd, deadband, clamps), `proportional` (kp only), **`on_off`** (stateful bang-bang hysteresis latch, §2.15) | **CLOSED** |
| **Convolution** | `Node::Convolution` | Match |
| **Previous Value** | `Node::Lag` + accumulator self-reference | Match |

### 2.3 Stock (integrator) elements

| GoldSim | WASiM `Stock` (now) | Status |
|---|---|---|
| **Integrator** (+ moving-average outputs; aging-chain Push) | `rate`/`inflows`−`outflows`, Euler; secondary port publication (grid-step aggregate rates) | Partial — no built-in moving-average outputs; **no aging-chain Push** |
| **Reservoir** (bounds; `Overflow_Rate`/`Withdrawal_Rate`/`Is_Full`; state-var feedback) | `floor`, `capacity`+`overflow_target`, priority `withdrawals`, `return_rate` | Near-match on mechanics; no `Is_Full` state-var feedback idiom |
| **Pool** (multiple named in/outflows w/ priority) | `inflows`/`outflows` lists + link priority | Partial-match |

### 2.4 Delay elements

| GoldSim | WASiM (now) | Status |
|---|---|---|
| **Material Delay** (conveyor, dispersion) | Link `transit_time` + `dispersion` (Ogata-Banks) + `decay_rate` | Match / richer |
| **Information Delay** (exp smoothing) | `Filter::Ema`; v1 `Delay` ring buffer | Partial |
| **Event / Discrete-Change Delay** (queue, capacity, service metrics) | **`Node::Queue`** (`delay_time`, `capacity`, `num_in_queue`, throughput; Conveyor/FixedAtEntry parsed) | **Largely closed** — disciplines parsed but both currently treated as fixed transit; `Mean_Time`/`Current_Service_Time` metrics not surfaced |

### 2.5 Discrete-event elements

| GoldSim | WASiM (now) | Status |
|---|---|---|
| **Timed Event** (5 occurrence types) | `Event` + `rate` (Poisson) / `Periodic` / `OnSchedule` triggers | Partial — stochastic-interval / remaining-time / cumulative-count types absent |
| **Triggered Event** | `Event` + `TriggerSpec` | Match |
| **Discrete Change** (Add/Replace/**Push**) | `EffectMode` `Additive`/`Multiplicative`/`Replace` (+ new `Spend`/`Deposit`/`Borrow`/`Interrupt`) | Near-match (**no Push**) |
| **Decision / Random Choice** | Composable via `If`/`Sample`; no importance sampling on choice | Partial |
| **Status** (latch) | **`Node::Status`** (set/reset triggers, set-wins) | **CLOSED** |
| **Milestone** (records event time) | **`Node::Milestone`** (first-fire elapsed; achieved-probability falls out of final-value distributions) | **CLOSED** |
| **Interrupt** (end realization) | **`EffectMode::Interrupt`** (holds last values for remaining steps) | **CLOSED** |

### 2.6 Logical elements

| GoldSim | WASiM | Status |
|---|---|---|
| And/Or/Not/Logic Tree (N-Vote) | `Gate` (+ inline `GateLogic`), Success/Failure semantics | Match / richer |

### 2.7 Structural / advanced elements

| GoldSim | WASiM (now) | Status |
|---|---|---|
| Container (hierarchy) | `container` / `ContainerDef` | Match |
| **Conditional Container** (dormant) | none | ABSENT (Tier C2, gated) |
| **Looping Container** (iterate-to-convergence) | none (v2 rejects cycles) | ABSENT (Tier C2, gated) |
| **Localized Container** (namespace scoping) | flat IDs | **BY-DESIGN non-goal** (emit-side id-qualification, per Tier C non-goals) |
| **SubModel** (nested Monte Carlo) | `ContainerDef{kind:Submodel}` + `SubmodelStat` + per-step dynamic optimization | Match |
| **Script** (procedural language) | none (single expression only) | **ABSENT — largest remaining gap (Tier C1)** |
| **Resource / Resource Store** | **`Primitive::Resource`** + Spend/Deposit/Borrow effects, priority, overdraw protection | **CLOSED** |
| **Spreadsheet / External(DLL) / File** | none (`spreadsheet` lowers to fixed-0 placeholder; `ExternCall`→0.0) | **BY-DESIGN non-goal** (Tier C4 salvageable slice = read snapshotted cell values, gated) |
| **Clone**, distributed processing, per-container clocks | none | **BY-DESIGN non-goals** |

### 2.8 Extension-module elements

| GoldSim module | WASiM (now) | Status |
|---|---|---|
| **Contaminant Transport / Flow** (Cell, Kd partitioning, decay chains, coupled ODEs) | `Cell` primitive: multi-media/species mass balance, `partitioning_equilibrium` (Kd, incl. **set-wide `species:null`** entries), `source_release`, radioactive **decay chains + daughter ingrowth**, advective/dispersive transport, **concentration output** `C=mass/(volume·fraction·porosity)` (S2, additive `:C` result id) | **Substantial partial match** — engine mechanisms exist and cell bodies now decode (volume/species/media populated), but **cell mass is ~0 corpus-wide**: no `initial_inventory`/`source` mass and 111/119 links are bare `{source,target}` shells (no species/rate), and **all 26 species have `half_life:null`** so no decay runs. `Link.fluxes`/`geometry` types exist but are **hard-coded empty in `v2_parse`**; no coupled-link stiff solver. **See §10a (emit-gated cell physics).** |
| **Reliability** (Action/Function, fault trees) | `Event.failure_process` FSM (ExposureTime/OperatingTime/Condition/Demand/**Event** bases + repair None/Repair/Replace/PM) + `Gate` fault trees | **Partial** — only `CapacityDemand` basis remains a **no-op** (needs schema fields) |
| **Financial** (Fund, annuity/PV/FV) | none (expressible via stocks/expressions) | ABSENT (low priority) |

---

## 3. Procedural scripting & control flow — **STILL OPEN**

- **GoldSim**: Script element = local variables, assignment, `if/else`, `for`,
  `do`, `while`, `repeat-until`, `break`/`continue`.
- **WASiM**: still **no procedural executor.** The only iteration is the functional
  array comprehension (`VectorMap`/`IndexRef`/`Index`); v1 `script` evaluates only
  `expressions[0]`; unparsed `Formula` strings evaluate to **0.0**. `WORKPLAN_TIER_C.md`
  C1 designs a `NodeRule::Script` + `script_v2.rs` interpreter with a step budget,
  no-RNG determinism, and per-realization static locals — **gated, not started.**

**Gap: large — now the #1 remaining engine gap** by corpus pressure (script-heavy
models such as `sac_sma`, `water_hammer`, `wind_model`, `precipgen_par` currently
evaluate to 0.0 where they use script logic).

---

## 4. Dynamics, integration & feedback

- **Integration**: both explicit Euler, fixed-rate-over-step — WASiM matches
  GoldSim's Appendix-F decision. Not a gap.
- **Discrete/continuous coupling**: WASiM now integrates stocks piecewise across
  sub-intervals when event-accurate mode is on, applying scheduled event *effects*
  at their instant (RNG-free), while event *firing* / Poisson counts stay grid-fenced
  for determinism. Off the scheduled set, timing is still grid-coarse.
- **Feedback vs recursive loops**: GoldSim distinguishes feedback loops (state
  variable, causality-ordered) from recursive loops (solved via Previous Value or
  **Looping Containers**). WASiM breaks cycles at `Lag`/stock back-edges and rejects
  other cycles. **Gap**: no iterate-to-convergence facility (Tier C2, gated).

---

## 5. Stocks, flows & mass balance

Still one of WASiM's strongest areas, near parity:

- **Match/near-match**: bounded reservoirs, overflow routing, priority
  withdrawals/allocations, compound growth, mass-conserving link transfers with
  plug-flow transit, Ogata-Banks dispersion, first-order transit decay.
- **Multi-cell mass transport**: mass per `(cell, species, medium)`, Kd
  partitioning, radioactive decay chains with daughter ingrowth — a real subset of
  GoldSim's Contaminant Transport module.
- **Remaining gaps**: (a) Cell **mass-delivery + decay** are emit-gated — the
  engine mechanisms (concentration S2, partitioning incl. set-wide Kd, decay-chain
  ingrowth) are in place and cell bodies now decode, but the corpus supplies no mass
  to move (0/148 cells carry `initial_inventory`/source; 111/119 links are bare) and
  no decay data (26/26 species `half_life:null`). **See §10a.**
  (b) `Link.fluxes`/`geometry` (advective/diffusive/settling/precipitation;
  pipe/aquifer/conduit) **defined in types but not parsed**; (c) no
  `Is_Full` state-variable output (though a `ref` to the stock compared against its
  capacity now reflects the mid-step crossing under `EventAccurate`).

  *(Bound-crossing subdivision — formerly listed here as unwired — is now wired into
  the step loop under `EventAccurate`; coupled downstream re-evaluation at the crossing
  is delivered.)*

---

## 6. Probability, distributions & sampling

### 6.1 Distribution roster — **largely closed**

WASiM now covers the GoldSim roster the original doc flagged as missing:
Log-Uniform, Log-Triangular, Triangular/Log-Triangular **10-90**, Log-Cumulative,
**Binomial**, **Negative Binomial**, **Poisson** (as a sampling distribution),
**Extreme Probability (min/max)**, **Beta(successes, failures)** — plus its prior
set and **Trapezoidal** (which GoldSim lacks). (`sampling.rs`,
`distributions_roster_v2.rs`.)

- **External distribution**: **PARTIAL** — now samples an inline `fallback`
  empirical table when present; **still degrades to 0.0** (with a warning) when no
  fallback is supplied.

### 6.2 Sampling & correlation

| Capability | GoldSim | WASiM (now) | Status |
|---|---|---|---|
| Monte Carlo | Yes | Yes (ChaCha8, per-realization stream) | Match |
| **Latin Hypercube** | Yes | **FULL** — real stratified pre-pass via `icdf_truncated`, composes with Iman-Conover; **falls back to MC** for non-closed-form-ICDF distributions (Gamma/Beta/Weibull/Pearson/Pert/StudentT/Binomial/NegBinom/Poisson/External) | **CLOSED (with fallback caveat)** |
| Rank correlation | Copulas / Iman et al. | Iman-Conover (v2) / Gaussian copula (v1) | Match |
| Autocorrelation | Yes | AR(1) per-step | Match |
| Truncation | Yes | Rejection; clamp under AR(1); ICDF-scaled under LHS | Match |
| **Realization weights** | Yes | **PARTIAL** — `RunConfig.realization_weights` reweight output **statistics** (weighted mean/percentile/CTE/bands) post-hoc; they do **not** bias draws | PARTIAL |
| **Importance sampling** | Yes (rare-event biasing) | **CLOSED (S4)** — `distribution.importance.bias` draws from g, carries w=f(x)/g(x) into the weighted reductions; PDFs for normal/lognormal/uniform/exponential (others error); importance nodes draw plain MC (LHS/IC skipped, phase-1) | CLOSED |
| **Bayesian updating** | Yes | Absent | ABSENT |

**Net:** LHS closed; roster closed; weights added as post-hoc reweighting;
**importance sampling closed (S4)**. Remaining: Bayesian updating.

---

## 7. Submodels, nested Monte Carlo & containers

- **Match**: `ContainerDef{kind:Submodel}` nested Monte Carlo with interface
  input-binding via `from` dependency-closure extraction; parent expressions pull
  `SubmodelStat` (`Mean`/`Percentile`/`Sd`/`CumulativeProb`). **New**: per-timestep
  **dynamic optimization** inside submodels (`Model.dynamic_optimization` +
  `optimization`).
- **Gaps** (all Tier C2, gated / non-goal): **Conditional** (dormant), **Looping**
  (iterate-to-convergence), **Localized** (scoping — non-goal), per-container
  internal clocks (non-goal).

---

## 8. Optimization, sensitivity & results analysis

### 8.1 Optimization — **constraints now enforced**

- Box's complex method over editable `Fixed` scalar variables; objective
  `Maximize`/`Minimize` reduced by `Mean/Percentile/Peak/Valley/Sum`; probabilistic
  objectives re-run the nested submodel per candidate.
- **CLOSED**: optimization **constraints are now enforced** — `optimize_v2` collects
  `constraint_refs`, evaluates them in the same run via
  `eval_harness::evaluate_point_with_extras`, and coerces cost to +∞ on violation.
  (A constraint the engine cannot evaluate is treated as satisfied — never
  spuriously rejects.)

### 8.2 Sensitivity analysis — **PARTIAL (was already implemented)**

- **FULL**: `sensitivity_v2.rs` implements **one-at-a-time** line sweeps (per-variable
  `VarCurve`) and **tornado** (`TornadoBar`, sorted by swing), reducing probabilistic
  targets by the objective-statistic set. Runtime-configured (`SensitivitySpec`),
  exported from `lib.rs`.
- **ABSENT**: statistical measures from a probabilistic run — coefficient of
  determination, correlation coefficients, standardized regression coefficients,
  partial correlation coefficients, importance measures. GoldSim provides all five.

### 8.3 Results / output analysis — **largely closed**

`results_spec.rs` + `ElementResults.analysis` (runtime-configured via
`RunConfig.results_spec`, all additive `Option` fields; default output
byte-identical):

| GoldSim result capability | WASiM (now) | Status |
|---|---|---|
| Time-history bands, **user-chosen percentiles** | `PercentileBand` per requested percentile (weighted when weights set) | **CLOSED** |
| **PDF / CDF / CCDF** (exceedance) | `Distribution { bin_centers, pdf, x, cdf, ccdf }` | **CLOSED** |
| **Capture Times** (snapshot at arbitrary times) | `CaptureSnapshot` at nearest stored step | **CLOSED** |
| Final-value stats: CI on the mean, skewness, kurtosis, **CTE** | `FinalStats { ci_*, skewness, excess_kurtosis, cte }` | **CLOSED** |
| **Reporting-period aggregation** (accumulated/average/change/rate) | `ReportingPeriods` / `ReportingReduction` | **CLOSED (fixed-length; not yet calendar-aware)** |
| **Realization classification & screening** (categories by condition, Net/Gross %, include/exclude) | none | **ABSENT** |
| **Scenarios** (store/compare input sets) | none | **ABSENT** |
| Importance-weighted statistics | weights honored in reductions; **importance sampling now produces them** (S4) | Match |

**Net:** the statistics layer went from a fixed `mean + 5 percentiles` summary to a
configurable probabilistic-analysis layer. The two remaining pieces are realization
**classification/screening** and **scenarios**.

---

## 9. Units, expression language & interop

### 9.1 Dimensional analysis — **CLOSED (static)**

- **NEW**: `units.rs::check_dimensions` is a real dimensional checker — an exponent
  vector `Dim([i8;5])` over {Time, Length, Mass, Volume, Temperature}, statically
  inferring each expression's dimension and comparing to the declared output;
  flagging add/subtract/compare of unequal dims, transcendentals of dimensioned
  args, `sqrt` of odd exponents, mismatched `if` branches, and lookup `TBL_*`
  adjustments. `UnitsMode::Strict` turns any inconsistency into a **hard load
  error**; `Warn` (default) logs and continues. Unknown units are exempt (no false
  positives). This is **static/load-time** analysis, not per-step runtime unit
  tracking, but it can hard-fail a run — closing the Rev 1 gap.

### 9.2 Calendar / dates — **CLOSED (anchor-gated)**

- True proleptic-Gregorian **leap-year calendar** when `calendar_start` is set;
  new time refs `Hour/Minute/Second/Start/ElapsedMonths/ElapsedYears` and date
  builtins `GetYear/Month/Day/Hour/Minute/Second`. Without an anchor, the legacy
  fixed 365-day calendar is preserved.

### 9.3 Built-in functions & arrays

- Function set still lacks GoldSim's `cot`, **Bessel**, **beta**, **erf**,
  standard-normal / Student-t **distribution functions**, `occurs`/`changed` —
  *update*: `occurs(event_id)`/`changed(ref)` **were added** (Tier A5) — importance-
  sampling functions, and financial functions. Small-medium gap remains on special
  functions.
- **Arrays**: first-class vectors with broadcasting, implemented comprehensions,
  array builtins. **True 2-D matrices and matrix algebra (solve systems) remain
  absent** (Tier C3, gated) — corpus matrix sites are currently opaque
  `extern_call`s.

### 9.4 External coupling — **OPEN by design**

- Excel/DLL/ODBC coupling remains absent and is an **explicit non-goal** (Tier C
  non-goals: contradicts the open-JSON/WASM thesis). `ExternCall` preserves round-trip
  but evaluates to 0.0; the `spreadsheet` element lowers to a fixed-0 placeholder.
  The one salvageable slice (reading Excel cell *values* snapshotted at export, Tier
  C4) is demand-gated. This is a deliberate scope difference, not a defect.

---

## 10. Consolidated, severity-ranked gap table (Rev 2)

Severity = engine impact for reproducing GoldSim-class models. **Status** reflects
the current code.

| # | Gap | Severity | **Status** | Notes |
|---|---|---|---|---|
| 1 | Full event-accurate / variable timestepping (periodic sub-stepping, scheduled non-uniform *global* grid) | High | **PARTIAL** | Scheduled-instant **and bound-crossing** sub-integration landed; remaining: periodic-trigger sub-stepping, Tier B2 (non-uniform global grid) |
| 2 | **Procedural Script element** (loops, if/else, locals) | High | **OPEN** | Tier C1, gated, not started — largest remaining gap |
| 3 | Results/analysis engine | High | **MOSTLY CLOSED** | Custom percentiles, PDF/CDF/CCDF, capture times, CI/skew/kurtosis/CTE, reporting periods all landed; **classification/screening + scenarios still absent** |
| 4 | Discrete-event depth (queues, resources, milestone, interrupt, status, PID) | High | **MOSTLY CLOSED** | All landed incl. `OnEvent` trigger; remaining: aging-chain **Push**, richer Timed-Event types |
| 5 | Sampling tooling (LHS, importance sampling, weights, Bayesian) | Med-High | **PARTIAL** | LHS **closed**; weights post-hoc; **importance sampling closed (S4)**; Bayesian absent |
| 6 | Runtime dimensional analysis; leap-year calendar | Medium | **CLOSED** | Static strict-units mode (hard-fail) + anchor-gated real calendar |
| 7 | Sensitivity analysis | Medium | **PARTIAL** | OAT + tornado **closed**; statistical measures (correlation/SRC/partial/importance) absent |
| 8 | Container semantics (Conditional / Looping / Localized) | Medium | **OPEN / BY-DESIGN** | Conditional+Looping = Tier C2 (gated); Localized = non-goal |
| 9 | Distribution roster; External-dist sampling | Medium | **MOSTLY CLOSED** | Roster expanded; External still 0.0 without an inline fallback table |
| 10 | Lookup tables (3-D, derivative/integral/inverse, log interp, cubic) | Medium | **MOSTLY CLOSED** | `TBL_Derivative`, log interp, monotone-cubic landed; verify 3-D + true cubic *eval* fidelity |
| 11 | Feedback controller; running extrema; history generator | Medium | **CLOSED / PARTIAL** | PID + running extrema **closed**; process reversion added; History Generator not a 1:1 element |
| 12 | Matrix algebra & label-set arrays | Medium | **OPEN** | Tier C3, gated |
| 13 | External coupling (Excel/DLL/ODBC) | Low-Med | **BY-DESIGN** | Explicit non-goal; C4 read-only cell slice gated |
| 14 | Known stubs: `Link.fluxes/geometry` unparsed; `CapacityDemand` basis no-op; `Formula` strings→0.0 | Varies | **OPEN** | Each strands specific models; low-effort to close individually. (`OnEvent` trigger + `Event` failure basis closed — S1; cell **concentration**/**set-wide Kd** closed — S2; `on_off`/`proportional` controller modes closed. Emit-gated cell mass/decay: §10a.) |
| 15 | Financial primitives/functions | Low | OPEN | Expressible via stocks/expressions |
| 16 | Aging-chain Push; Clone; distributed processing; per-container clocks | Low | **OPEN / BY-DESIGN** | Push is a real small gap; the rest are non-goals |

---

## 10a. Emit-gated capabilities (engine done; blocked upstream in re-gsm)

These are capabilities where the **engine mechanism is implemented and tested**, but no corpus
model exercises it because the **re-gsm emitter does not yet decode the required input**. They are
*not* engine gaps and *not* parse blockers — the affected models run clean, they just compute a
degenerate result (zero mass, no decay, an inert latch) until the emit side lands. Tracked here so
they are not mistaken for engine work or re-investigated. Corpus figures are from the 0.9.7
regeneration (220 files); re-verify after any regen.

| Capability | Engine state | Emit gap (the blocker) | Corpus evidence (0.9.7) |
|---|---|---|---|
| **Cell mass delivery** | `source_release`, `species_transport` links, `inflows`, partitioning (incl. set-wide Kd), concentration (S2) all implemented + tested (`cells_v2.rs`, `cell_concentration_v2.rs`) | re-gsm decodes cell **structure** (volume/species/media) but not the **mass sources**: no `initial_inventory`, no `release_rate`/`inventory` source, and transport **links are bare** (`{source,target}` only — no `species`/`rate`/`fraction`/`fluxes`). So cells have structure but nothing puts mass in them → all cell masses (and thus `:C` concentrations) read ~0. | 0/148 cells carry `initial_inventory`; 0 carry a source `release_rate`/`inventory`; **111/119 links are bare** (only 8 carry transport data). |
| **Radioactive decay chains** | Per-`(species, medium)` first-order decay + daughter ingrowth, parent-first topo order (`engine_v2.rs`; `cells_v2::decay_chain_ingrowth_conserves_mass`) | re-gsm emits the species set as **one dimension element with `half_life: null`**, not per-nuclide defs carrying each nuclide's half-life/decay_products. With null half-lives no decay runs for any nuclide. This is the "species-set → dimension vs. per-substance defs" question from the emit-pathology thread (#4): the members are a dimension, but decay needs per-member half-life/products to coexist with it. | **26/26 `species` elements have `half_life: null`.** |
| **`Link.fluxes` / `geometry` transport** | Types exist (`FluxSpec`, `FluxMechanism`, `LinkGeometry`) but execution is **not** wired (`v2_parse` hard-codes `fluxes: Vec::new()`) — this half is *also* an engine gap (§S3, deferred) | re-gsm emits `geometry` (10 links, bare tag) but **no `fluxes[]`** — so even if the engine executed fluxes there is no mechanism/rate data to drive them. Doubly blocked (emit + engine). | 0 links carry `fluxes`; 10 carry a bare `geometry` string. |
| **Event-driven `status` latches** | The **`OnEvent` trigger and `on_event` trigger *mode* are implemented** (S1, `failure_bases_v2.rs`; a status/event fires when its `source` event is in the fired-set). The engine consumes them fully. | re-gsm cannot yet **bind** the 6 event-driven status nodes' set/reset to their event sources — their triggers come from *drawn event links*, not in-body conditions, so re-gsm emits never-firing `on_condition` placeholders (they parse but the latch never toggles). Needs the re-gsm event-link → `on_event` `source` resolution (tracks with re-gsm `CONNECTION_WIRING`). **No engine work remains** — once re-gsm emits `{mode: on_event, source: <event>}`, the engine already handles it. | 6 status nodes (`discreteevents/Status2`, `option/Exercised`, `simple_stream_diversions/Diversion_ON`, `srm_snowmelt_runoff/{Melt_Transition_Period,RainExceedence}`, `statusmilestone/Status`) emit inert triggers. |
| **on_off controller output** | Stateful hysteresis-latch handler implemented + tested (`discrete_nodes_v2.rs`); reads top-level `controller_mode`/`output_cap`/`deadband_ref` | **Closed as of re-gsm R3** — the fields are now lifted top-level and the 18 on_off controllers compute the latch. Listed here only for completeness (previously emit-gated; now resolved). | 18/18 on_off carry `controller_mode`; 17 carry `output_cap` (`unscheduledtimesteps` has none → documented 0/1-gate default). |

**Reading this table:** the first two rows (cell mass, decay) are the substantive open items — both are
**re-gsm decode gaps** on the contaminant-transport line, and both must land before any cell/decay
model produces non-trivial output. The `Link.fluxes` row is additionally an engine gap (§S3,
deferred). The last two rows are **closed** (event-`OnEvent` mode + on_off) and shown only so the
engine side is not re-touched — the residual on `status` is purely re-gsm's event-link binding.

---

## 11. Where WASiM already matches or leads GoldSim's engine

- **Euler-only integration** — GoldSim's own deliberate design; not a gap.
- **Rank correlation (Iman-Conover), AR(1) autocorrelation, and now real LHS** —
  at or beyond parity with GoldSim's sampling machinery.
- **Mass-transport core** — multi-cell/species/medium mass balance with Kd
  partitioning and radioactive decay chains + daughter ingrowth: a genuine slice of
  GoldSim's *paid* Contaminant Transport module, in WASiM's core.
- **Transport physics on links** — plug-flow transit, Ogata-Banks dispersion,
  first-order transit decay: matches/exceeds the Material Delay element.
- **Fault-tree gates** + **failure/repair FSM** — a real slice of the Reliability
  module.
- **Nested-Monte-Carlo submodels** with dependency-closure input binding and a
  **constraint-enforcing optimizer** (Box's complex), plus **per-step dynamic
  optimization** inside submodels.
- **Rich results/analysis layer** (custom percentiles, PDF/CDF/CCDF, capture times,
  CTE/CI/skew/kurtosis, reporting periods) and a **static dimensional checker** that
  can hard-fail inconsistent models — both now first-class.
- **Markov chains, Convolution, Hysteresis, PID, Status/Milestone, Queues,
  Resources, rolling-window & running-extremum Filters** — first-class primitives.
- **Open, diffable JSON model format** with a WASM runtime — a deliberate,
  different design point from GoldSim's binary `.gsm`.
- **SELDM parity** (USGS) — validated real-world modeling capability.

---

## 12. Suggested engine roadmap (updated)

Most of Rev 1's roadmap (Tiers A + B) has landed. What remains, in leverage order:

1. **Procedural Script executor** (Tier C1, gap #2) — the highest-pressure open
   item; script-heavy corpus models currently evaluate to 0.0. Proposal doc →
   emit sign-off → interpreter with hand-authored fixtures → schema, per the tier's
   gate.
2. **Finish the timebase** (gap #1): wire the existing `BoundCrossing` provider into
   `engine_v2`'s step loop; allow periodic triggers to sub-step; land Tier B2
   (scheduled non-uniform global grid, needs frontend coordination).
3. **Results: realization classification/screening + scenarios** (gap #3 remainder)
   and **calendar-aware reporting periods** (pair B4 with the B6 calendar).
4. **Sampling depth** (gap #5 remainder): ~~importance sampling~~ (closed, S4);
   Bayesian updating; make the External distribution error (or require a fallback)
   instead of silently yielding 0.0.
5. **Statistical sensitivity measures** (gap #7 remainder): correlation / SRC /
   partial-correlation / importance measures from a probabilistic run.
6. **Close the small stubs** (gap #14): parse `Link.fluxes`/`geometry`, the
   `CapacityDemand` failure basis (needs schema fields), aging-chain **Push**.
   *(Done: cell concentration + set-wide Kd — S2; `OnEvent`/`Event` bases — S1;
   `on_off`/`proportional` controller modes.)*
7. **Contaminant-transport emit gaps** (§10a, upstream in re-gsm, not engine work):
   cell **mass delivery** (inventory/source/transport-link decode) and per-nuclide
   **half-life** for decay chains — the two blockers to any non-trivial cell/decay
   output.
7. **Demand-gated big bets**: matrix algebra + label-set arrays (C3), Looping/
   Conditional containers (C2), spreadsheet cell *reader* (C4) — only when a named
   model needs them.

Documented **non-goals** (do not re-litigate): DLL/ODBC/external coupling,
Localized-Container scoping, Clone, distributed processing, per-container internal
clocks, full adaptive error-controlled timestepping.

---

### Appendix — methodology & confidence

- WASiM claims are **high-confidence** (read from `engine/src/*.rs` and
  `engine/tests/*` at the current `main`; `FULL/PARTIAL/ABSENT` reflect whether a
  capability is wired into `engine_v2.rs`, not merely present as a type or
  library function — e.g. `BoundCrossing` is implemented in `timebase.rs` but
  ABSENT from the run loop, and is marked accordingly).
- GoldSim claims are **high-confidence for capability existence** (User Guide TOC,
  Glossary, Index, Appendix D–F, and detailed chapter slices). Mechanical details
  in non-extractable page bands are inferred from the Index/Glossary + cross-
  references and do not affect gap identification, only depth.
- "Gap" means an engine capability present in GoldSim with no faithful WASiM engine
  encoding. "BY-DESIGN" marks capabilities the project has explicitly chosen not to
  pursue (`WORKPLAN_TIER_C.md` non-goals). Status labels (CLOSED/PARTIAL/OPEN)
  are as of this re-run against the current engine.
