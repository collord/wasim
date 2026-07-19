# Workplan — Tier A: GoldSim-parity quick wins

**Source:** `GOLDSIM_ENGINE_GAP_ANALYSIS.md` (gap numbers below refer to its §10 table),
filtered through the feasibility triage of 2026-07-19. Tier A = items that are days each,
mostly *finishing declared features* or *exposing data the engine already computes*.

**State pinned at authoring (2026-07-19):** HEAD `f47387c` **plus uncommitted 0.9.2
changes** (reserved globals, TBL_* lookup modes, stock port roles, filter-input
tolerance — see `schema/CHANGELOG.md` 0.9.2 entry and `engine/tests/globals_ports_v2.rs`).
If those are not present in your checkout, stop and find the commit that landed them.

**Standing conventions (apply to every item):**
- Engine work in `engine/src/`, tests in `engine/tests/` (one topical `*_v2.rs` file per
  feature, self-contained JSON models — see `globals_ports_v2.rs` for the pattern).
- Anything touching `model.rs` / `model_v2.rs` / AST: run
  `cargo check --target wasm32-unknown-unknown --features wasm`, then rebuild the deployed
  artifact: `cd engine && ./build-wasm.sh && cd ../frontend && npm run build`. The
  browser loads `engine/pkg/`; `frontend/dist/` is the bundled copy. Both go stale silently.
- Schema/semantics changes: `wasim/schema` is a **symlink** into
  `openvsim/wasim/schema` (single copy). Bump `$id` + `CHANGELOG.md` per its convention.
- Full suite: `cargo test` — budget ~60+ min (corpus integration ~19 min,
  `optimize_v2` corpus studies ~50 min in debug). Use targeted `--test` runs while
  iterating.
- Result-shape changes must keep `SimulationResults`/`ElementResults` backward
  compatible (frontend consumes them through the wasm bridge).

---

## A1. Latin Hypercube Sampling (gap #5, "declared but unimplemented")

**Now:** `SamplingMethod::Lhs` parses but `engine_v2` always samples independent MC.
**Where:** realization-init sampling in `engine_v2.rs` (per-realization `rv_samples`
population + the Iman-Conover pre-pass; see `iman_conover_v2.rs` tests) and
`sampling.rs`.
**Approach:** LHS is a *pre-pass across realizations*: for each once-per-realization
sample node, stratify [0,1) into `n_real` bins, draw one uniform per bin, shuffle the
bin order (seeded), map through the distribution ICDF. This is the same
matrix-of-samples shape Iman-Conover already builds for correlated groups — reuse that
plumbing; Iman-Conover rank reordering composes with LHS unchanged (that is the
standard pairing). Per-step/resampled/autocorrelated nodes stay MC (document in
semantics §: LHS applies to once-per-realization draws only — matches GoldSim).
**Watch out:** distributions sampled by rejection (truncation) need ICDF-based
truncated sampling under LHS (scale the stratified uniform into [F(lo), F(hi)]).
**Tests:** n_real=100 stratification property (exactly one sample per decile for a
uniform), determinism by seed, truncation respected, Iman-Conover correlation preserved
under LHS.
**Acceptance:** a model with `sampling_method: lhs` produces stratified marginals;
default MC behavior bit-identical.

## A2. Enforce optimization constraints (gap #8.1)

**Now:** `OptConstraint` parses (`model.rs`) but `optimize_v2.rs` does box projection
only.
**Where:** `optimize_v2::evaluate` — it already coerces failed candidates to `+∞`.
**Approach:** after `evaluate_point` runs the model, evaluate each constraint element
(same run's results — extend `eval_harness::evaluate_point` to optionally return extra
element values so one run serves objective + constraints), compare against its
bound/direction, and return `+∞` on violation (Box's complex handles implicit
constraints exactly this way). Consider a soft-penalty fallback if all-∞ complexes
stall; only if observed.
**Tests:** constrained quadratic where the unconstrained optimum is infeasible → solver
lands on the constraint boundary; infeasible-everywhere → clean error, not a hang.
**Acceptance:** `dynamicoptimization`/study corpus models still converge (they have no
constraints); fixture with constraint converges to the constrained optimum.

## A3. Results/analysis layer (gap #3 — the High-severity one that is mostly free)

**Now:** `hist_store` already holds per-step values across realizations and
`final_store` per-realization finals; they are reduced to a fixed
`mean+p05/p25/p50/p75/p95` (`TimeHistoryStats`) and `final_values`.
**Where:** `engine_v2.rs` results assembly (~lines 1140–1250), `engine.rs`
(`ElementResults`/`TimeHistoryStats`), stat helpers `mean/percentile/std/
cumulative_prob` already in `engine.rs`.
**Approach:** runtime-configured (follow the `sensitivity_v2`/`RunConfig` precedent —
NOT schema): extend `RunConfig` with an optional `results_spec`:
- custom percentile list for time-history bands;
- distribution objects for selected elements: sorted samples → PDF (binned), CDF, CCDF/
  exceedance;
- **capture times**: snapshot distribution at requested elapsed times (index into
  `hist_store`);
- final-value stats: confidence bounds on the mean (t-interval), skewness/kurtosis,
  conditional tail expectation (mean beyond a percentile).
All additive fields on `ElementResults` (`Option`-typed, `skip_serializing_if`) so the
frontend and existing consumers are untouched until they opt in.
**Wasm:** expose the config through the wasm bridge (`wasm.rs`) — check how `RunConfig`
crosses today.
**Tests:** percentile-list correctness against hand-computed samples; CCDF monotone;
capture-time snapshot equals the same data sliced from history; CTE vs hand
computation.
**Acceptance:** default output byte-identical; opting in yields the new objects.

## A4. Distribution roster (gap #9)

**Where:** `model.rs` `DistributionKind` + `sampling.rs::sample` + `v2_parse.rs`
lowering + schema enum.
**Add:** Log-Uniform, Log-Triangular (+ 10–90 parameterizations of Triangular /
Log-Triangular), Log-Cumulative (transform wrappers: sample the base in log space,
`exp`); Binomial, Negative Binomial, Poisson (`rand_distr` has all three); Extreme
Probability min/max of N (draw u, use `u^(1/N)` / `1-(1-u)^(1/N)` through the base
ICDF); Beta by (successes, failures) → `Beta(s+1, f+1)` re-parameterization at parse.
**External distribution:** stop sampling 0.0 silently — return a load-time or
first-sample **error** naming the element (or, if an inline empirical fallback table is
present in the doc, sample that). Check corpus for `External` occurrences first; if any
model depends on the 0.0 degrade to load, gate the hard error behind validate-warning
for one round and coordinate with emit.
**Tests:** moment/shape checks per new distribution (mean/var within MC tolerance at
n=50k), truncation, LHS interaction (A1), schema round-trip.
**Note for emit (append to `EMIT_ISSUES_0.9.1_CORPUS.md` when done):** new enum values
available; 10-90 variants map GoldSim's alternate parameterizations directly.

## A5. Small discrete/stateful node rules (gaps #4, #11 slices)

All four follow the `hysteresis`/`filter` pattern: a `NodeRule` variant + a per-
realization state map in `engine_v2` + a `v2_parse` arm + schema fields + semantics §2
entry.
- **Status**: latch with independent set/reset `TriggerSpec`s (differs from hysteresis:
  triggers, not thresholds). Outputs 1/0.
- **Milestone**: records first-fire elapsed time of a trigger; outputs that time (or a
  sentinel/NaN policy — decide and document; GoldSim exposes achieved-probability via
  stats, which falls out of A3's final-value distributions).
- **Interrupt**: an event effect (`EffectSpec` mode) that ends the *realization* at end
  of the current step; remaining steps report last-held values (document choice in
  semantics). Check interactions: `final_store` capture, dynamic optimization
  (skip solve after interrupt), submodel runs.
- **PID / deadband controller**: node rule with setpoint/gains fields, integral +
  previous-error per-realization state, Euler discretization, optional output clamps.
- **Builtins `occurs(event_id)` / `changed(ref)`**: `occurs` needs the event-fire set
  for the current step exposed in `EvalCtx` (events currently resolve effects in their
  own pass — expose a read-only fired-set); `changed` compares `outputs` vs
  `prev_outputs`.
**Tests:** one topical test file per rule; controller closed-loop on a 1-stock plant
converges to setpoint.

## A6. Lookup-table leftovers (gap #10)

**Where:** `eval.rs` (`LookupMode` enum just added — extend it), `model.rs`
`LookupTable`/`InterpolationMethod`, `v2_parse.rs`, schema.
- **`TBL_Derivative`** mode: slope of the interpolated segment at x (step-interp →
  0.0); register the reserved name alongside the three existing `TBL_*` names
  (semantics §1b table).
- **Log-result interpolation**: flag on the table → interpolate `ln(y)` linearly,
  return `exp`.
- **Monotone cubic (Fritsch-Carlson) interpolation**: replaces the silent `cubic`→
  linear downgrade at the parse boundary. Keep `spline` mapping to it too (document).
- **3-D tables**: third axis + trilinear interpolation. Schema shape: nested
  `columns` per z-slice — design against what the emitter can actually produce; check
  corpus for any 3-D source tables *first* (if none, ship engine+schema and note it as
  available in the emit doc rather than inventing untested emit pressure).
**Tests:** derivative of a known table; log-interp against hand values; monotone-cubic
no-overshoot property; 3-D corner/center probes.

---

## Suggested order & effort

A2 (½ day) → A1 (1–2 d) → A4 (1–2 d) → A6 (1–2 d) → A5 (2–3 d) → A3 (2–4 d).
Independent items; parallelizable. A3 last only because its result-shape decisions
benefit from A4/A5 landing (Milestone/Interrupt stats ride on it).

## Definition of done (whole tier)

- `cargo test` fully green; new topical test files per item.
- wasm target checks; `engine/pkg` + `frontend/dist` rebuilt.
- Schema `$id`/CHANGELOG bumped once for the tier (batch the schema-touching items);
  semantics doc updated (§1b reserved names, §2 node rules, results doc note).
- Emit-facing deltas appended to `EMIT_ISSUES_0.9.1_CORPUS.md` (new distribution enum
  values, `TBL_Derivative`, new node rules available for decode mapping).
- Corpus check: 211 models still parse + build + run (`--test integration`).

---

## Kickoff prompt (copy-paste to start a fresh session on this tier)

```
Read WORKPLAN_TIER_A.md at the repo root and execute it item by item.

Context recovery, in order:
1. Read memory: MEMORY.md and project_wasim_schema_arc.md (auto-memory dir) — engine
   arc, wasm-rebuild rules, test-suite timing traps.
2. Read GOLDSIM_ENGINE_GAP_ANALYSIS.md §6, §8, §10 for the gap definitions behind
   these items (note: its §8.2 sensitivity claim and §2.2 extrema claim are stale —
   both are implemented).
3. Verify the 0.9.2 baseline is present: grep engine/src/engine_v2.rs for run_globals,
   engine/src/eval.rs for LookupMode, and confirm schema/CHANGELOG.md has a 0.9.2
   entry. If missing, locate the commit that landed them (search git log for
   "0.9.2" / "reserved globals") before proceeding — this plan builds on it.
4. Skim the code sites named per item BEFORE designing: engine_v2.rs realization-init
   sampling + results assembly, eval_harness.rs, optimize_v2.rs::evaluate, sampling.rs,
   eval.rs LookupMode + interp1d, model.rs DistributionKind/OptConstraint.
5. Check git log --oneline -15 and git status for changes since f47387c + the 0.9.2
   round; if any touch the files above, reconcile the plan's line references and
   assumptions before starting (do not trust stale line numbers; trust symbol names).
6. Baseline: run cargo test --test globals_ports_v2 --test optimize_v2 (fast subset)
   to confirm green before your first change.

Work one item at a time; write the item's tests first or alongside; run the full
suite + wasm rebuild only once at the end (it is slow). Follow the tier's
definition-of-done checklist. Commit only when asked.
```
