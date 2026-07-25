# Analytica → WASiM Translation: Status & Feasibility

**Scope.** Where `.ana`-to-WASiM translation stands today, what actually works
against real models, and an honest answer to: *can this be automated?*

Companion to [ANALYTICA_ENGINE_GAP_ANALYSIS.md](ANALYTICA_ENGINE_GAP_ANALYSIS.md)
(which assessed engine/schema *coverage* of Analytica semantics). This document
covers the *translation pipeline* — the format, the converter, evidence from two
real models, and the feasibility verdict. Tooling lives in [`tools/`](tools/);
per-construct mapping detail is in [`tools/README.md`](tools/README.md).

---

## 0. One-paragraph answer

**Format parsing and a mechanistic structural translation are solved and
shipping.** `tools/ana_to_wasim.py` reads real `.ana` files and emits
schema-valid, engine-runnable `model.json`. **A *faithful* automatic translation
is feasible only for the arithmetic + probabilistic *scalar* subset** — for a
small decision model (EVIU) that subset is ~the whole model; for a large applied
model (Platform_2017) it is roughly a third of the logic, because Analytica's
Intelligent-Array/subscript/GUI-decision idioms have no 1:1 WASiM form. **A fully
automatic, faithful importer for the general corpus is not feasible today** and
should not be scoped as one. What *is* feasible and valuable is an **assisted
pipeline**: mechanistic skeleton (done) → a handful of targeted engine/converter
features that lift the highest-frequency idioms → human review of a shrinking set
of flagged stubs. Each feature is independently sizable and independently useful.

---

## 1. What exists today

| Artifact | What it does | State |
|---|---|---|
| `tools/ana_to_wasim.py` | `.ana` → WASiM v0.1.0 `model.json`; stdlib-only | Working, dependency-free |
| `tools/examples/EVIU_plane_catching_3.ana` (+ `.wasim.json`, `.warnings.txt`) | Small textbook decision model, mechanically converted | Validates + runs in engine |
| `tools/examples/Platform_2017.ana` (+ `.wasim.json`, `.warnings.txt`) | 470-node real applied model, mechanically converted | Validates + runs in engine |
| `tools/examples/eviu_plane_catching_native.wasim.json` (+ generator) | Hand-authored *native* EVIU re-solution | Runs; rot-guarded |
| `tools/examples/platform_decommissioning_native.wasim.json` (+ generator) | Hand-authored *native* MADA re-solution | Runs; rot-guarded |
| `engine/tests/{eviu_native_v2,platform_native_v2,platform_conversion_v2}.rs` | Rot guards | Green |

The converter's design principle is **graceful degradation, never silent
wrongness**: any construct it can't faithfully translate becomes an inert stub
(`literal 0.0`, `source: "inferred"`, original Analytica text preserved in
`display`) plus a stderr warning. The output *always* validates against
`oldmodel.schema.json` and parses+runs in the engine — so a translation is a
runnable draft with a machine-readable list of exactly what needs human review,
not a black box.

---

## 2. Two tracks, and why both exist

Translation splits into two fundamentally different tasks:

- **Mechanistic conversion** — a *structural* transliteration: nodes → elements,
  Analytica expressions → WASiM AST, preserving topology and provenance. This is
  automatable and done. It yields a faithful skeleton for the representable
  subset and flagged stubs for the rest.

- **Native re-solution** — re-expressing the *problem* in WASiM's paradigm
  (time-stepping + submodel Monte-Carlo + sweep composition). This requires
  modeling judgment — choosing which quantities become across-realization
  reductions, how a decision axis becomes a swept dimension, how a value-of-
  information question maps onto `submodel_stat` — and is **not** automatable.
  The two `*_native.wasim.json` examples show the engine *has* the primitives;
  a human wields them.

The gap between the tracks is the measure of the problem: the wider the stub
list from track 1, the more track-2 modeling a faithful port needs.

---

## 3. Evidence — the two worked models

### 3.1 EVIU plane-catching (13 nodes — a textbook decision model)

| metric | value |
|---|---|
| elements emitted | 13 (3 `random_variable`, 1 `array`, 1 `constant`, 8 `expression`) |
| faithful expressions | 3 of 8; **the entire base model is faithful** |
| stubbed | 5 nodes — *exactly* the value-of-information layer (`Probability(…)`, `ArgMin(Mid/Mean(Cost), …)` ×2, the `Cost[leave=…]` subscript, `Mid(Cost)`) |

The uncertain inputs (two `Lognormal(median, gsdev)` → correct log-space, one
`Triangular`), the summed travel time, the `Cost` objective with its
`If/Then/Else` miss penalty, the decision grid, and the label index all convert
cleanly. What stubs is precisely the EVIU machinery — the §2 gap. This is the
**best case**: a self-contained scalar decision model is ~90% faithfully
automatic, with the residue being a known, named class.

### 3.2 Platform_2017 (470 nodes — a real applied model)

The PLATFORM offshore-decommissioning decision tool: multi-attribute value under
uncertainty, heavily dimensioned over Platform × Attribute × Option × Scenario.

| metric | value |
|---|---|
| statements parsed | 7651 → 479 elements, 71 containers |
| expression elements | 387 |
| — faithful (stub-free) | **114 (29%)** |
| — fully stubbed (`literal 0`) | 259 (67%) |
| — partial (some sub-exprs stubbed) | 14 |
| `random_variable` emitted | 1 of 14 `Chance` nodes (the rest are subscript/table lookups, or had formula-valued distribution params) |
| conversion warnings | 298 |

Warning breakdown (what a large Intelligent-Arrays model loses):

| count | class |
|---|---|
| 149 | unparseable definitions — `var x := …; …` local-variable blocks, label subscripts, quoted-string tables |
| 107 | unknown function / nested distribution / `INF` / `Undefined` |
| 39 | other (mostly array/GUI attributes) |
| 1 | distribution with formula-valued parameters |
| 1 | across-realization statistic (`Probability`) |
| 1 | dangling refs (`Time`, `Run`, a stray id) → inert 0 |

The model still **validates and runs** — structure, containers, constants, and
the constant-parameter distribution are preserved — but ~two-thirds of the
computational logic is stubbed. This is the **realistic case** for a mature
applied model, and it is dominated by a *small number of recurring idioms*
(§4), not a long tail of exotic functions.

---

## 4. What blocks faithfulness — ranked by frequency

From the Platform warnings, the blockers cluster hard. In descending order of
how much they'd buy:

1. **Subscript / label-reindex** — `x[Dim = label]`, `x[Dim = OtherVar]`,
   `Subtable`, `Slice`. Pervasive (the primary shape of `Chance`/`Objective`
   defs here). WASiM has positional `index`/`get_element` but no *label-keyed*
   reindex, and no runtime-computed index membership. **This is the single
   highest-frequency blocker.**
2. **`var … := …; result` local-variable blocks** — Analytica's `Definition`
   bodies routinely bind locals and return `result`. The converter has no
   let-binding lowering, so the whole definition fails to parse → stub. Purely a
   *converter* gap (the WASiM AST can represent the inlined result); high payoff.
3. **Multi-dimensional Intelligent Arrays + automatic broadcasting** — the model
   is natively `[Platform, Attribute, Option, Scenario]`. WASiM arrays are 1-D
   vectors over one named dimension with *explicit* comprehension. No automatic
   broadcast across shared indices.
4. **Across-realization reductions mid-graph** — `Probability`, `Mean`, `GetFract`
   feeding downstream nodes. The §2 gap. The engine does these at the submodel
   boundary (`submodel_stat`) and results layer, not as an arbitrary mid-graph
   value.
5. **`Choice` / `Checkbox` / `Handle` decisions & metaprogramming** — GUI-bound
   decision inputs and reflection. No declarative numeric equivalent; correctly
   dropped.

Note the shape: **#1 and #2 are the bulk of the Platform stubs, and #2 is
entirely a converter-side fix.** The list is short and idiom-driven — which is
what makes an *assisted* pipeline tractable and a *fully-automatic faithful* one
not.

---

## 5. Engine & format facts established this session

Concrete findings that any future pipeline should assume:

- **Format.** `.ana` is plain text, statement-per-line, but line-delimited with
  classic-Mac `CR` (2008 EVIU) *or* `CRLF` (2017 Platform), with `~`/`~~`
  soft/hard line-wrap markers inside long values. `normalize_ana` handles all
  variants. Parsing is **not** a barrier.
- **`submodel_stat` reduces scalars, not per-member vectors.** A probe confirmed
  that reducing a dimensioned submodel output returns member-0 only. Native
  across-realization curves must therefore use *scalar* submodel outputs with the
  swept value as the reducer `arg` (the exceedance-curve / EVIU pattern), or one
  scalar output per member.
- **Sweep composition works** — `vector_map` over a dimension + `submodel_stat`
  (`mean`/`percentile`/`exceedance`/`cumulative_prob`) is the native idiom for a
  decision/risk curve. Both native examples rely on it. `argmax_array` +
  `get_element` extract optima in-graph; `cumulative_prob` differences give
  P(category) for an integer-valued index.
- **Two robustness rules a converter must enforce** (both cost real crashes on
  Platform): distributions with formula-valued parameters must degrade to stubs
  (the engine evaluates such params as 0 → non-finite `lognormal` → sampler
  panic); references to non-emitted ids must be rewritten to inert 0 (else the
  graph builder rejects a dangling edge).

---

## 6. Is an automated pathway feasible?

Broken into the honest sub-questions:

| Capability | Feasible? | Status |
|---|---|---|
| Parse `.ana` reliably | **Yes** | Done |
| Emit schema-valid, runnable skeleton | **Yes** | Done |
| Faithfully translate the scalar arithmetic+probabilistic subset | **Yes** | Done |
| Faithfully translate a *small* self-contained decision model | **Mostly** (~90%) | EVIU: only the VoI layer stubs |
| Faithfully translate a *large applied* Intelligent-Arrays model | **No** | Platform: ~30% faithful |
| Auto-produce a *native* re-solution | **No** | Needs modeling judgment |

**Verdict.** A *fully automatic, faithful, general-purpose importer* is **not
feasible** — the paradigm gaps (§4.1, §4.3, §4.4) are architectural, not missing
builtins, and the residue on a real model is too large to trust unattended. This
matches the gap analysis's original recommendation, now confirmed against a
470-node model rather than estimated.

But that's the wrong target. The **feasible and useful** pathway is a
**semi-automated / assisted porting pipeline**, because:

- the automatable part (format + structural transliteration + the scalar subset)
  is *real leverage* — it does 100% of the tedious mechanical work and produces a
  running artifact;
- the non-automatable residue is **explicitly enumerated** (every stub carries
  its original text and a typed warning), so a human ports a *known, bounded*
  list instead of re-reading the whole model;
- that residue is **idiom-concentrated** (§4), so each converter/engine feature
  shrinks it in bulk, not one node at a time.

---

## 7. What would move the needle (ranked)

Smallest-intervention-first, each independently shippable and independently
useful beyond translation:

1. **`var`-block lowering in the converter** *(converter-only, small)* — parse
   `var x := e; … result` by inlining locals into the WASiM AST. Pure parser
   work, no engine change; would reclaim a large share of the 149 "unparseable"
   Platform defs. **Highest payoff per unit effort.**
2. **Label-keyed subscript** *(engine + schema, medium)* — a `subscript` AST node
   that reindexes an array by *label* against a named dimension (`x[Dim=label]`).
   Attacks the #1 blocker. Pairs with emitting Analytica `Index` label sets as
   first-class WASiM dimensions (the converter already recovers the labels).
3. **Across-realization reduction as a first-class AST node** *(engine, medium)* —
   the §2 gap. Move the results-layer weighted-empirical-CDF machinery one layer
   inward so `Probability`/`Mean`/`GetFract` can feed downstream nodes without a
   submodel wrapper. Unlocks the whole VoI/risk-decision class (both example
   models needed it).
4. **Multi-dimensional arrays + shared-index broadcasting** *(engine, large)* —
   the deepest gap (#3). Highest ceiling, highest cost; only worth it if applied
   Intelligent-Arrays models become a priority target.
5. **`Choice`/`Checkbox` → enumerated decision inputs** *(converter + schema,
   small)* — map GUI decisions to plain parameter inputs with an allowed set,
   instead of dropping them. Recovers decision *structure* even without the GUI.

Items 1–3 alone would plausibly lift a Platform-class model from ~30% toward a
majority-faithful conversion, and 1 is nearly free.

---

## 8. Recommendation

- **Ship the assisted pipeline framing.** The converter + warnings + a native
  example per domain *is* the product: mechanical skeleton, an explicit to-port
  list, and a worked reference for the hard parts. Don't market or scope it as a
  one-click importer.
- **Do item 1 (`var`-block lowering) next** — it's converter-only, small, and
  reclaims the largest single warning class.
- **Then items 2–3** if Analytica interop is a real priority; they're the
  engine-side gaps that both convert *and* enrich WASiM's own modeling power.
- **Do not scope item 4** unless applied multi-index models are the explicit
  goal — it's a paradigm port, not a feature.
- **Keep the graceful-degradation contract** (validate + run + flag) as the
  non-negotiable invariant; it's what makes a partial translation trustworthy.

---

*Evidence: `tools/ana_to_wasim.py`, `tools/examples/*` (EVIU + Platform_2017,
mechanistic and native), engine rot guards, and this session's engine probes.
Method: two real `.ana` models converted end-to-end and re-solved natively;
metrics measured from converter output, not estimated.*
