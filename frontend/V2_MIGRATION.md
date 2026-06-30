# Frontend → v2 engine migration — scope

Companion to `engine/V2_SCOPING.md`. The engine is fully migrated to the v2 primitives
model; the frontend is still entirely v1. This doc scopes catching it up.

## 0. Where things stand

- **Build un-broken (done).** The M2 distribution changes had broken `engine/src/wasm.rs`
  (cfg-gated to wasm32, so host `cargo test` never caught it). Fixed + committed; `pkg/`
  rebuilt. The UI runs **v1 models** again on the current engine.
- **The bridge targets the v1 engine.** `WasmEngine` (wasm.rs) uses `WasimModel` / `ElementKind`
  / `ModelGraph` / `run` — not `simulate`/`run_v2`/`parse_v2`. No v2 capability is reachable.
- **The frontend model layer is v1.** `types.ts` (`ModelElement` union keyed by `type`),
  `store.ts` editors, and the Dashboard/Model/Graph tabs all assume the v1 taxonomy.

**Good news that shrinks the work:** the **results contract is engine-agnostic**.
`SimulationResults` (`time_axis`, `elements{label,unit,final_values,time_history}`,
`output_ids`) is byte-identical between the v1 and v2 engines, so **ResultsTab needs no
change**. The migration is confined to: the bridge, the model *summary*, and the *editing*
flow.

## 1. Coupling inventory (what actually references v1)

| File | v1 coupling | v2 impact |
|---|---|---|
| `engine/src/wasm.rs` | v1 engine: `WasimModel`/`run`; `model_summary` matches `ElementKind`; `set_constant`/`set_rv_param` mutate v1 kinds | **rewrite** to hold a v2 `Model` + `ModelGraphV2`, run via `run_v2` |
| `frontend/src/engine.d.ts` | declares the WasmEngine method shapes | update if signatures change |
| `frontend/src/types.ts` | `ModelElement` union, `ElementSummary{type,editable,unit}` | extend / replace |
| `frontend/src/store.ts` | `setConstant`/`setRvParam`/`saveParameters` branch on `type==='constant'\|'random_variable'`; keeps a typed `parsedModel` | rework editing on v2 |
| `components/tabs/DashboardTab.tsx` | editable discovery: `type==='constant' && editable \|\| type==='random_variable'`; param editors for `ConstantElement`/`RandomVariableElement` | rework |
| `components/tabs/ModelTab.tsx` | deps from `parsedModel.elements` + `summary` | minor (uses `inputs`) |
| `components/tabs/GraphTab.tsx` | renders nodes from `summary.elements` (type label) | minor (primitive label) |
| `components/tabs/ResultsTab.tsx` | `SimulationResults` only | **none** |
| `worker/protocol.ts`, `sim.worker.ts` | message types `set_constant`/`set_rv_param`; `model_summary` | minor (generalize) |

## 2. Two decisions that shape the plan

> **Decided:** D1 = **both v1 and v2** (bridge detects + normalizes). D2 = **enriched bridge
> summary** (frontend is schema-shape-agnostic). The §3 plan reflects these.


**D1 — Input formats the bridge accepts.**
- *(recommended)* **Both v1 and v2**, via the engine's existing detection: `WasmEngine::new`
  uses the `simulate_json` logic — v2-native (first element has `primitive`) → `parse_v2`,
  else v1 `WasimModel` → `normalize_v1`. Internally the bridge holds one v2 `Model`. This keeps
  every existing v1 model file loadable while unlocking v2-native models.
- *Alternative:* v2-only (simpler bridge, but breaks all existing v1 model files until the
  transpiler emits v2).

**D2 — How the frontend represents the model for editing/rendering.**
- *(recommended)* **Drive the UI from an enriched bridge summary**, so the frontend never
  parses or owns the model schema. Extend `model_summary` to carry, per element: `primitive`,
  `value_rule` (nodes), active `traits`, `unit`, `editable`, **current editable values/params**,
  and `inputs`. The frontend renders + edits purely from this summary → it becomes
  schema-shape-agnostic and survives future schema changes. Drop the typed `parsedModel`;
  `saveParameters` reads the summary (or a new `params_json()` bridge method).
- *Alternative:* Port `types.ts` to the full v2 primitive/trait union and parse v2-native JSON
  in the frontend (more code, re-couples the FE to the schema shape).

## 3. Plan (assuming D1=both, D2=enriched summary)

**Phase 1 — Bridge to the v2 core (engine/src/wasm.rs).** *Medium.*
- `WasmEngine::new`: detect v1/v2, build a v2 `Model` (`parse_v2` or `normalize_v1`) + a
  `ModelGraphV2`. Surface unit-validation warnings (from `units::validate`).
- `run_json` → `run_v2`. (Results JSON shape is unchanged.)
- `model_summary` → v2-aware: per element emit `primitive`, `value_rule`, active `traits`
  (derived from field presence), `unit`, `editable`, `inputs`, and current editable
  values/params. Keep `element_count`/`containers`/`simulation_settings`.
- Editing: `set_constant` → set a node/`fixed` value; `set_rv_param` → set a node/`sample`
  distribution param. (Rename later to `set_editable`/`set_dist_param` if desired.)
- Add `params_json()` for `saveParameters` (or keep the FE's reducer over the summary).
- Update `engine.d.ts`. **Exit:** existing v1 models load + run identically through the v2
  core; a hand-authored v2-native model loads + runs.

**Phase 2 — Frontend model/editing on the enriched summary.** *Medium.*
- `types.ts`: replace `ModelElement`/`ElementSummary` with the enriched-summary types
  (`primitive`, `value_rule`, `traits[]`, `editable`, `value`/`params`). Drop the v1 union.
- `store.ts`: editor actions + `saveParameters` operate on summary elements (no typed
  `parsedModel`); live-value echo updates the summary entry.
- `DashboardTab`: editable discovery = `editable` flag; param editors keyed by
  `value_rule`/distribution family rather than `type`.
- **Exit:** Dashboard edits + save/load work for v1-imported and v2-native models.

**Phase 3 — Graph/Model views + polish.** *Small–Medium.*
- `GraphTab`/`ModelTab`: label nodes by `primitive`/`value_rule`; show active traits; keep
  the dependency graph from `inputs` (+ stock inflows/outflows, link source/target).
- Surface unit-validation warnings in the UI.
- **Exit:** the graph/model views read naturally for v2 primitives.

## 4. Risks / notes

- **wasm.rs is invisible to `cargo test`** (cfg `wasm32`). Add a CI step:
  `cargo check --target wasm32-unknown-unknown --features wasm` so bridge breakage is caught.
- The bridge must stay a thin adapter — all engine logic lives in `run_v2`. Avoid
  reimplementing trait detection in JS; emit active traits from Rust in the summary.
- New v2 distribution families have no in-browser param-editing hooks yet (the bridge returns
  an error). Wire the high-value ones (pert/triangular/etc.) in Phase 2 if needed.
- `pkg/` is gitignored — Phase 1 needs a `build-wasm.sh` rerun to land in the running app.

## 5. Rough sizing

| Phase | Size | Risk |
|---|---|---|
| P1 bridge → v2 core + enriched summary | M | medium (summary shape is the contract) |
| P2 FE editing on the summary | M | low–medium |
| P3 graph/model views | S–M | low |

The honest cost driver is **P1's summary contract** (get it right once — it's what P2/P3 build
on) and the **`types.ts` rework** in P2. ResultsTab and the whole results path are free.
