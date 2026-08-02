# Emitter → Lens Targeting — Design Note

**Status:** design note (no implementation).
**Question it answers:** when a transpiler emits a WaSim model from a 3rd-party file
format (XMILE, GoldSim, Analytica), how does the resulting model land in the *right lens*
in the authoring UI — instead of always falling back to the General lens?
**Relates to:** [`WASIM_LENS_MANIFEST_SPEC.md`](WASIM_LENS_MANIFEST_SPEC.md) (the lens/registry
architecture), [`WASIM_LENS_IMPLEMENTATION_PLAN.md`](WASIM_LENS_IMPLEMENTATION_PLAN.md)
(the shipped lenses + round-trip), [`XMILE_MAPPING_SCOUT.md`](XMILE_MAPPING_SCOUT.md) and
[`EMITTER_HANDOFF.md`](EMITTER_HANDOFF.md) / [`notes_to_transpiler.md`](notes_to_transpiler.md)
(the emit side).

---

## 0. Summary

- **The gap is real and previously uncovered.** No document says how an emitter picks a
  lens, and the two sides were designed to not cross the boundary: the emitter docs speak
  only engine-facing schema, and the lens docs assume interactive authoring.
- **A lens is carried by two engine-ignored tags** — model-level `view.lens` and per-element
  `lens_role`. Today both are stamped *interactively* (palette insertion, templates, lens
  selection). An imported model has neither ⇒ it opens in **General** (correct, but the
  domain vocabulary is lost).
- **Recommendation: a deterministic lens matcher on the import boundary**, not the emitter
  and not an LLM. It reuses the two registries the frontend already owns (the element
  registry for role vocabulary, the behavior plugins for structural invariants), infers
  `lens_role` from structural fields, then scores each lens's fit and stamps the winner —
  or leaves the model in General when nothing fits.
- **The emitter stays engine-only.** Its one optional contribution is a cheap *format hint*
  (`source.format: "xmile"`) that the matcher may use as a prior, because XMILE is
  single-paradigm and maps 1:1 onto the stock-flow lens.
- **LLM classification is explicitly out of scope** (§8).

---

## 1. Current state — why nothing targets a lens today

**How a lens is persisted.** Two tags, both ignored by the engine and both round-tripped by
the frontend (`WASIM_LENS_IMPLEMENTATION_PLAN.md` line 40, "engine-ignored `view` block is
persisted and round-trips"):

| Tag | Scope | Source of truth | Set today by |
|---|---|---|---|
| `view.lens` | model | `store.ts:441,737` (`useActiveLens`) | user selecting a lens / opening a template |
| `lens_role` | element | `types.ts:28` ("so the lens round-trips") | palette insertion + templates stamp it |

The manifest schema makes the coupling explicit: a palette item's `role` is *"stamped onto
the inserted element's `lens_role`"* (`wasim-lens-manifest-v1.json:36`), and `view.lens` is
*"the lens id; persisted in `view.lens`"* (`:11`). **Absent `view.lens` ⇒ General**, which is
the identity projection over the whole registry (`WASIM_LENS_MANIFEST_SPEC.md` §2, principle
3: "'no lens' is the widest lens").

**The emitter side never sets either.** `EMITTER_HANDOFF.md`, `notes_to_transpiler.md`, and
the emit self-check list target only the engine-facing schema — elements, ASTs, references,
`submodel_stat`, durations. None mention `view`, `view.lens`, or `lens_role`. And
`XMILE_MAPPING_SCOUT.md` §1 maps `<views>` / `<display>` → **"dropped / `connections[]` for
graph geometry"** — presentation is discarded, and no WaSim lens is assigned in its place.

**Consequence.** A transpiled XMILE stock-flow model — which is *inherently* a stock/flow
graph and maps cleanly onto the `stock-flow` lens — opens with no lens tag, in General, with
every element shown as a generic node. The governance vocabulary the lens exists to provide
(stock-flow consistency checks, box/valve glyphs, epidemic readouts, …) is silently absent.

---

## 2. The asymmetry that shapes the design

The source formats do not present the same problem:

- **XMILE is single-paradigm.** Stock / flow / aux with graphical functions — the whole
  language is one modeling idiom. It maps essentially 1:1 onto the `stock-flow` lens, and
  the construct→role map is a lookup table (`<stock>`→`stock`, `<flow>`→`flow`). Targeting is
  nearly free and nearly certain.
- **GoldSim and Analytica are multi-paradigm.** One model mixes reliability, stochastic
  process, optimization, and submodel constructs. There may be **no single lens** for the
  whole model; different **containers/submodels may want different lenses**; and much of the
  model is General-lens material with no domain lens at all.

So the design must handle both "one obvious lens for the whole model" and "no clean lens, or
different lenses per container," and must never *force* a lens where none fits.

---

## 3. Design — a deterministic lens matcher

### 3.1 Where it lives

Not in the emitter, and not in the engine. The matcher runs **on the import boundary in the
frontend**, as a post-emit pass over the parsed `ModelDoc`, because:

- Lens is a frontend concern; the manifest system is deliberately *"engine untouched;
  additive"* (`WASIM_LENS_MANIFEST_SPEC.md` §2, principle 5). Teaching the transpiler about
  lens roles would couple it to that frontend concern and duplicate the role vocabulary that
  already lives in the manifests.
- The two things a matcher needs already exist there: the **element registry** (the catalog
  of constructs and their default roles) and the **behavior plugins** (`behaviors/`), which
  encode each lens's structural signature.

Concretely: the matcher is the inverse of the invariant checker. `stockFlowInvariants`
*checks* an already-tagged model (`behaviors/stockFlow.ts`); the matcher *proposes* the tags
so that check would pass.

### 3.2 Two layers (order matters)

A subtlety that dictates the structure: **the shipped invariants presuppose `lens_role`.**
`stockFlowInvariants` keys off `e.lens_role === 'flow'` / `'stock'`; `metapopInvariants` keys
off `'compartment'` / `'transition'` / `'mixing'` / `'coupling'`. On an untagged imported
model those roles don't exist yet. So the matcher can't just run the invariants — it needs a
role-inference layer first, and then the invariants become the *confirmation/scoring* layer.

**Layer A — structural role inference (per element).** From the engine-level fields only —
`primitive`, `inflows`/`outflows`, `inputs`, `value_rule`, `outputs[].dimensions` — propose a
candidate `lens_role` for a given lens. Examples, all derivable without evaluating any
expression (mirroring how `metapopInvariants` is careful to use "structural fields, no
expression eval", `behaviors/metapop.ts:14`):

  - `primitive: "stock"` with `inflows`/`outflows` → `stock` (stock-flow) or `compartment` (metapop).
  - a `node` referenced in some stock's `inflows`/`outflows` → `flow` / `transition`.
  - a 2-D square array over one patch axis feeding a mixing term → `coupling`.

**Layer B — lens fit scoring (per lens).** Apply the candidate roles, then run each lens's
own invariants as a *scorer*: a lens whose conservation/well-formedness invariants come back
clean (no warnings) over the proposed roles is a strong fit; one that produces mostly
warnings is a weak fit. Pick the highest-scoring lens above a threshold; otherwise General.

This reuses the exact predicates the lenses already ship — the score for `stock-flow` is
"fraction of proposed stocks/flows that satisfy `stockFlowInvariants`," and likewise for
`metapop`, `reliability`, `decision`. No second definition of "what stock-flow means."

### 3.3 Known lens role vocabularies (the target of layer A)

| Lens (`behavior` id) | Roles it keys off | Source |
|---|---|---|
| `stock-flow` | `stock`, `flow` | `behaviors/stockFlow.ts` |
| `metapop` | `compartment`, `transition`, `mixing`, `coupling`, `parameter` | `behaviors/metapop.ts` |
| `reliability` | `component`, `redundancy`, `state`, `parameter` | `behaviors/reliability.ts`, `reliabilityTemplates.ts` |
| `decision` | `decision`, `objective` | `behaviors/decision.ts` |

Note `stock-flow` and `metapop` share the same *substrate* (stocks + flows) and differ by
network structure (a coupling matrix + a neighbour-mediated mixing term make it metapop, per
`behaviors/metapop.ts:78`). The scorer handles this naturally: a plain stock/flow graph
scores high on `stock-flow` and fails metapop's coupling invariant; a graph with a square
coupling matrix scores high on `metapop`. Ambiguity between the two is exactly what layer B
resolves.

### 3.4 Per-container lenses

`view.lens` is a single model-level tag today. For multi-paradigm imports, the honest result
is often *"the top model is General, but this submodel container is reliability."* Two ways to
handle it, in preference order:

1. **Match at container granularity.** Run the matcher over each submodel's interior element
   set as well as the top level; stamp roles throughout, and set the model-level `view.lens`
   only when the *whole* model scores cleanly for one lens. This needs a per-container lens
   tag if we want submodels to open in their own lens — a small additive `view` extension,
   consistent with the "engine-ignored `view` block" that already exists.
2. **Interim: stamp roles, leave the model in General.** Even with no model-level lens, having
   correct `lens_role` tags means the moment a user *does* pick a lens, the vocabulary is
   already there and round-trips. This is a strictly-better-than-today fallback and a safe
   first slice.

### 3.5 The emitter's (minimal) contribution

The emitter stays engine-only, with one optional, cheap addition: a **format provenance hint**
in the already-planned `source` block (`XMILE_MAPPING_SCOUT.md` §1 parks header metadata in
`source.*`). `source.format: "xmile"` lets the matcher apply a strong prior — an XMILE import
is stock-flow unless the scorer says otherwise — turning the near-certain single-paradigm case
into a near-zero-cost one. This is a *hint the matcher may use*, not the emitter deciding the
lens, so the coupling stays one-directional and weak.

---

## 4. Options considered

| Option | Who decides | Verdict |
|---|---|---|
| **1. Emit nothing** (status quo) | — | Correct but vocabulary-lossy; General for everything. The baseline to beat. |
| **2. Emitter sets the lens** | transpiler | Clean for XMILE, but forces every transpiler to hard-code lens-role tables, duplicating manifest vocabulary and coupling the engine-facing emitter to a frontend concern (violates §2 principle 5). Reasonable *only* as the XMILE format-hint prior in §3.5, not as the mechanism. |
| **3. Deterministic matcher on import** (recommended) | frontend import pass | Keeps the emitter engine-only; puts lens inference where the role definitions and invariants already live; reproducible and unit-testable. Cost: a real role-inference + scoring pass. |
| **4. LLM classification** | model call at import | Out of scope — see §8. |

**Recommendation: option 3, with option 2 demoted to the XMILE format-hint prior.**

---

## 5. First slice (proving the pattern)

Mirror the lens plan's "one end-to-end round-trip, then the rest is incremental" posture
(`WASIM_LENS_IMPLEMENTATION_PLAN.md` §203):

1. **Target the XMILE + stock-flow path** — the single-paradigm, near-1:1 case (`teacup`,
   the `XMILE_MAPPING_SCOUT.md` §5 first-slice model). Emit `source.format: "xmile"`, run the
   matcher, expect `view.lens: "stock-flow"` with every `<stock>`→`stock` and `<flow>`→`flow`.
2. **Acceptance test:** import teacup → matcher → assert the model opens in the stock-flow
   lens with correct per-element roles, and that **save → reopen round-trips** the lens (the
   same acceptance bar as the shipped lenses, `WASIM_LENS_IMPLEMENTATION_PLAN.md` §145).
3. **Negative test:** a deliberately mixed / non-SFC graph scores below threshold and stays in
   General (the matcher must never force a lens).
4. **Then incremental:** add the metapop discriminator (coupling matrix present), then the
   GoldSim per-container path (§3.4). Each is a new scorer, not a new mechanism.

---

## 6. Open questions

- **Scoring threshold & tie-breaks.** What fraction-clean counts as "a fit," and how are
  near-ties between `stock-flow` and `metapop` broken (presence of any `coupling`-shaped
  element is the natural tiebreak — confirm).
- **Per-container lens tag.** Do we extend `view` with a per-container lens (§3.4 option 1)
  this round, or ship roles-only-in-General (option 2) first?
- **Should the matcher run on *every* load or only on import?** Running it on any lens-less
  model (not just fresh imports) would also auto-lens hand-written JSON — attractive, but
  needs a "user explicitly chose General" opt-out so it doesn't fight the user.

---

## 7. What this does *not* change

- **The engine.** `view.lens` / `lens_role` remain engine-ignored; the matcher only writes
  tags the engine already skips.
- **The manifest/behavior architecture.** The matcher is a *new consumer* of the existing
  registries, not a change to them. No new rules DSL (the manifest spec's standing non-goal,
  §12).
- **The emitter contract.** Everything in `EMITTER_HANDOFF.md` / `notes_to_transpiler.md`
  stands; the only addition is the optional `source.format` provenance hint.

---

## 8. Explicitly out of scope — LLM classification

An LLM pass over the emitted model to guess a lens is **not** part of this design. The
deterministic matcher is reproducible, testable, and — because it reuses the lens invariants —
already has the structural signal it needs for free; an import step should be deterministic.
There may be a genuinely ambiguous residue (a GoldSim model that's a paradigm soup with no
clean single-lens fit) where a judgment call could help, but that is a hypothetical fallback
for the tail, not the mechanism, and we are not scoping it here. Build the deterministic
matcher; if the ambiguous-residue case ever proves painful in practice, revisit then.

---

*Provenance: lens-tag mechanics and role vocabularies verified by direct read of
`frontend/src/lenses/behaviors/{stockFlow,metapop,reliability,decision}.ts`,
`schema/wasim-lens-manifest-v1.json`, and `store.ts`. Emit-side silence on lens tags verified
against `EMITTER_HANDOFF.md`, `notes_to_transpiler.md`, and `XMILE_MAPPING_SCOUT.md`.*
