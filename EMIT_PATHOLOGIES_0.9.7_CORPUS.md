# Emit pathologies — re-gsm 0.9.7 corpus (`schema_examples/`)

**Scope.** This documents defects in the **re-gsm emitter** surfaced by running the freshly
regenerated v0.9.7 corpus (220 files, `source.generator: "re-gsm/decoder"`) through the WASiM
engine. These are *emit-side* bugs: the engine parser is behaving correctly by rejecting malformed
models (failing loud, not silently). Fixing them is re-gsm work, not engine work.

**Method.** Every `schema_examples/*.json` was run through `parse_v2` → `ModelGraphV2::build`.
Result: **190 / 220 parse+build clean; 30 fail to parse; 0 fail graph-build.** All 30 failures fall
into **three emit pathologies**, all instances of the same underlying bug shape: *re-gsm emits a
node's `value_rule` discriminator but omits the fields that rule requires* (or emits a definition
element with its identifying field null).

The engine parser requires these fields (see [v2_parse.rs:661-677](engine/src/v2_parse.rs#L661-L677)):
a `pid` rule needs `input` + `setpoint`; a `status` rule needs `set` + `reset`. When re-gsm emits
the rule name without the body, parse aborts with `InvalidModel("node '<id>' (<rule>) missing
'<field>'")`.

---

## Pathology 1 — `pid` nodes emitted without `input` (15 files)

**Symptom:** `InvalidModel("node '<id>' (pid) missing 'input'")`

**Affected:** `comparecontrollers`, `control_systems`, `control_systems_1`, `controlleronoff`,
`deadband`, `inflowoutflow`, `minewaterbalance`, `pid`, `pooladvanced`, `proportional`,
`proportionalforecast`, `proxy`, `reservoir`, `targetatlowerbound`, `unscheduledtimesteps`.

**What re-gsm emits** (`comparecontrollers.json`):

```jsonc
{
  "id": "Model/Stochastic_Inflow/PID/Outflow_Controller",
  "primitive": "node",
  "value_rule": "pid",
  "inputs": [                       // the wiring is present as bare input ids…
    "Model/Stochastic_Inflow/PID/Bias",
    "Model/Stochastic_Inflow/PID/Derivative_Gain",
    "Model/Stochastic_Inflow/Flow_Capacity",
    "Model/Stochastic_Inflow/PID/Integral_Gain",
    "Model/Stochastic_Inflow/PID/Pond",
    "Model/Stochastic_Inflow/PID/Proportional_Gain",
    "Model/Stochastic_Inflow/Target_Volume"
  ]
  // …but NO `input`, `setpoint`, `kp`, `ki`, `kd` — the PID body is missing entirely.
}
```

**Root cause.** re-gsm recognizes the GoldSim PID/controller element and stamps `value_rule: "pid"`,
and it captures the raw influence edges into `inputs[]`, but it never **binds** those edges to the
PID's semantic roles: which input is the process variable (`input`), which is the target
(`setpoint`), and which are the gains (`kp`/`ki`/`kd`). The engine needs `input` and `setpoint` as
first-class fields — a bare `inputs[]` id list doesn't tell it which is which. The gain nodes are
even present in `inputs[]` by name (`Proportional_Gain`, `Integral_Gain`, `Derivative_Gain`) but
not mapped to `kp`/`ki`/`kd`.

**Fix (re-gsm).** In the PID/controller emit path, resolve the influence edges to roles by the
connected elements' class/unit/name (the gains are named/typed distinctly; the process variable vs.
setpoint distinction is in the GoldSim controller body) and emit `input`, `setpoint`, `kp`, `ki`,
`kd`. This is the same "connection-role binding" problem noted in the re-gsm
`CONNECTION_WIRING_SESSION.md`.

---

## Pathology 2 — `status` nodes emitted without `set` / `reset` (13 files)

**Symptom:** `InvalidModel("node '<id>' (status) missing 'set'")`

**Affected:** `agingchain`, `discreteevents`, `hydropower_optimization`, `internalexternal`,
`option`, `overtop`, `precipgen`, `quantity_and_duration_statistics`, `simple_stream_diversions`,
`srm_snowmelt_runoff`, `statusmilestone`, `water_hammer`, `work_under_pressure`.

**What re-gsm emits** (`agingchain.json`):

```jsonc
{
  "id": "Model/Input_Data/Births_Period",
  "primitive": "node",
  "value_rule": "status"
  // NO `set`, NO `reset` — the status latch has no trigger conditions.
}
```

**Root cause.** A `status` node is a set/reset latch (§2): it turns on when `set` fires and off when
`reset` fires. re-gsm identifies the GoldSim Status element and stamps `value_rule: "status"` but
emits neither trigger. The node is a completely empty shell — even more degenerate than the PID case
(which at least carries `inputs[]`).

**Fix (re-gsm).** Decode the Status element's set/reset trigger expressions (GoldSim stores these as
condition definitions on the element) into the schema's `set`/`reset` `trigger_spec` fields. If a
Status element genuinely has no reset (a one-shot latch), emit `reset` as a never-firing trigger
rather than omitting it, since the engine requires both.

---

## Pathology 3 — `species` definition element with null `species` field (2 files)

**Symptom:** `Json(Error("missing field \`species\`", line: ~7496))` — a hard serde deserialize
failure (stricter than the `InvalidModel` cases; the document won't even deserialize).

**Affected:** `demo.json`, `demonstration_llw_sa_model_v1_15.json`.

**What re-gsm emits** (`demo.json`):

```jsonc
{
  "id": "Model/Contaminant_Transport/Materials/Species",
  "primitive": "species",
  "species": null,          // ← the identifying field is null
  "half_life": null
}
```

**Root cause.** This is the GoldSim **Species OrdinalSet** (the set of 24 contaminant species,
modeled as one `SSpeciesElem` whose members are array indices — see the re-gsm cell-emit notes).
re-gsm emits it as a `species` primitive but leaves the `species` name field `null`, because the
element is a *set container*, not an individual species. The engine's `SpeciesRef`/species-def
deserializer treats `species` as required and rejects `null`.

**Root-cause nuance.** Unlike pathologies 1 & 2 (missing required data that exists in GoldSim), this
is a **schema/model mismatch**: GoldSim models species as one set element with N members, whereas
the WASiM schema expects one `species` element per substance. The correct fix is likely on the
*emit* side — expand the OrdinalSet into N individual `species` elements (one per member name) — but
it may also warrant a schema decision about how species-sets map. This is the same
"OrdinalSet → per-member expansion" pattern the cell-emit path uses for a cell's `species[]` list.

---

## Cross-cutting observation — the "discriminator without body" shape

All three pathologies are the same emit bug: **re-gsm classifies an element (assigns its
`primitive`/`value_rule`) but does not decode the element's body into the fields that classification
requires.** This is the emit analog of a stub. It is distinct from — and downstream of — the
*cell-body* gap documented separately (cells emit `volume`/`species`/`media` but not
`inflows`/`initial_inventory`/`partitioning`, so cell mass stays zero; see the S2 validation notes).

The pattern to watch for in re-gsm: any `_em_*` handler that stamps a `value_rule`/`primitive`
should be paired with a check that every field the WASiM schema marks **required** for that
discriminator is populated — ideally gated by `emitcheck.py` so a partial emit fails CI rather than
reaching the corpus.

---

## What the engine does right (do not "fix" these)

- **Fails loud.** All 30 malformed models are rejected at parse with a specific
  `missing '<field>'` message naming the element and field — not silently coerced to a default that
  would produce wrong results. This is the correct contract; the engine should keep rejecting these.
- **No graph-build failures.** Every model that parses also builds — the failures are purely the
  emit-omitted required fields, not structural graph problems.

## Not in scope here

- **Cell mass = 0** (cells populated with structure but no inflows/inventory) — a *different*
  re-gsm gap (cell mass-delivery decode), covered in the S2 corpus-validation notes. Cells parse and
  run fine; they just carry no mass yet.
- **Run-time model-data issues** (e.g. `spoil_heap_runoff`'s truncation-rejection, currency-node
  NaNs) — these are individual model/parameter issues, not systematic emit pathologies, and the
  engine handles them gracefully (Result errors / NaN propagation, not crashes).

---

*Generated 2026-07-21 against `~/openvsim/wasim/schema_examples` (re-gsm/decoder, 220 files).*
