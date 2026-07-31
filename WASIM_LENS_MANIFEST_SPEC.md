# WaSim Lens & Palette Manifests — Specification & Plan

**Status:** spec + plan (no implementation).
**Extends:** [`WASIM_LENS_UI_DESIGN.md`](WASIM_LENS_UI_DESIGN.md) (why lenses),
[`WASIM_LENS_IMPLEMENTATION_PLAN.md`](WASIM_LENS_IMPLEMENTATION_PLAN.md) (how the shipped lenses were
built). This document specifies the *next* architectural step: moving the **declarative** surface of
lenses (palette taxonomy, per-element relabeling, glyphs, roles, templates) out of TypeScript literals
and into **diffable JSON manifests** over a canonical **element registry**, while the **behavioral**
surface (validation invariants, canvas gestures, result readouts) stays as first-party code plugins
keyed by lens id. It also specifies a **browsable function/expression reference** for the authoring UI.

---

## 0. Summary

- Introduce two JSON artifacts — an **element registry** (the canonical catalog of every buildable
  WaSim construct) and per-lens **lens manifests** (a projection over that registry with re-theming
  and re-grouping). A **function registry** does the same for the expression language.
- Keep the imperative parts of a lens (invariants, `connect`, `resultReadouts`) as **code plugins**
  registered by lens id — the standard "declarative manifest + code contribution points" pattern
  (VS Code, Grafana, ESLint).
- The **General lens** becomes the *identity projection*: it surfaces **all** engine constructs in the
  authoring UI, grouped by a functional taxonomy (§5).
- Add a **Functions reference** panel so the expression language's ~58 builtins are discoverable, not
  just autocomplete-on-type.
- Net effect: lenses (and the palette layout, and the function list) become **user-customizable,
  shareable, diffable JSON** — extending WaSim's models-as-data thesis to the authoring surface
  itself — without touching the engine and without a rules DSL.

---

## 1. Motivation & current state

Today a lens is a `LensSpec` object (`frontend/src/lenses/types.ts`) that mixes data and code:

| Field | Kind | Serializable? |
|---|---|---|
| `id`, `label`, `tagline` | data | ✅ |
| `palette: (all) => PaletteGroup[]` | fn (but pure selection/relabel) | ✅ (as data) |
| `roleLabels: Record<role,string>` | data | ✅ |
| `glyphOf: (role) => NodeShape` | fn (but a lookup) | ✅ (as a map) |
| `templates: ModelTemplate[]` | data + `build()` | ✅ (as embedded/`ref`'d docs) |
| `preferredResultId: (doc, ids) => id` | fn (small rule) | ✅ (as `roleOrder`) |
| `invariants: (summary, doc) => Issue[]` | **code** | ❌ |
| `connect: (doc, from, to) => doc` | **code** | ❌ |
| `resultReadouts: (results, doc) => Readout[]` | **code** | ❌ |

The palette itself is hand-coded in two places: the entry catalog `PALETTE` in
`frontend/src/model/edits.ts` (9 entries) and each lens's palette-projection arrays in
`frontend/src/lenses/registry.ts`. The expression builtins live in `BUILTINS`
(`frontend/src/model/ast.ts`, ~58 entries grouped Math/Trig/Array/Finance/Calendar/Events) but are
only surfaced as type-to-filter autocomplete — there is no browsable list.

**The load-bearing distinction** (and the reason this is a hybrid, not "pure JSON"): the top half of
the table above is presentation and parameterizes cleanly; the bottom half is behavior — graph
traversal, arithmetic, doc transforms — and does **not** reduce to static JSON without inventing a
rules/predicate DSL. The most *valuable* part of a lens (checkable governance: "guaranteed
stock-flow-consistent") is exactly the part that resists serialization. So this spec keeps behavior in
code and parameterizes only presentation. **Non-goal:** an invariant/expression DSL (see §11).

---

## 2. Design principles

1. **Declarative manifest + code plugin.** A manifest owns presentation; a code plugin (keyed by a
   `behavior` id) owns invariants/gestures/readouts. A manifest with no `behavior` is a fully
   declarative, user-authorable lens (relabel/regroup/reglyph only — the common case).
2. **Registry is the single source of truth for "what can this engine build."** Every palette item
   in every manifest is a `ref` into the registry; a dangling ref is a load error.
3. **General = identity projection.** No special-casing: "no lens" is the widest lens, and it lists
   every registry entry.
4. **Diffable, git-native, mergeable.** Manifests are JSON reviewed like models. Precedence is
   layered: built-in defaults → project overlay → user overrides (§7).
5. **Engine untouched; additive; non-breaking.** No schema change to the model beyond the already-
   shipped engine-ignored `view.lens` / `lens_role` tags. The manifest system is a frontend concern.
6. **Safe by construction.** Manifests are data — safe to load from a project or user. Behavior stays
   first-party code; user-supplied manifests can never execute code, only re-theme.
7. **Backward-compatible.** The loader must reproduce the four shipped lenses byte-for-byte in
   behavior (a parity acceptance test, §12).

---

## 3. Architecture

```
                 ┌─────────────────────┐
 element-        │  Element Registry   │  canonical catalog of constructs
 registry.json ─▶│  (built-in + user)  │  key → construct, glyph, scaffold, editor, section
                 └─────────┬───────────┘
                           │ ref
 lenses/*.json ──▶ ┌───────▼──────────┐        ┌──────────────────────────┐
 (built-in +       │  Lens Manifest   │        │  Behavior Plugin Registry │  code, keyed by
  project + user)  │  projection +    │◀──behavior──│ invariants / connect /   │  behavior id
                   │  re-theming      │  id    │  resultReadouts           │
                   └───────┬──────────┘        └──────────────────────────┘
                           │  build()
                   ┌───────▼──────────┐
                   │  LensSpec (live) │  the existing runtime object, now assembled from
                   │                  │  manifest (data) + plugin (code) instead of hand-written
                   └──────────────────┘
```

The runtime `LensSpec` interface is **unchanged**. What changes is its *source*: a **loader** reads
the registry + manifests, resolves `ref`s and overrides, attaches the named behavior plugin, and emits
the same `LensSpec` the app already consumes (`useActiveLens()`, `Palette.tsx`, `StatusBar.tsx`,
`EditableCanvas.tsx`, `ResultsTab.tsx`, `OptimizationTab.tsx`). This keeps the blast radius tiny — the
consumers don't move.

A parallel path does the same for the expression language: a **function registry** (`functions.json`)
feeds a new Functions reference UI and the existing autocomplete.

---

## 4. Element Registry schema

The registry is an ordered list of **construct entries** — the things a palette can insert. It is the
General-lens superset; every lens references a subset.

```jsonc
// element-registry.json
{
  "version": "1.0.0",
  "sections": [                       // default section order + collapse state (General uses these)
    { "id": "inputs",     "label": "Inputs" },
    { "id": "functions",  "label": "Functions" },
    { "id": "accumulate", "label": "Accumulation & delay" },
    { "id": "events",     "label": "Events & switches" },
    { "id": "processes",  "label": "Stochastic processes", "advanced": true },
    { "id": "transport",  "label": "Resources & transport", "advanced": true }
  ],
  "entries": [
    {
      "key": "stock",                 // stable id referenced by manifests
      "section": "accumulate",        // default section (a lens may re-section)
      "label": "Stock / Reservoir",   // default label (a lens may relabel)
      "construct": "stock",           // engine primitive or node value_rule this builds
      "glyph": "box",                 // default NodeShape
      "editor": "structured",         // "structured" | "raw" — inspector support today
      "scaffold": { "primitive": "stock", "initial_value": { "value": 0, "unit": "1" },
                    "inflows": [], "outflows": [] },
      "doc": "Accumulates its inflows minus outflows each step (an integrator)."
    }
    // … one per construct (full catalog in §5)
  ]
}
```

**Entry fields**

| Field | Req | Meaning |
|---|---|---|
| `key` | ✓ | Stable id; manifests reference this. |
| `construct` | ✓ | Engine target: a `Primitive` (`stock`, `event`, `gate`, `link`, `cell`, `species`, `medium`, `resource`) or a node `value_rule` (`fixed`, `sample`, `expression`, …). |
| `section` | ✓ | Default section id (from `sections`). |
| `label` | ✓ | Default palette label. |
| `glyph` |  | Default `NodeShape` (`box`\|`circle`\|`valve`\|`hex`\|`default`). |
| `editor` | ✓ | `structured` (a real inspector editor exists) or `raw` (edited via the JSON escape hatch until an editor is built). Drives a "raw-JSON" hint in the UI. |
| `scaffold` | ✓ | The default element JSON inserted on click; must reconcile to a valid model. |
| `doc` |  | One-line description (shown as a tooltip / in a registry reference). |

**Validation:** every manifest `ref` must resolve to an `entry.key`; every `entry.construct` must be a
known engine construct; `editor: "structured"` must correspond to a real inspector case
(`Inspector.tsx` currently: fixed, sample, expression, filter, lag, lookup, series, stock, event, gate).

---

## 5. The General lens catalog (full palette)

The registry ships all insertable constructs, grouped by the functional taxonomy. This is what the
**General lens surfaces in full** (advanced sections collapsed by default). "Editor" marks whether a
structured inspector exists today or the construct scaffolds to raw-JSON editing.

| Section | Item (label) | `construct` | glyph | Editor |
|---|---|---|---|---|
| **Inputs** | Constant | `fixed` | default | structured |
| | Stochastic | `sample` | circle | structured |
| | Time Series | `series` | default | structured |
| | Lookup Table | `lookup` | default | structured |
| **Functions** | Expression | `expression` | default | structured |
| | Logic Gate | `gate` | box | structured |
| | Smoothing / Filter | `filter` | default | structured |
| | PID Controller | `pid` | default | raw |
| | Terminal Value | `terminal_expression` | default | raw |
| **Accumulation & delay** | Stock / Reservoir | `stock` | box | structured |
| | Previous Value (delay) | `lag` | default | structured |
| | Convolution | `convolution` | default | raw |
| | Queue | `queue` | default | raw |
| **Events & switches** | Failure / Event | `event` | box | structured |
| | Status (latch) | `status` | default | raw |
| | Milestone | `milestone` | default | raw |
| | Hysteresis | `hysteresis` | default | raw |
| **Stochastic processes** *(advanced)* | Stochastic Process (GBM/OU) | `process` | default | raw |
| | Markov Chain | `markov` | default | raw |
| **Resources & transport** *(advanced)* | Resource | `resource` | box | raw |
| | Cell | `cell` | box | raw |
| | Species | `species` | circle | raw |
| | Medium | `medium` | default | raw |
| | Link (transfer) | `link` | valve | raw |

Notes:
- `gate` (primitive, has the structured editor) is the surfaced "Logic Gate"; the node-rule
  `gate_logic` encoding is **not** in the palette — it stays reachable only when a loaded model already
  contains it.
- 10 of 24 constructs have structured editors today; the other 14 scaffold to valid raw-JSON-editable
  elements. Building structured editors for them is out of scope here (Phase 7, §10) — but surfacing
  them in the palette is not blocked on those editors.

---

## 6. Lens Manifest schema

A manifest is a **projection** over the registry plus re-theming.

```jsonc
// lenses/stock-flow.json
{
  "version": "1.0.0",
  "id": "stock-flow",
  "label": "Stock & Flow",
  "tagline": "Forrester stock-and-flow — stocks accumulate, flows are rates, guaranteed consistent.",
  "extends": "general",              // optional: inherit sections/entries then override (default: none)
  "behavior": "stock-flow",          // optional: id of the code plugin (invariants/connect/readouts)
  "palette": [
    { "section": "Stocks", "items": [
        { "ref": "stock", "label": "Stock", "role": "stock", "glyph": "box" } ] },
    { "section": "Flows", "items": [
        { "ref": "expression", "label": "Flow", "role": "flow", "glyph": "valve" } ] },
    { "section": "Auxiliaries", "items": [
        { "ref": "expression", "label": "Auxiliary", "role": "auxiliary" },
        { "ref": "constant",   "label": "Constant",  "role": "auxiliary" },
        { "ref": "sample",     "label": "Uncertain input", "role": "auxiliary" } ] }
  ],
  "roleLabels": { "stock": "Stock", "flow": "Flow", "auxiliary": "Auxiliary" },
  "glyphByRole": { "stock": "box", "auxiliary": "circle", "flow": "valve" },
  "templates": ["bathtub", "population"],     // ids into a template registry (or inline docs)
  "preferredResult": { "roleOrder": ["stock"] }
}
```

**Palette item override fields** (each overlays the referenced registry entry for this lens):

| Field | Req | Meaning |
|---|---|---|
| `ref` | ✓ | A registry `entry.key` (which `make()`/scaffold to use). |
| `label` |  | Lens-facing label (defaults to the entry's label). |
| `role` |  | Value stamped onto the inserted element's `lens_role` (round-trip tag). |
| `glyph` |  | Canvas glyph override for this item's role. |
| `advanced` |  | Collapse this item's section by default. |

**Manifest top-level fields**

| Field | Req | Meaning |
|---|---|---|
| `id` | ✓ | Lens id (persisted in `view.lens`; resolves the active lens). |
| `label`, `tagline` | ✓/– | Picker chip + description. |
| `extends` |  | Base manifest id to inherit and override (usually `general`). |
| `behavior` |  | Code-plugin id (§7). Absent ⇒ a purely declarative lens. |
| `palette` | ✓ | Sections → items (the projection). |
| `roleLabels` |  | `lens_role` → inspector header label. |
| `glyphByRole` |  | `lens_role` → `NodeShape` (feeds the runtime `glyphOf`). |
| `inspectorLabels` |  | *(future)* per-role field relabels for the definition editors. |
| `templates` |  | Template ids (or inline `ModelDoc`s). |
| `preferredResult.roleOrder` |  | Ordered roles; the first whose element is an output is opened in Results. |

The loader compiles a manifest into a runtime `LensSpec`: `palette` → a `(all) => PaletteGroup[]`
projection; `glyphByRole` → `glyphOf`; `preferredResult.roleOrder` → `preferredResultId`;
`roleLabels`/`templates` pass through; and `behavior` attaches the plugin's `invariants` / `connect` /
`resultReadouts`.

---

## 7. Behavior plugins (code) & load/merge semantics

### 7.1 Behavior plugin contract

Behavior stays code, registered by id in `frontend/src/lenses/behaviors/`:

```ts
export interface LensBehavior {
  id: string                                                   // matches manifest.behavior
  invariants?: (summary: ModelSummary, doc: ModelDoc) => Issue[]
  connect?: (doc: ModelDoc, fromId: string, toId: string) => ModelDoc | null
  resultReadouts?: (results: SimulationResults, doc: ModelDoc) => LensReadout[]
}
```

Built-in behaviors: `stock-flow` (SFC invariants + draw-flow connect), `reliability` (component/gate
invariants + availability/MTTF readouts), `decision` (needs-decision/objective invariants + objective
readout). `general` has no behavior. These are the functions that already exist in `registry.ts`,
lifted verbatim into named plugins.

### 7.2 Load order & precedence

Sources, lowest to highest precedence:
1. **Built-in** — the four shipped lenses + the registry, bundled with the app.
2. **Project** — `.wasim/lenses/*.json` and `.wasim/element-registry.json` in the repo, so a team's
   lens config is versioned with its models.
3. **User** — a per-user overrides directory (editor settings).

Merge is by `id`: a later manifest with the same `id` overrides (deep-merge of palette sections and
role maps; behavior id from the highest layer that sets it). Registry entries merge by `key`. A
project may add new lenses/entries or re-theme built-in ones, but **cannot** supply new behavior code
(only reference an existing `behavior` id) — the trust boundary.

### 7.3 Validation & versioning

- Each artifact carries a `version`; the loader validates against a bundled JSON Schema and applies
  migrations (parallel to the model schema's `schema/CHANGELOG.md`).
- Load-time checks: every `ref` resolves; every `construct` is engine-known; every `behavior` resolves
  to a registered plugin; `glyph` values are valid `NodeShape`s. Failures are surfaced (a load error
  with the offending manifest/field), never silently dropped.

---

## 8. The four current lenses as manifests

The shipped lenses re-expressed declaratively (behavior lifted to plugins). Abridged; General is full.

**general.json** — identity projection over the registry (all entries, default sections). No behavior.
```jsonc
{ "id": "general", "label": "General",
  "tagline": "The full engine, unfiltered — every element type, no domain vocabulary.",
  "palette": "@registry" }        // sentinel: use the registry's sections/entries verbatim
```

**stock-flow.json** — §6 example above. `behavior: "stock-flow"`.

**reliability.json**
```jsonc
{ "id": "reliability", "label": "Reliability", "behavior": "reliability",
  "tagline": "Repairable components and failure FSMs — simulate-first RAM, not static block arithmetic.",
  "palette": [
    { "section": "Components",  "items": [{ "ref": "event", "label": "Component", "role": "component" }] },
    { "section": "Redundancy",  "items": [{ "ref": "gate",  "label": "Redundancy gate", "role": "redundancy" }] },
    { "section": "State",       "items": [{ "ref": "stock", "label": "Damage state", "role": "state" }] },
    { "section": "Inputs",      "items": [
        { "ref": "constant",   "label": "Parameter",       "role": "parameter" },
        { "ref": "sample",     "label": "Uncertain input", "role": "parameter" } ] } ],
  "roleLabels": { "component": "Component", "redundancy": "Redundancy", "state": "Damage state", "parameter": "Parameter" },
  "glyphByRole": { "component": "box", "state": "box", "redundancy": "box" },
  "templates": ["run-to-failure", "redundant"],
  "preferredResult": { "roleOrder": ["redundancy", "component"] } }
```

**decision.json**
```jsonc
{ "id": "decision", "label": "Decision", "behavior": "decision",
  "tagline": "Decisions, chance inputs, and an objective — optimize under uncertainty and price information (VOI).",
  "palette": [
    { "section": "Decisions",   "items": [{ "ref": "constant",   "label": "Decision",     "role": "decision" }] },
    { "section": "Uncertainty", "items": [{ "ref": "stochastic", "label": "Chance input", "role": "chance" }] },
    { "section": "Objective",   "items": [{ "ref": "expression", "label": "Objective",    "role": "objective" }] } ],
  "roleLabels": { "decision": "Decision", "chance": "Chance input", "objective": "Objective" },
  "glyphByRole": { "decision": "box", "chance": "circle", "objective": "hex" },
  "templates": ["capacity-choice"],
  "preferredResult": { "roleOrder": ["objective"] } }
```

Note how each shipped lens's palette array in `registry.ts` maps 1:1 onto a manifest — the port is
mechanical.

---

## 9. Function / Expression reference

### 9.1 Problem

Builtins (`BUILTINS` in `ast.ts`: ~58, grouped Array 20 / Math 18 / Trig 10 / Calendar 6 / Events 2 /
Finance 2, plus `TIME_REFS`) are surfaced only as type-to-filter autocomplete
(`ExpressionEditor.tsx`), so a user who doesn't already know a function's name cannot discover it.

### 9.2 Proposal — a function registry + a reference surface

- **`functions.json`** (parallel to the element registry): each entry `{ name, sig, group,
  doc, example? }`. Source of truth for both the autocomplete and the new reference; keeps the
  function catalog diffable and documentable. `ast.ts`'s parser keeps its own arity/precedence tables
  (that's grammar, not catalog).
- **A "Functions" reference panel** — a third left-pane tab beside Browser/Palette (or a disclosure in
  the expression editor): all functions grouped by category, each showing `sig` + `doc`, with
  **click-to-insert** into the focused expression. Includes the Time refs group.
- **Discoverable autocomplete** — when the "insert reference / function…" box is focused but empty,
  show a few suggestions (recent + common) instead of nothing.
- **Optional (future):** a lens may expose a **function subset** (`manifest.functions: ["min","max",…]`)
  so a domain lens can hide finance/array builtins it doesn't need — the same projection idea applied
  to the expression language. Off by default (all functions visible).

---

## 10. Implementation plan (phased)

Each phase is independently shippable; parity with today is the gate between phases.

- **P1 — Schemas.** Author the JSON Schemas for the element registry, the lens manifest, and the
  function registry (under `schema/`). No behavior change. *Deliverable:* schemas + validation.
- **P2 — Element registry + full General palette.** Port the 9 `PALETTE` entries into
  `element-registry.json`; add scaffolds for the ~14 missing constructs (each verified to reconcile);
  regroup into the §5 taxonomy; General surfaces all, advanced sections collapsed. Missing-editor
  constructs route to the raw-JSON escape hatch with a hint. *Gate:* general lens builds every
  construct; existing lenses unchanged.
- **P3 — Manifest loader + behavior plugins.** Lift `invariants`/`connect`/`resultReadouts` from
  `registry.ts` into `behaviors/`; build the loader that assembles a `LensSpec` from manifest + plugin;
  port the four lenses to manifests. *Gate:* the full e2e suite (`e2e/lens.spec.ts`) passes unchanged —
  behavioral parity.
- **P4 — Palette UI for sections.** Collapse carets for `advanced` sections; the raw-JSON-editor hint
  on `editor: "raw"` items.
- **P5 — Function reference.** `functions.json` + the Functions panel + focused-empty autocomplete.
- **P6 — User/project overlays.** Load `.wasim/lenses/*.json` and user overrides; merge/precedence;
  a settings surface to pick/reorder. *Deliverable:* a user can ship a custom relabel lens with no code.
- **P7 — (Follow-on, not this effort) structured editors** for the newly-surfaced constructs
  (`process`, `pid`, `queue`, `status`, `milestone`, `hysteresis`, `convolution`, `terminal_expression`,
  and the `cell`/`species`/`medium`/`link`/`resource` family), promoting them from `raw` to
  `structured` one at a time.

---

## 11. Risks, boundaries & non-goals

- **No invariant/expression DSL.** Behavior stays code behind a `behavior` id. If limited declarative
  rules are ever wanted, add a *closed* vocabulary (`requiresRole`, `refMustResolve`, `countBounds`) —
  never a general expression language. This is the primary discipline the whole design depends on.
- **Two schemas, one sync point.** Manifests reference engine constructs; when the engine gains one,
  the registry must learn it. Enforced by load-time ref/construct validation, not convention.
- **Trust.** Manifests are pure data — safe from any source. User-supplied lenses can re-theme but
  never execute code (they may only *name* a built-in `behavior`).
- **Editor coverage is orthogonal.** Surfacing a construct in the palette does not require a structured
  editor; it requires a valid scaffold + the raw-JSON fallback. Don't couple P2 to P7.
- **Non-goals:** engine changes; a template-authoring UI; per-user cloud sync of lenses.

---

## 12. Verification / acceptance

- **Parity:** with the loader active, each shipped lens renders and validates identically to today —
  the existing `frontend/e2e/lens.spec.ts` (9 tests) passes unchanged.
- **General completeness:** the General lens palette lists all registry constructs; inserting each
  scaffolds an element that reconciles to a valid model (a generated test iterating the registry).
- **Round-trip:** a model authored under a manifest lens saves `view.lens` + `lens_role` and re-opens
  in that lens (already covered; unchanged).
- **Customization:** a project `.wasim/lenses/my-lens.json` (relabel-only, no `behavior`) loads,
  appears in the picker, reprograms the palette, and round-trips — with zero TypeScript changes.
- **Functions:** the Functions panel lists every `functions.json` entry grouped by category and
  click-inserts into an expression; a bad `ref` in a manifest fails the load with a clear error.

---

## References

- Runtime surface: `frontend/src/lenses/{types.ts,registry.ts}`, `frontend/src/model/edits.ts`
  (`PALETTE`), `frontend/src/model/ast.ts` (`BUILTINS`, `TIME_REFS`),
  `frontend/src/components/{browser/Palette.tsx, inspector/{Inspector.tsx,ExpressionEditor.tsx},
  shell/StatusBar.tsx, canvas/EditableCanvas.tsx, tabs/{ResultsTab.tsx,OptimizationTab.tsx}}`.
- Engine constructs: `engine/src/model_v2.rs` (`NodeRule`, `Primitive`).
- Prior lens docs: `WASIM_LENS_UI_DESIGN.md`, `WASIM_LENS_IMPLEMENTATION_PLAN.md`,
  `WASIM_VALUE_PROP_THESIS.md`, `FRONTEND_ASSESSMENT_2026-07.md`.
- Pattern precedent: VS Code contribution points (declarative `package.json` + activation code),
  Grafana panel plugins, ESLint config + plugins.
