# WaSim Lens System — Implementation Plan

**Companion to** [`WASIM_VALUE_PROP_THESIS.md`](WASIM_VALUE_PROP_THESIS.md) (the *why*) and
[`WASIM_LENS_UI_DESIGN.md`](WASIM_LENS_UI_DESIGN.md) (the *what it looks/behaves like*). This
document is the *how to build it*.

**Scope (agreed):** the reusable **LensSpec machinery** with **stock-and-flow as the first
concrete lens** (the thesis's beachhead), plus the engine-side **value-of-information (VOI)
reduction** scoped as its own workplan for the **Decision/VOI follow-on lens**. The frontend
Decision/VOI lens stays mockup-only until the VOI engine op lands.

> **Grounding note.** Every frontend claim below is cited to a file I read directly. The
> engine-internal specifics in Part C (exact `optimize_v2` location and signature, sweep
> plumbing) are reasoned from the worker protocol and the optimization UI, and are flagged
> **[confirm in `engine/src/`]** — verify them as step 0 of Part C rather than trusting this doc.

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
| Optimization already exists (`optimize_v2`, Box's complex) but the spec is **transient tab state, never persisted** | `types.ts:206`, `components/tabs/OptimizationTab.tsx:16-19`, `store.ts:496` `runOptimization` |
| Range-sweep already exists (`SensitivitySpec` over `SweepVar[]`) | `types.ts:174` |
| Reconcile round-trip: store → worker → summary + validation | `store.ts:419` `_scheduleReconcile`, `worker/protocol.ts:31` |

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
   change (already inside the free-form `view` block). **[confirm]** the engine ignores unknown
   element fields on parse — if it's strict, `lens_role` must live under `view` too. This is the
   single acceptance-critical bit: the lens must *round-trip* (thesis §9), or the whole thesis
   fails.

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

The Decision/VOI *authoring* is mostly reuse — `optimize_v2` already does the optimize half, and
decisions/objective just need to become **persisted, authored nouns** (`lens_role: "decision"` /
`"objective"`) instead of the transient `OptimizationSpec` (`types.ts:206`). The one genuinely
new engine capability is **VOI itself**: the objective delta across information structures.

**Step 0 — [confirm in `engine/src/`]:** locate `optimize_v2` (Box's complex), its input struct
(objective = element + statistic + direction; bounded decision variables), its WASM entry point
(`engine/src/wasm.rs`, reached via `worker` message `run_optimization`, `protocol.ts:28`), and
how a decision axis is swept (named-dimension array vs. an explicit sweep loop — see
`WORKPLAN_SWEEP_COMPOSITION.md`). Confirm **no** VOI/EVPI/EVPPI exists today.

**The new reduction (EVPI / EVPPI):**
- **EVPI** (expected value of *perfect* information) `= E_θ[ max_d objective(d, θ) ] − max_d
  E_θ[ objective(d, θ) ]` — the gain from choosing the decision *after* the uncertainty θ
  resolves, minus the best decision *under* uncertainty. The second term is exactly what
  `optimize_v2` computes today; the first term is an **outer expectation over θ of an inner
  optimize**, i.e. a sweep over realizations/scenarios of the uncertainty with an inner
  optimization each — buildable by composing the existing optimizer with the existing
  realization/sweep machinery.
- **EVPPI** (partial: information on one chosen uncertainty) = same shape, conditioning only on
  the probed variable (the mockup's "VOI probe" tag marks which uncertainty).
- **Deliverable:** a named engine op `value_of_information(spec)` returning
  `{ evpi, per_probe: { element_id, evppi }[] }`, plus a `StudyResults` extension.

**Bridge + FE:** new worker message `run_voi` (`worker/protocol.ts`), store action `runVoi`
mirroring `runOptimization` (`store.ts:496`), and only *then* flip the frontend Decision lens
from mockup to real. Until then, the Decision tab in the mockup documents the target.

---

## Sequencing

1. **A1–A3, A8** — LensSpec + store `activeLens` + `view.lens`/`lens_role` + palette driven by
   spec. Ships as a no-op refactor (general lens = today). *Smallest safe first PR.*
2. **A4–A7** — inspector relabel, glyph routing, lens invariants → status bar, lens picker.
3. **Part B** — stock-and-flow lens end to end (the product-defining milestone).
4. **Part C step 0** — engine confirmation spike; then the VOI reduction + bridge; then the real
   Decision lens.

---

## Verification

- **Machinery (e2e, extend `frontend/e2e/authoring.spec.ts`):** pick a lens → palette sections
  swap; general lens renders identically to pre-change (snapshot).
- **Stock-flow (e2e):** build bathtub (stock + inflow) → SFC invariant fires on an unbalanced
  flow, then clears when sourced; **save → reopen round-trips the lens** (the acceptance test);
  run → Results opens on the trajectory fan chart.
- **VOI (engine):** `cargo test` in `engine/` — a golden 2-decision / 1-uncertainty model with a
  hand-computed EVPI; assert `evpi ≥ 0` and matches to tolerance. Then a FE e2e that runs
  `run_voi` and shows a non-negative number.
- **Non-breaking check:** load every existing `schema_examples*` model with no `view.lens` →
  opens in the general lens, unchanged; `lens_role` absent is tolerated everywhere.

---

## Risks

- **Lens leaks the substrate → thesis fails.** Mitigation: `lens_role` round-trip + the reopen
  test as a hard CI gate; the general lens is the only place substrate vocabulary appears.
- **Schema strictness.** If the engine rejects unknown element fields, `lens_role` moves under
  the `view` block (per-id map) — still round-trips, slightly less local. Resolve in A8 step 0.
- **SFC-check cost on large models.** Run lens invariants on the same debounce as reconcile
  (`store.ts:22` `RECONCILE_DEBOUNCE_MS`), not per keystroke.
- **VOI scope creep.** EVPI/EVPPI only; full decision-tree / sequential-information value is out
  of scope. Log any sampling/scenario caps explicitly (don't silently truncate).

---

*Provenance: frontend facts verified by direct read of the cited files. Engine-internal
specifics in Part C are reasoned from `worker/protocol.ts`, `OptimizationTab.tsx`, and
`types.ts`; confirm against `engine/src/` before building Part C.*
