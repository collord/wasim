# Workplan — Tier B: pluggable timebase + medium GoldSim-parity items

**Source:** `GOLDSIM_ENGINE_GAP_ANALYSIS.md` (§10 gap numbers), feasibility triage of
2026-07-19, and the pluggable-timebase design discussion of the same date (captured in
full in B1 below — this document is the canonical record of that design).

**Prerequisite:** Tier A (`WORKPLAN_TIER_A.md`) is assumed done — B4 rides on A3's
results layer, B7 on A3's stat reductions. B1–B2 are independent of Tier A but touch
`engine_v2.rs` heavily; do not run them concurrently with another engine_v2 workstream.

**State pinned at authoring (2026-07-19):** HEAD `f47387c` + uncommitted 0.9.2 changes
(reserved globals, TBL_* modes, stock port roles, filter-input tolerance). Tier A will
have moved things — the kickoff prompt's drift check is mandatory, not advisory.

**DRIFT CHECK PERFORMED (2026-07-19, post-Tier-A):** Tier A landed as commit `d885932`
("schema 0.9.2 detention-pond parity + Tier-A GoldSim-parity round"); schema is now
**0.9.3** (not 0.9.2). Findings that update this plan (applied below in place):
- **Step loop moved and grew.** `engine_v2::run`'s `for step_idx in 0..n_steps` loop now
  opens with (a) an **interrupt guard** (A5: `if interrupted { hold last values; continue }`)
  and (b) an **event fired-set pre-pass** (A5: populates `fired_events` for `occurs` before
  the topo loop). B1's inner sub-interval loop goes *between* the redraw pass and history
  recording, as designed — but it must sit **inside** the interrupt guard (an interrupted
  grid step does no integration at all) and **after** the fired-set pre-pass.
- **New per-step-stateful maps to classify grid-only** (A5): `status_state` (latch),
  `milestone_time` (first-fire elapsed), `pid_state` (integral + prev-error). All are
  grid-step triggers/records — they advance **per grid step**, never per sub-interval, and
  join `hyst_state`/`filter_*`/`markov_state`/`conv_buf` in the "outside the inner loop" set.
  The `fired_events` set and `interrupted`/`interrupt_now` flags are likewise grid-only.
- **PID coupling (new, add to the audit list):** `NodeRule::PidController` reads `input`'s
  current value and uses `dt` for its integral/derivative terms. It is a discrete controller
  on the reporting lattice → evaluate **once per grid step with the grid `dt`**, NOT per
  sub-interval. (Sub-stepping a PID would change its effective sample time and detune it.)
  Put PID in the grid-only topo pass, or evaluate it on the final sub-interval's outputs.
- **A3 results layer is clean for B1/B4/B7.** A3 added `RunConfig.results_spec` +
  `ElementResults.analysis`, computed in a **post-run** pass over `hist_store`/`final_store`.
  Because B1 keeps `hist_store` `n_steps`-shaped (the invariant), A3 needs no change; B4
  (reporting periods) and B7 (weights) ride on A3's `compute_analysis`/`reduce` exactly as
  planned. `results_spec.rs` is the module to extend for B4/B7.
- **Schema/semantics:** next bump is **0.9.4** (0.9.3 is Tier A). Distribution roster,
  node rules, and lookup modes from Tier A do not touch the step-loop timebase.

**Standing conventions:** identical to Tier A's (wasm rebuild after model/AST changes;
schema symlink + `$id`/CHANGELOG bump; topical test files; slow full suite; result-shape
changes additive/Option-typed).

---

## B1. Pluggable timebase, phase 1 — sub-step refinement on a fixed grid (gap #1 core)

### The invariant that makes this incremental (do not violate it)

**The grid remains the statistical, state-machine, and reporting lattice; sub-steps
refine *integration* only.**

Concretely — advance **per grid step only**: filter buffers (emit's whole-run-window
running-extremum encoding depends on evaluation *count*; sub-step pushes would silently
break it), Markov transitions, AR(1)/process redraws and ALL RNG draws (sub-steps must
consume no randomness — seed/realization streams stay stable), convolution buffer
advancement, per-step dynamic optimization (§13a), history recording (`hist_store`
stays `n_steps`-shaped; results contract untouched — frontend/sensitivity/optimizer
never see sub-steps). **Also grid-only (Tier A additions):** the `status` latch, the
`milestone` first-fire record, the `pid` controller (a discrete controller on the
reporting lattice — evaluate once per grid step with the grid `dt`), the event
fired-set pre-pass + `occurs`/`changed` visibility, and the interrupt guard/flag.

Advance **per sub-interval** with the actual sub-`dt`: topo expression evaluation,
stock integration (incl. floor/capacity/overflow/withdrawals), link transfers, cell
transport, event *effects* whose instants fall in the sub-interval.

Enforce the boundary structurally: the sub-step evaluation context should simply not
carry the RNG or per-step state handles, so a future edit cannot violate the invariant
by accident.

### Implementation progress (2026-07-19 session)

**Landed (green, committable):**
- `engine/src/timebase.rs` — `TimebaseProvider` trait + `FixedGrid` / `ScheduledTimes` /
  `BoundCrossing` (closed-form floor/capacity crossing) / `Composite`, with `StepView` /
  `StockBoundView` read-only views and `sub_boundaries()`. 7 unit tests. FixedGrid yields
  no splits.
- `engine/tests/timebase_bit_identity.rs` + `tests/fixtures/timebase_snapshot.json` — the
  **bit-identity regression gate** (reservoir/pond/markovd digests: mean series + final mean;
  regen with `WASIM_WRITE_SNAPSHOT=1`). Green against current engine. ~7s.
- Coupling #2 DONE: `CalendarState::from_step` → `from_elapsed` (elapsed-based; uniform-grid
  behavior identical; gate-verified).
- Coupling #1 REFINED (see below): transit buffers stay grid-step-keyed (no time re-key).

**Derived design for the loop restructure (the remaining core — execute this next):**
Split the topo loop by classification, then wrap integration in a sub-interval inner loop.
- **Grid-only node rules (run ONCE per grid step, on the final sub-interval's outputs — they
  advance state / consume RNG once):** `Hysteresis`, `Filter`, `Status`, `Milestone`,
  `PidController`, `Markov` (RNG!), `Convolution`. These read prev-grid-step context and their
  state maps advance per grid step.
- **Integration/pure (re-evaluate per sub-interval with `sub_dt`/`sub_t`):** the `eval_element`
  fallback (fixed/expression/sample/process/lookup/series/lag — pure fns of current outputs +
  rv_samples/sp_state), `GateLogic`/`Gate`, the **link-transfer** pass (rate·dt → rate·sub_dt),
  the **event-effects** pass (effects whose instant falls in the sub-interval; Poisson λ·dt →
  λ·sub_dt — but RNG draws are grid-only, so Poisson counts stay per-grid-step: fence event
  *firing* to grid, apply only scheduled-instant effects per sub-interval), **stock integration**
  (rate·dt → rate·sub_dt, floor/capacity/overflow/withdrawals per sub-interval), **cell transport**.
- **Per-grid-step (outside the inner loop):** redraw pass, fired-set pre-pass, interrupt guard
  (wraps the inner loop — an interrupted step does zero integration), dynamic-opt (§13a), stock
  secondary-port publication (aggregate applied rates across sub-intervals ÷ grid dt), history
  recording.
- **Structure:** `for step_idx { <interrupt guard> <redraws> <fired-set> for sub in
  sub_boundaries.windows(2) { sub_t, sub_dt; <pure topo> <link> <event effects@instant> <stock>
  <cell> }; <grid-only node rules on final outputs> <dyn-opt> <history> }`. On FixedGrid the
  inner loop runs once with sub_dt==dt, sub_t==elapsed → bit-identical (the gate proves it).
- **Careful bit:** stateful node rules currently interleave with pure rules in ONE topo pass, and
  a pure rule may read a stateful rule's output (and vice-versa). Because grid-only rules read
  the PREVIOUS grid step's context (stocks in `outputs` hold prev level during topo; filters read
  `outputs[input]`), running them once on the final sub-interval preserves their inputs. VERIFY
  this per-rule against the gate; if any stateful rule's input is itself sub-stepped within the
  same grid step, that pair needs the stateful rule to read the final sub-interval value (which
  running-last already gives). Add a targeted test if a corpus model interleaves them.
- **BoundCrossing value note:** for a SINGLE stock, Euler already conserves mass exactly at a
  crossing (`excess = level+rate·dt − cap`, routed to overflow) — the current code is already
  exact. BoundCrossing's real payoff is COUPLED subsystems (a downstream rate/event that changes
  once the stock is full), which needs the full topo re-evaluation per sub-interval. So the inner
  loop is load-bearing; there is no stock-pass-only shortcut.

### Design

- `trait TimebaseProvider`: given (step_idx, t_start, dt, &model-state view) yields the
  ordered interior split points for this grid step. Compose providers (union of split
  points).
- **`FixedGrid`** (default): yields none → current behavior, **bit-identical** (this is
  the phase-1 regression gate).
- **`ScheduledTimes`**: exact known instants — At-ETime/At-Date event triggers and any
  schedule-typed `TriggerSpec`s. Collect at run start from the model's triggers.
- **`BoundCrossing`**: Euler holds rates constant across a step, so a stock's
  within-step trajectory is **linear** — crossing times of `floor`/`capacity` are
  closed-form, no root-finding. Loop: evaluate rates at sub-interval start → if any
  stock crosses a bound before sub-interval end, split at the earliest crossing, apply
  up to it, re-evaluate the graph, continue. Cascading crossings fall out because each
  sub-interval re-evaluates; guard with max-splits-per-step (e.g. 32) + a warning.
- Step loop restructure in `engine_v2::run`: the body between "per-step redraws" and
  "history recording" becomes an inner loop over sub-intervals `[t_i, t_{i+1})`. The
  per-step-stateful blocks stay outside the inner loop, evaluated once per grid step on
  the final sub-interval's outputs. **Post-Tier-A shape:** the inner loop nests INSIDE
  the A5 interrupt guard (`if interrupted { … continue }` short-circuits the whole grid
  step — no sub-stepping) and AFTER the A5 event fired-set pre-pass. `status`/`milestone`/
  `pid` node rules and the event pass evaluate once per grid step (grid-only), not per
  sub-interval.

### Known couplings to fix (audited 2026-07-19 — re-verify at kickoff)

1. **Link transit buffers** `link_buf: HashMap<String, HashMap<usize, f64>>` key
   releases by **step index**. **DESIGN REFINEMENT (2026-07-19, post-Tier-A audit):**
   transit *buffer advancement* is on the invariant's grid-only list (like the convolution
   buffer), and plug-flow delay is already quantized to grid steps (`steps = tt/dt`). So the
   buffer stays **grid-step-keyed** — link *transfers* within a sub-interval accumulate into the
   current grid step's buffer entry, and delivery (`buf.remove(&step_idx)`) happens once per grid
   step. No time re-key is needed for phase 1; keeping step-index keys preserves the whole-run
   dispersion encoding. (A future phase-2 non-uniform grid may revisit this.)
2. **`CalendarState::from_step(step_index, dt, dt_unit)`** (eval.rs) derives calendar
   from step count — change to derive from elapsed time (worthwhile cleanup
   regardless; keep behavior identical for uniform grids).
3. **Triggered-event condition crossings** (At-Stock-Test style, arbitrary
   expressions): NOT analytic in t. **Fence out of phase 1** — they stay
   grid-quantized. Document in semantics. (Optional later: linear interpolation of the
   trigger operand.)
4. **`elapsed` uses**: audit every `ctx_at(...)` call site — sub-interval contexts get
   true `t` and sub-`dt`; per-step contexts keep grid `t`/`dt`.
5. Withdrawal-allocation and stock-port publication (§1c roles): ports report the
   grid-step *aggregate* applied rates (integrate sub-interval contributions, divide by
   grid dt) so consumers see unchanged semantics.
6. **(Tier A) PID controller** (`NodeRule::PidController`): its integral/derivative use
   `dt`. Evaluate once per grid step with grid `dt` (grid-only) — do NOT sub-step it, or
   the effective sample time changes and the loop detunes.
7. **(Tier A) `occurs`/`changed` + fired-set pre-pass**: the fired-set is populated once
   per grid step before the topo loop; keep it grid-only so `occurs` semantics (a fire is
   a grid-step event) are unchanged. `changed` compares grid-step outputs vs prev grid step.
8. **(Tier A) interrupt**: the `if interrupted { hold; continue }` guard must wrap the
   inner sub-interval loop — an interrupted realization does zero integration thereafter.

### Semantics & schema

- Semantics doc: new § "Timebase and unscheduled updates" — the invariant, what runs
  per sub-interval vs per step, the phase-1 fencing (condition triggers grid-quantized),
  determinism guarantee (no RNG on sub-steps).
- Schema: **no change required for phase 1** (scheduled times come from existing
  trigger fields; bound crossing is automatic). `RunConfig` flag to disable
  (`timebase: fixed|event_accurate`) for A/B comparison; default ON after the corpus
  A/B shows no regressions, OFF until then.

### Tests

- Bit-identity: `FixedGrid` vs pre-change results across a corpus sample (assert equal
  time histories, not just finals).
- Mass conservation at capacity crossing: single stock, constant inflow, capacity hit
  mid-step → overflow amount exact (vs analytic), level pinned at capacity from the
  crossing instant.
- Scheduled event at t=33.65d on a 1d grid: effect applied at the exact instant;
  downstream integral reflects the partial step.
- RNG stability: probabilistic model with/without sub-steps → identical draws.
- Determinism across provider composition order.

**Effort:** 1–2 weeks. **Risk:** medium — the audit in "couplings" is the risk list;
anything else discovered goes in the semantics doc before code.

## B2. Pluggable timebase, phase 2 — scheduled timestep changes

**Only after B1 soaks.** The grid itself becomes non-uniform (GoldSim: "1 hr during
storm windows, 1 d otherwise").
- Schema: `simulation_settings.timestep_schedule: [{from, to, timestep}]` (+ semantics
  §; `$id` bump).
- Results: `TimeHistoryStats` gains a `timestamps: Vec<f64>` (additive). Frontend plot
  axis must consume it — coordinate; until then a run using a schedule reports
  timestamps and the frontend interpolates or steps.
- **Statistical scaling on non-uniform grid steps:** AR(1) per-step ρ →
  ρ^(dt/dt_nominal); per-step Markov matrices → same exponent question (matrix power /
  rate-matrix approximation — document the chosen approximation honestly); Poisson
  event rates already scale by dt (no change). Filter windows: document that `window`
  counts grid steps of *whatever* size (emit's whole-run encoding still holds because
  evaluation count still ≤ window).
- `n_steps`/`hist_store` sizing from the schedule; `duration_in_dt` logic generalizes.
**Effort:** ~1 week + frontend coordination.

## B3. Queue/capacity delays + Resources (gap #4 core)

- **Event Delay / Discrete-Change Delay**: new node rule (or Event trait) with
  `delay_time` (QuantityOrFormula), `capacity` (max in transit/queue), service
  discipline (conveyor vs fixed-at-entry), outputs `Num_in_Queue`, throughput. The
  link transit buffer (`link_buf` schedule maps) is the machinery template — a
  discrete-entity sibling of it. Under B1, releases key by time.
- **Resource / Resource Store**: new definition-type element (like Species/Medium) +
  per-realization balance; event effects gain `spend`/`deposit`/`borrow` (+ return on
  repair for reliability interplay); allocation by priority using the existing
  link-priority pass as the ordering template. Exhaustion = a bound; under B1 it can
  emit an exact-time split point (nice-to-have, not required).
- Schema: new fields + effect modes; semantics § each; emit note (GoldSim Resource
  decode mapping) appended to the emit-issues doc.
**Tests:** M/D/1-ish queue sanity (arrivals > service ⇒ queue growth rate), capacity
blocking, resource exhaustion ordering by priority, spend/deposit round-trip.
**Effort:** 1.5–2 weeks.

## B4. Reporting-period aggregation (gap #1's easy corner; needs A3)

Post-pass over `hist_store`: accumulated / average / change / rate-of-change per
calendar month/year (or fixed-length periods pre-B6). Exposed through A3's
`results_spec` (`reporting_periods: monthly|annual|{seconds}` + which reduction).
Pairs with B6 for true calendar months. **Effort:** 2–3 days.
*(Tier-A landed: extend `engine/src/results_spec.rs` — `ResultsSpec` + `compute_analysis`
+ `ElementAnalysis`. Add a `reporting_periods` field to `ResultsSpec` and a period-reduction
struct alongside `CaptureSnapshot`. The A3 tests in `results_analysis_v2.rs` are the template.)*

## B5. Dimensional strict mode (gap #6)

**Now:** `units.rs` has `parse_unit → (factor, UnitDim)`; `validate()` warns only.
**Approach:** static dimension propagation over ASTs at **graph-build time** (not
runtime): infer each element's output dim from declared units; propagate through ops
(add/sub/compare require equal dims; mul/div compose; `sqrt` halves; transcendentals
require dimensionless; lookup uses declared axis units; TBL_Integral multiplies by
x-dim, TBL_Derivative divides — mind the 0.9.2 modes). Reserved globals carry their §1b
dims. Unknown/unparseable units → that subtree exempt (warn), so partially-emitted
models still load.
**Mode:** `RunConfig`/load flag `units: warn|strict`; default `warn`. Corpus gate:
run strict over all 211 models, triage every error into (real modeling bug — file to
emit doc) vs (checker gap — fix) before ever defaulting strict.
**Effort:** ~1 week. High leverage: the corpus OU/basis explosion (emit-issues §1) was
exactly a silent units bug.

## B6. True calendar / leap years (gap #6 remainder)

Replace the fixed 365-day `CalendarState` with a real proleptic-Gregorian civil-date
computation anchored at `calendar_start_seconds` (already in schema 0.9.1 series meta —
promote to `simulation_settings.calendar_start` for the model clock). Keep the current
behavior when no anchor is declared (document). Touches: `CalendarState` (post-B1 it is
elapsed-based already), `get_year/get_month/...` builtins, series `calendar_based`
playback. **Tests:** leap-year Feb 29, day-of-year across the boundary, month lengths.
**Effort:** 2–3 days.

## B7. Realization weights (+ importance sampling stretch) (gap #5 remainder)

- **Weights:** `RunConfig.realization_weights: Vec<f64>` (normalized at load) threaded
  through every stat reduction A3 exposes (weighted mean/percentile/CTE/bands) and
  `eval_harness::reduce`. Mostly mechanical once A3 exists. *(Tier-A landed: the reduction
  helpers to weight live in `engine/src/results_spec.rs` (`build_distribution`,
  `build_final_stats`, the percentile-band map) and `engine/src/engine.rs`
  (`mean`/`percentile`/`std`/`cumulative_prob` — add weighted variants) and
  `eval_harness::reduce`.)*
- **Stretch — importance-sampled stochastics:** per-distribution biasing (sample from a
  biased distribution, weight = f/g). Only pull this in if a target model needs it;
  Bayesian updating stays out (niche).
**Effort:** 2–3 days (weights); stretch unbounded — timebox it.

---

## Suggested order

B1 → (soak: corpus A/B) → B5 ∥ B3 → B4 → B6 → B7 → B2 last (frontend coordination).
Rationale: B1 first because it restructures the step loop everything else touches;
B2 deferred because it's the only item with an external (frontend) dependency.

## Definition of done (whole tier)

- Tier A conventions checklist (suite green, wasm rebuilt, schema/CHANGELOG/semantics
  updated, corpus 211/211 parse+build+run).
- B1 bit-identity gate archived as a permanent regression test (FixedGrid mode).
- Strict-units corpus triage written up (even if default stays `warn`).
- Emit-facing deltas (Resources/queue decode mapping, timestep_schedule, calendar
  anchor) appended to `EMIT_ISSUES_0.9.1_CORPUS.md`.

---

## Kickoff prompt (copy-paste to start a fresh session on this tier)

```
Read WORKPLAN_TIER_B.md at the repo root. Do NOT start implementing until you have
completed the context-recovery and drift-check steps below and updated this workplan
in place where reality has moved.

Context recovery:
1. Read memory: MEMORY.md + project_wasim_schema_arc.md (wasm rebuild rules, suite
   timing, schema-symlink layout, 0.9.2 arc).
2. Read GOLDSIM_ENGINE_GAP_ANALYSIS.md §1, §4, §10 (gaps #1, #4, #5, #6) for intent;
   note its known-stale claims (sensitivity + running extrema are implemented).
3. Read the B1 design section of this workplan as the canonical timebase design; the
   invariant ("grid owns statistics/state/reporting; sub-steps refine integration
   only") is load-bearing — if you believe you need to violate it, stop and surface
   that to the user instead of proceeding.
4. Code analysis to refresh before B1: engine_v2.rs step loop end-to-end (redraw pass,
   topo loop, link pass, event pass, stock integration incl. 0.9.2 port publication,
   cell pass, history recording, prev_outputs handoff), eval.rs EvalCtx/CalendarState,
   link_buf structure, trigger_fires. Build your own line map — do not trust line
   numbers in this document.

Drift check (mandatory):
5. Run: git log --oneline --since=2026-07-18 -- engine/src engine/tests schema
   and git status. Diff-read any commits touching engine_v2.rs, eval.rs, model*.rs,
   v2_parse.rs, or the schema dir. Specifically check: (a) did Tier A land, and did
   its A3 results layer / A5 node rules change the results-assembly or step-loop
   regions B1 restructures? (b) has the schema version moved past 0.9.2? (c) any new
   per-step stateful node rules added since this plan (they must be classified into
   the grid-only list in B1's invariant). Update the workplan's coupling list and
   item designs in place to reflect what you find, THEN begin.
6. Baseline: cargo test --test stock_traits_v2 --test triggers_v2 --test links_v2
   green before the first change; capture a corpus-sample results snapshot for the
   B1 bit-identity gate before touching the step loop.

Execute in the suggested order. B1's bit-identity regression gate must pass before
any other B item starts. Commit only when asked.
```
