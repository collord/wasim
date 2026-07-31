# Agent-Based Modeling on WaSim — evaluation + a working proof

This documents (1) how ABM maps onto WaSim's execution model, (2) an evaluation of the
krABMaga Rust ABM framework and how much of what it does WaSim can do, and (3) a proof —
`engine/tests/network_sir_v2.rs` — that network contagion runs on today's engine with **no new
primitives**.

## 1. The reframe: agents are array members, not realizations

The apparent tension — "N agents interact, but realizations are independent" — dissolves once
agents are mapped onto the **array axis**, not the realization axis. Realizations stay the
independent, embarrassingly-parallel, bit-reproducible outer loop (the moat). Agents are cells of
a dimensioned element inside one realization, and **interaction is reduction over the agent axis**.

The design question then reduces to one axis: *what kind of interaction, and does it close a cycle
within a single instant?*

| Interaction | Mechanism | Engine work |
|---|---|---|
| Lagged mean-field (respond to population aggregate) | array + `sum`/`mean` reduction | none |
| Lagged network/local (respond to neighbors) | array + adjacency matrix + **axis-selective reduction** | none (shipped) |
| Instantaneous / equilibrium (within-step mutual dependence) | array + **within-step relaxation loop** | one new bounded fixed-point node |
| Heterogeneous types / sequential update | **N orchestrated submodels** | scheduler work; last resort |

`lag` is the step-boundary commit: an agent reading `lag(aggregate)` sees last step's state, so
lagged interaction has no cycle and needs nothing new. Only genuinely instantaneous simultaneity
(market clearing, congestion pricing) needs the within-step fixed-point loop — and even there the
sub-steps are *relaxation iterations*, not per-agent machinery.

## 2. krABMaga evaluation

[krABMaga](https://github.com/krABMaga/krABMaga) is a MASON re-implementation in Rust (MIT). Core:
`Agent` + `State` traits; a **sequential, priority-ordered scheduler** (`Priority { time, ordering }`,
agents run one at a time, self-reschedule at `time + 1`); **double-buffered fields** (continuous
`Field2D`, dense/sparse object grids, `NumberGrid2D` scalar fields, `Network`/`HNetwork` graphs,
`kdtree` spatial index); and an `explore` module (sweep, genetic, random, Bayesian optimization,
MPI-distributed).

**The architectural fault line.** WaSim is synchronous and declarative (`lag` reads the previous
snapshot; the step boundary is the swap). krABMaga is imperative and sequential (agents mutate
shared fields in priority order). But its **double-buffered fields + `update()`** *are* WaSim's
`lag`/step-commit pattern — so the **synchronous subset transfers**. What does not transfer is the
asynchronous, order-dependent, self-rescheduling event model.

This is the classic **"patches vs turtles"** split: WaSim natively does **field ABM and network
ABM** (agents as cells or graph nodes, synchronous, aggregate/neighbor-mediated); it does **not**
natively do **mobile-agent ABM** (turtles in continuous/grid space, immediate mutation,
order-dependent conflict resolution, individual schedules, dynamic populations).

**Scorecard on the six shipped examples:**

| Example | Structure | WaSim today | Why |
|---|---|---|---|
| Forest Fire | scalar grid CA, synchronous | Native | 2-D array + `lag`; spread = neighbor stencil via axis reduction |
| Virus on Network | fixed graph, SIR | Native | adjacency + state vector + axis reduction (**proven below**) |
| Ants Foraging | pheromone field + mobile ants | Half | pheromone diffusion/evaporation natural; ant movement is the turtle half |
| Wolf-Sheep-Grass | grid, 2 species, birth/death, movement | Partial | grass = scalar field ✓; aggregate predator-prey ✓; spatial movement + reproduction + species heterogeneity awkward |
| Schelling | grid, async move-to-empty | Awkward | synchronous approximation changes the dynamics; move-to-empty is sequential |
| Flockers (Boids) | continuous space, radius neighbors | Weak | only as an O(N²) all-pairs matrix; small N, no spatial index |

Net: **2 of 6 land cleanly, 2 contribute their field components, 2 fight the engine** — and the
clean wins are exactly the synchronous field/network models.

**Borrowing code (MIT).** The paradigm impedance is high, so most engine code does not transfer:
fields are imperative mutable containers; WaSim's graph is a *static adjacency array*, not a
mutable `HashMap`. Take the *design*, not the struct. The genuinely valuable borrows are (a) the
six examples as a **conformance/benchmark suite** (port Forest Fire + Virus-on-Network; keep
Boids/Schelling as honest boundary markers), and (b) the **`explore`/optimize module** (sweep, GA,
Bayesian) as a reference for the decision/VOI lens — which isn't ABM at all. The `kdtree` is
borrowable but only matters for the mobile-agent corner WaSim should *not* chase.

## 3. The proof — `engine/tests/network_sir_v2.rs`

Deterministic threshold-SI (susceptible → infected, no recovery) on an explicit 6-node graph: a
path 1–2–3–4–5–6 **plus a chord 2–5**. The rule per step is
`infected[i] ← infected[i] OR (any neighbor infected last step)`, with neighbor aggregation

```
exposure[i] = Σ_j  W[i,j] · infected_prev[j]
```

expressed as an elementwise product over the `[Node, NodeB]` adjacency grid **reduced over axis 2
(the neighbor axis)** — the axis-selective reduction from `axis_reduction_v2`. Two equal-size
dimensions (`Node`, `NodeB`) give the matrix two distinct axes of the same node set; `inf_prev`
(over `Node`) is reindexed onto `NodeB` by a `vector_map` + `index` gather — align-by-name matrix
algebra with **no transpose primitive**. State advances through `lag`.

Every construct is already in the engine: `vector_map`, `index_ref`, `index`, `lag`, elementwise
ops, and axis-selective `sum_array`. **No new primitives.**

Because the dynamics are deterministic, every value is a BFS frontier from the seed (node 1):

```
step:            1    2      3        4      5   6
newly infected:  {1}  {2}    {3,5}    {4,6}  -   -
total infected:   1    2      4        6     6   6
```

The test asserts the exact `total` history `[1,2,4,6,6,6]`, and — the load-bearing check — that
**node 5 infects at step 3 via the chord**. On the bare path node 5 is 4 hops from the seed and
would not infect until step 5; `infected#5 = [0,0,1,1,1,1]` proves the reduction is using the
graph's edge structure, not linear diffusion. All assertions pass.

This closes rows 1–2 of the interaction table end-to-end: **lagged mean-field and network/local
interaction run on today's engine.** The only open engine question for ABM is the within-step
relaxation loop (row 3) for genuine instantaneous simultaneity.

## 4. Strategic conclusion

"ABM" is not one lens — it is at least two, and the krABMaga field taxonomy draws the line:

1. **Field + Network lens** (patches, diffusion, cellular automata, graph contagion, synchronous)
   — fits WaSim natively and, crucially, **composes with stock-and-flow**: stocks-and-flows on
   network nodes is the *metapopulation* model class (**network / metapopulation epidemiology** on
   a contact/mobility graph, supply-chain contagion, inter-regional diffusion). This is the natural
   second lens after stock-and-flow, not a separate product.

   **Caveat — this is topology, not geometry.** WaSim has no spatial data type: no coordinates, no
   distance metric, no radius/nearest-neighbor query, no raster or continuous field. "Space" here
   means *connectivity* — who is coupled to whom, via an adjacency/coupling matrix (or the
   `cell`/`medium`/`flux` compartment-transport primitives, which are likewise a cell-and-link
   network, cells carrying a `volume` but no position). Any real geometry — patch coordinates,
   gravity/distance kernels — must be **precomputed off-engine** and fed in as a constant coupling
   matrix `W = f(D)`; the engine consumes baked geometry but cannot compute it. So the honest claim
   is metapopulation/network models, not GIS-style spatial modeling.
2. **Mobile-agent lens** (turtles in space, async, dynamic populations) — fits poorly; doing it
   means bolting an imperative, order-dependent sidecar onto the engine, which sacrifices the
   synchronous determinism and diffability that are the moat. Disclaim it, don't chase it.

The answer to "how much of krABMaga's ABM can WaSim do" is: **the synchronous field-and-network
half, cleanly — and that half is the half that composes with the stock-and-flow thesis into
metapopulation modeling.** The mobile-agent half it cannot do without sacrificing the moat, and
should not try.

## 5. Authoring an ABM lens: the Metapopulation lens as a `.json` manifest

The frontend ships a working lens system (`frontend/src/lenses/`, `WASIM_LENS_MANIFEST_SPEC.md`).
This section is grounded in that code as it stands — the manifest shape below is the one the loader
(`loader.ts`) actually compiles and the JSON Schema (`schema/wasim-lens-manifest-v1.json`) actually
validates — and it answers one concrete question: **how much of an ABM (metapopulation) lens can be
authored as a `.json` file, and what genuinely needs first-party code?**

### 5.1 The trust boundary: what a `.json` lens is, and is not

A lens is **three parts across a trust boundary**:

- a **manifest** `lenses/<id>.json` — *data*: `version`, `id`, `label`, `tagline`, `behavior` (names
  a code plugin, never supplies code), `palette` (sections whose items `ref` an
  `element-registry.json` key and stamp a `role`), `roleLabels`, `glyphByRole`, `templates` (ids),
  `preferredResult` (`roleOrder`). Manifests re-theme and are safe to load from users/projects.
- a **behavior** `behaviors/<id>.ts` — *code*: a `LensBehavior` = `{ id, invariants?, connect?,
  resultReadouts? }`, registered by name in `behaviors/index.ts`. This is the executable half.
- **templates** (`<id>Templates.ts`) — canonical starting docs built through the real expression
  parser (`parseExpr`/`printAst`/`refsOf`), tagged `lens_role` per element so they open with zero
  warnings.

The boundary is load-bearing for authoring, and the current code enforces it in two places that a
sketch must respect:

1. **An imported `.json` is pure data.** `customLenses.ts`/`loader.ts` let a user import a manifest
   at runtime, but `registerManifest` compiles it against the *existing* registry and behavior
   registry. A manifest's `behavior` field may only **name** a built-in plugin (`stock-flow`,
   `reliability`, `decision`) — `resolveBehavior` throws on an unknown id — and its `templates` are
   **ids into a compiled-in map**, so a pure `.json` cannot ship a new `two-patch-sir` starting doc.
   A dangling palette `ref` also throws. So an imported metapop `.json` can re-theme and reuse a
   built-in behavior, but it cannot introduce metapop-specific checks, wiring, readouts, or
   templates.
2. **A built-in behavior binds to specific `lens_role` strings.** `stockFlowBehavior` checks and
   wires elements whose `lens_role` is exactly `'stock'` or `'flow'`; `reliability` keys off
   `'component'`/`'redundancy'`; `decision` off `'decision'`/`'objective'`. This is the hinge that
   makes a data-only ABM lens genuinely useful: **relabel the surface, keep the underlying role**,
   and the borrowed behavior still fires.

**The key realization is unchanged and now precise: MetaPop = the stock-flow lens, dimensioned over
a patch axis, plus a coupling matrix and a mixing reduction.** A compartment *is* a `stock`; a
transition *is* a flow with the existing inflow/outflow `connect` gesture. The second lens is mostly
the first lens with an extra axis — which is exactly why Tier 1 below works at all.

### 5.2 Tier 1 — the metapop lens as pure data (importable today, zero code, zero engine change)

Because a compartment is a stock and a transition is a flow, a metapop authoring surface can be
built **entirely as a manifest** that reuses the shipped `stock-flow` behavior. The trick is to keep
the two behavior-bearing roles canonical (`stock`, `flow`) while relabeling them, and to give the
two *new* nouns (coupling, mixing) their own roles purely for glyphs — the stock-flow invariants
ignore any role that is not `stock`/`flow`, so those extra roles cost nothing.

```json
{
  "version": "1.0.0",
  "id": "metapop",
  "label": "Metapopulation",
  "tagline": "Compartments on a coupling network — SIR/SEIR across patches, network-mediated mixing.",
  "behavior": "stock-flow",
  "palette": [
    { "section": "Compartments", "items": [
      { "ref": "stock", "label": "Compartment (S / I / R)", "role": "stock" }
    ] },
    { "section": "Transitions", "items": [
      { "ref": "expression", "label": "Transition", "role": "flow" }
    ] },
    { "section": "Network", "items": [
      { "ref": "expression", "label": "Coupling network (W)", "role": "coupling" },
      { "ref": "expression", "label": "Force of infection", "role": "mixing" }
    ] },
    { "section": "Parameters", "items": [
      { "ref": "constant", "label": "Parameter (β, γ …)", "role": "parameter" },
      { "ref": "stochastic", "label": "Uncertain input", "role": "parameter" }
    ] }
  ],
  "roleLabels": {
    "stock": "Compartment", "flow": "Transition",
    "coupling": "Coupling network", "mixing": "Force of infection", "parameter": "Parameter"
  },
  "glyphByRole": { "stock": "box", "flow": "valve", "coupling": "hex", "mixing": "circle" },
  "preferredResult": { "roleOrder": ["stock"] }
}
```

Every field above is real: the palette `ref`s (`stock`, `expression`, `constant`, `stochastic`) are
`element-registry.json` keys with `structured` editors; the glyphs (`box`/`valve`/`hex`/`circle`)
are all in `NodeShape`, so **no new glyph is needed**; `preferredResult.roleOrder: ["stock"]` opens
Results on a compartment's trajectory — the epidemic curve — the same simulate-first gesture
stock-flow uses for a stock. `templates` is omitted deliberately (see below).

**What Tier 1 gives you, today, with no code:**
- A correctly-themed palette (Compartments / Transitions / Network / Parameters) and inspector
  headers ("Compartment", "Transition", …) via `roleLabels`.
- The **stock-flow `connect` gesture**: because transitions carry `role: "flow"` and compartments
  `role: "stock"`, dragging a transition onto a compartment wires it as an inflow, and a compartment
  onto a transition as an outflow — real inflow/outflow accounting, for free.
- The **conservation / reconciliation invariants** from `stockFlowInvariants`: a transition not
  connected to any compartment warns ("a flow must move something in or out"); a compartment with no
  transitions warns ("it can never change"). This is the population-conservation check, inherited.
- Coupling and mixing render as hex / circle nodes and are authored as `expression`s.

**What Tier 1 does *not* give you (the honest gaps):**
- **No metapop-specific invariants.** The coupling-squareness check, the transposed-reduction guard,
  and "a mixing term must reference both a coupling matrix and a prevalence compartment" are not
  expressible as data — they are graph/shape analysis, i.e. behavior code (Tier 2).
- **Borrowed warning text leaks.** `stockFlowInvariants` emits the literal words "Flow" / "Stock",
  so a mis-wired transition warns as *"Flow … is not connected to a stock"*, not *"Transition …"*.
  Cosmetic, but a real tell that the behavior is stock-flow underneath.
- **No shipped templates.** A pure `.json` can only reference *existing* template ids; it cannot add
  `two-patch-sir`. The author starts from a blank canvas (or a stock-flow template).
- **No metapop readouts** (R₀, attack rate) — those are `resultReadouts` code.
- **The coupling matrix `W` is authored as a raw `matrix()`/`vector_map` expression.** The
  dimensioning itself is supported today (declare a `Patch` dimension in Settings; the inspector's
  "Array over" makes a compartment a per-patch vector), but there is no structured 2-D matrix
  widget — `W` is typed as an array comprehension in the expression editor.

So Tier 1 is a **real, shippable, importable metapop lens** — it just borrows stock-flow's brain.
For a teaching or exploratory surface over the constructs the `network_sir_v2` proof already
exercises, it is enough.

### 5.3 Tier 2 — the first-party additions that make it a true metapop lens (implemented)

Everything Tier 1 can't do reduces to **additive lens-layer code**, in exactly the mold of the three
shipped behaviors, and none of it touches the Rust engine (§5.4). This tier is now **built and
shipped** as a fourth domain lens — the pieces below are the actual modules, and both templates are
validated end-to-end against the engine (`serializeModel` → `parse_v2`/`run_v2`).

- **`behaviors/metapop.ts` — a `LensBehavior`** registered in `behaviors/index.ts`, named by
  `"behavior": "metapop"` in the manifest (which uses domain roles `compartment` / `transition` /
  `coupling` / `mixing` / `parameter`, since the code now owns them):
  - `invariants` — the metapopulation twin of `stockFlowInvariants`, keyed off `lens_role`,
    `inflows`/`outflows`, `inputs`, and `outputs[].dimensions` (structural fields — no expression
    eval): (a) **population conservation** — a transition must move population between compartments;
    an unmatched add (inflow of a compartment, outflow of none) is a leak → warning; (b) **mixing
    completeness** — a `mixing` term's dependency closure must reach both a `coupling` and a
    prevalence `compartment` (a cycle-safe BFS over `deps`, because the graph closes cycles through
    `lag`), else it is mean-field, not a network model; (c) **coupling well-formedness** — a
    dimensioned `coupling` must be square over two axes of equal size (checked against
    `doc.dimensions`); (d) **transposed-reduction guard** — a dimensioned `mixing` term must retain
    the coupling's *first* (home) axis and drop its *second* (neighbor) axis; still carrying the
    neighbor axis is the classic silent bug → warning.
  - `connect` — `transition → compartment` wires the transition as an inflow, `compartment →
    transition` as an outflow (the stock-flow `wireFlow` gesture, re-keyed to metapop roles). (The
    original sketch's "coupling → compartment sets the mixing source" gesture is deferred — it has no
    concrete backing field yet.)
  - `resultReadouts` — derived from result data with no engine change, like reliability's
    availability/MTTF: a per-compartment **peak** + **time-to-peak** row, plus an **Epidemic**
    summary — **peak prevalence** and **time-to-peak** of the aggregate infected curve, **epidemic
    duration**, **attack rate** (final ΣR / initial total population, a ratio over one consistent
    aggregate so it is scale-invariant), and an **R₀** estimate (β/γ, when both are exposed as
    parameters).
- **`metapopTemplates.ts`** — `two-patch-sir` (two coupled cities: scalar SIR stocks, a
  between-patch mixing weight carrying the epidemic from the seeded city; the "bathtub" of this lens)
  and `sir-on-network` (the `network_sir_v2` graph promoted to a full SIR with recovery, as real
  dimensioned S/I/R **stocks** over the patch axis with per-node infection/recovery flows — see §5.4),
  tagged per-element `lens_role` so they open warning-clean. Both are wired into the loader's
  `TEMPLATES` map and listed in the manifest's `templates`; the manifest is added to
  `BUILTIN_MANIFESTS` so the lens appears in the picker without an import.

The one subtlety the templates exposed: the array forms (the coupling matrix and the force-of-
infection reduction) are built as **explicit AST literals**, not via `parseExpr`, because the text
parser doesn't synthesise the engine's array ops (`index_ref` with `depth`, positional `index`
gather, axis-selective `sum_array(_, axis)`) — the same "raw" authoring §5.2 flagged. Scalar
expressions still go through the parser.

### 5.4 Isolated changes required

- **Rust engine: no new primitives.** §3's proof established that rows 1–2 of the interaction table
  (mean-field and network/local) run on today's engine with **no new primitives**, and the metapop
  lens targets exactly those two rows. The one open engine item on the ABM roadmap is the
  **within-step relaxation loop** (row 3, genuine instantaneous simultaneity — market clearing,
  congestion pricing), and SIR/SEIR metapopulation does not need it. What this work *did* change in
  the engine is array-evaluation **robustness** (below) — no new primitive, but three fixes so the
  natural dimensioned stock-and-flow authoring works instead of silently misbehaving.
- **Shared registry / schema: one *optional* addition.** A new `element-registry.json` entry
  (`matrix` / `coupling`) plus a structured 2-D editor for `W` would replace the raw-expression
  authoring of the coupling matrix noted in §5.2. This is the single genuinely new *widget*, and it
  is a frontend convenience, not a requirement — `W` is already authorable as a `matrix()`
  comprehension. Everything else in Tier 2 (`behaviors/metapop.ts`, `metapopTemplates.ts`, the
  `behaviors/index.ts` and `loader.ts` registrations) is **additive code isolated to
  `frontend/src/lenses/`** — no change to the engine, the store, or the on-disk model schema.
- **Engine array-eval fixes (done).** Building the `sir-on-network` template first surfaced three
  sharp edges in the engine's array evaluation; all three are now **fixed** (`engine/src/eval.rs`,
  `engine/src/engine_v2.rs`), which is what let the network template be rewritten as a genuine
  dimensioned stock-and-flow SIR sharing one formulation with `two-patch-sir`:
  1. **Element-wise math builtins.** `min` / `max` and the unary math builtins (`exp`, `abs`, …) now
     broadcast over a dimension via `Value::map`/`zip_with` — the same machinery the operators use —
     instead of collapsing every array arg to element 0 (`min([10,20,30],[1,2,3])` was `[1,0,0]`, now
     `[1,2,3]`). Scalar calls stay bit-identical; date/finance builtins remain scalar-arg.
  2. **Dimensioned stocks integrate per-cell.** A scalar `initial_value` on a dimensioned stock now
     broadcasts to every cell (was: cell 0 only), and inflows/outflows are summed as `Value`s so the
     stock keeps per-cell identity (was: collapsed to cell 0 via `as_scalar`). So a compartment can
     be a real `stock` over a patch axis with per-node flows — the network template now uses S/I/R
     stocks + `infect`/`recover` flows, dropping the earlier manual-Euler lagged-expression
     scaffolding and the `elapsed==0` seed. The conservation / `wireFlow` invariants now apply to
     *both* templates.
  3. Remaining, documented non-goals: array stocks with `floor`/`capacity` bounds use the existing
     aggregate bound-split (per-cell clamping is out of scope; SIR needs no bounds), and
     cumulative-flow/withdrawal side-channels record the aggregate flow total for array stocks (the
     per-cell *level* is correct).

### 5.5 The one honest boundary to encode in the UI

The `coupling` matrix is *topology, not geometry* (§4). Let the user author `W` as an
adjacency/edge list or import a precomputed mobility/distance kernel, and say plainly that real
coordinates are baked in upstream — the lens never pretends to hold geography. Everything else is a
manifest relabel (Tier 1) plus a behavior plugin and templates (Tier 2) over constructs the engine
and the `network_sir_v2` proof already exercise.
