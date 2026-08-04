# Lens Hinting — Frontend Reconciliation

**Status:** design note **+ implemented** (see §8 for what landed). **Supersedes the
stamping-matcher model** that earlier versions of this file described.
**What changed:** the consensus moved lenses from a *persisted classification* (a matcher that
stamps `view.lens` / `lens_role` into the model at import) to a **pure, ephemeral UI hint** — a
side-effect-free `lensHint(container, doc)` that reads only fields already in the emitted JSON
and writes nothing back. The emitter is entirely uninvolved.
**Canonical spec for the hint model:** *WaSim Lenses — UI Hint Model & Modeling-Type
Signatures* (`WASIM_LENS_MODELING_TYPES.md`). **This note does not restate it.** Its job is the
part that doc leaves open: **reconciling the hint model with the four lenses already shipped in
the frontend**, where `lens_role` / `view.lens` are currently load-bearing.
**Grounded in:** `frontend/src/lenses/behaviors/*.ts`, `frontend/src/model/schema.ts`,
`frontend/src/store.ts`, `schema/wasim-schema-v2.json` (all cited inline).

---

## 0. The pivot, in one paragraph

A lens is now a **hint, not a stored property**. `lensHint(containerId, doc) →
{ lens, confidence, overlays }` is computed on container activation, re-computed when contents
change, and never written back. `confidence` is advisory (no threshold to tune). Overlays
(control, Monte-Carlo/uncertainty) are **computed joins over an owner element's ref fields**, so
no element ever carries more than one — and usually zero — lens tags. This cleanly retires two
problems the matcher model carried: the per-element multi-lens question (overlays need no tags)
and the fit-threshold question (advisory confidence needs none). See §5 for the full reversal
log. The rest of this note is the *cost of getting there* given what's already shipped.

**One framing point that governs everything below (type vs. instance).** When the specs say a
construct "routinely coexists with" or "commonly surfaces as an overlay on" another lens, that is
a statement about a *type* recurring across models — the `pid` type is the dominant paradigm in a
control-heavy container in one model and a lone overlay on a stock-flow plant in another; the
`event` type appears under reliability in one container and discrete-event in another. It is
**never** a single element instance holding two lens memberships. Classification is
**per-container and instance-singular**: `lensHint` picks one dominant lens for a container, and
each element belongs to exactly that one. An **overlay** is a computed *view* — a highlight set
joined from an owner element's refs (a `pid`'s `input`/`setpoint`) — that touches plant elements
**without relabeling them**. Both `discrete-event` and `control-systems` are first-class lenses;
"overlay" is a rendering mode for a non-dominant lens (or a non-topology facet like MC), not a
separate registry and not a second membership. This is why the withdrawn "secondary roles" idea
was correctly dropped, and why nothing below needs multi-lens plumbing.

---

## 1. The starting reality: the four shipped lenses persist their roles

The consensus doc reads as greenfield, but in the current frontend `lens_role` and `view.lens`
are **not free to drop** — they are how the shipped lenses work:

| Lens | Reads stored `lens_role` at | Stored by | Active lens from |
|---|---|---|---|
| `stock-flow` | `behaviors/stockFlow.ts:19,26` (`'flow'`,`'stock'`) | `stockFlowTemplates.ts` stamps `lens`+`lens_role` | — |
| `metapop` | `behaviors/metapop.ts` (`'compartment'`,`'transition'`,`'mixing'`,`'coupling'`) | palette insert + templates | — |
| `reliability` | `behaviors/reliability.ts:12,44` (`'component'`,`'redundancy'`) | `reliabilityTemplates.ts` | — |
| `decision` | `behaviors/decision.ts:12,15` (`'decision'`,`'objective'`) | palette insert | — |
| *all* | — | — | `view.lens` via `useActiveLens` (`store.ts:441,737`) |

`lens_role` is a single optional string per element (`model/schema.ts:83`). So **adopting the
hint model is a migration, not an addition**: each behavior must derive its roles by *computation*
instead of *lookup*, the templates must stop stamping, and `useActiveLens` must derive the active
lens from the activation stack + `lensHint()` instead of from `doc.view.lens`.

**This is tractable — the roles are recoverable from structure** (§2) — but it is real rework of
shipped code, and it must be sequenced so the existing lenses keep working through the change.

---

## 2. How each lens computes its roles without stored tags

The one thing the consensus doc omits: it specifies signatures for the two *new* paradigms
(`discrete-event`, `control`) but not for the four that already ship. Here is where each one's
signal actually lives, using only fields in the emitted JSON.

| Lens | Computed role source (no `lens_role` needed) |
|---|---|
| `stock-flow` | `primitive:"stock"` → `stock`; any node id appearing in some stock's `inflows`/`outflows` → `flow`. Pure topology. |
| `metapop` | stocks over a patch axis → `compartment`; the flows between them → `transition`; a **square 2-D array** feeding a force term → `coupling`; that term → `mixing`. Already computed structurally today (`metapop.ts` deliberately uses "structural fields, no expression eval"). |
| `decision` | the top-level **`optimization` block** is the signal: `optimization.variables[].element_id` → `decision`; the objective element → `objective`. No per-element tag required. |
| `reliability` | **`primitive:"gate"`** (fault-tree nodes, `reliability.ts:21`, `op ∈ {and,or,n_vote}`) plus the `event` components they aggregate. **The gates are the discriminator** — see §3. |

Two honest limits fall out:

- **Natively-authored models carry no `source_type`.** The consensus doc's "corroborating
  prior" (`source_type ∈ {...}`) is (a) absent on hand-built models and (b) **read nowhere in
  `frontend/src` today** (grep is empty). So every signature must stand on
  `primitive`/`value_rule`/topology alone; `source_type` is at best a *bonus* prior on
  emitter-produced models, never a load-bearing input.
- **`pid` and `status` have no structured inspector** (`Inspector.tsx:97-105` handles `event`
  and the scalar rules; `pid`/`status` fall to raw JSON). A usable `discrete-event` or `control`
  lens therefore needs new inspector editors — the hint function alone doesn't make the paradigm
  authorable.

---

## 3. The discriminator the consensus doc still needs: reliability vs discrete-event, per container

This is a **container-classification** gap, not an element-level collision (see the type-vs-
instance framing in §0). The `event` type legitimately recurs across paradigms — a reliability
container and a discrete-event container each hold `event` instances, in *different* containers.
No instance is ever both; the classifier picks one dominant lens per container. But *because* the
same type recurs, the classifier **cannot score on element type alone**. The new discrete-event
predicate is exactly that kind of type test:

```
isDiscreteEvent(el) := el.primitive === "event" || (el.primitive === "node" && el.value_rule === "status")
```

A reliability *component* is itself an `event` (a failure FSM — exactly what `reliability.ts`
reads as a survival time-history). Aggregate that predicate as "fraction of interior elements
matching" and a reliability container earns a **high discrete-event score**, because its
components are events. Today the stored `lens_role === 'component'` tag keeps the two paradigms
apart; drop it — and with `source_type` unread/absent (§2) — and the container scorer
**systematically mis-hints reliability containers as discrete-event**. Note this is the strongest
argument *for* structural scoring: since the type recurs across containers, the surrounding
structure, not the element's type, must drive the per-container score.

The discriminating structure is already in the model — reliability and discrete-event differ by
what the events feed:

- **Reliability** = `event` components aggregated by a **fault tree of `gate` primitives**
  (`and`/`or`/`n_vote`). Presence of gates over events is the reliability signature.
- **Discrete-event** = events wired to each other by **`on_event` trigger chains** and
  set/reset **`status`** latches — *no* fault-tree gate aggregation.

So the signatures must be made mutually aware, not defined in isolation:

```
reliabilityScore(container) ∝  #{gate primitives}  +  #{events that fan into a gate}
discreteEventScore(container) ∝ #{event/status els NOT feeding a fault-tree gate}
                              +  #{on_event trigger chains among them}   // the strong signal
```

An `event` that fans into an `and`/`or`/`n_vote` gate should count toward **reliability**, not
discrete-event. The consensus doc's coarse `primitive === "event"` test needs this
gate-awareness before it's safe to ship alongside the existing reliability lens.

(Note the parallel near-miss the consensus doc already caught: `controller_mode === "on_off"` is
a hysteresis latch, structurally adjacent to a `status` latch — disambiguated by `value_rule`.
The reliability/discrete-event case is the same *kind* of shared-construct disambiguation one
level up — at the container score rather than a single field — and needs the same explicit
treatment.)

---

## 4. Can we A/B the two hinting models? (short answer: yes, cheaply — but not as two stacks)

The two "versions" are **not** independent implementations:

```
persisted-matcher model  =  lensHint()  +  a write-back/stamping layer
ephemeral-hint model     =  lensHint()  alone
```

The expensive, error-prone part — the **signatures and scoring** (§2, §3) — is *identical* in
both. The only difference is whether the computed result is written into `view.lens`/`lens_role`
and round-tripped. So:

- **A full parallel A/B (two production stacks) is too much, and worse, self-defeating.** The
  persisted arm re-imports everything the ephemeral model was designed to shed: a schema surface,
  round-trip, the four-lens migration *twice*, override-vs-stored-tag conflict rules, and
  **staleness** (a stamped `lens_role` that silently goes wrong after the user edits the graph —
  precisely the failure a recompute-on-change hint cannot have). Carrying that just to compare is
  a bad trade.
- **The cheap, honest experiment is one computed core + instrumentation.** Ship the ephemeral
  `lensHint()`, keep the per-lens scoring functions swappable behind one interface, and log the
  metric that actually matters: **override rate** (how often the user rejects the hint), sliced by
  paradigm. That directly measures hint quality — which is the *only* thing an A/B here could tell
  you — without a second persistence model. If you want to compare *scoring strategies*
  (e.g. structure-only vs. structure + `source_type` prior on emitted models), that is an A/B
  *inside* `lensHint()`: swap the scorer, hold everything else fixed, compare override rates.

**Recommendation:** don't build the persisted arm. Build the pure core, gate the two new
signatures behind the gate-awareness fix in §3, and instrument override rate to validate — and
to A/B scoring variants if desired. That captures all the evaluation value of an A/B at a
fraction of the cost, and never resurrects the staleness the hint model exists to avoid.

---

## 5. Reversal log — what this note drops from the earlier matcher model

Everything in this file's previous (stamping-matcher) version is superseded as follows:

| Earlier proposal (this file, matcher era) | Now |
|---|---|
| A matcher that **stamps** `view.lens` + `lens_role` at import | **Dropped.** Pure `lensHint()`, nothing stamped. |
| Per-container `view.lens` **schema extension** | **Dropped.** Scope is per-container by *activation*, not by a stored tag. |
| Emitter emits a `source.format` provenance **hint** | **Dropped.** Hint infers from content; for GoldSim a format prior was inert anyway. |
| Per-element multi-lens / "secondary roles" for overlays | **Dropped.** Overlays are computed joins over ref fields; no element needs a second tag. |
| Open question: fit **threshold** / tie-break | **Dissolved.** `confidence` is advisory; a wrong hint is a one-click override. |

What this note **adds** on top of the consensus doc: the migration path for the four shipped
lenses off stored tags (§1–§2), the **reliability ⊂ discrete-event** collision and its
gate-aware fix (§3), and the A/B recommendation (§4).

---

## 6. What is unchanged

- **The engine.** It never read lens tags; it still doesn't.
- **The emitter contract.** `EMITTER_HANDOFF.md` / `notes_to_transpiler.md` stand as-is — this is
  now purely a consumer of the existing JSON, with *zero* emitter asks (the `source.format` hint
  is withdrawn).
- **The manifest/behavior architecture.** `behaviors/` plugins remain the code half of a lens;
  what changes is that their role inputs become *computed* rather than *read from the model*.

---

*Provenance: lens-tag dependence and role sources verified by direct read of
`frontend/src/lenses/behaviors/{stockFlow,metapop,reliability,decision}.ts`,
`frontend/src/model/schema.ts:83`, `frontend/src/store.ts:441,737`, and
`frontend/src/components/inspector/Inspector.tsx:97-105`. `source_type` absence in `frontend/src`
verified by grep. `event`/`status`/`pid`/`trigger` shapes verified in
`schema/wasim-schema-v2.json`. Hint-model semantics per the consensus doc
`WASIM_LENS_MODELING_TYPES.md`.*

---

## 8. Implementation status (landed on `claude/emitter-lens-targeting-t2r2s1`)

Built to the agreed scope — **all six lenses (full classifier), thin activation, lens-first
then Gap A** — in the frontend (`frontend/`). Every increment is committed green:
`tsc -b` clean and the vitest suite passing (124 baseline → **170**). The only build step that
fails is `vite build`, on a **pre-existing missing WASM artifact** (`engine/pkg/wasim_engine`,
needs `npm run build:engine`) — unrelated to this work; all app modules, including the new
manifests, transform cleanly.

**Done**
- **`src/lenses/hint/`** — the pure `lensHint(container, doc)` classifier over six structural
  signatures + `activeLensId` priority resolver; `structure.ts` predicates; `scope.ts` container
  interior; `roles.ts` computed roles + the `withComputedRoles` bridge (stored roles authoritative
  when present → non-regressive; computed fills in only for untagged imports). 27 tests incl. the
  migration parity gate (computed roles never contradict stamped ones; all four shipped lenses stay
  warning-clean on their templates).
- **Loader wiring** — `invariants`/`resultReadouts` see injected computed roles, so the shipped
  lenses' governance works on imported/untagged models; `connect` deliberately un-injected (it
  returns the doc; injecting would persist roles).
- **Store wiring** — `useActiveLens` resolves via `activeLensId`: explicit `view.lens` wins
  (non-regressive), else the hint for the drilled container; non-persisted `activeContainerId` +
  `setActiveContainer`, wired to GraphTab's submodel drill → **per-submodel lenses**. Selectors
  stay render-loop-safe.
- **Two new lenses** — `discrete-event` and `control-systems` as manifest + behavior plugins
  (structural invariants: broken `on_event` chain; open control loop), registered and picker-listed.
- **Monte-Carlo loop** — `createSubmodelFromSelection` (submodel + `n_realizations` + auto-derived
  interface), store `groupIntoSubmodel` (drills into the new submodel), and a **Cmd/Ctrl-G**
  keyboard entry point on the current multi-select.

**Deliberately deferred (documented, not silently dropped)**
- The **lasso "loop" stroke** — a net-new canvas interaction that can't be unit-verified without a
  running canvas; MC grouping ships via the keyboard gesture on the existing multi-select instead.
- **Structured inspectors for `pid` / `status`** — the two new lenses classify and validate but
  those constructs still edit via raw JSON (`Inspector.tsx` has no case yet).
- **Inert-until-activated gating** and **sticky localStorage overrides** (thin activation only).
- **Metapop computed-role precision on imports** — the scalar-`coupling` / `mixing` heuristic is
  conservative; stored roles cover authored docs, but imported metapop role-tagging is rough.
- **Gap A (`effects[]`) is emitter-side** (`re-gsm`/`emit.py`) — no schema change needed here
  (`effect_spec` already exists); the discrete-event lens's effect-edge rendering lights up once it
  lands.
