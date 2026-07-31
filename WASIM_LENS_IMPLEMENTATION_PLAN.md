# WaSim Lens System — Implementation Plan

**Companion to** [`WASIM_VALUE_PROP_THESIS.md`](WASIM_VALUE_PROP_THESIS.md) (the *why*) and
[`WASIM_LENS_UI_DESIGN.md`](WASIM_LENS_UI_DESIGN.md) (the *what it looks/behaves like*). This
document is the *how to build it*.

**Scope (agreed):** the reusable **LensSpec machinery** with **stock-and-flow as the first
concrete lens** (the thesis's beachhead); the **reliability / RBD lens** as the second concrete
lens (Part D) — which needs no new engine work and is the cleanest "a lens is a spec file, not a
rewrite" proof; plus the engine-side **value-of-information (VOI) reduction** (Part C) scoped as
its own workplan for the **Decision/VOI follow-on lens**. The frontend Decision/VOI lens stays
mockup-only until the VOI engine op lands.

> **Grounding note.** Every claim below — frontend *and* engine — is cited to a file read
> directly (`engine/src/` included). Nothing in this plan is reasoned-but-unverified.

---

## Context

The thesis sells **lenses** — thin, domain-specific authoring surfaces over one general engine.
The frontend already contains an *accidental half-built lens system*: `PALETTE` entries carry a
`group` field (`frontend/src/model/edits.ts:279`) and `Palette.tsx` renders one section per
group — but **all groups show at once**, which is exactly the "general tool wearing three
half-lenses" trap the thesis names. The store already renders from the engine summary and
persists an **engine-ignored `view` block** (`frontend/src/store.ts:215`), so a lens needs *no
new source of truth and no engine change for the machinery itself*.

The goal of this plan: promote "which group is visible" into a first-class, **data-defined
`LensSpec`** that reprograms palette + glyphs + inspector + validation + results together, ship
**stock-and-flow** complete as the proof, and scope the one genuinely-new piece of engine work
(the VOI reduction) that the Decision/VOI lens will eventually need.

---

## Verified starting state

| Fact | Evidence |
|---|---|
| Engine-ignored `view` block is persisted and round-trips | `store.ts:215`, `edits.ts` `serializeModel` |
| Palette entries already carry a `group` (proto-lens) | `edits.ts:257` (`PaletteEntry`), `:279` (`PALETTE`) |
| Palette renders one section per group, all at once | `components/browser/Palette.tsx:11` |
| Inspector is section-based (Info/Definition/Output/Save), switches on element kind | `components/inspector/Inspector.tsx:41` |
| Status bar is fed **entirely** by the engine's validation `issues` | `components/shell/StatusBar.tsx:16` |
| Range-sweep already exists (`SensitivitySpec` over `SweepVar[]`) | `types.ts:174` |
| Reconcile round-trip: store → worker → summary + validation | `store.ts:419` `_scheduleReconcile`, `worker/protocol.ts:31` |
| **Optimization is a real, persistable schema field** — `model.optimization: Option<OptimizationSpec>` (objective + bounded variables + constraints); the FE just doesn't *save* it (builds the spec transiently and injects it per run) | `engine/src/model.rs:151,204,212,246`; `engine/src/wasm.rs:182,186` `optimize_json` → `model.optimization = Some(spec)`; `types.ts:206` |
| `optimize()` = Box's complex over `evaluate_point`; deterministic given seed | `engine/src/optimize_v2.rs:454` (`optimize`), `:233` (`solve`) |
| `evaluate_point` exposes the objective's **per-realization `final_values` before reduction** — the primitive VOI needs | `engine/src/eval_harness.rs:66,80` |
| `set_variable` sets only Fixed **scalars** and **rejects Sample nodes** | `engine/src/eval_harness.rs:26,33` |
| Engine parse ignores unknown fields (**no `deny_unknown_fields` anywhere**); `RawElement` is all `#[serde(default)]` | `engine/src/v2_parse.rs:71` + repo-wide grep |
| **No VOI / decision / EVPI anywhere in the engine** — net-new work | repo-wide grep of `engine/src/` |
| **Reliability FSM is native + already authorable** — `Event` + `FailureProcess` (basis: Condition/OperatingTime/Demand/CapacityDemand/Event/ExposureTime), `RepairSpec` (policy: None/Repair/Replace/PreventiveMaintenance) | `engine/src/model_v2.rs:396,440,448,463`; FE `EventEditor` per `FRONTEND_ASSESSMENT_2026-07.md` |
| **Series / parallel / k-of-n redundancy is native** — `gate` primitive `GateNode::{And, Or, Not, NVote{threshold,children}}` (currently raw-JSON-only in the FE) | `engine/src/model_v2.rs:480`, `v2_parse.rs:535,728` |
| **Availability / MTBF / MTTR are *derived in-model*, not native reductions** — computed via `masked_mean`/`masked_count` + `results_spec` CCDF/exceedance/CTE; the proven RAM arc does exactly this | `STAGE3_FLEET_SCOPE.md:64`, `STAGE1_RAM_DEMO.md:37`, `engine/src/results_spec.rs:142` |

**Consequence:** the machinery is a *selection layer* over existing surfaces, not a rewrite.

---

## Part A — Lens machinery (Phase 0, frontend-only, non-breaking)

Introduce a `frontend/src/lenses/` module and thread one new piece of store state.

1. **`LensSpec` type + registry** (`frontend/src/lenses/types.ts`, `registry.ts`). A lens is
   pure data (so the 2nd/3rd lens is a spec file, not a fork):
   ```ts
   interface LensSpec {
     id: 'general' | 'stock-flow' | 'reliability' | 'decision'
     label: string; tagline: string
     palette: (all: PaletteEntry[]) => PaletteGroup[]   // pick + relabel + reorder from PALETTE
     roleOf: (el: FlatElement) => LensRole | null        // reads lens_role tag (Part A.8)
     glyphOf: (el, role) => IconType                      // canvas/inspector glyph
     inspectorLabels: Partial<Record<LensRole, FieldLabelMap>>
     invariants: (summary: ModelSummary, doc: ModelDoc) => Issue[]  // FE author-time checks
     resultPreset: ResultPreset
     templates: ModelTemplate[]
     primaryVerbs: { label: string; paletteKey: string }[]
   }
   ```
   The `general` lens is the least-restricted spec (palette = full union = today's behavior), so
   "no lens" is not a special case.

2. **Store: `activeLens` + persistence.** Add `activeLens: LensId` to the store
   (`store.ts`). On `loadModel`, read `doc.view?.lens ?? 'general'`. On lens switch and on any
   edit, write `view.lens` through the existing view-block path (extend `edits.ts` `setPosition`
   sibling — a `setLens(doc, id)` transform). **No reconcile needed** — `view` is engine-ignored,
   same fast-path as `moveNode` (`store.ts:376`).

3. **Palette driven by the spec** (`Palette.tsx`). Replace the `groups = [...new Set(PALETTE.map
   group)]` line (`Palette.tsx:11`) with `activeLens.palette(PALETTE)`. `PaletteEntry.make` is
   unchanged; the lens only chooses/relabels which entries show.

4. **Inspector relabel** (`Inspector.tsx`). Keep the four `Section`s; source their field labels
   and the header `kindLabel`/`iconTypeOf` (`Inspector.tsx:33,36`) from
   `activeLens.inspectorLabels[role]`, falling back to today's generic labels. No new editor
   components — a stock's "Initial level" is the accumulator's initial field relabeled.

5. **Canvas glyphs** (`ui/typeIcons.tsx` `iconTypeOf`, `canvas/EditableCanvas.tsx`). Route the
   node glyph and edge style through `activeLens.glyphOf`. Edge kinds (influence/flow/event —
   today collapsed to one arrow per `FRONTEND_ASSESSMENT_2026-07.md`) become visually distinct
   only inside a lens that gives them meaning.

6. **Lens invariants merged into the issue stream.** Add a `useLensIssues()` hook that runs
   `activeLens.invariants(summary, doc)` and a `StatusBar.tsx` change to render
   `[...engineIssues, ...lensIssues]`. **Keep lens validation FE-side** — this honors the store's
   "engine is the arbiter of meaning / engine stays general" principle (`store.ts` design
   principle 2); the lens is an *author-time governance overlay*, not new engine semantics.

7. **Lens picker + toolbar switch.** First-run card chooser on `newModel`; a lens dropdown in
   `components/shell/Toolbar.tsx`. Switching is non-destructive re-skinning (see design doc §6:
   filter-not-cage; switching *up* into a stricter lens is a conformance report, never a silent
   conversion).

8. **Schema: additive `lens_role`.** Add an optional per-element `lens_role` string to
   `schema/wasim-schema-v2.json` (distinct from the existing stock-port `role` field, which is
   flow-accumulation semantics — do **not** reuse it). Document-level `view.lens` needs no schema
   change (already inside the free-form `view` block). **Confirmed non-breaking:** the engine has
   no `deny_unknown_fields` and `RawElement` is all `#[serde(default)]` (`engine/src/v2_parse.rs:71`),
   so an unknown `lens_role` is silently ignored on parse and simply rides along in the FE's
   canonical doc. This is the single acceptance-critical bit: the lens must *round-trip*
   (thesis §9), or the whole thesis fails.

---

## Part B — Stock-and-flow lens (the first concrete lens)

A `stockFlowLens: LensSpec` plus the pieces that can't be pure config:

- **Palette:** Stocks (Stock), Flows (Flow, Auxiliary, Converter) — relabels of existing
  `stock`/`expression`/`lag` entries; `primaryVerbs` = "Add stock", "Draw flow".
- **Forrester glyphs:** stock = box, flow = valve-on-pipe with cloud source/sink, aux = circle,
  info link = thin arrow. New glyph renderers in `typeIcons.tsx` + `EditableCanvas.tsx`. A
  **draw-flow gesture** (create a rate expression bound as a stock inflow/outflow) is the one
  genuinely new canvas interaction — the rest of the visual-wiring gap
  (`FRONTEND_ASSESSMENT_2026-07.md` item 2) can stay deferred.
- **SFC invariants** (`lenses/stockFlow/invariants.ts`): conservation (every flow leaves one
  place and enters another) and reconciliation (no stock leaks) — the Godley-Lavoie accounting
  invariants, computed over `summary` + `doc.elements` inflows/outflows. These emit `Issue`s that
  surface the governance claim *"guaranteed stock-flow consistent"* live in the status bar.
- **Trajectory-first result preset:** on run, open Results (`ResultsTab.tsx`) on the stock's
  fan-chart trajectory — mandated by the bathtub-dynamics finding (thesis §6): simulate and show
  accumulation, never present the stock symbolically.
- **Templates:** bathtub, SIR epidemic, inventory-under-noise (`lenses/stockFlow/templates.ts`).
- **Round-trip acceptance test:** author → save (`view.lens: "stock-flow"` + per-element
  `lens_role`) → reopen → tool re-enters the lens, re-runs SFC, shows the same green assertion.

---

## Part C — VOI engine reduction (workplan for the Decision/VOI follow-on)

The Decision/VOI *authoring* is almost entirely reuse. The engine already has everything except
VOI itself:
- `optimize()` (`optimize_v2.rs:454`) computes `max_d E_θ[obj(d,θ)]` — the optimal expected
  objective — via Box's complex over `evaluate_point`.
- `model.optimization` (`model.rs:151`) is a real, persistable field. So the Decision lens
  authors it **directly**: decision variables become `optimization.variables` entries and the
  objective becomes `optimization.objective`, all persisted (today the FE injects them per run
  via `optimize_json`, `wasm.rs:186`, and discards them — the lens simply *keeps* them and tags
  the referenced elements `lens_role: "decision" | "objective" | "chance"` for the
  influence-diagram view).

The one genuinely new engine capability is **VOI**: the objective delta across information
structures.

**The new reduction (EVPI / EVPPI).** Definitions:
- **EVPPI(X)** (partial info on one uncertainty X — what the mockup's "VOI probe" tag marks, and
  the common case) `= E_X[ max_d E_{θ|X}[ obj(d, θ) ] ] − max_d E_θ[ obj(d, θ) ]`.
- **EVPI** (perfect info on all uncertainties) = the same with the outer expectation over the
  full joint θ and the inner objective deterministic.
- The **second term is exactly `optimize()` today** (statistic = Mean). The **first term** is an
  outer expectation over scenarios of X, each requiring an **inner `optimize()`** with X pinned.

**What must be built (small, composes existing pieces):**
1. **`resolve_uncertainty(model, id, value)`** — a new sibling of `set_variable`
   (`eval_harness.rs:26`) that pins a **Sample** node to a scenario value for the inner run
   (temporarily treat it as fixed). `set_variable` explicitly rejects Sample nodes
   (`eval_harness.rs:33`), which is *why* this is the one new primitive. Small and local.
2. **Scenario draw for X** — sample K scenario values of X from its distribution using the
   existing seeded ChaCha8 stream (reproducibility is preserved; reuse `sampling.rs`). K is a
   bounded, logged parameter — **never silently truncate** (log the cap in results).
3. **`value_of_information(model, probe_ids, config)`** in a new `engine/src/voi_v2.rs`:
   baseline `= optimize(model)`; for each probe X, for each of K scenarios: `resolve_uncertainty`
   then `optimize()` on the remaining problem, average the optimized objectives, subtract
   baseline → `EVPPI(X)`. Returns `{ baseline_objective, per_probe: [{ element_id, evppi }] }`.
   Reuses `optimize` and `evaluate_point` wholesale.
4. **Schema:** additive `optimization.voi_probes: [element_id]` (optional; `#[serde(default)]`,
   non-breaking like everything else).

**Bridge + FE:** a `voi_json` WASM entry mirroring `optimize_json` (`wasm.rs:182`); a `run_voi`
worker message (`worker/protocol.ts:28` pattern); a `runVoi` store action mirroring
`runOptimization` (`store.ts:496`); and only *then* flip the frontend Decision lens from
mockup to real. Until then, the Decision tab in the mockup documents the target.

**Cost note.** EVPPI is `K × (optimize evaluations)` model runs — genuinely expensive. Ship it
with an explicit K (e.g. 64) surfaced in the UI and logged in results, and gate it behind an
explicit "compute VOI" action (never on the reconcile path).

---

## Part D — Reliability / RBD lens (second concrete lens; **no new engine work**)

This is the thesis's "prove the pattern round-trips, then the second lens is incremental" claim,
made concrete. Unlike Decision/VOI, reliability needs **zero engine additions** — every semantic
is already present:

- **Failure/repair FSM** is native *and already authorable* — the `Event` primitive with
  `FailureProcess` (`model_v2.rs:396,440`) and the FE `EventEditor` (trigger/basis/repair
  pickers) ship today. The lens **relabels** it into RAM language (time-to-failure, repair
  policy, redundancy role) via `inspectorLabels`.
- **Series / parallel / k-of-n redundancy** is native via the `gate` primitive —
  `GateNode::{And, Or, Not, NVote{threshold, children}}` (`model_v2.rs:480`). `And` = series,
  `Or` = parallel, `NVote{threshold=k}` = k-of-n. This is a **real engine primitive**, not an
  authored expression — the mockup's redundancy blocks map straight onto it.
- **Availability / MTBF / MTTR** are *derived in-model* (not native reductions): availability =
  `masked_mean` of per-component operating status; MTBF/MTTR from event timing; tail metrics like
  *P(availability < X% for > Y days)* via `results_spec` CCDF/exceedance (`results_spec.rs:142`).
  The proven RAM arc computes exactly these (`STAGE3_FLEET_SCOPE.md:64`, `STAGE1_RAM_DEMO.md:37`).

**A `reliabilityLens: LensSpec` plus the pieces that can't be pure config:**

- **Palette:** Components (Component, Series block), Reliability (Failure mode, Repair policy,
  k-of-n / redundancy gate) — relabels of the existing `event`, `gate`, and `stock` (accumulated
  damage) palette entries.
- **RBD glyphs:** component blocks, parallel branches, redundancy — the gate `And`/`Or`/`NVote`
  render as series/parallel/k-of-n topology (mockup already shows the target visual).
- **The one real FE gap — a structured gate/redundancy editor.** The `gate` primitive is
  engine-complete but **raw-JSON-only in the FE today** (`FRONTEND_ASSESSMENT_2026-07.md` item 1
  lists GateLogic/Gate as lacking a structured editor). Building an And/Or/NVote redundancy
  editor is the reliability analog of stock-flow's draw-flow gesture — the lens's one genuinely
  new authoring surface. Everything else is relabel + glyph.
- **Invariants (FE):** every component has a failure mode; every gate child resolves to a real
  element; redundancy well-formed (`NVote.threshold ≤ children.len()`).
- **Result preset:** availability / MTBF / MTTR readouts + a failure-count histogram + a tail
  card (P(avail < X%)), all riding `results_spec` CCDF.
- **Templates:** single repairable component; k-of-n redundant system; the **single-truck RAM
  model** (already proven end-to-end on the current engine — `STAGE1_RAM_DEMO.md`).

**Lean into the differentiator (don't fake static RBD).** WaSim reliability is **simulate-first**
— event FSMs + damage stocks + Monte Carlo — not the static availability arithmetic of classical
RBD tools. The fleet-availability *feedback loop* (`HAUL_FLEET_MODEL_SPEC.md:64`) is a hybrid
SD+reliability structure the RBD incumbents structurally cannot express. The lens should present
familiar RBD notation but compute by simulation (thesis §5–6). That is the wedge against GoldSim,
not a reimplementation of it.

**Optional, deferrable engine nicety:** native `availability`/`mtbf`/`mttr` reductions in
`results_spec` (so a template doesn't have to wire `masked_mean` by hand). Small, additive, and
explicitly *not required* to ship the lens — the reliability counterpart to VOI, but a
convenience rather than a capability gap.

---

## Sequencing

1. **A1–A3, A8** — LensSpec + store `activeLens` + `view.lens`/`lens_role` + palette driven by
   spec. Ships as a no-op refactor (general lens = today). *Smallest safe first PR.*
2. **A4–A7** — inspector relabel, glyph routing, lens invariants → status bar, lens picker.
3. **Part B** — stock-and-flow lens end to end (the product-defining milestone).
4. **Part D** — reliability lens: `LensSpec` relabel + RBD glyphs + the structured gate/redundancy
   editor + RAM templates. **No engine work**, so it can land right after the machinery proves
   out — it's the cheapest way to prove "second lens = spec file" and open the RAM vertical.
5. **Part C** — `resolve_uncertainty` primitive → `voi_v2.rs` reduction (`cargo test` against a
   hand-computed EVPPI) → `voi_json` bridge + `run_voi` → flip the real Decision lens on. The
   engine confirmation is already done (Part C is fully cited), so this starts at code, not a
   spike. Sequenced last because it's the only part with a hard engine dependency.

---

## Verification

- **Machinery (e2e, extend `frontend/e2e/authoring.spec.ts`):** pick a lens → palette sections
  swap; general lens renders identically to pre-change (snapshot).
- **Stock-flow (e2e):** build bathtub (stock + inflow) → SFC invariant fires on an unbalanced
  flow, then clears when sourced; **save → reopen round-trips the lens** (the acceptance test);
  run → Results opens on the trajectory fan chart.
- **Reliability (e2e):** open the proven single-truck RAM model → it enters the reliability lens
  (tagged `view.lens`), the FSM inspector reads in RAM language, availability/MTBF readouts
  render; build a 1-of-2 redundancy gate in the structured editor → the `NVote.threshold ≤
  children` invariant fires when misconfigured, clears when valid; save → reopen round-trips.
- **VOI (engine):** `cargo test` in `engine/` alongside the existing `solve_tests`
  (`optimize_v2.rs:425`) — a golden 1-decision / 1-uncertainty model (e.g. newsvendor) whose
  EVPPI is analytically known; assert `evppi ≥ 0` (a hard invariant — information never hurts)
  and matches to tolerance. Then a FE e2e that runs `run_voi` and shows a non-negative number.
- **Non-breaking check:** load every existing `schema_examples*` model with no `view.lens` →
  opens in the general lens, unchanged; `lens_role` absent is tolerated everywhere.

---

## Risks

- **Lens leaks the substrate → thesis fails.** Mitigation: `lens_role` round-trip + the reopen
  test as a hard CI gate; the general lens is the only place substrate vocabulary appears.
- **SFC-check cost on large models.** Run lens invariants on the same debounce as reconcile
  (`store.ts:22` `RECONCILE_DEBOUNCE_MS`), not per keystroke.
- **VOI is genuinely expensive** (`K × optimize`). Keep it behind an explicit action, surface K,
  and log any scenario cap — never let it near the reconcile path.
- **VOI scope creep.** EVPI/EVPPI only; full decision-tree / sequential-information value is out
  of scope. Log any sampling/scenario caps explicitly (don't silently truncate).

---

*Provenance: all facts — frontend and engine — verified by direct read of the cited files
(`engine/src/optimize_v2.rs`, `eval_harness.rs`, `model.rs`, `model_v2.rs`, `wasm.rs`,
`v2_parse.rs`, `results_spec.rs`, the RAM/fleet scope docs, and the `frontend/src/` surfaces).
No claim in this plan is unverified.*
