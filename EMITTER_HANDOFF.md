# Emitter Handoff — WASiM v2 (schema 0.8.0 → 0.8.2)

**For:** the agent working on the re-gsm decoder/emit side.
**From:** the WASiM schema/engine owner.
**Purpose:** one place to orient on what the schema now expects, what emit already
does well, and the concrete work left — the encoding to produce and the data bugs
to fix — so the next regeneration lands the SubModel + optimization features cleanly.

Counts and file lists in this doc were measured against the corpus at
`/Users/collord/openvsim/wasim/schema_examples/` (162 files) on 2026-07-12. They
move between regenerations — re-measure before acting.

---

## 1. The arc so far (why these features exist)

Three schema rounds added the machinery for GoldSim **probabilistic optimization**
studies, driven by `WASIM_OPTIMIZATION_PROPOSAL.md`:

| Version | Added | Companion doc |
|---|---|---|
| **0.8.0** | The primitives rewrite (node/stock/link/event/gate/cell + traits). Dropped 0.7.0's submodel support in the process. | `wasim-schema-v2.json`, `wasim-engine-semantics.md` |
| **0.8.1** | **SubModels restored** on `container_def` (`kind`/`interface`/nested `simulation_settings`); new top-level **`optimization`** study block; **global id-uniqueness invariant** made explicit. | CHANGELOG 0.8.1 (design decisions answer the proposal's §5) |
| **0.8.2** | **`submodel_stat` AST node** — a real encoding for the `pdf_*` operations that were being stubbed to `literal 0.0`. | `SUBMODEL_STAT_ENCODING.md` |

All schema/spec pieces for "execute optimization end-to-end" are now in place. The
remaining gates are **all on the emit side** — populate the new encoding and fix
the data regressions below. The engine executor (nested submodel runs + Box's
complex solver) is the *next* engine round, and it is blocked until emit produces
real objective ASTs (today most objectives evaluate to a constant, so there is
nothing meaningful to optimize).

**Read these for detail (do not duplicate them — this doc indexes them):**
- `SUBMODEL_STAT_ENCODING.md` — the pdf/submodel-statistic AST node shape + lowering.
- `WASIM_OPTIMIZATION_PROPOSAL.md` — the original intent (your proposal); the
  schema owner's answers to its §5 questions are in CHANGELOG 0.8.1.
- `/Users/collord/openvsim/wasim/schema/wasim-engine-semantics.md` §1 (identity),
  §2.13 (`submodel_stat`), §12 (SubModels), §13 (Optimization).
- `/Users/collord/openvsim/wasim/schema/CHANGELOG.md` — 0.8.0–0.8.2 entries.

---

## 2. What emit already does well (don't regress these)

- SubModel interiors are now populated (e.g. designoptimization's submodel has
  10 interior elements; wgen's has real elements) — the earlier "hollow shell"
  problem is fixed.
- Element ids are **globally unique** — 0 duplicate ids across 162 models (down
  from 60 models / 690 collisions). The path-based id scheme works.
- Reference resolution has improved sharply — dangling refs dropped from ~1,482
  to **345** between regenerations.
- Optimization objective *elements* now exist and resolve (the earlier
  `ObjFunc`/`WQ_Diff` dangling objectives are gone).
- The full corpus validates against the schema (the only 8 invalid files are
  older stale-format models unrelated to this work).

---

## 3. Open tasks

### P0 — data bugs that BLOCK execution (fix before next regeneration)

**(a) `duration: 0` on 18 top-level models.** The engine rejects a zero-duration
model with `InvalidModel("duration must be > 0")`, so these are unrunnable, and
one is a load-bearing test model (`randomsequencegenerator`, in the engine's
`must_pass` set). This looks like the master clock being dropped/zeroed —
possibly the submodel's own zero-duration bleeding into the parent.

Affected (18): cloningcontainers, coffeemachinepurchasedecision, convolution,
customimportance, designoptimization, distributions, localizedcontainer,
montecarlostatistics, oil_sands_production, plume, precipgen_par,
probabilisticoptimization, randomsequencegenerator, rectangular_weir, sensitivity,
uncertaintyvariability, unscheduledtimesteps, wind_model_parameters.

Also 4 **submodels** carry `duration: 0` (dynamicoptimization, montecarlostatistics,
probabilisticoptimization Model/SubModel1; wind_model_parameters
Model/Generate_Statistics) — a submodel needs a real nested clock to run its
Monte-Carlo loop. Emit the actual submodel duration (GoldSim gives it), or `null`
to inherit the parent, never `0`.

**(b) `submodel_stat` still stubbed.** 9 files carry `pdf_*` operations that are
still emitted as `literal 0.0` (or a partial tree with the pdf sub-term zeroed),
with the real intent only in `expression.display`. This makes the *probabilistic*
optimization objectives constant. Files: bayesianupdating, customimportance,
designoptimization, montecarlostatistics, pra_class_ii_texas,
pra_class_ii_txcollocog, probabilisticoptimization, uncertaintyvariability,
wgen_par. **Fix: emit the `submodel_stat` node per `SUBMODEL_STAT_ENCODING.md`**
(including when it's a sub-term of a larger expression — keep the surrounding ops).

### P1 — correctness/consistency (should fix, not strictly blocking)

**(c) Interface-output naming is inconsistent.** 3 submodels mix bare names and
full slash-paths within a single `interface.outputs` (oil_sands_production
Model/Oil_Sands_Model = 7 outputs; scs_design_storm_simulator Model/Storm_Window
= 3; wgen_par Model/WGEN/TS_Stats/Empirical = 26). The identity rule (§1) requires
**full slash-path ids** everywhere a reference appears — normalize all interface
inputs/outputs to full ids so `submodel_stat.output` and the interface agree.

**(d) 345 dangling references remain**, in three buckets:
- **~249 unknown-symbol** (`Age`, `Months`, `Species`) — array-dimension / subscript
  symbols, not elements. If these are array indices, they should be encoded as such,
  not as element refs. (Bigger array-model question; lowest priority.)
- **~88 path-not-found** (`Model/Reservoirs/S1`) — refs into containers/submodels
  whose interior elements weren't emitted. Should shrink as more interiors are filled.
- **~8 clock/builtin** (`ETime`, `SimDuration`) — GoldSim clock symbols, not
  elements. Emit these as `time_ref` AST nodes (the engine has a `TimeRef` node for
  exactly this) rather than element `ref`s, and they resolve cleanly.

### P2 — the optimization objectives themselves

Per-model objective status (after the above are fixed, these should become
runnable): calibrationoptimization / dynamicoptimization / srm_snowmelt_runoff
already have real ASTs with resolving variables. designoptimization &
probabilisticoptimization need the `submodel_stat` fix (P0-b) + duration (P0-a).
hydropower_optimization has a stubbed objective AST **and** an optimization
variable whose `element_id` doesn't resolve — fix both. oil_sands_production just
needs duration (P0-a).

---

## 4. The `submodel_stat` encoding (summary — full spec in SUBMODEL_STAT_ENCODING.md)

Replace the `literal 0.0` stub for `pdf_mean` / `PDF_Value` / `pdf_sd` /
`PDF_CumProb` with:

```jsonc
{
  "op": "submodel_stat",
  "submodel_id": "Model/Prob_StormwaterModel",         // full-path id of the submodel container
  "output":      "Model/Prob_StormwaterModel/total_cost", // full-path id of the interface output element
  "statistic":   "mean",                               // mean | percentile | sd | cumulative_prob
  "arg":         { "op": "literal", "value": 95.0 }    // percentile: p in [0,100]; cumprob: threshold (may carry unit); omit for mean/sd
}
```

- Case-normalize the display fn: `pdf_mean`/`PDF_Mean`/`PDF_mean` → `mean`;
  `pdf_value`/`PDF_Value` → `percentile`; `pdf_sd`/`PDF_SD`/`PDF_sd` → `sd`;
  `pdf_cumprob`/`PDF_CumProb` → `cumulative_prob`.
- Percentile: display uses a fraction (`0.95`) → store **p in [0,100]** (`95.0`).
  Open question in the spec §8: confirm [0,100] vs raw fraction is acceptable.
- Lower the dotted `Submodel.Output` to the two full-path ids.

---

## 5. How to self-check a regeneration

Before handing back a regenerated corpus, run these — they're the same checks the
schema/engine side runs:

1. **Schema validation:** every file validates against `wasim-schema-v2.json` via
   `jsonschema.Draft7Validator` (expect only the ~8 known stale-format failures).
2. **No `duration: 0`:** no top-level `simulation_settings.duration.value == 0`;
   no submodel with `duration == 0` (use `null` to inherit instead).
3. **Id uniqueness:** `id` unique across all `elements` (0 collisions).
4. **Reference integrity:** every `inputs[]` / ast `ref`/`lookup_call` `element_id`
   / `container.elements` / `interface` / `optimization` element-id resolves to a
   real `id`, OR is a deliberately-encoded non-element (clock `time_ref`, array
   index). Report the residual dangling count by bucket.
5. **No stubbed pdf:** no element whose `display` contains a `pdf_*` call while its
   `ast` is `literal 0.0` — those must now be `submodel_stat` nodes.

The engine tolerates dangling refs (evaluates to 0.0) and unimplemented nodes
(`submodel_stat` → 0.0 placeholder today), so these won't crash a run — but they
silently produce wrong numbers, which is why the self-checks matter.
