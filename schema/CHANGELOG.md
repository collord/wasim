# wasim schema changelog

Breaking changes to `model.schema.json` and `quantity_or_formula` / element
shapes. One line per change. The `$id` is bumped on every breaking change so
engines can detect stale schemas at load time.

## 0.9.9 — 2026-07-29

Three additive engine feature capabilities (RK4 integration deferred). `$id` bumped. Drift guard
unaffected (no `ast_node` op or `distribution` family added). All prior corpus still validates.

- **Queue `leak_rate`** (Feature C): optional first-order decay over a `queue` node's transit — a
  scheduled parcel of size `a` with transit `T` exits as `a·exp(-leak_rate·T)`, modeling loss/reneging
  in transit. Mirrors link `decay_rate`.
- **Queue `discipline`** documented (`conveyor` | `fixed_at_entry`): previously parsed by the engine
  but undeclared in the schema (pre-existing drift; the branch is `additionalProperties:true`).
- **Stock `initial_value`** (Feature B): `quantity` → `quantity_or_formula` — a stock may now seed its
  t=0 level from an expression over other elements, including other stocks' evaluated initials
  (resolved by an init-only topological order; a circular initial dependency falls back to the
  scalar/0 seed + warning). Mirrors the 0.9.8 `process_spec.mean` widening.

## 0.9.8 — 2026-07-29

Engine↔schema drift catch-up. The engine's JSON parser had gained constructs the schema never
declared (engine work on cloud containers where the schema symlink was absent). Additive/descriptive
only — the schema now documents what the engine already accepts; no calculus or role/lens changes.
`$id` bumped. A `schema_drift_guard` engine test now fails if `AstNode` / `DistributionKind` gain a
variant with no schema entry, so this cannot silently recur.

- **9 new `ast_node` ops** (already implemented in the engine): `submodel_stat2`, `nested_stat`,
  `subscript`, `run_stat`, `run_stat2`, `run_regress`, `run_split_beta`, `lsm`, `lsm_dual`.
- **2 new `distribution` families:** `discrete_uniform` (integer min/max), `bernoulli` (`prob`).
- **`process_spec.mean` / `.stddev`:** `quantity` → `quantity_or_formula` (the engine types these
  `QuantityOrFormula`; the three sibling reversion fields already allowed formulas).
- **`delay_time`** added to the node value_rule branch (required by `queue` nodes; the branch is
  `additionalProperties:true`, so this is documentation completeness).
- **`submodel_stat.statistic`** enum widened from 4 to the full 9 (`…, exceedance, cte, sum, min,
  max`) to match the engine's `SubmodelStatKind` — a pre-existing partial drift.

(Note: 0.9.7 — the stock secondary-output `role` / `output_kind` split — predated this changelog and
has no entry of its own.)

## 0.9.6 — 2026-07-19

Calendar `time_ref` property additions (BRIEF_schema_mods_for_builtins.md §A1 — the parts not
already covered by reserved globals / the real calendar). Additive/optional (all 213 corpus docs
still validate); `$id` bumped. Requires a `calendar_start` anchor (B6) to be meaningful.

- **New `time_ref.property` values (semantics §14):**
  - `hour` / `minute` / `second` — clock time-of-day components (calendar-aware; 0 without an
    anchor). GoldSim Hour/Minute/Second.
  - `start` — the calendar anchor (`calendar_start`, seconds since epoch); 0 without one.
    GoldSim StartTime.
  - `elapsed_months` / `elapsed_years` — calendar-field boundaries crossed since the start
    (GoldSim EMonth/EYear); NOT derivable from `elapsed` because month/year lengths vary.

  (GoldSim `SimDuration` and `Realization` are already handled as reserved globals — §1b — and
  `TBL_*` table modes as `lookup_call` reserved names — §1b; no new construct needed for those.)

## 0.9.5 — 2026-07-19

Tier-B B6 (true calendar / leap years, WORKPLAN_TIER_B.md). Additive/optional (all 213 corpus
docs still validate); `$id` bumped. (B4 reporting periods is a runtime `results_spec` extension —
no schema change.)

- **`simulation_settings.calendar_start` (semantics §14).** Optional model-clock anchor as seconds
  since the Unix epoch (1970-01-01). When present, the `time_ref` calendar properties
  (`year`/`month`/`day_of_month`/`day_of_year`/`days_in_month`) use a **real proleptic-Gregorian
  calendar with leap years** anchored here; `year` returns the actual calendar year. Absent = the
  fixed 365-day calendar (behavior unchanged).

## 0.9.4 — 2026-07-19

Tier-B B3 (discrete-event depth: queues + resources, WORKPLAN_TIER_B.md). Additive/optional
(all 213 corpus docs still validate); `$id` bumped per the breaking-change rule. (B1 timebase and
B5 strict-units are runtime `RunConfig` flags — no schema change.)

- **New node `value_rule: "queue"` (semantics §B3).** An event/discrete-change delay: entities/
  amount arriving via `input` wait `delay_time` (quantity_or_formula) then exit; optional
  `capacity` caps the number waiting (excess blocked), `discipline` is `conveyor` (default) or
  `fixed_at_entry`. Primary output = throughput; a secondary output with role `num_in_queue`
  reports the queue level (resolved via an output-qualified `ref`, like stock ports).
- **New primitive `resource` (semantics §B3).** A finite-supply Resource Store: `initial_value`
  balance + optional `capacity`. Its output is the current balance. Node element props are open,
  so `initial_value`/`capacity` need no new shape.
- **New `effect_spec.mode` values `spend` / `deposit` / `borrow` (semantics §B3).** Event effects
  that adjust a target Resource's balance: spend withdraws (limited to available; partial when
  short), deposit adds (clamped to capacity), borrow spends with a tracked outstanding balance a
  reverse (repair/return) transition restores. `num_in_queue` joins the stock output roles.

## 0.9.3 — 2026-07-19

Tier-A GoldSim-parity round (WORKPLAN_TIER_A.md, engine-side). Additive/optional
(all 213 corpus docs still validate); `$id` bumped per the breaking-change rule.
Most Tier-A features are runtime-configured (not schema): LHS uses the existing
`simulation_settings.sampling_method: lhs`; optimization-constraint enforcement uses
the existing `optimization.constraints`; the results/analysis layer is a `RunConfig`
option (§A3). Only the schema shapes below changed.

- **Distribution roster additions (§6, changes what validates).** New `distribution.family`
  values: `log_uniform`, `log_triangular`, `log_cumulative`, `triangular1090`,
  `log_triangular1090` (GoldSim 10-90 alt-parameterizations), `binomial`,
  `negative_binomial`, `poisson`, `extreme_probability` (order-statistic of a nested
  `base` distribution), `beta_success_failure`. Each has its own required-parameters
  branch. `external.parameters.fallback` (optional inline `{samples, weights}` empirical
  table) added: the engine samples it instead of degrading to 0.0.
- **Node `value_rule` additions (semantics §2, changes what validates).** `status`
  (latching set/reset triggers → 1/0), `milestone` (records first-fire elapsed time),
  `pid` (Euler-discretized PID / deadband controller). Node element properties are open
  (`additionalProperties: true`), so their fields (`set`/`reset`/`trigger`/`setpoint`/
  `kp`/`ki`/`kd`/`output_min`/`output_max`/`deadband`) need no shape change.
- **`effect_spec.mode` addition (semantics §2, changes results).** `interrupt` ends the
  realization at the end of the current step; remaining steps hold last-computed values.
- **Reserved lookup mode `TBL_Derivative` (semantics §1b, contract not syntax).** A fourth
  `lookup_call` second-argument reserved name (joins `TBL_Integral`/`TBL_Inverse`/
  `TBL_Inv_Integral`): returns dy/dx of the interpolated segment at x.
- **Lookup `interpolation` semantics clarified (no shape change).** `spline` now maps to
  monotone-cubic (Fritsch-Carlson, no overshoot) instead of a silent linear downgrade;
  `log_linear` now interpolates ln(y) and returns exp. N-D tables (2-D/3-D) use the
  existing `table.z = [axis2_bp, (axis3_bp)?, flat_values]` packing with multilinear
  interpolation. Builtins `occurs(event_id)` / `changed(ref)` are available (open AST).

## 0.9.2 — 2026-07-18

Detention-pond parity round (EMIT_ISSUES_0.9.1_CORPUS.md, engine-side response). Additive/
optional (existing corpus docs still validate); `$id` bumped per the breaking-change rule.

- **Reserved global identifiers (semantics §1b, changes results).** The engine resolves
  `gee` (9.80665 m/s²), `TimestepLength` (Δt, s), `SimDuration` (s), and `Realization`
  (1-based) before the dangling-ref → 0.0 fallback; model elements with the same id shadow
  them. No schema shape change — the names are contract, not syntax. Previously these
  GoldSim run properties evaluated to 0.0 (e.g. `Orifice_Flow ≡ 0` in detention_pond).
- **`lookup_call` table modes (semantics §1b, changes results).** `TBL_Integral` /
  `TBL_Inverse` / `TBL_Inv_Integral` as the second argument select cumulative-integral,
  inverse-table, and inverse-of-integral (stage-storage) evaluation. Previously the mode
  ref evaluated as a dangling column index.
- **`output_spec.role` + output-qualified refs (semantics §1c, changes results).** Optional
  `role` (`addition_rate` | `withdrawal_rate` | `overflow_rate` | `net_change`) on a
  stock's secondary outputs; the engine publishes the step's applied rate under
  `"<id>#k"` and `ref.output` resolves it (fallback: primary value). Gives extrema/filters
  a real discharge signal instead of the stock's level.

## 0.9.1 — 2026-07-17

Faithful-representation round from `WASIM_SCHEMA_ADDITIONS.md` — the two correctness-critical
gaps plus fidelity additions. Additive/optional (existing corpus docs still validate); `$id`
bumped per the breaking-change rule.

- **Mean-reverting processes (Gap 5, §16, changes results).** `process_spec` gains optional
  `reversion_rate`, `reference_value`, `initial_value`. A non-zero `reversion_rate` makes the
  process Ornstein-Uhlenbeck (reverts toward the reference), producing a level series; absent =
  unchanged GBM. Engine executes it; previously a mean-reverting SHistoryGenerator ran as plain
  GBM (wrong numbers). Emit nests the (already-decoded) reversion params into `process`.
- **Expression-valued convolution response (Gap 4, §17, changes results).** A `convolution`
  `response` may now be an expression over the lag variable (`extern_call fn:"lag"`), sampled
  onto the lag grid at run time, instead of only a baked `{times, values}` table. Keeps a
  referenced calibration parameter live (e.g. a GR4J unit-hydrograph shape parameter that an
  optimization varies) rather than freezing it at emit time. Engine samples it; emit emits it.
- **Function vocabulary (Gap 1a, §1a).** `call.fn` enum gains `erf`, `erfc`, date extraction
  (`get_year`/`get_month`/`get_day`/`get_hour`/`get_minute`/`get_second`), finance
  (`pv_factor`, `annuity_factor`), and table introspection (`table_min`/`table_max`/
  `column_count`) — all implemented in the engine (previously opaque `extern_call` → NaN). Also
  syncs `gamma` (shipped 0.8.5) into the emitter's promoted-function set.
- **Series calendar/ensemble metadata (Gap 7, §18).** `series` gains optional `calendar_based`,
  `calendar_start_seconds`, `n_histories`, `extra_value_rows` (formalizes fields emit already
  carried on additionalProperties). Fidelity/round-trip.
- **Financial payoffs (Gap 6, §19).** `event` gains an optional `payoff` (`payoff_spec`) for
  option/insurance threshold-conditional payouts. Provenance-complete; not executed yet.
- **Linked-Excel elements (Gap 3, §20).** New `spreadsheet` value_rule with `cells` /
  `external_file`, giving SSpreadSheet a formal home (was the one unmapped class). The engine
  loads/runs it as a fixed-0 placeholder (workbook is external). Emit population of the
  cell-range binding is deferred to the decoder/emit side.
- `$id` bumped to `…/model/0.9.1`.

## 0.9.0 — 2026-07-16

Dynamic (per-timestep) optimization. A submodel may now carry its own
`optimization` block; when present, the engine re-solves that inner optimization
**at each outer timestep** against the objective evaluated at that step, so the
optimized variables become per-timestep series (a time history) rather than a
single once-only study result. This reproduces GoldSim's "Dynamic Optimization"
(the inner-optimization-inside-a-submodel case), where the optimum tracks a
time-varying driver — e.g. `dynamicoptimization.gsm`'s `Parameter = √Driver(t)`.

- **New optional `optimization` property on `container_def`** (reusing the existing
  `$defs/optimization` shape unchanged — objective/variables/constraints). Valid only
  for `kind: "submodel"`. Absent = the submodel is not optimized.
- **The top-level `optimization` block is unchanged** and remains a **once-only study**
  (§13). The two loci are distinct: top-level = static study; submodel-scoped =
  dynamic per-timestep. This *extends*, and does not replace, the 0.8.1 decision that
  optimization is a block referencing elements by id (not a node trait) — it adds a
  second, submodel-scoped locus of that same block.
- **Why the locus is the signal (not a new flag):** GoldSim stores `COptimization` on
  each model's `MasterClockInfo` (per-clock), and a submodel has its own clock — so
  "optimization enabled on a submodel" is exactly a submodel-clock optimization. The
  emitter scopes the decoded per-clock `COptimization` to its owning model container
  (reusing the identity-scoping already used for per-submodel `simulation_settings`).
- **Semantics §13a** added describing dynamic optimization; §13's "not a per-timestep
  behavior" statement now cross-references §13a as the submodel-scoped exception.
- **Breaking** (hence the 0.9.0 rollover, not a 0.8.x minor): the top-level-only
  optimization contract in §13 is widened, and consumers that pin `$id` must move. All
  existing corpus docs still validate — the new property is optional and additive at
  the container level; only `dynamicoptimization.json` is re-emitted to move its
  (already-present) optimization spec from the top level under its submodel.
- `$id` bumped to `…/model/0.9.0`.

## 0.8.5 — 2026-07-15

Distribution parameters may now be reference/formula-valued (not just scalar
literals). Widening, fully back-compatible — every existing corpus doc still
validates (162/162), a scalar literal validates unchanged; only new formula-valued
params become expressible.

- **Continuous-family distribution params widened from `quantity` to
  `quantity_or_formula`** — matching `normal`/`lognormal` (already `quantity_or_formula`).
  Applies to all 12 scalar-only continuous families (33 params): `uniform` (min/max),
  `triangular` (min/mode/max), `trapezoidal` (min/lower/upper/max), `gamma`
  (shape/scale), `beta` (alpha/beta/min/max), `weibull` (shape/scale), `pert`
  (min/mode/max), `pareto` (scale/shape/location), `extreme_value` (location/scale),
  `pearson_iii` (mean/stddev/skewness), `pearson_v` (shape/scale), `student_t`
  (degrees_of_freedom/location/scale).
- **Why:** GoldSim frequently parameterizes a distribution from another element (a
  link/reference) rather than a literal — including the marquee probabilistic-
  optimization case where a distribution parameter *is* the optimization variable
  (`probabilisticoptimization`'s Weibull shape ← `Slope`). The scalar-only type forced
  emit to drop such references and fall back to `family: external` (an opaque
  placeholder the engine samples as 0). Resolves the root cause diagnosed in
  `SUBMODEL_EXECUTION_FINDINGS.md`; proposal in `DISTRIBUTION_PARAM_FORMULA.md`.
- **Blanket, not minimal:** widened all continuous families in one round (the corpus
  needs only weibull/gamma/uniform today) to avoid a repeat schema+engine+regeneration
  round each time a future model links a different family's param — the widening is
  uniform, mechanical, and back-compatible.
- Engine consumes it: `DistributionKind` params for these families are now
  `QuantityOrFormula`, resolved to scalars before sampling via the existing
  `resolve_distribution` path (the same mechanism normal/lognormal already used).
- **New `gamma` builtin** (added to the `call` `fn` enum): the gamma function Γ(x),
  needed for the derived Weibull/gamma scale expressions this widening enables
  (e.g. `scale = mean / Γ(1 + 1/shape)`). Emit should use `call fn:"gamma"` for these,
  **not** `extern_call gamma` (which evaluates to 0.0 and would silently break the
  division). Implemented in the engine via the Lanczos approximation. Semantics §15.
- `$id` bumped to `…/model/0.8.5`.

## 0.8.4 — 2026-07-14

Explicit submodel interface-input binding. Breaking shape change to
`container.interface.inputs` (a clean cutover — emit regenerates the corpus each
round). Companion `wasim-engine-semantics.md` §12 updated; footer 0.8.4.

- **`container.interface.inputs` is now an array of `{input, from}` objects**
  (was a bare `string[]`). `input` = the interior consumer element the value flows
  into; `from` = the parent driver element that supplies it (`null` for an
  engine/dashboard-supplied input with no model driver, e.g. a realization count).
  This replaces the engine's leaf-name inference of the parent→interior binding
  with an explicit, decode-recoverable mapping — resolving Gap 2 of
  `SUBMODEL_EXECUTION_FINDINGS.md`. `outputs` is unchanged (`string[]` of interior
  element ids). Proposal + decode details in
  `SUBMODEL_INTERFACE_INPUT_BINDING.md`.
- Decisions on the two open points from the proposal: unresolvable/built-in
  drivers keep the port visible with `from: null` (not omitted), so the engine
  knows an input exists it must supply; and this is a **clean cutover** (object-only,
  not a `string[] | object[]` union) since emit regenerates the whole corpus.
- `$id` bumped to `…/model/0.8.4`.

## 0.8.3 — 2026-07-13

Restores the array-comprehension AST nodes dropped in the 0.8.0 primitives rewrite,
plus a first-class dimension concept. Additive (new `ast_node` branches + new
optional top-level `dimensions`); existing models still validate — no new corpus
regressions. Companion `wasim-engine-semantics.md` bumped to 0.8.3 with a new §15.

- **Array-comprehension AST nodes restored** (branches of `ast_node`, mirroring
  0.7.0): `vector_map` (`{over, body}` — comprehension over a dimension),
  `index_ref` (`{axis: row|col}` — the loop index), `index` (`{array, indices[]}`
  — dimension-indexed subscript), and `extern_call` (`{fn, args[]}` — an
  unimplemented source function preserved for round-tripping). The 0.8.0 rewrite
  dropped these, forcing array-valued formulas — including 6 `pdf_*`-in-
  comprehension objectives — to stub to `literal 0.0`.
- **New first-class dimension concept:** top-level `dimensions[]` of
  `{id, name, size, labels?}` (ordinal sets — named ordered index sets like
  Months = 12), which `vector_map.over` iterates and `output_spec.dimensions`
  reference. Restores `output_spec.dimensions` (a `string[]` of dimension ids,
  also dropped in 0.8.0). Member numeric values stay in the elements that carry
  them, indexed by position — not duplicated on the dimension.
- Engine decodes all four nodes and preserves their graph dependencies, but the
  dimension-aware array executor is not yet implemented — placeholder evaluation
  (`vector_map`/`index_ref`/`extern_call` → 0.0; `index` → scalar view), same
  degrade-to-zero policy as a dangling ref. Semantics §15.
- `$id` bumped to `…/model/0.8.3`.

## 0.8.2 — 2026-07-12

Additive AST node for probabilistic/submodel-statistic expressions. Back-compatible
(new `ast_node` branch; existing ASTs still validate — verified: no new corpus
regressions). Companion `wasim-engine-semantics.md` bumped to 0.8.2 with a new
§2.13.

- **New `submodel_stat` AST node** (branch of `ast_node`). Encodes a Monte-Carlo
  statistic of a submodel output — the `pdf_*` operations (`pdf_mean`,
  `pdf_value`/percentile, `pdf_sd`, `pdf_cumprob`) that previously survived only as
  `expression.display` text while the AST was stubbed to `literal 0.0`. Fields:
  `submodel_id` + `output` (full slash-path ids, §1 identity rule), `statistic`
  (`mean`|`percentile`|`sd`|`cumulative_prob`), and an optional `arg` sub-node
  (percentile p in [0,100]; or a unit-bearing `cumulative_prob` threshold).
  Evaluation contract in semantics §2.13; emit-side lowering (dotted
  `Submodel.Output` → the two ids, display-fn → `statistic`, ×100 percentile
  normalization) specified in `SUBMODEL_STAT_ENCODING.md` at the repo root.
- **Single-evaluation (`duration: 0`) models clarified (semantics, no shape change).**
  Documented that a `simulation_settings.duration` of 0 is valid and means a
  single-evaluation driver/instant model: `n_steps = max(1, round(duration/timestep))`,
  so the engine evaluates once at t=start and stops (stocks return `initial_value`).
  These are GoldSim optimization/statistics drivers, single-period calcs, and
  sequence/parameter generators whose real timeline is a nested submodel run. The
  source `.gsm` genuinely stores 0 for ~18 corpus models; the engine now accepts
  `duration >= 0` (rejecting only negatives) rather than requiring `> 0`. Semantics
  §9; no `$id` bump (description-only).
- `$id` bumped to `…/model/0.8.2`.

## 0.8.1 — 2026-07-11

Additive restore + new study block, on top of the 0.8.0 primitives model. All
new fields are optional, so 0.8.0 output still validates (verified: no new
regressions across the 162-file example corpus; the 5 optimization models still
validate). Companion `wasim-engine-semantics.md` bumped to 0.8.1 with new §12
(SubModels) and §13 (Optimization) sections.

- **SubModels restored on `container_def`.** Re-adds the 0.7.0 submodel fields
  that the 0.8.0 primitives rewrite dropped: `kind` (`container`|`group`|
  `submodel`, default `container`), an optional nested `simulation_settings`
  (`null` = inherit the parent clock/MC settings), and an `interface` (named
  `inputs`/`outputs` boundary element ids). A `kind: "submodel"` container is a
  nested simulation with runtime behavior (§12); other kinds stay organizational.
  Adapted to the 0.8.0 container shape (kept the `elements[]` array; nested
  settings use the `oneOf: [{$ref}, {null}]` idiom).
- **New top-level `optimization` block** (optional; new `$defs/optimization`).
  Represents a study over the model: an `objective` (`element_id` + `direction`
  maximize/minimize + optional MC `statistic`), `variables[]` (each an
  `element_id` with SI `lower`/`upper`/`initial` quantity bounds + optional
  `integer`), optional `constraints[]` (`quantity_or_formula` condition + label),
  and optional `sampling.realizations_per_trial` for probabilistic studies. Reuses
  `quantity` (bounds) and `quantity_or_formula` (constraints). The problem
  definition only — the solver (e.g. Box's complex method) is an engine concern,
  not schema (§13). Closes the gap where optimization variables emitted as
  ordinary single-valued nodes with their ranges/objective/direction dropped.
- **Element identity made explicit (contract, not a shape change).** Documented
  that every element `id` MUST be globally unique across `elements` — not merely
  within a container — and that all string references (`inputs[]`, AST `ref`/
  `lookup_call`, `container.elements`/`interface`, `optimization` element ids)
  resolve to an `id` by exact string equality, with no relative-name or
  scope-aware resolution. Source names are container-scoped and collide, so
  emitters qualify ids (e.g. a path `Model/CoverLayer/nCells`); `name` stays the
  (possibly-colliding) display label. A dangling reference evaluates to `0.0` and
  should warn; engines may reject duplicate ids at load. JSON Schema cannot
  enforce this uniqueness — it is stated on `element_base.id`, the `elements`
  array, and semantics §1. (Surfaced while restoring submodel interiors: exposing
  container interiors revealed the same bare name recurring across containers.)
- `$id` bumped to `…/model/0.8.1`.

**Design decisions** (answering `WASIM_OPTIMIZATION_PROPOSAL.md` §5, for the emit side):

- **Q1 — top-level block, not a node trait.** A variable's range only means
  something in the context of *this* study, so the study config lives in one
  optional top-level `optimization` block referencing existing elements by id,
  rather than an `optimization_variable` trait scattered across nodes.
- **Q2 — SubModels restored; objective statistic hangs off the submodel.** The
  probabilistic objective is a Monte-Carlo statistic of a submodel output (e.g.
  `pdf_percentile(Sub.total_cost, 95)`); the reduction happens at the submodel
  boundary, whose own nested `simulation_settings.n_realizations` governs the
  inner run. This re-closes the 0.7.0→0.8.0 submodel regression rather than
  encoding nested-run semantics inside the optimization block. `sampling.
  realizations_per_trial` remains as the fallback for a probabilistic study whose
  objective is not a submodel output.
- **Q3 — constraints reuse `quantity_or_formula`.** Each constraint is a
  `{condition: quantity_or_formula, label?}` — no dedicated shape.
- **Q4 — bounds are `quantity` (SI value + `display_unit`).** `lower`/`upper`/
  `initial` reuse the standard `quantity` def, normalized to SI like every other
  value; the original unit rides in `display_unit`.
- **Q5 — landed in 0.8.1.** Additive/optional, so a minor bump on the 0.8.x line
  (not 0.9.0); `$id` bumped per the breaking-change rule.
- **Q6 — semantics documented.** New §12 (SubModels) and §13 (Optimization) in
  `wasim-engine-semantics.md` specify the contract for a future engine round; the
  block is not provenance-only — it is meant to be executed once emit populates it.

Two TBDs the proposal flagged (§3, §6) are **emit-side** and do not affect this
shape: the exact objective-reference decode slot and the maximize/minimize flag
semantics. Emit resolves those against the corpus optimization models when it
populates the block.

## 0.8.0 — primitives rewrite (retroactive entry)

The schema was rewritten from the fixed-type element taxonomy (through 0.7.0)
into six composable **primitives** — `node`, `stock`, `link`, `event`, `gate`,
`cell` — plus two definition types (`species`, `medium`), with behavior
determined by field presence (**traits**) rather than a declared type. The root
and `$defs` moved to `wasim-schema-v2.json` ($id `…/model/0.8.0`), companioned by
`wasim-engine-semantics.md`. This entry is recorded retroactively; the rewrite
shipped without a changelog entry.

- **Breaking:** container/element/settings shapes changed; `container_def` became
  `additionalProperties: false` and, in the process, **dropped the 0.7.0 submodel
  fields** (`kind`, nested `simulation_settings`, `interface`) — restored in 0.8.1
  above. Optimization was likewise not carried over (it had been excluded as run
  infrastructure in 0.7.0); added as a first-class block in 0.8.1.
- `$id` set to `…/model/0.8.0`.

## 0.7.0 — 2026-06-26

Broad additive expansion so the schema can encapsulate every non-graphics
element the structural decoder recovers, named in vendor-neutral
stochastic-simulation vocabulary (no source-format/tool jargon). Existing 0.6.0
element types and field shapes are unchanged — prior output still validates
(verified: 162/162 example corpus, zero regressions). Mostly additive; the one
soft-breaking change is the `goldsim_object_id` → `source_object_id` rename
(the legacy key still validates via `additionalProperties: true`).

- **New element families (29 types).** Stocks/dynamics: `pool` (discrete inflows +
  prioritized withdrawals), `reservoir` (capacity + overflow), `material_delay`,
  `allocator`, `splitter`. Functions: `selector`, `aggregator`, `previous_value`,
  `convolution`, `controller`. Discrete events: `event_generator`, `random_choice`,
  `consequence`, `interrupt`, `status`. Inputs: `spreadsheet`. Reliability:
  `failure_mode`, `logic_tree`, `reliability_function`. Financial: `fund`,
  `investment`, `cash_flow`, `option`, `insurance`. Mass transport: `transport_cell`,
  `transport_pathway`, `transport_source`, `species`, `medium`. (`accumulator`
  stays the pure integrator; `pool`/`reservoir` are now distinct.)
- **Distribution families added (12):** `cumulative` (CDF table), `pert`, `pareto`,
  `extreme_value`, `extreme_probability`, `student_t`, `poisson`, `binomial`,
  `negative_binomial`, `beta_binomial`, `sampled`, `external`. The existing 14
  families are unchanged.
- **New shared `$defs`:** `trigger_spec` (generic event/timing condition),
  `flux_spec` (advective/diffusive transfer), `gate_node` (recursive
  and/or/not/n_vote/condition/reference/input for logic trees), `failure_process`,
  `medium_ref`, `species_ref`, and `simulation_settings` (factored out of the
  top-level so submodel containers can reuse it).
- **`simulation_settings`** gains optional `reporting_periods` (array of duration
  quantities) — the only sim-settings field the example corpus used that no prior
  schema defined.
- **`container_def`** gains `kind` (`container`|`group`|`submodel`),
  an optional nested `simulation_settings`, and an `interface` (named
  inputs/outputs) — submodels are modeled as containers, not a separate element.
- **`random_variable`** gains a `resampling` (`trigger_spec`) field; the legacy
  `trigger` provenance object is retained.
- **Genericization:** `goldsim_object_id` → `source_object_id` (in `element_base`,
  `container_def`, `time_history_displays`, and `connection_edge` prose); all
  source-format/tool references removed from descriptions.
- **Unit contract made explicit.** A top-level `$comment` states the rule that was
  previously only implied: every `unit`/`*_unit` field is a free-form string, not
  validated structurally, resolved to SI at load time via the companion registry
  `units.json` ($id `…/units/0.1.0`); it names where units attach (quantities,
  per-axis `*_unit` fields, `outputs[].unit`). `quantity.unit` and `output_spec.unit`
  descriptions now reference the registry.
- **Unit-bearing bounds widened (non-breaking).** `accumulator`/`pool`/`reservoir`
  `min_value` and `constant`/`controller` `bounds.min`/`max` now accept a `quantity`
  in addition to a bare number, so a floor/limit can state its unit explicitly. Bare
  numbers (the prior shape) still validate and are documented as inheriting the
  bounded value's unit. `distribution.truncation` documented likewise.
- **Scope boundary (excluded by design — not simulation elements):** the diagram
  layer (graphics symbols, ports, dashboard widgets), result/output recorders and
  chart styling, the internal expression-AST node classes (the parsed AST is
  carried in `expression_field` instead), unit/type descriptors, and run
  infrastructure (clocks, version/scenario/optimization/sensitivity managers,
  resource stores, container internals). These decode but carry no simulation
  semantics for an engine.
- `$id` bumped to `…/model/0.7.0`.

## 0.3.0–0.6.0 — iterated in place (no changelog entries)

The schema advanced from 0.2.0 to 0.6.0 in the working tree without changelog
entries (new distribution families, discrete-event element types
`decision`/`milestone`/`discrete_change`/`discrete_change_delay`/`timed_event`,
`stochastic_process`, `time_history_displays`, `connections`, and richer
`accumulator`/`expression_field` shapes). 0.7.0 takes 0.6.0 as its baseline.

## 0.2.0 — 2026-05-14

- `array` element gains a required `mode` sub-discriminator (`"constant"` |
  `"expression"`). Disambiguates the previously-overloaded `type: "array"`
  shape. Engines that accepted both shapes under one variant must now branch
  on `mode`. Transpiler emits `mode: "constant"` for Data-element arrays;
  the expression-based shape is reserved for future use.
- `distribution.normal.parameters.{mean,stddev}` and
  `distribution.exponential.parameters.mean` re-typed from `quantity` to
  `quantity_or_formula` to match transpiler output (already shipped
  `expression_field` here). `triangular`, `uniform`, etc. remain `quantity`
  pending evidence the transpiler emits ASTs for them.

## 0.1.x — pre-changelog

Schema iterated in place at `$id` `0.1.0`. Notable changes the engine
absorbed by re-syncing:

- `procedural` field added to `script` element
- `quantity_or_formula` gained `expression_field` variant
- `inferred_stub` added to `ExpressionSource` enum
- `inferred_ast` added to `ExpressionSource` enum
- new constant `array` shape introduced (later disambiguated in 0.2.0)
