# Notes from the engine side — last updated 2026-05-15

Current-state snapshot of what the engine sees from transpiler output.
Verified against the corpus on 2026-05-15 (schema 0.2.0).

## Resolved since the last notes

Several earlier items are now fixed in the 0.2.0 schema + regenerated
corpus — recording so they aren't re-reported:

- **`array` overloaded discriminator** — fixed. `array` now carries a
  required `mode` sub-discriminator (`"constant"` | `"expression"`).
  Engine branches on it.
- **Distribution parameter typing** — fixed. `normal.{mean,stddev}` and
  `exponential.mean` are now `quantity_or_formula` in the schema,
  matching transpiler output.
- **Schema CHANGELOG + `$id` versioning** — done. `$id` is now bumped
  per breaking change (`0.2.0`); `CHANGELOG.md` exists. This is exactly
  what was asked for — thank you.
- **Invalid sim settings** — `hydropower_optimization.json` (was
  `timestep: 0.0`) and `demonstration_llw_sa_model_v1_15.json` (was
  `uniform min > max`) both run cleanly after regeneration.
- **TimeHistoryResult demotion** — `watershed_yield_nrcs.json`'s
  `Watershed Yield` is now a real `expression` element;
  `time_history_displays[]` is empty. The model's primary output is in
  the graph where it belongs.

## Still open

### Feedback cycles among `expression` elements

Three models have dependency cycles the engine's topo-sort rejects.
Earlier notes called these all "self-reference" — that was wrong, only
one is. The accurate breakdown:

| Model | Cycle | Shape |
|---|---|---|
| `designoptimization.json` | `Orifice_Outflow → Orifice_Outflow` | direct self-reference (1-cycle) |
| `plume.json` | `Hnum → f0 → Hnum` | mutual reference (2-cycle) |
| `minewaterbalance.json` | `ET_Loss → P → ET_Loss` | mutual reference (2-cycle) |

All participants are `type: "expression"`. These look like GoldSim
implicit feedback loops, which GoldSim resolves with previous-timestep
values. The engine has no previous-step semantics for `expression` —
only `accumulator` carries forward state (via `prev_outputs`).

For the transpiler, the fix is to break the loop at emit time: one
element in each cycle needs to either become an `accumulator`, or have
its back-edge reference explicitly marked as a previous-value read so
the graph builder can exclude it from topo edges (the same treatment
accumulator `rate` inputs already get). A plain `expression` cycle is
unresolvable downstream.

### Ghost element references in `input_ports[]`

`watershed_yield_nrcs.json` still has `input_ports` entries pointing at
element IDs that were never materialized:

- `Annual_Yield.input_ports` → `Avg_Yield`, `Result Box1`
- `Scale_Factor.input_ports` → `Sample_Distribution`
- `annual_runoff_depth.input_ports` → `Result Box1`

Engine doesn't consume `input_ports[]` (provenance only), so runs
aren't broken — but these look like GoldSim Result Boxes / sub-model
objects the port-extractor saw but the element-materializer skipped.
Likely the port-binding pass runs before/independently of element
emission.

## What's working well

- **Script element format** (`expressions[]`, `variables[]`,
  `procedural`) — clean. `~Result` self-reference resolves through the
  engine's `prev_outputs` fallback with no special handling. The
  `procedural` flag is the right "control flow ignored" signal.

- **Constant arrays** — `mode: "constant"` with `provenance`. All 124
  array elements in the corpus use this. `provenance:
  "extraction_pending"` (empty `values`) is a clear signal for "type
  known, binary body not yet decoded."

- **Distribution-parameter ASTs** — engine evaluates these
  per-realization, ordered with RV sampling. One standing assumption:
  the engine uses **document order** of RVs and assumes a
  distribution-parameter AST references only constants or RVs declared
  earlier in `elements[]`. If you can guarantee topo order at emit
  time, a model with `B.mean = ref(A)` where A follows B would be safe;
  otherwise B silently sees `A = 0.0`.

## Engine-side notes (FYI, no action needed)

- `engine::run()` guards `dt > 0` and `duration > 0`, returning
  `EngineError::InvalidModel` instead of panicking.
- The engine still evaluates `time_history_displays[]` if present
  (surfacing them in results). The corpus no longer uses that field —
  kept as defensive support in case future models emit it.
