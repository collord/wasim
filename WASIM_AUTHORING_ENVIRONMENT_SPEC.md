# WASiM Authoring Environment — User-Level Specification

**What this is.** A user-level specification for a **graphical model-authoring
environment** for WASiM, built on the existing web stack (`frontend/`), designed
*in parallel to the authoring patterns in the GoldSim User Guide* but native to a
browser SPA and to WASiM's open-JSON model format. It describes what the user sees
and does — workspace, canvas, palette, property editors, run/analysis, dashboards —
not the low-level implementation, though it pins each surface to the concrete stack
so a frontend team can build from it.

**Why now.** WASiM today has a mature Rust/WASM engine and a **read-only viewer**
(`frontend/`): you load a `model.json`, inspect it (Graph / Model tabs), tweak
*editable constants and distribution parameters* (Dashboard), run, and view Results
/ Sensitivity. There is **no way to author a model in the GUI** — elements are
created by transpilation from GoldSim or by hand-editing JSON. This spec closes that
gap: it turns the viewer into an editor.

**Scope & non-scope.**
- *In scope:* the GUI authoring experience for the WASiM v2 model
  (`model_v2.rs` primitives) — creating, wiring, configuring, validating, running,
  and analyzing models entirely client-side.
- *Out of scope:* engine semantics (covered by `GOLDSIM_ENGINE_GAP_ANALYSIS.md` and
  the Tier workplans) and any capability the engine does not have. This spec never
  proposes a UI for a feature the engine cannot execute; where a GoldSim authoring
  pattern maps to an engine gap, it is called out as **deferred** and tied to the
  relevant workplan item.

**Relationship to the current code.** Every section names the existing artifact it
extends: `frontend/src/store.ts` (Zustand store, the model-of-record), the worker
protocol (`frontend/src/worker/protocol.ts`), the WASM bridge
(`frontend/src/engine.d.ts`, `sim.worker.ts`), and the tabs
(`GraphTab`/`ModelTab`/`DashboardTab`/`SensitivityTab`/`ResultsTab`).

---

## 0. Design principles

1. **`model.json` stays the source of truth.** The authoring tool is a *view over a
   diffable JSON document*, never a proprietary project file. Everything the user
   builds serializes back to a schema-valid `model.json` that reads and diffs
   cleanly in git. This is WASiM's core thesis and the tool must protect it.
2. **The engine is the arbiter of meaning.** The FE already treats the engine's
   `model_summary()` as its *model of record* — it renders from the summary and
   never parses the schema itself (`store.ts`). Authoring extends this to a
   **round-trip**: the FE edits a canonical model, hands it to the engine, and the
   engine returns an updated summary + validation. The FE never needs a second,
   drifting understanding of the schema.
3. **Client-side and offline-capable.** No server. The WASM engine in a Web Worker
   does parsing, validation, summary, simulation, sensitivity, and (future)
   optimization. A model is a file you open and save locally (File System Access
   API where available, download/upload fallback).
4. **Progressive disclosure, GoldSim-parallel where it aids adoption.** Users
   coming from GoldSim should recognize the shape — an influence-diagram canvas, an
   element browser, per-element property dialogs, Edit/Result modes, dashboards —
   but the tool is web-native (keyboard-first, undoable, URL-shareable), not a
   pixel clone.
5. **Unit-aware and validated as you type.** Units and dimensional consistency are
   first-class (the engine has a real dimensional checker); the tool surfaces
   canonical-vs-display units and live dimensional/reference feedback rather than
   failing only at run time.

---

## 1. Workspace layout & modes

### 1.1 The shell (parallel to GoldSim's graphics pane + browser + property dialog)

The current tabbed shell (`App.tsx`) evolves into a **three-pane workspace** with a
mode switch, rather than five sibling tabs:

```
┌──────────────────────────────────────────────────────────────────────┐
│  Toolbar: [New] [Open] [Save]  |  Palette ▾  |  ⟳ Run ▸  |  Edit│Result│
├───────────────┬────────────────────────────────────┬─────────────────┤
│  Model        │                                     │  Inspector      │
│  Browser      │            Canvas                   │  (properties of │
│  (tree +      │      (influence diagram —           │   the selection)│
│   search +    │       place, wire, drill in)        │                 │
│   palette)    │                                     │                 │
│               │                                     │                 │
├───────────────┴────────────────────────────────────┴─────────────────┤
│  Status bar:  ● valid / ⚠ 3 issues · topo OK · units: warn · 42 elems │
└──────────────────────────────────────────────────────────────────────┘
```

- **Left — Model Browser + Palette** (§3, §4): the element/container tree and the
  insert-palette, collapsible.
- **Center — Canvas** (§2): the influence diagram, in Edit mode; the results
  workspace in Result mode.
- **Right — Inspector** (§5): the property editor for the current selection.
- **Bottom — Status bar** (§8): live validation summary, topo/units state, element
  count, run status.

The existing tabs are preserved as **Result-mode views** (Results, Sensitivity,
Dashboard) reached by the mode switch, so nothing is lost.

### 1.2 Modes (parallel to GoldSim Edit / Run / Result modes)

| Mode | Purpose | Center pane |
|---|---|---|
| **Edit** | Author the model | Canvas (influence diagram) |
| **Result** | Inspect a completed run | Results / Sensitivity / Dashboard views |

A run transitions Edit → Result (as `store.ts` already does: `complete` sets
`activeTab: 'results'`). Re-entering Edit keeps the last results available until the
model changes structurally.

### 1.3 Container drill-down (parallel to GoldSim container navigation)

Containers and submodels are navigable, not just visual. The canvas shows a
**breadcrumb** (`Model / Watershed / Storm submodel`); double-clicking a container
descends into it; the browser tree mirrors the location. `GraphTab` already models
submodel **expand/collapse in place** — authoring keeps that *and* adds true
**drill-in navigation** for deep hierarchies.

---

## 2. The canvas — influence diagram (parallel to GoldSim's graphics pane)

The canvas is the heart of authoring. It extends `GraphTab`'s SVG renderer (Dagre
layout, pan/zoom, per-type icons/colors, submodel framing) from read-only to
editable.

### 2.1 Placing elements

- **Drag from palette** (§3) onto the canvas, or **double-click empty canvas** to
  open a quick-insert menu. A new element is created with a generated `id`, a
  default name, and type-appropriate defaults, and is immediately selected with its
  Inspector open.
- **Free placement with persisted positions.** Unlike the current viewer (which
  computes every position with Dagre on each render), authored nodes carry
  **user-set coordinates**. Layout lives in a `view` block the engine ignores (see
  §13.3) so `model.json` stays the single artifact.
- **Auto-layout on demand.** Dagre remains available as a **command** ("Tidy
  layout", "Tidy selection") rather than the only layout — the user can auto-arrange
  then nudge, matching how GoldSim auto-routes influences but lets you place icons.

### 2.2 Wiring — three edge kinds

WASiM's engine distinguishes reference/influence from material flow from
event effect; the canvas makes that visible (extending the single grey arrow
`GraphTab` draws today):

| Edge | Meaning | How created | Rendering |
|---|---|---|---|
| **Influence** | An expression/parameter references another element's output | *Automatically*, when you type a reference in a property (GoldSim-style), **or** by dragging output-port → input-port | thin, dashed, grey, arrowhead |
| **Flow link** | A `Link` primitive moving material between `Stock`/`Cell` | Drop a `Link` and connect `source`→`target` ports, or drag stock→stock with the Link tool | solid, weighted, teal (matches `link` color) |
| **Event effect** | An `Event`'s effect acts on a target (`Additive`/`Replace`/`Spend`/…) | set in the Event inspector; drawn as a dotted red connector to the target | dotted, red, small badge = effect mode |

**Influences follow references** — the defining GoldSim pattern. When you write
`inflow * 0.8` in a Stock's rate, an influence arrow from `inflow` appears; deleting
the reference removes the arrow. The canvas is a *projection of the model's
dependency graph*, not a separate wiring database — which is exactly how the engine
builds `graph_v2`.

### 2.3 Ports

Each node exposes typed connection points:
- **Inputs** (left): the referenceable inputs of that element (a Stock's
  `inflows`/`outflows`/`rate`; a Link's `source`/`target`; an Event's `trigger`).
- **Outputs** (right): the element's output(s) — the primary value, plus secondary
  ports the engine publishes (a Stock's overflow/withdrawal ports; a Cell's
  `cell:species@medium` mass ports; a Queue's `num_in_queue`). Multi-output
  elements show a small port list, mirroring GoldSim's multiple-output dialogs.

### 2.4 Selection, editing, and canvas ergonomics

- Single/marquee/`⌘`-click multi-select; drag to move; arrow-key nudge; align/
  distribute; delete with dependency-aware warnings (see §8).
- **Copy / paste / duplicate** within and across containers (parallel to GoldSim
  Clone, but as plain copies — Clone-as-reference is a documented engine non-goal).
- **Undo/redo** across every model mutation (§13.4).
- Pan/zoom/Fit are already implemented (`GraphTab`) and carry over; add a
  **minimap** for large models and **search-to-focus** (jump the canvas to a
  browser hit).
- **Container framing**: dropping elements inside a container frame sets their
  `container`; dragging a node into/out of a frame re-parents it (updates
  `element.container`, the authoritative field per `types.ts`).

---

## 3. The element palette (parallel to GoldSim's element toolbar)

The palette is the WASiM primitive catalog, grouped GoldSim-style and reusing the
existing per-type icons/colors from `GraphTab` (`TypeIcon`, `TYPE_STROKE`,
`TYPE_BG`). Each entry inserts a specific primitive/`NodeRule`.

| Palette group | Entry → WASiM construct | GoldSim analogue |
|---|---|---|
| **Inputs** | Constant → `Node::Fixed` (scalar/array; `editable`+`bounds`) | Data |
| | Stochastic → `Node::Sample` (distribution) | Stochastic |
| | Time Series → `Node::Series` | Time Series |
| | Lookup → `Node::Lookup` (1-D/2-D) | Lookup Table |
| | History/Process → `Node::Process` (GBM/OU + reversion) | History Generator |
| **Functions** | Expression → `Node::Expression` | Expression |
| | Selector → `Node::Expression` with nested `if` | Selector |
| | Filter/Extrema → `Node::Filter` (Mean/Min/Max/Sum/Ema; whole-run window = running extrema) | Extrema / Information Delay |
| | PID Controller → `Node::PidController` | Controller (PID/Deadband) |
| | Convolution → `Node::Convolution` | Convolution |
| | Previous Value → `Node::Lag` | Previous Value |
| **Stocks** | Stock/Reservoir/Pool → `Stock` (traits: `floor`, `capacity`+`overflow_target`, `withdrawals`, `return_rate`, `inflows`/`outflows`) | Integrator / Reservoir / Pool |
| **Delays / Queues** | Queue → `Node::Queue` (`delay_time`, `capacity`, discipline) | Event / Discrete-Change Delay |
| **Events & logic** | Event → `Event` (trigger + effects; Poisson `rate`; `failure_process`) | Timed / Triggered Event |
| | Milestone → `Node::Milestone` | Milestone |
| | Status → `Node::Status` (set/reset latch) | Status |
| | Interrupt → `EffectMode::Interrupt` | Interrupt |
| | Gate → `Gate` (And/Or/Not/N-Vote, Success/Failure) | And/Or/Not/Logic Tree |
| **Transport** | Link → `Link` (rate/fraction, transit, dispersion, decay, priority) | Material Delay / Splitter / Allocator |
| | Cell → `Cell` (media, species, partitioning, source) | Contaminant-Transport Cell |
| | Species → `Species` (half-life, decay chain) | CT Species |
| | Medium → `Medium` (phase, density, porosity) | CT Medium |
| | Resource → `Resource` (initial, capacity; Spend/Deposit/Borrow) | Resource / Resource Store |
| **Structure** | Container → organizational `ContainerDef` | Container |
| | Submodel → `ContainerDef{kind:submodel}` (+ interface) | SubModel |

Palette entries the engine does **not** support are simply **absent** (no Script
element yet — Tier C1; no Spreadsheet/DLL — non-goals), so the palette never
promises what the engine cannot run.

---

## 4. The model browser (parallel to GoldSim's element browser)

A left-pane tree with two lenses, toggleable:
- **By containment** — the container/submodel hierarchy with elements as leaves;
  mirrors the canvas breadcrumb. Reuses the "Constants" grouping idea from
  `GraphTab` (bare constants fold into one node) so large flat lists stay legible.
- **By type** — elements grouped by primitive/rule (all Stochastics, all Stocks…),
  the way GoldSim's browser offers an element-type view.

Features: incremental **search/filter** (by name, id, type, unit, trait), select →
focus-on-canvas + open Inspector, multi-select for bulk ops, drag to re-parent,
context menu (rename, duplicate, delete, "add to dashboard", "set as result").
Every element the browser lists comes straight from `ModelSummary.elements`
(`types.ts`), so the browser and canvas never disagree.

---

## 5. The Inspector — per-element property editors (parallel to GoldSim element dialogs)

The Inspector is the right pane; it edits the selected element. It is organized like
a GoldSim property dialog: a **Definition** section (the element's core inputs), plus
**Info** (id/name/description), **Output & units**, and **Save results** options.
Every editor writes back into the canonical model (§13) and re-validates.

### 5.1 Fields common to all elements

- **Identity**: `id` (slugified, uniqueness-checked), `name`, `description`
  (surfaced as the canvas tooltip — `GraphTab` already renders `description`).
- **Container**: parent container/submodel (also settable by canvas drag).
- **Output & units**: canonical `unit` plus optional **display unit** with the
  affine map the engine computes (`display = value·factor + offset`, already in
  `ElementSummary`/`QtyDisplay`). The user edits in display units; the store keeps
  canonical (the pattern `store.ts` already uses for duration/timestep).
- **Save results**: which outputs are retained (final value / time history), the
  `SaveSpec` analogue — so heavy models can trim output.

### 5.2 Value-rule editors (the `Node` rules)

| Rule | Inspector editor |
|---|---|
| **Fixed** | number field (in display units) + `editable` toggle + optional `bounds{min,max}` (bounds drive dashboard sliders and optimization variables) |
| **Sample** | **distribution picker** (grouped roster: Uniform, Normal, Lognormal[±moments], Triangular[/10-90/log], Trapezoidal, Exponential, Gamma, Beta[/succ-fail], Weibull, Pearson III/V, PERT, Pareto, Extreme Value/Probability, Student-t, Binomial, Neg-Binomial, Poisson, Discrete/Cumulative[/log], Sampled, External) → **parameter fields** (each a `QuantityOrFormula`: number, or an expression referencing earlier draws) → **truncation** `{min,max}` → **correlation** (partners + Spearman ρ) → **autocorrelation** ρ → **resampling** trigger. A live sparkline previews the sampled PDF. |
| **Expression** | the **expression editor** (§6) |
| **Lookup** | **table editor** (grid of x/y[/columns], units per axis; interpolation Linear/Step/Cubic/Log-result; extrapolation clamp/extend; `TBL_Derivative`/inverse modes) with an inline curve preview |
| **Series** | **time-series editor** (timestamp/value grid or paste; interpolation; time unit) with a preview |
| **Process** | drift/volatility/mean-reversion fields (GBM/OU) |
| **Lag** | input + initial value (1-step delay) |
| **Queue** | input, `delay_time`, `capacity`, discipline (Conveyor/FixedAtEntry); shows the `num_in_queue` output port |
| **Filter** | input, window, statistic (Mean/Min/Max/Sum/Ema) |
| **PidController** | input, setpoint, kp/ki/kd, deadband, output clamps |
| **Hysteresis** | input, high/low thresholds, above/below outputs |
| **Markov** | states, initial, transition matrix (fixed or expression rows), output values |
| **Status** | set trigger, reset trigger (set-wins latch) |
| **Milestone** | trigger (records first-fire elapsed time) |
| **GateLogic** | inline boolean tree editor (shared with §5.6) |

### 5.3 Stock editor

Initial value + a **flows table** (`inflows`/`outflows` as referenced elements *or*
a direct `rate` expression), plus trait toggles that reveal fields: `floor`,
`capacity` + `overflow_target`, `withdrawals` (priority-ordered rows),
`return_rate` (compound growth). Secondary output ports (overflow/withdrawal/
cumulative) are listed and can be referenced or charted. Parallel to the GoldSim
Reservoir/Pool dialog (Additions vs Withdrawal Requests, bounds, priorities).

### 5.4 Link editor

`source`/`target` (pickers or canvas ports), transfer as `rate` or `fraction`,
`priority`, and transport traits: `transit_time` (plug flow), `dispersion` (Péclet),
`decay_rate`, `schedule`, and species/medium binding for cell transport. Parallel to
Material Delay + Splitter + Allocator. *(Note: `fluxes`/`geometry` are engine stubs —
their editors are **deferred** until the parser populates them; flagged in §14.)*

### 5.5 Event editor

Trigger (mode: Always / OnCondition / Periodic / OnSchedule; Poisson `rate`),
**effects table** (target + change expression + mode: Additive / Multiplicative /
Replace / Spend / Deposit / Borrow / **Interrupt**), `count_limit`, and an optional
**failure process** (basis ExposureTime/OperatingTime/Demand/Condition; repair
policy None/Repair/Replace/PM). Parallel to Timed/Triggered Event + Discrete Change
+ Reliability. *(OnEvent trigger and CapacityDemand/Event failure bases are engine
no-ops — offered greyed with a "not yet modeled" note, per §14.)*

### 5.6 Gate editor

A **boolean-tree builder** (And/Or/Not/N-Vote/Condition/Reference), Success vs
Failure semantics — a fault-tree editor, parallel to GoldSim's Logic Tree.

### 5.7 Cell / Species / Medium editors

- **Cell**: volume, media list, species list, `inflows`, **partitioning** (Kd rows),
  and source terms (`inventory`/`release_rate`/`release_schedule`/`release_target`).
- **Species**: half-life, decay products (branching fractions), molecular weight.
- **Medium**: phase (Solid/Fluid/Gas/ReferenceFluid), density, porosity.

Parallel to the Contaminant-Transport module's Cell/Species/Medium authoring.
*(Cell output is mass, not concentration — the Inspector labels it "mass" and links
to the concentration-output roadmap item, §14.)*

### 5.8 Submodel / Container editor

- **Container**: name, parent, membership.
- **Submodel**: its own `simulation_settings` (or inherit), and the **interface** —
  an inputs table (`{input, from}` binding a parent driver to an interior consumer)
  and an outputs list. This is the nested-Monte-Carlo boundary; the editor makes the
  `from`-driver binding explicit (the mechanism `SUBMODEL_INTERFACE_INPUT_BINDING.md`
  documents). Parallel to GoldSim SubModel + its exposed inputs/outputs. Parent
  expressions consume submodel statistics via a **SubmodelStat picker**
  (Mean/Percentile/Sd/CumulativeProb).

---

## 6. The expression editor (parallel to GoldSim's expression fields)

A focused editor used wherever a formula is accepted (Expression nodes, Stock rates,
distribution parameters, triggers, effects, link rates):

- **Reference autocomplete** — type to search element names/ids; picking one inserts
  a reference and (on the canvas) draws the influence arrow. This *is* the
  influence-creation mechanism (§2.2).
- **Builtin & time-ref palette** — the engine's builtins (math/array: `min`, `max`,
  `sqrt`, `sum_array`, `interp_array`, `dot_product`, …), time refs (`elapsed`,
  `year`, `month`, `hour`, `elapsed_months`, `start`, …), and event predicates
  (`occurs`, `changed`), inserted from a categorized list.
- **Live dimensional feedback** — as you type, the editor calls the engine's
  dimensional checker (`units.rs::check_dimensions`) and underlines unit errors
  (adding metres to seconds, transcendental of a dimensioned arg) with the offending
  subexpression — *before* run time. Honors the model's units mode (warn/strict).
- **Inline diagnostics** — dangling references, arity errors, and unparated-formula
  warnings (a raw `Formula` string that the engine would evaluate to 0.0 is flagged
  as not-yet-parsed).
- **Array comprehensions** — support for the implemented `vector_map`/index forms.

There is **no procedural scripting editor** — the Script element is an engine gap
(Tier C1). Until it lands, the palette and expression editor expose only functional
expressions and array comprehensions, and say so.

---

## 7. Units & dimensions (parallel to GoldSim's Units Manager)

- Unit entry everywhere a `Quantity` appears, with **canonical vs display**
  separation already in the summary (`unit` + `display_unit`/`display_factor`/
  `display_offset`). The user works in display units; the model stores canonical.
- A **units mode toggle** (Warn / Strict) surfaced in Simulation Settings and the
  status bar, mapping to `RunConfig.units`. Strict mode blocks a run on any
  dimensional inconsistency (engine `check_dimensions` hard-fail); Warn annotates
  but runs.
- A **dimensional inspector** on hover/selection shows an element's inferred
  dimension and, on mismatch, why — the same information the engine computes.
- Custom/compound units are entered as strings (`m/yr`, `kg/m3`) and validated
  against the engine's unit registry.

---

## 8. Validation & causality (parallel to GoldSim's causality sequence)

A persistent **status bar** plus an expandable **Issues panel**, both fed by the
engine (the FE never re-derives schema truth):

- **Structural validation**: duplicate/dangling ids, unresolved references, empty
  required fields, uniqueness — from a `validate` round-trip (§13.2).
- **Graph/causality**: cycle detection with the engine's rules (`Lag`/stock
  back-edges are legal; other cycles are errors), and a **topological-order view**
  (from `topo_order_json`, already in the WASM API) — the WASiM analogue of
  GoldSim's causality sequence, letting the author see evaluation order and spot
  ambiguous loops.
- **Unit issues**: from §7, inline and aggregated.
- **Model smells**: an `External` distribution with no fallback table (evaluates to
  0.0), an unreferenced sink, a formula string that never parsed — surfaced as
  warnings that jump-to-element on click.

Deleting an element that others reference prompts with the downstream list
(dependency-aware delete), rather than silently producing dangling refs.

---

## 9. Simulation settings (parallel to GoldSim's Simulation Settings dialog)

A settings dialog editing `simulation_settings` + the runtime `RunConfig` the store
already threads:

- **Time**: duration, timestep, time display unit; **timebase mode** (Fixed vs
  **EventAccurate** — the latter enabling sub-interval integration at scheduled/
  bound-crossing instants); optional `calendar_start` anchor (enables the true
  leap-year calendar and calendar time-refs).
- **Monte Carlo**: `n_realizations`, `sampling_method` (**monte_carlo / lhs** — LHS
  is now real), `seed`, optional **realization weights**.
- **Reporting periods**: fixed-length period + reductions (accumulated / average /
  change / rate) for the results layer.
- **Results spec**: default custom percentiles, distribution (PDF/CDF/CCDF) and
  final-value stats (CI / skew / kurtosis / CTE) to compute — the `results_spec`
  the engine already accepts at runtime.

These extend the existing run-config fields in `store.ts` (`nRealizations`, `seed`,
`simDuration`, `simTimestep`) rather than replacing them.

---

## 10. Running & run control (parallel to GoldSim's Run Control)

- A **Run** button in the toolbar (already `run()` in `store.ts`) with live status
  (`idle`/`running`/`done`/`error`) from the worker; a cancel/abort affordance
  (terminate + respawn the worker). Runs stay off the UI thread (Web Worker), as
  today.
- **Deterministic vs probabilistic**: `n_realizations = 1` gives a single
  deterministic trace; >1 the probabilistic ensemble — the same distinction GoldSim
  draws between deterministic and Monte Carlo runs.
- Errors surface in the Issues panel with the engine's message (not a raw stack),
  and select the offending element when the engine names one.
- *(A pause/step debugger is a later enhancement; the engine runs to completion per
  call today.)*

---

## 11. Results & analysis (parallel to GoldSim's Result elements)

Result mode reuses and extends the current `ResultsTab`/`SensitivityTab` (Recharts):

- **Element result picker** driven by `output_ids` (already ordered sinks-first).
- **Time-history charts**: mean + percentile bands; **user-configurable
  percentiles** and **reporting-period aggregation** via the results spec (§9),
  going beyond today's fixed `p05/p25/p50/p75/p95`.
- **Distribution charts**: PDF / CDF / **CCDF (exceedance)** of final or
  capture-time values — the probabilistic displays GoldSim's Distribution Result
  offers. **Capture times** snapshot the distribution at arbitrary elapsed times.
- **Final-value statistics**: mean, confidence interval, skewness, kurtosis,
  **conditional tail expectation** — surfaced as a stats table.
- **Sensitivity** (exists): one-at-a-time curves + tornado (`SensitivityTab`). A
  clear roadmap note that **statistical measures** (correlation/SRC/importance) are
  not yet in the engine.
- **Optimization** (new UI over the existing `optimize_v2`): pick objective
  (element + statistic + Max/Min), decision **variables** (from editable Fixed nodes
  with bounds), and **constraints** (now enforced) → run Box's-complex → show the
  optimum and the search trace.
- **Realization classification/screening** and **scenarios** are GoldSim result
  features that map to engine gaps — shown as **deferred** placeholders tied to the
  roadmap, not faked.

Result views export to CSV/PNG; the model + its results remain separable files.

---

## 12. Dashboards (parallel to GoldSim's Dashboards / Player)

The current `DashboardTab` (edit editable constants, run) becomes an
**author-configurable dashboard**: the modeler curates a panel of **input controls**
(sliders/number fields bound to editable Fixed nodes, using their `bounds`;
distribution-parameter knobs) and **output displays** (selected result charts, stat
tiles). This is the WASiM analogue of a GoldSim Dashboard/Player — a simplified
"what-if" surface for an end user who should not see the full model. Dashboard
layout is saved in the `view` block (§13.3). The existing `.params.json` **overlay
export** (`saveParameters` in `store.ts`) remains the mechanism for sharing a set of
input values without forking the model.

---

## 13. Persistence & the editing architecture (the stack-level contract)

This is the load-bearing part: how authoring works on *this* stack without breaking
the model-of-record pattern.

### 13.1 The FE owns a canonical, editable model

Today the store holds the raw `modelJson` string plus the engine `modelSummary`, and
mutates only two things (`set_constant`, `set_rv_param`). Authoring promotes the
store to hold a **canonical, mutable model document** (the parsed `model.json`), with
the engine summary derived from it. Every editor writes to this document.

### 13.2 The reconcile loop (edit → engine → summary + validation)

```
user edit ──▶ mutate canonical model in store ──▶ (debounced)
        post {type:'reconcile', model} to worker ──▶ engine rebuild
        ◀── {type:'reconciled', summary, validation, topo}
                    │
                    └─▶ store updates modelSummary + issues + canvas
```

- **Fine-grained value edits** (an editable constant, a distribution parameter)
  keep using the fast `set_constant`/`set_rv_param` path — no rebuild.
- **Structural edits** (add/remove element, change a rule, rewire, re-parent) send
  the whole model for a rebuild. WASM engine construction is cheap, and this keeps a
  *single* source of schema truth in Rust. Debounce + a dirty flag avoid rebuilding
  on every keystroke.
- The worker returns `summary` (render/edit source), `validation` (issues panel),
  and `topo` (causality view). `model_summary()` and `topo_order_json()` exist;
  `validate` is a thin addition returning the engine's structured diagnostics.

### 13.3 Layout & dashboard metadata

Canvas node positions, container collapse state, and dashboard configuration live in
an optional top-level **`view`** object in `model.json` that the engine **ignores on
parse** (it already accepts unknown top-level fields; `v2_parse` reads only what it
needs). This keeps everything in one diffable file while preserving the engine's
indifference to presentation. (A sidecar `model.view.json` is the fallback if the
team prefers to keep the model file presentation-free.)

### 13.4 Undo/redo, new/open/save

- **Undo/redo**: a command stack over model mutations (each edit is a pure
  transform of the canonical document) — trivial because the model is plain JSON.
- **New**: an empty `model.json` scaffold (version, default `simulation_settings`).
- **Open**: file drop / picker (exists, `FileDropZone`) → parse → reconcile;
  transpiled-from-GoldSim models open the same way.
- **Save**: serialize the canonical document (File System Access API where
  available; download fallback). Round-trips through the engine's `model_json()`
  getter are available to normalize/verify output.

### 13.5 Worker protocol additions (concrete)

Extend `MainToWorker` / `WorkerToMain` (`protocol.ts`):

```
// main → worker (added)
| { type: 'reconcile'; model: object }          // structural edit → full rebuild
| { type: 'validate'; model: object }           // validation only
| { type: 'run_optimization'; spec: OptSpec }   // expose optimize_v2
// worker → main (added)
| { type: 'reconciled'; summary: ModelSummary; validation: Issue[]; topo: string[] }
| { type: 'optimization_complete'; results: StudyResults }
```

Existing messages (`load_model`, `set_constant`, `set_rv_param`, `run`,
`run_sensitivity`) are unchanged, so the viewer path keeps working during the build-out.

---

## 14. Fidelity notes — where the UI must not over-promise

Because the tool is engine-truthful, these known engine limits shape the UI (each
ties to `GOLDSIM_ENGINE_GAP_ANALYSIS.md` / the workplans):

- **No Script element** (Tier C1): no procedural editor; expression-only.
- **Timebase**: EventAccurate covers scheduled + bound-crossing instants; **no
  scheduled non-uniform global timestep** (Tier B2) and no periodic-trigger
  sub-stepping — the settings UI offers Fixed/EventAccurate only.
- **`Link.fluxes`/`geometry`**: parser stubs — no editor yet.
- **Cell = mass, not concentration**: outputs labeled "mass".
- **`OnEvent` trigger, `CapacityDemand`/`Event` failure bases**: engine no-ops —
  shown disabled with a note.
- **`External` distribution**: samplable only with an inline fallback table — the
  editor requires one or warns.
- **Statistical sensitivity, realization classification/screening, scenarios,
  importance sampling, matrix algebra, DLL/Excel/ODBC**: engine gaps or documented
  non-goals — represented as deferred/absent, never as working UI.

---

## 15. Phased roadmap (MVP → full)

**Phase 1 — Property authoring (no canvas rewrite).** Promote the store to a
canonical editable model; build the Inspector (§5) and expression editor (§6) for
all existing primitives; add the reconcile/validate loop (§13.2) and **Save
`model.json`** (§13.4). The user can now fully edit any loaded/transpiled model and
save it — the biggest capability jump, reusing the existing viewer for canvas/results.

**Phase 2 — Structural canvas.** Turn `GraphTab` into an editor: palette insert
(§3), free placement + persisted layout (§2.1, §13.3), port-based wiring for the
three edge kinds (§2.2), delete/duplicate/re-parent, undo/redo. Now models can be
built from scratch.

**Phase 3 — Analysis & dashboards.** `results_spec`-driven Results (§11), the
Optimization UI, and author-configurable Dashboards (§12).

**Phase 4 — Depth.** Scenarios and realization classification *when the engine gains
them*; a run debugger; collaborative/URL-shared models.

---

## 16. Non-goals

- Not a visual clone of GoldSim; web-native interaction wins over pixel fidelity.
- No authoring UI for engine non-goals: DLL/Excel/ODBC coupling, localized-container
  namespace scoping (an emit-side id-qualification concern), reference-Clone,
  distributed processing, per-container internal clocks.
- No server, no proprietary project format — `model.json` (+ optional `view`) is the
  only artifact.
- The tool never invents model semantics: if the engine cannot execute it, the tool
  does not offer it.

---

## 17. LLM-assisted authoring — the "describe it in words" copilot

**What this adds.** A configurable LLM endpoint and an **interactive loop** that turns a
natural-language problem description ("model a two-tank system that overflows into a
creek", "30-year retirement projection with lognormal returns and tax-aware
withdrawals") into a schema-valid WASiM `model.json`, then refines it through
conversation. This is the highest-leverage on-ramp for the authoring environment:
it lets a domain expert start from intent instead of from an empty canvas.

**Design thesis — the engine is the ground truth, the LLM is a drafting aid.** The
copilot never gets the last word on correctness. Every model the LLM proposes is
**parsed, validated, and (optionally) run by the WASM engine** before it reaches the
canvas, and the engine's structured diagnostics are fed back to the LLM to fix. This
is the same reconcile loop as §13.2, with the LLM as one more producer of candidate
models. It means the copilot can be genuinely useful without the LLM having perfectly
memorized the schema — hallucinated fields, bad references, and unit errors are caught
by the engine and corrected in-loop, not shipped.

### 17.1 Configuring the LLM endpoint

A **Settings → AI** panel configures a provider. The config is a small object held in
the store (extending `store.ts`) and persisted to browser storage
(`localStorage`/IndexedDB) — **never** committed to `model.json` and never sent
anywhere except the chosen provider:

```ts
type LlmConfig =
  | { provider: 'anthropic';     model: string; apiKey: string }
  | { provider: 'openai';        model: string; apiKey: string; baseUrl?: string }
  | { provider: 'azure-openai';  deployment: string; apiKey: string;
      resource: string; apiVersion: string }
```

- **Anthropic** — Messages API (`POST https://api.anthropic.com/v1/messages`, headers
  `x-api-key`, `anthropic-version: 2023-06-01`). Model picker defaults to
  **`claude-opus-4-8`** (most capable; best at one-shot schema-faithful generation),
  with **`claude-sonnet-5`** offered as a faster/cheaper option and
  **`claude-haiku-4-5`** for cheap iterative edits.
- **OpenAI** — Chat Completions/Responses (`Authorization: Bearer …`), optional
  `baseUrl` override for compatible gateways; user selects the model.
- **Azure OpenAI** — deployment-based endpoint
  (`https://{resource}.openai.azure.com/openai/deployments/{deployment}/…?api-version={ver}`,
  header `api-key`).

A thin **provider abstraction** normalizes these to one internal interface — a
`chat(messages, tools)` call returning text and/or tool calls — so the copilot loop
(§17.3) is provider-agnostic. A **"Test connection"** button issues a trivial
completion and reports latency/model/errors. Config is validated before use; a missing
or rejected key surfaces inline, not at first prompt.

### 17.2 What the LLM is given as context (the schema question)

The model needs to know the WASiM model format — but the tool does **not** paste the
raw JSON Schema on every turn. The context is layered, cheapest-first:

1. **A compact "authoring guide"** *generated from the schema* (so it can't drift): the
   primitive/`NodeRule` catalog, trait fields, the distribution roster, the builtin
   function and time-ref list, the expression-AST shape, and the simulation-settings
   fields — each with a one-line description. This is far more token-efficient and
   semantically richer than raw JSON Schema, and regenerating it from the schema on
   build keeps it honest.
2. **A short set of few-shot exemplars** — 2–3 complete, engine-validated `model.json`
   documents (e.g. the repo's `retirement_planning.json` and `two_tank_hydraulic.json`)
   that show idiomatic structure end-to-end.
3. **The engine's fidelity constraints (§14)** stated explicitly: no Script element, no
   `OnEvent` trigger, Cell outputs are mass not concentration, External distributions
   need a fallback table, etc. — so the LLM does not propose constructs the engine
   can't run.
4. **The current model** (when editing an existing document) and the **conversation**.

**Prompt caching makes "bundle the schema" cheap.** On Anthropic, items 1–3 are stable
across a session, so they go in a **cached prefix** (`cache_control: {type:
"ephemeral"}`, in the `tools`→`system`→`messages` render order, stable content first).
The first call pays to write the cache; every subsequent turn in the interactive loop
reads it at ~0.1× cost. (Mind the minimum cacheable prefix — 4096 tokens on Opus 4.8,
2048 on Sonnet 5 — which the authoring guide comfortably exceeds.) OpenAI/Azure apply
automatic prefix caching to the same stable-first layout. Use the provider's
token-counting endpoint to keep the guide within a sane budget; if the schema ever
outgrows the context window, retrieve only the relevant slices per turn (a
`get_schema_section` tool, §17.3) instead of sending all of it.

**Why not rely on structured-output/JSON-schema constraints?** Providers can constrain
a response to a JSON Schema, but WASiM's schema is **recursive** (nested
containers/submodels, a self-referential expression AST) and uses constructs those
constrained-decoding features don't support. So the copilot does **not** lean on
one-shot constrained generation for the whole document — it relies on **tool-calling +
engine validation** (§17.3), which handles recursion and gives precise, correctable
errors. Constrained output is still useful for *small, flat* sub-tasks (e.g. emitting a
single distribution object).

### 17.3 The interactive loop

The copilot is an **agentic loop** orchestrated by a controller (a dedicated worker or
the main thread) that bridges the LLM (network) and the engine (the existing sim
worker):

```
user: NL description ─▶ LLM ─▶ (tool call) propose_model / apply_edit
                                   │
                     controller ──┼─▶ sim worker: reconcile → { summary, validation, topo }
                                   │                     (and optionally: run → results)
                                   ▼
        LLM ◀── tool results (validation errors, run summary) ── iterate until clean
                                   │
                                   ▼
        ◀── proposed model + rationale ──▶  user reviews diff, Accept / Reject / refine
```

**Tools exposed to the LLM** (each backed by the engine or the store):

| Tool | Purpose |
|---|---|
| `get_schema_section(topic)` | Retrieve the authoring-guide slice for a primitive/distribution/function on demand (keeps the base prompt small) |
| `propose_model(model_json)` | Submit a full candidate model |
| `apply_edit(patch)` | Submit an incremental change to the current model (add/modify/remove elements) — preferred for edits, cheaper and more precise than regenerating the whole doc |
| `validate()` | Run the engine's parse + dimensional + graph validation → structured diagnostics (dangling refs, cycles, unit errors, unknown fields) |
| `run(config?)` | Run a quick simulation → a compact results summary (means/percentiles for key outputs) so the LLM can sanity-check behavior |

The loop runs the LLM's tool calls, returns **engine truth** (not the LLM's own
belief) as tool results, and repeats until `validate()` is clean (and, if the user
asked, `run()` looks sane). Cap iterations to avoid runaway loops; surface a spinner
with a cancel that terminates the in-flight request.

**Turn types the loop supports:**
- **Cold start** — "build me X" → the LLM drafts a full `propose_model`, self-corrects
  against `validate()`, and returns a working model.
- **Refinement** — "make the tank bigger and add a second inflow", "why does the
  balance go negative?" → `apply_edit` / explanation grounded in `run()` results.
- **Explain / review** — "what does this element do?", "is this dimensionally
  consistent?" — read-only, no model mutation.

### 17.4 UX and human-in-the-loop

- A **Copilot panel** (docked beside the Inspector) with the conversation and a
  prominent **model-diff preview** (JSON diff + a canvas overlay highlighting
  added/changed elements) for every proposal.
- **Nothing auto-applies.** The user explicitly **Accepts** a proposal (it enters the
  canonical model via the normal reconcile path, undoable) or **Rejects/refines** it.
  The copilot is a drafting surface, not an autonomous editor of the user's document.
- **`run()` is read-only and sandboxed** — a simulation has no external effects, so the
  LLM may call it freely; but the copilot never triggers anything outside the sandbox
  (no file writes, no network beyond the provider, no publishing).
- **Provenance** — accepted proposals can be tagged in the `view` metadata (§13.3) as
  LLM-assisted, and the conversation is exportable, so a reviewer can see how a model
  was derived. The generated `model.json` remains the same diffable artifact regardless
  of how it was authored.

### 17.5 Security & privacy

This feature sends data to a third-party provider and handles a secret, so it is
called out explicitly:

- **Bring-your-own-key, stored locally.** The API key is the user's own, kept in
  browser storage and sent **only** to the configured provider. It is never written to
  `model.json`, never committed, never sent to any WASiM-controlled server (there is
  none).
- **Browser-direct-call caveat (state it plainly).** Calling a provider's API directly
  from the browser exposes the key to any script running on the page and requires the
  provider's browser-access opt-in (e.g. Anthropic's
  `anthropic-dangerous-direct-browser-access` header / the SDK's
  `dangerouslyAllowBrowser` flag; OpenAI's equivalent). This is acceptable for a
  **local, single-user, BYO-key** tool and is consistent with WASiM's no-server thesis
  — but the Settings panel must say so, and teams who can't accept it are offered an
  **optional thin proxy** (the one place a server may appear) that holds the key and
  forwards requests.
- **Model content leaves the machine.** The problem description and the `model.json`
  are sent to the provider. The panel makes this explicit and offers a **"description
  only — don't send the model"** mode for sensitive models, plus an optional redaction
  pass for element names/descriptions.
- **Prompt-injection hygiene.** Text the LLM produces is treated as an *untrusted
  proposal*: it only ever flows into the model through `propose_model`/`apply_edit` →
  engine validation, never executed as code and never applied without the user's
  Accept. The engine's schema/validation is the security boundary.

### 17.6 Fidelity — the copilot cannot exceed the engine

The same rule as the rest of the tool (§14): the copilot is told the engine's
capabilities and **non-goals**, and — crucially — any construct it hallucinates that
the engine can't parse is rejected by `validate()` and corrected in-loop. So the copilot
can never smuggle in a Script element, an unsupported trigger, or an invented field: the
engine is the gate. When a user asks for something the engine genuinely can't do, the
copilot says so and points at the roadmap item rather than emitting a model that
silently misbehaves.

### 17.7 Phasing

- **Phase 1 — Generate & validate.** Endpoint config + provider abstraction; cold-start
  `propose_model` with the `validate()` loop; diff-preview + Accept/Reject. This alone
  delivers "describe it → get a working model."
- **Phase 2 — Converse & refine.** `apply_edit`, `run()`-grounded explanations, and
  multi-turn refinement over the current model.
- **Phase 3 — Depth.** On-demand `get_schema_section` retrieval for very large schemas;
  provenance metadata; the optional key-holding proxy; redaction/"description-only"
  privacy modes.

### 17.8 Copilot non-goals

- It does **not** autonomously edit or run the user's document — every change is
  user-Accepted, and it triggers no effect outside the simulation sandbox.
- It does **not** bypass engine validation — the engine, not the LLM, decides what is a
  valid model.
- It does **not** exfiltrate or embed the API key anywhere but the configured provider,
  and the key never enters `model.json`.
- It does **not** replace the direct authoring surfaces (§2–§6) — it is an on-ramp and
  an assistant beside them, not the only way in.

---

### Appendix — mapping summary (GoldSim authoring pattern → WASiM tool surface)

| GoldSim authoring pattern | WASiM tool surface | Section |
|---|---|---|
| Graphics pane / influence diagram | Canvas (SVG + Dagre, editable) | §2 |
| Element toolbar | Palette (primitive catalog) | §3 |
| Element browser (containment / type) | Model Browser | §4 |
| Element property dialogs | Inspector (per-rule editors) | §5 |
| Expression fields that auto-create influences | Expression editor w/ reference autocomplete | §6 |
| Units Manager / dimensional checking | Units mode + dimensional inspector | §7 |
| Causality Sequence | Topo-order view + Issues panel | §8 |
| Simulation Settings (time, Monte Carlo) | Settings dialog (+ timebase, LHS, weights, results spec) | §9 |
| Run Control | Toolbar Run + worker status | §10 |
| Result elements (Time History / Distribution / stats) | Results workspace (Recharts, results_spec) | §11 |
| Sensitivity / Optimization | Sensitivity (exists) + Optimization UI | §11 |
| Dashboards / Player | Author-configurable Dashboard | §12 |
| Containers / SubModels | Container drill-down + submodel interface editor | §1.3, §5.8 |
| `.gsm` project file | `model.json` (+ optional `view`) | §13 |
