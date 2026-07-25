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
pipeline**: mechanistic skeleton (done) → targeted features that lift the
highest-frequency idioms → human review of a shrinking set of flagged stubs. Two
converter-only passes have shipped (`let`-block lowering + `Choice`/`Checkbox`
decisions), cutting full-stub definitions on Platform_2017 from 259 to 163. The
remaining blockers, however, are **not** a list of independent point features:
label subscript, multi-dimensional broadcast, and mid-graph sample reductions all
converge on one engine change — a **labeled n-d value type** (§9.3) — which is the
honest next step rather than more piecemeal work.

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
| elements | 479 |
| — full-stub (`literal 0`) | **163 (34%)** |
| — non-full-stub (faithful or partial) | **316 (66%)** |
| faithful constants | 103 |
| `random_variable` emitted | 1 of 14 `Chance` nodes (the rest are subscript/table lookups, or had formula-valued distribution params) |
| real dependency edges in graph | 345 |

> **Numbers reflect two landed converter tweaks — `let`-block lowering (§7 item 1)
> and `Choice`/`Checkbox` decisions (§7 item 5).** The untouched baseline measured
> **259 full-stub (54%)**, 40 constants, 310 edges. The two passes cut full-stubs
> to **163 (−37%)**, raised faithful constants 40 → 103, and recovered +35 edges.
> `let`-lowering also confirmed empirically that idioms #1 and #2 **stack**: most
> reclaimed blocks now expose a label-subscript inside (§4) — which is why §7's
> ranking is revised below.

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
   bodies routinely bind locals and return `result`. ~~The converter has no
   let-binding lowering, so the whole definition fails to parse → stub.~~
   **Landed (§7 item 1):** the converter now inlines these. Pure *converter* work;
   it reclaimed 34 Platform definitions from full-stub. What remains after
   lowering is usually a label-subscript inside the block (blocker #1).
3. **Multi-dimensional Intelligent Arrays + automatic broadcasting** — the model
   is natively `[Platform, Attribute, Option, Scenario]`. WASiM arrays are 1-D
   vectors over one named dimension with *explicit* comprehension. No automatic
   broadcast across shared indices.
4. **Across-realization reductions mid-graph** — `Probability`, `Mean`, `GetFract`
   feeding downstream nodes. The §2 gap. The engine does these at the submodel
   boundary (`submodel_stat`) and results layer, not as an arbitrary mid-graph
   value.
5. **`Choice` / `Checkbox` decisions** — GUI-bound decision inputs. **Landed
   (§7 item 5):** lowered to their value (`Checkbox(v)`→`v`, `Choice(idx,n)`→`n`),
   reclaiming ~63 nodes as faithful constants. **`Handle` / metaprogramming**
   (reflection, `Parent_module`, `definition of`) stays correctly dropped — no
   declarative numeric equivalent.

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
| Faithfully translate a *large applied* Intelligent-Arrays model | **No** | Platform: 66% of nodes non-stub after tweaks 1 & 5; the faithful *logic* is less |
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

## 7. What would move the needle (ranked, revised as items land)

Two converter-only items have shipped and taught us how the rest sequence.
The **key correction from implementing #1:** label subscript is *not*
independently shippable at useful fidelity — see the boxed note below.

1. **`var`-block lowering in the converter** *(converter-only, small)* — ✅ **DONE.**
   Inlines `var x := e; … result` into the WASiM AST (`parse_var_block`, tested in
   `tools/test_ana_to_wasim.py`). Reclaimed 34 Platform definitions, +35 edges.
5. **`Choice`/`Checkbox` → decision values** *(converter-only, small)* — ✅ **DONE**
   (promoted from #5 because it turned out independently shippable with the biggest
   converter payoff). `Checkbox(v)`→`v`, `Choice(idx,n)`→`n`, including inside
   `Table(…)`. Reclaimed ~63 nodes as faithful constants; full-stubs 259→163 with #1.
2. **Label-keyed subscript** *(engine + schema, **large**, not medium)* — a
   `subscript` node reindexing an array by label against a named dimension
   (`x[Dim=label]`). **Reclassified.** The Platform evidence is that real subscripts
   are **multi-dimensional with variable (not literal) labels over base tables that
   are themselves array-shaped** (`table[Cost='Site Clearance'][Platform=SelVar][Alt=SelVar]`).
   A bounded 1-D static-label version — the only piece that fits today's flat
   `Value::Vector` — helps neither the converter (its subscripts aren't that shape)
   nor real models. Faithful subscript therefore **requires the NamedArray value
   type** (§9.3 / tweak table #2) and runtime index resolution (#4) underneath it;
   it is *not* the cheap standalone win the first draft implied.

> **Roadmap correction.** After landing #1 and #5, the converter-only wins are
> largely spent. The remaining blockers (label subscript, multi-dim broadcast,
> mid-graph sample reductions) are **all engine-side and all sit on the same
> foundation** — a labeled n-d value type (§9.3). The honest next step is not
> another point feature; it is the **`NamedArray` core**, after which label
> subscript, `Run`-as-axis, and runtime indices become small riders rather than
> independent projects.

3. **Across-realization reduction as a first-class AST node** *(engine, medium)* —
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
- **Items 1 and 5 (converter-only) are done** — `let`-lowering and
  `Choice`/`Checkbox`, together cutting full-stubs 259→163. The cheap
  converter-side wins are now largely spent.
- **The next real step is the `NamedArray` value type (§9.3), not another point
  feature.** Implementing #1 proved that label subscript, multi-dim broadcast, and
  mid-graph sample reductions all sit on that one foundation; doing them piecemeal
  on the flat `Value::Vector` yields bounded versions that help neither the
  converter nor real models. Land the labeled n-d core in v2, then subscript /
  `Run`-as-axis / runtime indices fall out as small riders. **A phased engineering
  plan for exactly this is scoped in
  [WASIM_NAMEDARRAY_DESIGN.md](WASIM_NAMEDARRAY_DESIGN.md)** (6 bit-identity-preserving
  phases; recommended first milestone P0–P3 = type + broadcast + label subscript +
  ≥2-D, with `Run`-as-axis isolated as its own step).
- **Do not scope item 4** unless applied multi-index models are the explicit
  goal — it's a paradigm port, not a feature.
- **Keep the graceful-degradation contract** (validate + run + flag) as the
  non-negotiable invariant; it's what makes a partial translation trustworthy.

---

## 9. Appendix — array model & architectural directions

The blockers in §4 aren't a pile of missing builtins; they're **one missing
abstraction** showing up in different disguises. This appendix names it, relates
it to the "adopt a Rust array library?" question, and lists the architectural
tweaks that generalize the problem space — each of which pays off well beyond
Analytica (`inter alia`: GoldSim vectors, the results layer, engine simplicity).

### 9.1 "Intelligent Arrays" is xarray, not numpy — and that's the whole point

numpy is **positional**: axes are anonymous and ordered; you broadcast by
trailing-shape rules and reshape/transpose by hand. Analytica's Intelligent
Arrays are **named**: every axis is a first-class Index with an identity, and
operations **align by name** — multiply an array over `[Platform, Year]` by one
over `[Year, Scenario]` and it aligns on `Year`, outer-products the rest, with no
axis-order bookkeeping. On top of that you **subscript by label**
(`cost[Platform='Gail']`) and index membership can be **built at runtime**
(`Subset`, `Sequence`, `SortIndex`).

The right analogy is **xarray / pandas / APL–J / Julia AxisArrays** — *labeled*
tensors. numpy is the layer *underneath* xarray. So the part a Rust array library
gives you (n-d storage, elementwise kernels, reductions) is **not** the blocker;
the named-axis semantics (align-by-name, label subscript, runtime indices) are —
and no Rust crate provides those.

### 9.2 Does WASiM need a Rust array library?

Two separable layers:

| Layer | Provider | Is it the gap? |
|---|---|---|
| n-d storage, broadcast kernels, reductions, BLAS | a crate (`ndarray`, `faer`) | **No** — the easy half |
| named-axis identity, align-by-name, label index, runtime index sets | **in-house** (nobody in Rust) | **Yes** — §4's blockers |

If/when n-d *storage & kernels* are wanted, **`ndarray`** is the fit: pure-Rust,
numpy-like, **WASM-clean** (BLAS is optional/feature-gated). Avoid the heavier
options given this engine's posture — `faer` (matrix-shaped, not labeled),
`polars`/`arrow` (tabular, named *columns* not n-d axes), `candle`/`burn`/`tch`
(ML stacks: GPU, autodiff, dozens of transitive deps). Three hard constraints
from *this* codebase rule out the aggressive choices:

1. **WASM target.** BLAS/LAPACK backends don't cross to `wasm32`; keep any array
   lib on its pure-Rust path, feature-gate native acceleration to the CLI build.
2. **Bit-reproducibility.** The engine sorts with `total_cmp` and fixes reduction
   order on purpose; SIMD/parallel/BLAS reductions reorder float ops and break the
   bit-identity guarantee. Adopted kernels must run in deterministic, fixed order.
3. **Minimal-dependency ethos.** Today the engine has exactly one dependency
   (serde). `ndarray` is a defensible add; an ML tensor stack is a philosophical
   break.

**Bottom line:** an array crate is an eventual *substrate for speed*, not the
thing that unblocks fidelity. Reach for it under a WASiM-owned named-array type,
not instead of one.

### 9.3 The unifying idea — axes as one algebra (the north star)

Today the runtime value is `Value::Scalar(f64) | Value::Vector(Vec<f64>)`. A
`Vector` carries **no axis identity** — *which* dimension it ranges over lives
out-of-band in `outputs[].dimensions` and the `<id>#k` result-naming convention.
And "reduce across a population" exists **three times**: `results_spec` (across
realizations at the output boundary), `submodel_stat` (across realizations
mid-graph, via a submodel), and `sum_array`/`mean_array` (across an array axis).
Time and Run(sample) are engine-privileged axes, not values.

Analytica's model — and xarray's — is the generalization: **a value is a labeled
array over named axes, and Time and Run are just two of those axes.** If WASiM
moved to

```rust
enum Value { Scalar(f64), Array(NamedArray) }
struct NamedArray { dims: Vec<DimId>, shape: Vec<usize>, data: Vec<f64> }  // row-major
```

then the disguises in §4 collapse into one abstraction:

- **Align-by-name broadcast** (Intelligent Arrays, §4.3) = the binary-op rule on
  `NamedArray`: union the dim sets, broadcast missing axes. Falls out for free.
- **Label subscript** `x[Dim=label]` (§4.1) = index an axis by label → drop that
  axis. One operation.
- **Across-realization reduction** (§4.4, the §2 gap) = **reduce over the `Run`
  axis**, identical to reducing over any user axis. `submodel_stat`,
  `results_spec`, and `sum_array` become one `reduce(axis, stat)` — the mid-graph
  `Probability`/`Mean`/`GetFract` that both example models needed is then just
  `reduce(Run, …)`, no submodel wrapper required.
- **The `#k` expansion hack disappears** — results carry their axes, so array
  outputs no longer need per-member `<id>#k` series stitched back together.
- **Time-history unifies with array results** — if Time is (optionally) an axis,
  a time series and an array output are the same shape of thing.

That is simultaneously the **Analytica-alignment** move *and* the engine's biggest
**simplification** (three reduction paths → one; the `#k` convention retired; v1
`Vector` and v2 array results reconciled). It generalizes the problem space rather
than bolting on cases.

### 9.4 The tweaks, ranked — Analytica payoff *and* general payoff

Smallest-first; each shippable alone.

| # | Tweak | Layer / size | Helps Analytica | Helps generally (`inter alia`) |
|---|---|---|---|---|
| 1 | **`let`-bindings** (`var x:=e; … r`) ✅ **done** | converter-only / small | reclaimed 34 Platform stubs, +35 edges (§4.2) | any transpiler (GoldSim), readable emitted expressions |
| 5 | **`Choice`/`Checkbox` → decision values** ✅ **done** | converter-only / small | ~63 nodes → faithful constants (§4.5) | typed enumerated inputs for any front end |
| 2 | **`NamedArray` value type** (§9.3) — *the keystone* | eval core / large | intelligent arrays, ≥2-D tables, **enables label subscript** | GoldSim vectors/matrices, cleaner results, retires `#k` |
| 3 | **`Run` as a reducible named axis** | eval + reductions / medium (rider on #2) | sample-as-axis (§2/§4.4) mid-graph | unifies 3 reduction mechanisms → less engine code |
| 4 | **Label subscript + runtime index sets** | eval + schema / medium (rider on #2) | `x[Dim=label]`, `Subset`, `SortIndex` (§4.1) | data-driven models (cohorts, scenario tables) |
| — | `ndarray` as `NamedArray` backing store | dep / opt | (perf only) | n-d kernel speed once #2 is the bottleneck |

**Revised sequencing (post-landing #1, #5).** The converter-only wins (1, 5) are
done. Everything left converges on **#2 (`NamedArray`)** as the keystone: #3 and #4
are riders on it, not independent projects — a `NamedArray` whose axis list
*includes* `Run` gives §4.3 and §4.4 together, and label subscript is just indexing
one axis by label. Attempting #4 on the flat `Value::Vector` first (a 1-D
static-label version) was considered and rejected: it fits neither Platform's real
subscripts (multi-dim, variable-labeled) nor the converter's v1 output, so it would
be throwaway. Do #2, then 3/4 fall out.

### 9.5 Sequencing & risks (be honest about cost)

- **Not a big-bang.** Ship `let`-lowering (1) now. Prototype `NamedArray` (2)
  behind the existing `Value` enum — `Scalar`/`Vector` become the 0-D/1-D cases —
  so the migration is additive, not a rewrite. Add `Run` to the axis set (3), then
  retire `submodel_stat`/`results_spec` onto the unified reducer once it's proven.
- **Determinism is the sharpest risk.** A named-array reducer must fix summation
  order (stable fold, no SIMD/rayon reordering) or the bit-identity guarantee
  breaks. Design the reduce kernel deterministic-by-construction.
- **WASM binary size & the `Scalar` fast path.** Most nodes are scalar; keep the
  0-D case a plain `f64` so the common path pays nothing and the wasm stays small.
- **The v1/v2 split.** There are two model/eval paths (`model.rs`/`engine.rs` and
  `model_v2.rs`/`engine_v2.rs`). Land `NamedArray` in v2 only; treat it as the
  convergence point rather than porting both.
- **Scope discipline.** Runtime *dynamic* index membership (index length computed
  mid-run) is the genuinely hard tail (§4.1); pre-declared label indices cover
  most of the corpus. Ship label subscript over fixed dimensions first; defer
  data-length-varying indices until a model actually demands it.

---

*Evidence: `tools/ana_to_wasim.py`, `tools/examples/*` (EVIU + Platform_2017,
mechanistic and native), engine rot guards, and this session's engine probes.
Method: two real `.ana` models converted end-to-end and re-solved natively;
metrics measured from converter output, not estimated.*
