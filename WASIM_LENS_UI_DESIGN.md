# WaSim — Lens-Driven Authoring UI: Design & Behavior Plan

**What this is.** A design plan for the load-bearing UX claim in
[`WASIM_VALUE_PROP_THESIS.md`](WASIM_VALUE_PROP_THESIS.md): that WaSim's one general
engine is sold through **lenses** — thin, domain-specific authoring surfaces, each with a
pre-committed vocabulary and its own validation invariants. The thesis argues *why* lenses
are the go-to-market move. This document works out *how a lens actually reshapes the
authoring tool* — what the user sees and does differently when the tool is "in" the
stock-and-flow lens versus the reliability lens versus no lens at all — and pins each
behavior to the concrete frontend in [`frontend/`](frontend/).

**What this is not.** Not new engine work (the thesis is explicit: the calculus stays
general and hidden). Not a rewrite of the authoring spec — it extends
[`WASIM_AUTHORING_ENVIRONMENT_SPEC.md`](WASIM_AUTHORING_ENVIRONMENT_SPEC.md) with the one
axis that spec does not yet have: *the surfaces are parameterized by a lens.*

---

## 0. The one-sentence design

> **The shell never changes; the lens reprograms what fills it.** A lens is a declarative
> bundle — palette, glyphs, inspector forms, validation invariants, result presets,
> templates, copilot vocabulary — that swaps the *contents* of the existing three-pane
> workspace while the engine, the store, the reconcile loop, and the JSON on disk stay
> exactly what they are today.

Everything below is an elaboration of that sentence.

---

## 1. The operational test the UI must honor

The thesis's discriminator is the whole design constraint, restated as a UI rule:

> **Delete the visualization. If the *model you author* and *how it's validated* change,
> it was a lens. If only the picture changes, it was a view.**

This forces a clean split in the tool:

- **Lenses reprogram authoring + validation.** They change the palette nouns, the inspector
  forms, and the invariants the status bar enforces. Different lens ⇒ you can build a
  *different kind of model wrong* in a different way, and the tool catches it.
- **Views reprogram rendering only.** Sankey of flows, tornado of sensitivities, feedback-loop
  highlighting, "influence-diagram coloring" — all ride the existing DAG canvas and are **free
  and always available**, independent of the active lens. Ship all of them; none is a mode.

The practical consequence for this plan: **we design lens machinery, and we get views for
free.** A view is just a toggle on the results/canvas; it never touches the palette or the
validator. So the rest of this document is about the lens machinery.

---

## 2. Where the tool is today (the proto-lens already in the code)

The current frontend already contains an *accidental, half-built* lens system — which is the
best possible evidence the abstraction fits:

- **`frontend/src/model/edits.ts` → `PALETTE`** — every insertable element carries a
  `group: string` field. Today the groups are `Inputs`, `Functions`, `Stocks`, `Reliability`.
- **`frontend/src/components/browser/Palette.tsx`** renders one section per distinct `group`,
  all groups visible at once.
- **`frontend/src/store.ts`** owns the canonical `ModelDoc`, renders from the engine's
  `model_summary()` (it never parses the schema itself), and round-trips an engine-ignored
  **`view` block** for layout persistence.
- **`frontend/src/components/inspector/Inspector.tsx`** switches editor forms on element type,
  with a raw-JSON escape hatch for unsupported types.
- **`frontend/src/components/shell/StatusBar.tsx`** shows a live validation summary
  (`● valid / ⚠ N issues · topo OK · units: warn`).

This is a general tool showing *all* groups at once — which is precisely the "general tool
wearing three half-lenses" failure the thesis names. The fix is not more code per element; it
is a **selection layer** that decides which vocabulary is visible, how it's validated, and how
results are framed. That selection layer is the lens.

**Key architectural gift:** because the store already persists an engine-ignored `view` block
and renders from the engine summary, a lens needs **no engine change and no new source of
truth** — the active lens is a tag in `view`, and everything else is FE configuration.

---

## 3. What a lens *is*, as data (the `LensSpec`)

A lens must be **data, not a forked code path**, or we will never ship the second and third
one cheaply (which is the thesis's closing argument). Proposed shape, living in the FE
(`frontend/src/lenses/`):

```ts
interface LensSpec {
  id: string;                    // 'stock-flow' | 'reliability' | 'decision' | 'general'
  label: string;                 // "Stock & Flow"
  tagline: string;               // shown on the lens picker card
  // ---- PALETTE: which nouns the user authors, in this lens's words ----
  palette: PaletteGroup[];       // reuses today's PaletteEntry; relabeled + reordered
  primaryVerbs: string[];        // the 2-3 actions the toolbar promotes ("Add stock", "Draw flow")
  // ---- GLYPHS: how the canvas draws this lens's element roles ----
  glyphs: Record<Role, GlyphSpec>;   // stock = box; flow = valve-on-pipe; aux = circle
  edgeKinds: EdgeKindStyle[];        // flow-edge (double line) vs influence-edge (thin arrow)
  // ---- INSPECTOR: the forms, in domain language ----
  inspectorForms: Record<Role, FormSpec>;   // a "Stock" form, not a "lag accumulator" form
  // ---- VALIDATION: the invariants that survive deleting the diagram ----
  invariants: LensInvariant[];       // conservation, reconciliation, series/parallel wellformedness
  // ---- RESULTS: what "done" looks like in this domain ----
  resultPresets: ResultPreset[];     // SFC: stock trajectories first. RBD: availability/MTBF.
  defaultView: string;
  // ---- TEMPLATES: canonical starting points ----
  templates: ModelTemplate[];        // "SIR epidemic", "bathtub", "repairable component"
  // ---- COPILOT: the words the assistant speaks (Phase 4) ----
  copilotVocabulary: TermMap;        // maps domain terms → engine constructs for NL authoring
  // ---- ROUND-TRIP: how elements tag their role so re-open reconstructs the lens ----
  roleOf(el): Role | null;           // reads the schema role/lens annotation (see §7)
}
```

Every existing surface becomes a function of the active `LensSpec`. The shell reads
`activeLens` from the store and asks the spec what to show. **No lens = the `general` lens** —
a `LensSpec` whose palette is the full union of all groups (today's behavior, verbatim), so
"no lens selected" is not a special case, it's just the least-restricted spec.

---

## 4. How each surface shifts, lens by lens

This is the heart of "what it looks/behaves like." For each pane, what a lens reprograms:

### 4.1 The lens picker (new — the only genuinely new chrome)

- **On new model:** a first-run card chooser — "What are you modeling?" → *Stock & Flow ·
  Reliability · Decision analysis · General (advanced).* Picking one loads that spec.
- **On open:** the lens is read from the model's `view.lens` tag (§7); the tool opens already
  wearing it. No prompt.
- **Live switch:** a lens dropdown in the toolbar (where "Palette ▾" is now). Switching is
  **non-destructive and reversible** — it re-skins the same model. Switching *down* to
  `general` always works. Switching *up* into a stricter lens runs that lens's invariants and
  shows what doesn't yet conform (see §6, "the lens is a filter, not a cage").

### 4.2 Palette (`Palette.tsx` + `edits.ts`)

The palette stops showing all groups. It shows **only the active lens's vocabulary, in the
lens's words**:

| Surface | General (today) | Stock & Flow lens | Reliability lens |
|---|---|---|---|
| Palette sections | Inputs · Functions · Stocks · Reliability (all) | **Stocks · Flows · Auxiliaries · Converters** | **Components · Failure modes · Repair · Redundancy blocks** |
| Same engine construct | `lag` accumulator | shown as **"Stock"** | (hidden — not a reliability noun) |
| `event` FSM | "Failure / Event" | (hidden) | shown as **"Failure mode"** |
| Toolbar primary verbs | generic "Add ▾" | **"Add stock" · "Draw flow"** | **"Add component" · "Add failure mode"** |

Same `PaletteEntry` records; the lens picks the subset, relabels them, and orders them so the
domain's *primary* nouns are one click away and the substrate vocabulary is gone.

### 4.3 Canvas (`EditableCanvas.tsx`)

Same DAG, same layout engine (Dagre) — the lens changes **glyphs and edge semantics**:

- **Stock & Flow:** stocks render as **boxes** (the bathtub), flows as **valve-on-a-pipe**
  double-line edges with a cloud source/sink, auxiliaries as small circles, information links
  as thin arrows. This is the 65-year-proven Forrester notation the market already reads.
- **Reliability:** components as **RBD blocks**, redundancy as **parallel branches**, failure
  modes as annotations on the block. Series/parallel topology is drawn, not typed.
- **General:** today's uniform node + single arrow.

Critically, the thesis's §2.2 three edge kinds (**influence / flow / event**) — which today
"collapse to one arrow" per the frontend assessment — become **visually distinct only inside a
lens that gives them meaning**. The flow edge is a first-class draw-a-flow gesture in the S&F
lens; in the general lens it stays a plain arrow. *The lens is what makes an edge kind worth
distinguishing.*

### 4.4 Inspector (`Inspector.tsx`)

The property form is relabeled and re-scoped to the domain:

- A stock's inspector says **"Initial level," "Inflows," "Outflows," "Non-negative?"** — not
  "lag input / rate expression / clamp." Same underlying fields; domain words.
- A reliability component's inspector shows **"Time-to-failure distribution," "Repair policy,"
  "Redundancy role"** — the `event`-FSM fields (already built per the 2026-07-29 update) dressed
  in RAM language.
- The **raw-JSON escape hatch stays** (§8) — but it's the exception, not the default surface.

### 4.5 Status bar / validation (`StatusBar.tsx`)

This is where "lens, not view" earns its keep. The status bar runs the **active lens's
invariants** and reports them in the lens's terms:

- **Stock & Flow:** *"✔ stock-flow consistent · every flow has a source and sink · no stock
  leaks"* — the Godley-Lavoie accounting invariants (conservation, reconciliation) checked at
  author time. A flow that adds to a stock but subtracts from nothing is flagged.
- **Reliability:** *"✔ every component has a failure mode · redundancy blocks well-formed."*
- **General:** today's topo/units/reference checks only.

These invariants are the concrete governance claim the thesis sells — *"guaranteed stock-flow
consistent"* is literally a status-bar assertion the tool can make structurally. **This is the
part a view could never do**, and it is the reason the whole exercise is a lens.

### 4.6 Results / dashboards (`ResultsTab.tsx`, `DashboardTab.tsx`)

The lens supplies **result presets** so "run" lands on the domain's answer, not a generic
chart dump:

- **Stock & Flow** opens on **stock trajectories with p05–p95 bands** — because the one
  empirical result in the thesis (fewer than half of MIT grad students can integrate a flow into
  a stock) *dictates* that the tool must **simulate and show the accumulation**, never present
  the stock symbolically. The S&F lens's default result is the trajectory, first, always.
- **Reliability** opens on **availability / MTBF / MTTR** and a failure-count histogram.
- **Decision/VOI** opens on the **expected-objective-vs-decision** sweep and the optimum.

Views (sankey, tornado, loop highlighting) sit alongside as toggles, lens-independent.

### 4.7 Templates

Each lens ships **canonical, runnable starting points** so a first-time user is never staring
at a blank DAG: S&F → *bathtub, SIR epidemic, inventory-with-noise*; Reliability → *repairable
component, k-of-n redundant system*; Decision → *build-vs-buy under uncertainty*. Templates are
the fastest proof that the lens vocabulary is real.

---

## 5. A worked walkthrough (stock-and-flow, the first lens)

1. **New model → picks "Stock & Flow."** Shell is unchanged; palette now reads *Stocks · Flows
   · Auxiliaries*; toolbar promotes **Add stock / Draw flow**; canvas is in Forrester notation.
2. **Drops a stock "Population."** Inspector: *Initial level = 1000. Non-negative ✔.* Under the
   hood: a `lag` accumulator — never named as such.
3. **Draws a flow "births" into it.** The flow-draw gesture creates a rate expression bound as
   an inflow. Status bar immediately: *"⚠ flow `births` has no source"* — the conservation
   invariant firing at author time. User marks the source a cloud (exogenous) → *"✔ stock-flow
   consistent."*
4. **Makes the birth rate stochastic.** `birth_rate ~ Normal(...)`. This is the differentiator
   the thesis leans on — per-realization uncertainty is first-class, not a run mode.
5. **Runs.** Result mode opens **on the Population trajectory with uncertainty bands** — the
   accumulation simulated and shown, per the bathtub finding. Not a number; a curve with a fan.
6. **Saves.** JSON carries `view.lens: "stock-flow"` and per-element role tags (§7). Diffs
   cleanly in git — the "diffable successor to XMILE" claim, concrete.
7. **A reviewer opens it.** Tool reads the tag, opens already in the S&F lens, re-runs the SFC
   invariants, shows the same green governance assertion. **The lens round-tripped** — which the
   thesis names as the primary acceptance test of the whole strategy.

---

## 6. Behavior of switching lenses (the hard cases)

- **The lens is a filter, not a cage.** Switching lenses never deletes or rewrites elements. A
  reliability element viewed in the S&F lens doesn't vanish — it renders with the general glyph
  and a subtle *"outside this lens"* affordance, and the inspector offers the raw-JSON form.
  Nothing is ever unreachable.
- **Switching *up* into a stricter lens is a conformance report, not a conversion.** Move a
  hand-built raw model into the S&F lens and the status bar shows *"6 of 9 elements are stock-flow
  typed · 3 unclassified"* with a one-click "treat as stock/flow/aux" per element. The lens never
  silently guesses; it *offers* classifications and the invariants light up as elements conform.
- **Mixed-lens models are allowed but not celebrated.** The thesis is firm: *ship one lens, not
  three half-lenses.* The tool permits elements outside the active lens (the general substrate is
  always underneath) but the palette/validation commit to one lens at a time. This is the UI
  encoding of "one complete lens reads as a product."

---

## 7. The one schema change (round-trip), and how the FE uses it

The thesis allows exactly **one** schema-adjacent change: an **engine-ignored `role`/`lens`
annotation** so a lens round-trips. Grounding it in this repo:

- The schema's *existing* `role` field (`schema/wasim-schema-v2.json`) is **stock-port
  semantics** (`addition`/`withdrawal`/`overflow`/`net_change`) — **not** the lens tag. The lens
  annotation is genuinely new and must not collide with it. Recommended name: **`lens_role`**
  (e.g. `"stock" | "flow" | "auxiliary" | "component" | "failure_mode"`), plus a document-level
  **`view.lens`** naming the active lens.
- **Where it lives:** `view.lens` rides the **already-persisted, engine-ignored `view` block**
  in `store.ts` — zero engine involvement. Per-element `lens_role` is the single additive,
  non-breaking schema field.
- **Why round-trip needs it:** without the tag, re-opening a saved model yields a raw DAG and
  the lens must *re-infer* which node is a stock. With it, `LensSpec.roleOf(el)` is a lookup, the
  vocabulary reconstructs exactly, and the invariants re-run. **If the lens ever forces the user
  back to raw ASTs, the whole thesis has failed** (thesis §9) — so this tag is the primary build
  risk and the primary acceptance test.

---

## 8. Progressive disclosure & the escape hatch (non-negotiable)

The thesis's fatal-failure mode is *"a lens that leaks the substrate."* The UI defenses:

1. **The substrate vocabulary is never in the palette of a domain lens.** No modeler in the S&F
   lens ever sees the word "lag."
2. **But the substrate is always one deliberate step away.** The raw-JSON per-element editor
   (already built) and the `general` lens are the pressure-release valves — for the 5% case,
   reachable, never *required*.
3. **Advanced surfaces (units manager, `results_spec`, containers/submodels) are lens-gated
   disclosure**, not always-on clutter — they appear when the lens or the user asks for them.

---

## 9. Build plan (mapped to the frontend assessment)

Framed against [`FRONTEND_ASSESSMENT_2026-07.md`](FRONTEND_ASSESSMENT_2026-07.md) (viewing
~85%, authoring ~35–40%):

- **Phase 0 — Lens machinery (the enabling refactor).** Introduce `LensSpec`, `activeLens` in
  the store, `view.lens` persistence, and make `Palette.tsx` render from the active spec instead
  of all groups. *Low risk — it's a selection layer over the existing `PALETTE`.*
- **Phase 1 — The stock-and-flow lens, end to end.** Forrester glyphs on the canvas; stock/flow/
  aux inspector forms; the SFC conservation + reconciliation invariants in the status bar;
  trajectory-first result preset; 3 templates; the `lens_role` round-trip. *This is the beachhead
  — one complete lens that reads as a product.*
- **Phase 2 — Views for free.** Sankey-of-flows, feedback-loop highlighting, flow tornado —
  lens-independent toggles on the existing canvas/results.
- **Phase 3 — The second lens (reliability/RBD).** Proves the pattern is data, not a fork —
  mostly a new `LensSpec` over the *already-built* `event`-FSM and dimensioned-array authoring.
  If this lens costs a rewrite, the abstraction failed; if it's mostly a spec file, it's proven.
- **Phase 4 — Copilot vocabulary + decision/VOI lens** (the one lens still needing an engine
  half — the optimize/VOI reduction named in thesis §3).

The ordering deliberately front-loads the *machinery* and *one* lens, matching the thesis: prove
the lens pattern round-trips on one, and the second and third are incremental, not rewrites.

---

## 10. Open questions

1. **Lens granularity of validation** — do invariants run continuously (like today's reconcile)
   or on demand? Continuous is truer to "validated as you type" but the SFC check across a large
   model may need debouncing.
2. **Multi-lens documents** — permitted (§6), but do we *warn* on save, or stay silent? Leaning
   silent-but-inspectable, to honor "the substrate is always underneath."
3. **`lens_role` inference for imported models** — XMILE and GoldSim transpilation should emit
   the tag directly; hand-written legacy JSON needs the §6 conformance-report path.
4. **Where the decision/VOI lens's missing engine half lands** — this is the only lens that is
   not purely authoring work; it needs the named optimize/VOI reduction. Out of scope here;
   flagged for the engine track.

---

## 11. The plan restated

The authoring tool already has the bones of a lens system (grouped palette, engine-as-arbiter
store, persisted engine-ignored `view` block). The plan is to **promote "which group is
visible" into a first-class, data-defined `LensSpec`** that reprograms palette, glyphs,
inspector, invariants, and results together — so that choosing "Stock & Flow" or "Reliability"
visibly changes *what you author and how it's validated*, not just how it's colored. Ship the
stock-and-flow lens complete and first; get every view for free; keep the substrate one
deliberate step away but never in the way; and make lenses cheap enough — because they're data —
that the second and third ship without a rewrite. That last property is the thesis's whole
point: **the generality you were tempted to sell is exactly what lets you not rewrite.**
