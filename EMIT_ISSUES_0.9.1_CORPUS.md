# Emit-side issues found validating the regenerated 0.9.1 corpus

**Status:** findings for the emit/decoder side (`re-gsm`). Authored 2026-07-18 from the
0.9.1 corpus validation round (engine repo `wasim`, engine commit f47387c; emitter commit
be96e96; corpus = `openvsim/wasim/schema_examples`, 211 files, regenerated 2026-07-18).

**Headline:** the regenerated corpus is **211/211 schema-valid** against
`wasim-schema-v2.json` (0.9.1), all six schema additions are populated where expected, and
the engine test suite is green. Everything below is an **emit-side data problem** surfaced
by actually *running* the regenerated models — none of it is a schema or engine execution
regression, but items 1–3 are results-changing and item 2 blocks a model from running at
all. Verification method: each model was run through the v2 engine directly
(`parse_v2 → ModelGraphV2::build → engine_v2::run`, seed 42, 50 realizations) and its
time-history/final values inspected.

Ranked by severity:

| # | Issue | Affected models | Effect |
|---|-------|----------------|--------|
| 1 | Process ports emitted without a time basis (unit `"1"`) | 5 reverting + ~8 non-reverting | **wrong results** — levels explode to ±1e300/NaN |
| 2 | `sumv(x)` emitted as `sum_array(x, 0.0)` (2 args) | 10 models | **run failure** where evaluated (`wgen_par`) |
| 3 | Unit coercion `expr\|unit\|` dropped from emitted ASTs | `cashflow` (2 elements) | **wrong results** — pv_factor gets seconds, overflows to inf/NaN |
| 4 | Distribution moment ports stubbed to `fixed 0.0` | `demonstration_llw_sa_model_v1.15` | **run failure** — gamma shape = (0/0)² |
| 5 | 8 stale pre-rename duplicate files left in corpus | 8 files | corpus hygiene — stale 0.9.0 stamps |
| 6 | (known/deferred, listed for completeness) | various | see §6 |

---

## 1. Process ports emitted without a time basis → mean-reversion explodes

**Symptom.** `historygenerator.json` — the reference model for the Gap 5 mean-reversion
round — produces divergent garbage: `HighVol_Reverts` reaches ±1e300 and then NaN within
~50 daily steps instead of reverting toward its reference. Same for the reverting
processes in `capturetimes`, `positions`, `variableexchange`, `reportingperiods`.
Non-reverting siblings (`LowVol`, `HighVol`, `Volatile`, and the processes in
`investment`, `option`, `precipgen`, `calibrationoptimization`) hit the same problem
through the plain-GBM path and go to `inf` (this half is **pre-existing**, not new to
0.9.1 — reversion just made it loud).

**Root cause.** The emitted `process_spec` ports carry **dimensionless units**:

```json
"process": {"family": "gbm", "mean_type": "arithmetic",
  "mean":           {"value": 1.0,  "unit": "1"},
  "stddev":         {"value": 0.1,  "unit": "1"},
  "reversion_rate": {"value": 10.0, "unit": "1"},
  "initial_value":  {"value": 1.0,  "unit": "1"}}
```

Per §16 semantics the engine normalizes the timestep into the volatility's time unit
(`sample_ou_step` / `sample_gbm` in `engine/src/sampling.rs`): the denominator of
`stddev.unit` is the time basis. A bare `"1"` has no denominator and resolves to
**1 second**, so with the model's 86400 s timestep every step is scaled by
`dt_ratio = 86400`. For `HighVol_Reverts` that makes the Euler reversion coefficient
`κ·dt_ratio = 10 × 86400 = 864,000` (stable requires < 2) → geometric divergence. The
GBM drift path overflows the same way (`exp(0.995 × 86400)` = inf).

In GoldSim, SHistoryGenerator's drift/volatility/reversion-rate ports are **per-time**
quantities — the source model has a real time basis that the emit is dropping.

**Contrast (proves the mechanism, and that the engine is fine):** the corpus models whose
process ports *do* carry rate units run stably and revert correctly through the same
engine code path:

- `control_systems`, `control_systems_(1)` — σ in `l/s` → OK
- `cashflowalternatives` — σ in `item/yr` → OK
- `portfolio` — σ in `%/yr` → OK

**One inconsistency worth checking while in there:** `reportingperiods` emits
`mean: m3/day` but `stddev: m^3/s` for the *same* element — mixed time bases on one
process (and it explodes). And `portfolio`'s `reversion_rate` is an array formula
(`vector_map`), which the engine currently resolves to scalar 0 (documented scalar-only
limitation) — not an emit bug, but the emit side should know arrays don't revert yet.

**Ask.** Emit the source time basis on `mean` / `stddev` / `reversion_rate` (e.g.
`1/yr`, `$/day`), consistent within each process element. Engine-side we will separately
consider the exact OU discretization (`exp(-κΔt)`) so that bad units degrade instead of
exploding, but emit units are the real fix — without them even a stable integrator
computes the wrong process.

**Affected (reverting, results-changing):** `historygenerator`, `capturetimes`,
`positions`, `variableexchange`, `reportingperiods`.
**Affected (non-reverting, pre-existing inf):** `historygenerator`, `investment`,
`option`, `precipgen`, `calibrationoptimization` (bare `"1"` ports).

---

## 2. `sumv(x)` emitted as 2-arg `sum_array` → run failure

**Symptom.** `wgen_par.json` fails to run:
`Eval("function 'sum_array' expects 1–1 args, got 2")`.

**Root cause.** Source formulas like `sumv(XM)` (one argument) emit as:

```json
{"op": "call", "fn": "sum_array", "args": [
  {"op": "ref", "element_id": ".../XM"},
  {"op": "literal", "value": 0.0}]}
```

The engine's `sum_array` contract is exactly 1 argument (arity check dates to May, schema
enum semantics likewise). The spurious trailing `literal 0.0` looks like a leftover
default/axis argument in the `sumv` mapping. This shape also **predates this regen** —
the Jul 16 (0.9.0) leftover `demonstration_llw_sa_model_v1_15.json` already had it — so
it's a standing emit↔engine contract mismatch that the new corpus models finally put on
an evaluated path.

**Affected models (18 elements total):** `wgen_par` (14 elements, **fails to run**),
`srm_snowmelt_runoff` (6), `minewaterbalance` (1), `agingchainarray`,
`populationgrowthagingchain` (3), `portfolio`, `demonstration_llw_sa_model_v1.15`,
`windgen_par`/`precipgen_par` variants as applicable. Models other than `wgen_par`
currently run only because the offending expression isn't reached (or its error is
swallowed by an untriggered path) — they are latent failures.

**Ask.** Drop the second argument from the `sumv` → `sum_array` mapping (and audit
`sumr`/`sumc`/`meanv`-family mappings for the same pattern). Alternative is widening the
engine arity, but the extra `0.0` carries no information — fixing emit is the honest fix.

---

## 3. Unit coercion `expr|unit|` dropped from emitted ASTs → pv_factor overflows

**Symptom.** `cashflow.json` — the reference model for the promoted `pv_factor` builtin —
degrades to inf/NaN: `Development_Costs` is finite only at t=0, `Operating_Costs` goes to
inf.

**Root cause.** Source formula:

```
ptof(Inflation_Rate, ETime|yr|)
```

`ETime|yr|` is GoldSim's inline unit coercion — elapsed time *expressed in years*. The
emitted AST is:

```json
{"op": "call", "fn": "pv_factor", "args": [
  {"op": "ref", "element_id": "Model/Inflation_Rate"},
  {"op": "time_ref", "property": "elapsed"}]}
```

The `|yr|` coercion is gone. The engine's `elapsed` is in model time units (seconds
here), so it computes `(1.035)^63,115,200` → inf. The engine is faithful to its input;
the time argument is simply 3.15e7× too large. Before 0.9.1 this was masked because
`ptof` was an opaque `extern_call` → 0.0; the promotion turned silently-zero into
loudly-wrong.

**Ask.** Preserve the coercion in the AST — e.g. wrap the operand in a divide by a
1-of-that-unit literal (`{op:"divide", left:<expr>, right:{op:"literal", value:1, unit:"yr"}}`)
or whatever normalization the emitter already uses for display-unit conversion. Then
audit other `|unit|` sites in the corpus: any source formula using inline coercion as an
argument to the newly-evaluated builtins (`pv_factor`, `annuity_factor`, date extraction)
has the same exposure.

**Affected:** `cashflow` (`Model/Development_Costs`, `Model/Operating_Costs`).

---

## 4. Distribution moment ports stubbed to `fixed 0.0` → gamma sampling fails

**Symptom.** `demonstration_llw_sa_model_v1.15.json` (new in this corpus) fails to run:
`Sampling("shape is not positive in gamma distribution")`.

**Root cause.** `Model/Contaminant_Transport/StatesAndRates/DisposedInventoryBq` is a
gamma random variable with formula-valued moments, `shape = (mean/sd)^2`, referencing
`DisposedInventoryBq_Mean` and `DisposedInventoryBq_SD` — and both of those emit as
`fixed {value: 0.0, unit: "Bq"}`. So shape = (0/0)² = NaN. The source model presumably
carries real values/formulas on those ports; they are being stubbed.

**Ask.** Check the decode of those two ports (this smells like the known
stubbed-input-expression family, but on a new model — flagging so it isn't lost).

---

## 5. Corpus hygiene: 8 stale pre-rename duplicates

The regen changed filename sanitization (`_1` → `_(1)`, `_v1_3` → `_v1.3`,
`class_ii` → `class-ii`) and regenerated every model under its new name, leaving the old
files behind — still stamped `wasim_version: 0.9.0`, dated Jul 16:

```
control_systems_1.json            → control_systems_(1).json
coolingcoffeeexample_1.json       → coolingcoffeeexample_(1).json
greatfalls_hydropower_v1_3c.json  → greatfalls_hydropower_v1.3c.json
demonstration_llw_sa_model_v1_15.json → demonstration_llw_sa_model_v1.15.json
pra_class_ii_texas.json           → pra_class-ii_texas.json
pra_class_ii_txcollocog.json      → pra_class-ii_txcollocog.json
risk_battle_simulation_v1_3.json  → risk_battle_simulation_v1.3.json
risk_dice_strategy_v1_3.json      → risk_dice_strategy_v1.3.json
```

**Ask.** Delete the 8 old files (or have the regen script clean its output directory).
They double-count models in every corpus-wide test sweep and are the only non-0.9.1
stamps in the corpus.

---

## 6. Known/deferred (no action this round; listed so the record is complete)

These were expected going into the round and confirmed present; they are **not** new
findings:

- **Spreadsheet cell-range population (Gap 3, deferred).** All 5 spreadsheet models
  (`plume`, `plume_(1)`, `reservoir_carryover_realization`, `spreadsheet`, `wgen_par`)
  carry `value_rule: "spreadsheet"` nodes with an **empty** `spreadsheet` object. Schema +
  engine placeholder verified: they parse and run as fixed 0.0. Population of the
  cell→range binding remains the one open item from WASIM_SCHEMA_ADDITIONS.
- **`duration: 0` models.** `convolution.json` (and `conceptualdailyrunoff`, the
  `dynamicoptimization` submodel, etc.) still emit zero duration → single-step runs. The
  Gap 4 response expressions in `convolution.json` are correct (verified equivalent
  formulas run live in `earthquake` / `warranty_costs`); the model just doesn't step.
- **Empty `series` values.** `gr4j`'s `Model/Precip_TS` emits `values: []`, so the whole
  rainfall-runoff chain (including the two §17 unit hydrographs, whose weights verify
  correct: X4 = 1.35 d → [0.472, 0.528]) computes zeros. Input-side, not convolution.
- **First-history-only ensembles / `extra_value_rows`** (§18) confirmed carried
  (`multipleseries` etc.); engine plays back the first history per contract.

## What verified clean (for the record)

- 211/211 schema-valid; 203/203 regenerated files stamped 0.9.1.
- Feature scoping exact: reversion in 9 models, convolution expressions in 4
  (`convolution`, `earthquake`, `gr4j`, `warranty_costs`), payoffs in 4 (`option`,
  `positions`, `insurance`, `reinsurance` — well-formed, run without crash), spreadsheet
  in 5, §13a submodel-scoped optimization only in `dynamicoptimization` +
  `oil_sands_production`, top-level optimization in the other 5 study models.
- Promoted builtins all evaluate (no promoted name survives as `extern_call`):
  `erf` (`pathwaycomparison` — correct Ogata-Banks terms), `table_min`/`table_max`
  (`tablefunctions` — true table extremes), `get_year`/`get_month`/`column_count`
  present and parsed. Remaining `extern_call`s are the intentionally-opaque set
  (matrix linalg, `vIndex`, ImpProb/ImpWeight, and convolution's `lag` variable).
- Browser corpus smoke: 211 models load + run, no uncaught page errors (2 reported
  failures were test-harness races, verified passing in isolation).

---

## Resolutions (2026-07-18, emit-side response — re-gsm)

Worked the five items. Each finding was re-derived against the emit/decoder code before
acting (the report is an engine-side hypothesis; verified independently). Gate after all
changes: `emitcheck.py` — 205 files, 0 emit errors, 0 assertion failures, all 0.9.1-valid.

- **#2 `sum_array` arity — FIXED.** Confirmed at the source: GoldSim's `sumv`/`sumc`/`sumr`
  are genuinely 2-arg (`reduction(array, axis)`); all **41 corpus sites** carry a constant
  axis-0 selector (`ValueSNode 0.0`). Added `_fit_fn_arity`: when mapping onto a 1-arg engine
  reducer (`sum_array`/`mean_array`/`min_array`/`max_array`/`size_array`), a trailing literal-0
  axis arg is dropped. Applied at all AST-build sites (native walk, string parse, v2 sanitizer).
  Lossless — never strips a real operand or a non-zero axis. `wgen_par` now emits 1-arg.

- **#3 `expr|unit|` coercion — FIXED (generalized).** The coercion wasn't "dropped" — a
  `UnitCastSNode` over a *non-literal* was a silent no-op (only literals were being normalized).
  `ETime|yr|` decodes as `UnitCastSNode(_us=[31557600,0])` over the `ETime` IDSNode. Fix: for a
  non-literal `UnitCastSNode` emit the inverse affine `x_display = (x_SI - offset)/scale`, so the
  sub-expression is expressed in the cast's unit. `cashflow`'s `pv_factor` now gets
  `elapsed/31557600` (= years). Corpus-wide this corrects **62 non-trivial unit casts across
  ~26 files** (e.g. `L|ft|` → `L/0.3048`), not just `pv_factor`.

- **#4 gamma moment ports stubbed to 0 — FIXED (broader than reported).** Not an SDataDefn
  decode gap — the values ARE decoded, in `SDataInputDef.datadef` (a `DataDefArray`), because
  these are **array-valued** Data elements (36 values over the `Species` set, e.g. `3.58E+14 Bq`).
  `_em_constant` only read the scalar `_oc` and fell through to `0.0`. Added `_datadef_array_values`
  → emit a `fixed` node with `values[]`. Fixes `demonstration_llw`'s gamma shape, and populates
  **108 array-valued Data elements across 41 files** that were silently zero before.

- **#1 process ports without a time basis — NOT an emit bug (engine-side).** Re-derived: the
  dimensionless ports faithfully reflect the source — GoldSim stores HistoryGenerator's
  drift/volatility/reversion as **unitless** (`_dim` all-zero). Contrast: where the modeler
  assigns a rate unit (`control_systems` σ = `l/s`, `_dim=[0,3,-1,…]`) emit already carries it and
  the engine runs stably. The **reversion-rate port is dimensionless in every model** (a
  per-step pull, never unit-bearing). So there is no source time basis for emit to "drop"; the
  explosion is the engine reading bare `"1"` as per-*second* when GoldSim's random-walk applies
  these **per timestep**. This is the OU-discretization / bare-unit-default fix the report already
  flags as engine-side — the correct locus. Emit fabricating a `/day` basis would be guessing at
  semantics the decode can't confirm, converting "explodes loudly" into "wrong but plausible."
  **Recommendation:** engine defaults a dimensionless process rate to per-timestep. If, after
  that, an explicit basis on the emit side is still wanted, emit can pass the model timestep — but
  only once per-timestep is confirmed the intended semantics.

- **#5 stale duplicate files — addressed on regen.** The regen writes only the current
  sanitized stems and the 8 curated-only files are preserved intentionally (per the corpus owner);
  the pre-rename duplicates are the same-model stale copies. Cleaning them is a corpus-hygiene
  step on the regen output dir (not an emit-logic change).

**#6 (deferred items):** spreadsheet cell population is now DONE (SSpreadSheet emits `cells` with
Excel ranges — see EMITTER_HANDOFF §1c); `duration:0` and empty-series remain open (separate
P0s, unchanged this round).

---

## Resolutions round 2 (2026-07-18, detention_pond validation — emit items 5–6)

The three emit-side items from the `detention_pond_optimize` engine run, each re-derived against
the source `.gsm` decode before fixing. Gate after: `emitcheck.py` — 205 files, 0 emit errors, 0
assertion failures, all 0.9.1-valid; corpus regenerated.

- **#6 SSum signs — FIXED.** The signs are NOT in dropped edges — they're in the SSum's SElement
  base `inputs[]` as native expressions: `Net_Benefit` port[1] = `-Construction_Cost` (a
  `UMinusSNode`), `Reduction_Peak` = `7 cfs` + `-Peak_Discharge`. The old operand recovery used
  only graph edges (`src_elem` names), discarding the port expressions. `_wire_aggregator_operands`
  now emits, per input port, the port's native expression AST (carrying the sign/literal) and falls
  back to the influence edge only for a bare link. `Net_Benefit` → `sum_array([Detention_Benefit,
  neg(Construction_Cost)])`. (Corpus: only detention_pond has signed sums — targeted, not
  over-firing.) NOTE: `Reduction_Peak`'s first operand is a literal `7 cfs`, not `Peak_Inflow` as
  the report inferred; `Peak_Inflow` genuinely feeds nothing in the source.

- **#5a wrong output port — FIXED for AST refs; a schema limit for signal-inputs.** The decode
  DOES know the port: `Peak_Discharge`'s edge `src_port` is Reservoir output index 4 (`m3/min`
  discharge), not the primary volume (index 0). `_src_output_ref` now names a non-primary output
  `<Element>#<k+1>` (matching `_element_outputs`). This works in an `operands[].ast` ref
  (`{ref, output}`). BUT `filter.input` / `delay.input` / `convolution.input` are bare element-id
  strings with NO output-port qualifier — so for an extremum watching a secondary port, emit
  synthesizes a companion `expression` node (`<id>__signal` = `ref(Reservoir, output=Reservoir#5)`)
  and filters over that. **Flag for schema:** the signal-`input` fields can't address a secondary
  output; the `__signal` shim is the workaround. A first-class `{element, output}` input form would
  remove it.

- **#5b extremum-as-running-peak — FIXED.** SExtrema is a running max/min over time, which
  `max_array` (a per-step spatial reduction) can't express. Now maps to the **`filter`** node:
  `statistic` from `_b90` (1→max, 0→min — verified semantically: min-mode elements are named
  Valley/Minimum_Volume/Tmin_Ext), `window` = full-run step count (`ceil(duration/timestep)`, e.g.
  96 for detention_pond). **27 of 29 corpus SExtrema** now emit as `filter` (the 2 exceptions have
  an unresolvable input → no `input`, still valid). ASSUMPTION for the engine: a whole-run `window`
  gives the cumulative running peak (the filter buffer spans the run). If `filter` caps window size
  or only supports a trailing window, the running-peak-since-t0 semantic needs an engine confirm or
  a dedicated cumulative-extremum mode.

Not addressed (engine-side, per the report): the three dangling globals (`gee`, `TimestepLength`,
`TBL_Inv_Integral`) and the OU/basis explosion (§1) remain engine items — `Orifice_Flow ≡ 0` still
zeroes the optimization until `gee` resolves, so these emit fixes are necessary but not sufficient
for the model to reproduce the article end-to-end.

---

## Engine-side confirmation (2026-07-18, regenerated corpus verified against engine f47387c)

Re-ran `detention_pond_optimize.json` (regenerated) through `parse_v2 → build → run_v2`.

- **#5b whole-run window = cumulative peak — CONFIRMED.** The filter buffer is a trailing
  window with **no size cap**: it evicts only while `len > window`
  (`engine_v2.rs` `NodeRule::Filter`, ~line 507). The engine evaluates the filter exactly
  `n_steps = round(duration/dt)` times; emit's `window = ceil(duration/timestep)` ≥ that
  always (ceil ≥ round), so nothing is ever evicted and the trailing max degenerates to the
  running max since t=0. No dedicated cumulative-extremum mode needed. Empirical:
  `Peak_Inflow` now reports 0.4723 (true peak, held from the peak step onward), previously
  0.0368 (final instantaneous). Keep emitting `ceil` (or anything ≥ round). One caveat: the
  filter reads its input from the **current-step** outputs with no previous-step fallback,
  so a filter inside a dependency cycle would push 0.0 for steps where its input evaluates
  later — not the case in any current corpus use.

- **#5a `{ref, output}` port qualifier — emit is correct, but the ENGINE drops it.** New
  engine item: `eval_ast` destructures `Ref { element_id, .. }`, ignoring `output`, and the
  engine publishes a single value per element id (stocks publish only the primary
  volume/level; rate ports `#2..#5` are never materialized). So
  `Peak_Discharge__signal = ref(Reservoir, output=Reservoir#5)` still resolves to the
  volume (2468 m³), and `Peak_Discharge`/`Reduction_Peak` remain wrong despite the correct
  emit. Engine needs: (a) publish stock secondary outputs, (b) honor `Ref.output`. The
  schema flag stands too: `filter/delay/convolution.input` still can't address a secondary
  port first-class; the `__signal` shim is the interim contract.

- **#6 SSum signs — CONFIRMED in-engine.** `Net_Benefit = Detention_Benefit −
  Construction_Cost` and `Reduction_Peak = 7 cfs − Peak_Discharge` now evaluate with the
  emitted signs (final Net_Benefit goes negative as expected under the still-broken
  physics).

Still open (engine, unchanged): `gee`/`TimestepLength`/`TBL_Inv_Integral` dangling → 0.0,
so `Orifice_Flow ≡ 0`, the reservoir never drains, and the optimization objective stays
flat in `Outlet_Area`. Order of impact for reproducing the article: `gee` (unblocks the
feedback loop and the optimizer), `TBL_Inv_Integral` inverse-integral lookup mode
(Water_Level/Construction_Cost), `TimestepLength` (Max_Storage_Release), then #5a's
stock-port publication (Peak_Discharge/Reduction_Peak/Detention_Benefit).

---

## Engine/schema response (2026-07-18, schema 0.9.2) + outstanding emit work

The engine items from the confirmation section above are now DONE, shipped as schema
0.9.2 (additive; `$id` bumped; existing 0.9.1 corpus docs still validate). Semantics
§1b/§1c define the contracts; `schema/CHANGELOG.md` has the summary.

**What the engine now does (no emit change needed):**

- **Reserved globals resolve** (§1b): `gee` = 9.80665 m/s², `TimestepLength` = Δt in
  seconds, `SimDuration` = duration in seconds, `Realization` = 1-based index — resolved
  before the dangling-ref fallback; a real element with the same id shadows the global.
  Emit's verbatim pass-through of these names is now the CORRECT encoding — do not lower
  them to literals.
- **`TBL_*` table modes** (§1b): `TBL_Integral` / `TBL_Inverse` / `TBL_Inv_Integral` as
  `lookup_call` `input2` select cumulative-integral, inverse, and inverse-of-integral
  (stage-storage) evaluation. Again, emit's current encoding is now correct as-is.
- **Output-qualified refs work** (§1c): `{ref, element_id, output: "<Name>#k"}` resolves a
  published port under `"<id>#k"`, falling back to the primary value if unpublished. The
  `__signal` shim composes with this unchanged (the shim node's qualified ref now reads the
  real port).

**Outstanding emit work, ranked:**

1. **Populate `output_spec.role` on stock secondary outputs** (new optional 0.9.2 field;
   enum `addition_rate` | `withdrawal_rate` | `overflow_rate` | `net_change`) from the
   SPool port decode, then regenerate. The engine publishes a port ONLY for role-declared
   outputs. Verified end-to-end: patching just `Reservoir` `outputs[4]` (`Reservoir#5`) to
   `withdrawal_rate` in `detention_pond_optimize` makes the whole article workflow
   reproduce — run: Net_Benefit $56,294 at the stored optimum; optimization: converged,
   Outlet_Area 0.1841 ft², $56,295 (GoldSim article: 0.1838 ft², $56,290.9). Populate
   roles for every SPool port the decoder can name, not just referenced ones.
2. **Script-variable scoping leaks** — the largest remaining dangling-ref population:
   bare local names emitted as refs (`sac_sma` ~19 names ~150 refs, `water_hammer`,
   `wind_model`, `precipgen_par`, plus `Result`/`Category`/`index` in the dashboard
   models and `And_1`/`And_2` in 7 logical-gate models). These still evaluate as 0.0.
   Needs id-qualification (or local declaration) at emit.
3. **Calendar property lowerings** — `EDay`/`EMonth`/`Hour`/`Minute`/`Second` (5 models)
   are NOT reserved engine names. Lower at emit to existing AST where expressible
   (e.g. elapsed-days = the §3 unit-cast form `elapsed/86400`); anything needing true
   calendar-of-day (Hour of day) should be flagged for a `time_ref` property addition
   rather than left dangling.
4. **(Unchanged/known)** `duration:0` + empty-series P0s; `filter.input` stays a bare
   element-id string in 0.9.2 (first-class `{element, output}` input deferred — the
   `__signal` shim remains the contract); stage_area-style `x_unit: "1"` hygiene.

---

## Tier-A engine deltas available for emit (WORKPLAN_TIER_A.md, schema 0.9.3)

The engine now executes the Tier-A GoldSim-parity features. Emit-facing hooks:

1. **Distribution roster (§6).** New `distribution.family` values are live and validated:
   `log_uniform`, `log_triangular`, `log_cumulative`, `triangular1090`,
   `log_triangular1090` (map GoldSim's 10-90 alt-parameterizations directly),
   `binomial`, `negative_binomial`, `poisson`, `extreme_probability`
   (`{base, n, extreme}`), `beta_success_failure` (`{successes, failures, min?, max?}`).
   **HIGH VALUE:** `extendeddatabase.json` and `distributions.json` currently emit
   Poisson / Binomial / Negative-Binomial / Beta(succ-fail) / Extreme-Probability as
   `external` placeholders (which now warn + degrade to 0.0). Re-emit those as their
   real families — the decode targets exist. `external` without an inline
   `parameters.fallback` `{samples, weights}` table no longer silently yields a usable
   value; supply a fallback or a concrete family.

2. **`TBL_Derivative` (§1b).** A fourth reserved `lookup_call` second-argument name
   (joins `TBL_Integral`/`TBL_Inverse`/`TBL_Inv_Integral`): slope of the interpolated
   table at x. Map GoldSim table-derivative element references to it.

3. **Lookup interpolation.** `interpolation: "spline"` now runs monotone-cubic
   (Fritsch-Carlson, no overshoot) instead of a silent linear downgrade;
   `interpolation: "log_linear"` interpolates ln(y). N-D tables: the existing
   `table.z = [axis2_bp, (axis3_bp)?, flat_values]` packing is now consumed via
   multilinear interpolation (2-D bilinear proven on `basictable.json`; 3-D trilinear
   engine-ready — 3-D `lookup_call`s currently pass only the 2nd-axis coord via
   `input2`, so a 3rd-axis call arg is the one missing emit piece if a model needs it).

4. **New node `value_rule`s (§2).** `status` (`{set, reset}` triggers → latch 1/0),
   `milestone` (`{trigger}` → first-fire elapsed time, NaN before), `pid`
   (`{input, setpoint, kp, ki, kd, output_min?, output_max?, deadband?}`). Map
   GoldSim Status / Milestone / Controller elements to these.

5. **`interrupt` event effect (§2).** `effect_spec.mode: "interrupt"` (no target/change)
   ends the realization at end-of-step; remaining steps hold last values. Map GoldSim
   Interrupt elements to it.

6. **Event predicates.** `occurs(event_id)` and `changed(ref)` AST `call` builtins are
   live; their single argument is an element `ref` (read as an id).

7. **LHS + constraints + results are runtime, not emit.** Latin Hypercube uses the
   existing `simulation_settings.sampling_method: "lhs"`; optimization constraints are
   now *enforced* from the existing `optimization.constraints[].condition` (encode a
   feasibility predicate as a comparison AST that evaluates to 1.0/0.0); the richer
   results/analysis layer is a `RunConfig.results_spec` (host-configured), no schema.
