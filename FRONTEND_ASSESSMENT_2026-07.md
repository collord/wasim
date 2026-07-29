# Visual Authoring Frontend — Completeness Assessment

**Date:** 2026-07-29 · **Against:** [`WASIM_AUTHORING_ENVIRONMENT_SPEC.md`](WASIM_AUTHORING_ENVIRONMENT_SPEC.md)
**Frontend:** `frontend/` (React + TS + Vite + zustand + recharts + dagre; Rust engine as WASM in a web worker)

## Verdict in two numbers

| Capability | Completeness |
|---|---|
| **Viewing / running an existing model** | **~85%** |
| **Authoring a model from scratch** | **~35–40%** |

Framed against the spec's roadmap (§15): **Phase 1 (property authoring) ~70% done, Phase 2
(structural canvas) ~30% done, Phases 3–4 (analysis depth, dashboards, copilot) largely
aspirational.** The foundations are real and correct; the surface area of authorable modeling
constructs is what's thin.

## What's genuinely strong (spec-faithful)

- **Canonical editable model + reconcile loop** (§13.1–13.2): the store owns a `ModelDoc`, edits
  are pure transforms, a debounced reconcile runs the engine for summary + validation.
- **Undo/redo, new/open/save** (§13.4): snapshot stack; File System Access API with download
  fallback; blank scaffold; `.params.json` overlay export. Save round-trips a `view` block the
  engine ignores (layout persistence, §13.3).
- **Real expression editor** (§6, `model/ast.ts`): a true text↔AST Pratt parser — 50+ builtins,
  time-refs, ref/builtin autocomplete, live parse errors; committing recomputes `inputs`, which
  drives the influence arrows. *This is the best-developed surface.*
- **Results/analysis consume real engine output**: multi-series fan charts (p05–p95 bands),
  final-value histograms, a real one-at-a-time **sensitivity** tornado, and a working
  **optimization** UI (Box's complex, apply-optimum).
- **e2e-proven** (`frontend/e2e/authoring.spec.ts`): "build a model from scratch (new → add →
  wire → run)" and "run an optimization over an editable variable" pass.

## What's thin (the real limiters)

1. **Only 7 of ~26 engine constructs are authorable.** Palette (`model/edits.ts:197`): Constant,
   Stochastic, Time Series, Lookup, Expression, Previous-Value (lag), Stock. **Missing from
   authoring entirely:** node rules Process, Convolution, Markov, Hysteresis, PID, Queue,
   Status, Milestone, GateLogic, TerminalExpression; and every non-node primitive — **Link,
   Event (failure FSM), Gate, Cell/Species/Medium, Resource.** That is the transport,
   reliability, logic, and controller half of the engine.
2. **No visual wiring.** The canvas (`canvas/EditableCanvas.tsx`) is a *read-only projection* of
   the dependency graph — edges are derived from `inputs`; there are no ports, no edge-drawing,
   no edge deletion, and the three edge kinds (influence/flow/event, §2.2) collapse to one
   arrow. You wire only indirectly (type a reference / pick an inflow).
3. **No container / submodel authoring** (§1.3, §5.8): can't create containers, no drill-down,
   no submodel interface / `from`-binding editor → hierarchy and nested Monte-Carlo are
   unreachable from the GUI.
4. **No dimensional feedback in the expression editor** (§6–7): only syntactic parse checking;
   the engine's `check_dimensions` is not surfaced. No Units Manager.
5. **No `results_spec` UI** (§9, §11): fixed percentiles, no CCDF/exceedance, no CTE/skew/kurt,
   no capture-time distributions, and **no per-member array `#k` results** (results keyed by
   plain element id only).
6. **Dashboards not author-configurable** (§12): still the viewer-era auto-list.
7. **LLM copilot (§17) entirely absent.**
8. **Distribution picker: 14 families vs ~30 in the engine** — missing Poisson/Binomial/
   NegBinomial, Pareto/EV/Student-t, and the log-scale variants.

## Two correctness bugs (not just missing features) — FIXED ✅

- **Rename/delete did not track references inside expression ASTs.** `renameId` rewrote
  `inputs`/`inflows`/`outflows`/`view`, but a renamed id used *inside a formula* silently
  dangled. **Fixed** (`frontend/src/model/edits.ts`): rename now deep-rewrites every AST `ref`
  node (expression, stock rate, event trigger/condition, effect `change`), the scalar id fields
  (lag `input`, event `source`, `effects[].target`), and refreshes the cached `display`
  strings; delete scrubs those same fields and drops effects targeting the deleted element (AST
  refs are left on delete so the reconcile/validate loop reports them).
- **Dead escape hatch.** The unsupported-type inspector told the user to "edit the model JSON
  directly," but there was **no JSON editing surface**. **Fixed**
  (`frontend/src/components/inspector/Inspector.tsx`): the fallback is now a real per-element
  raw-JSON editor (Apply/Revert, live parse errors, id-lock so renames still route through the
  ref-rewriting path), via a new `replaceEl` store action + `replaceElement` transform.

## Strategic connection (matters for the RAM beachhead)

**The WASM bridge is fully v2** (`engine/src/wasm.rs`: holds a v2 `Model` + `ModelGraphV2`, runs
`run_v2`, auto-detects v1 vs v2-native by the `primitive` field, emits an enriched summary). So
`frontend/V2_MIGRATION.md §0` ("frontend is entirely v1 / no v2 capability reachable") is
**stale** — the run path is migrated.

Consequence: **the browser can *run* the RAM/fleet/finance v2-native models, but the GUI cannot
*author* them.** The haul-truck `condition`-FSM demo, the fleet dimensioned-array model, and the
LSM/nested-finance models are all hand-written JSON that loads and executes but has no palette /
inspector coverage. A maintenance manager could open and run the RAM model, tweak its editable
constants, and see results — but could not build it.

**Highest-leverage frontend work for the RAM direction:** palette + inspector editors for the
**Event / failure-FSM** primitive and the **dimensioned-array** (Fleet-dimension + `vector_map`)
pattern. Those two unlock authoring the reliability and fleet models — turning "we can run it"
into "a domain expert can build it." Runner-up: port-based visual wiring (§2.2) and
container/submodel authoring (§5.8).

## Build note

A fresh clone can't run the UI without setup: `frontend/node_modules` is not vendored
(`npm install` needed) and the WASM module isn't pre-built (`npm run build:engine` →
`engine/build-wasm.sh`). These are build steps, not completeness gaps.
