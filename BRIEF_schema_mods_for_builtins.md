# BRIEF: schema mods for GoldSim built-in names (+ internals classification)

**Context.** The `emitcheck.py --exec` executability report surfaced every unresolved
(`dangling`) reference across the 205-file emit corpus. After the emit-side fixes below,
the survivors fall into three groups: (A) GoldSim built-ins the v2 schema has no construct
for — these need schema additions; (B) GoldSim internals that are display-only or
element-internal — identified, deliberately not emitted, non-blocking; (C) a small tail of
per-file emit gaps. Counts are corpus occurrences at 2026-07-19.

## A. Suggested schema mods (execution-bearing, blocked on schema)

### A1. `time_ref` property additions

Current enum: `elapsed, timestep, year, month, day_of_year, day_of_month, days_in_month`.
Corpus formulas also use these run properties (reported as `builtin:<name>`):

| GoldSim name | occurrences | proposed property | semantics |
|---|---|---|---|
| `SimDuration` | 8x / 5 files | `duration` | total simulation duration (same quantity as `simulation_settings.duration`), as a time value |
| `Hour` | 6x / 2 files | `hour` | clock time-of-day hour component (0-23), calendar-aware |
| `Minute` / `Second` | 2x+2x / 1 file | `minute`, `second` | time-of-day components (Wind Model interpolates diurnal profiles from these) |
| `StartTime` | 3x / 2 files | `start` | simulation start as an absolute calendar value (date-serial seconds — the same epoch the series re-basing uses); corpus usage is calendar arithmetic |
| `EMonth` (`EYear`) | 4x / 1 file | `elapsed_months` (`elapsed_years`) | calendar-aware count of month boundaries crossed since start; NOT derivable from `elapsed` (irregular month lengths) |

### A2. Monte-Carlo realization index

`Realization` (3x / 3 files; e.g. an SInterrupt halting on a specific realization).
Proposal: either a `time_ref`-style property `realization` (pragmatic) or a separate
`run_ref` node `{op: "run_ref", property: "realization"}` (cleaner — it is not a time).
1-based index, constant within a realization.

### A3. `lookup_call` mode keywords (TBL_*)

GoldSim table elements evaluate in four modes via a keyword second argument:
`Table(x, TBL_Inverse)` etc. Corpus: `TBL_Inverse` 4x/3 files, `TBL_Inv_Integral` 4x/4,
`TBL_Integral` 2x/2. Today these parse as a bogus ref argument.
Proposal: optional `lookup_call.function` field,
`enum: ["value", "inverse", "integral", "inverse_integral"]`, default `"value"`.
(Emit will map the keyword arg into the field once the schema lands.)

### A4. Looping containers (note only)

`LoopCount` (2x / 2 files) is the iteration counter of a GoldSim *looping container* —
a construct the schema doesn't model at all. Not worth a point fix; record that
container looping is an unmodeled feature.

## B. GoldSim internals — identified, intentionally not emitted

- **Display-only refs** (now non-blocking `dangling_display_ref` / `stub:display`):
  time-history plots of element-internal outputs — reliability children (`Child_1/2`),
  `OpTimeSincePM`, `Total_Time`, contaminant source internals (`Drums`, `Box_Interior`),
  and plots of a splitter's pre-split identity (`.../Splitter1`). The schema declares
  `time_history_displays` engine-inert, so these never block execution.
- **Element input-port placeholders** — FIXED emit-side (see C): a GoldSim formula can
  name the element's OWN drawn-link input ports (`And_1`, `Or_2`, `Amount`), which shadow
  outer names. Not model references at all.
- **Script locals** — FIXED emit-side: SScript declares its variables; refs to them are
  locally scoped.

## C. Emit-side fixes shipped with this brief (no schema change needed)

1. **SScript `variables`**: read the declared locals from `SScriptManager._vars` (`_xs`
   names) — SAC_SMA alone went from 255 dangles to 1.
2. **SAnd/SOr operand slots + Splitter `Amount`**: substitute own-input-port placeholder
   refs with the wired source's output ref (the aggregator-operands pattern;
   `_placeholder_port_refs`/`_subst_port_refs`).
3. **`TimestepLength`** → `time_ref timestep` (run-property alias for DT).
4. **`EDay`** → `elapsed / 86400` (fixed-period elapsed count; the UnitCast-inverse shape).
   EMonth/EYear intentionally NOT lowered this way (calendar-irregular, see A1).
5. **`gee`** → literal 9.80665 (GoldSim's lowercase standard-gravity constant).

## D. Remaining tail (per-file emit gaps, 12 files / 31 refs)

- **AgingChain (11x)**: `S1.Advance` / `S1#3` reference a *splitter's* named output ports;
  splitters emit as `<name>_to_<target>` link elements, so the `S1` identity vanishes.
  Needs either a resolver alias (`S1` + output → the matching `S1_to_X` link) or a parent
  node carrying the splitter's outputs. Emit design decision, not schema.
- **PopulationGrowthAgingChain (2x)**: `vector(Decades, 10 pers)` — the set-typed
  `vector()` constructor lost its ordinal set (ValueType `os1` absent on that node) and
  fell back to a plain array. Real fix: emit `sets` declarations and resolve set names at
  walk time (schema already has `sets` + `vector_map`; emit populates neither yet).
- **CoffeeMachinePurchaseDecision (6x)**: not an emit bug — the file is a `decode_partial`
  STOP (`implausible CArray count` in clockmc), so no connection graph exists for the
  port-placeholder substitution. Clears when the decode stop clears.
- Singletons to triage individually: `N` (Monod), `Spares_Available` (resource-pool query,
  reliability-resources subsystem unmodeled), `init_Target_Level` (Control Systems x2),
  Oil Sands x2, RandomSequenceGenerator, WGEN PAR x2, CashFlowAlternatives, Water Hammer.

---

## RESPONSE (2026-07-19, engine/schema state at 0.9.6)

Assessed against the current engine — several proposals were already implemented after this brief
was written; the rest that are useful are now done.

- **A1 calendar/time_ref remainder — DONE (schema 0.9.6).** Added `time_ref.property` values
  `hour`/`minute`/`second` (clock time-of-day), `start` (the `calendar_start` anchor), and
  `elapsed_months`/`elapsed_years` (EMonth/EYear — calendar-field boundaries crossed). These ride
  on the B6 real calendar and are calendar-aware; 0 without an anchor.
- **A1 `SimDuration` — ALREADY DONE (0.9.2).** Handled as a **reserved global** (§1b), resolves as
  `ref{SimDuration}`. The proposed `duration` time_ref property would be redundant — NOT added.
- **A2 `Realization` — ALREADY DONE (0.9.2).** A **reserved global** (1-based, per-realization),
  resolves as `ref{Realization}`. The proposed `run_ref` node would be redundant — NOT added.
- **A3 `TBL_*` modes — ALREADY DONE (0.9.2 + TBL_Derivative in 0.9.3).** `lookup_call` with a
  reserved-name `input2` (`TBL_Inverse`/`TBL_Integral`/`TBL_Inv_Integral`/`TBL_Derivative`) works
  and emit targets it. The proposed `lookup_call.function` enum is a redundant second encoding —
  NOT added.
- **A4 looping containers (`LoopCount`)** — unmodeled; note-only, agreed.
- **B/C/D** — emit-side; no schema action.

Net: the only schema work this brief still warranted was the calendar-of-day / calendar-elapsed
`time_ref` properties (0.9.6). Everything else was already covered by the reserved-globals and
table-mode mechanisms.
