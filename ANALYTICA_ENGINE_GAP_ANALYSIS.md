# WASiM ↔ Analytica Engine Gap Analysis

**Purpose.** Assess how much of **Analytica** (Lumina Decision Systems' visual
quantitative-modeling tool) the WASiM **schema** can represent and the WASiM
**engine** can run. Scoped — like [GOLDSIM_ENGINE_GAP_ANALYSIS.md](GOLDSIM_ENGINE_GAP_ANALYSIS.md) —
to *engine / model-semantics*. It says nothing about authoring UI, influence-diagram
editing, or dashboards; those are out of scope by request.

**Sources.**
- **Analytica:** direct read of the [Example Models](https://docs.analytica.com/index.php/Example_Models)
  corpus. One model read in full — *Hubbard & Seiersen, Cybersecurity Risk Ch. 3*
  (`Hubbard_and_Seiersen_cyberrisk.ana`, 269 lines) — plus a representative
  8-model spread surveyed for feature frequency (dynamic, optimizer, risk,
  decision, data-analysis, function, geometry categories). The `.ana` format is
  plain UTF-8 text: each node is `Variable|Chance|Decision|Objective|Index|Determ Name`
  followed by `Definition:` lines.
- **WASiM:** the semantics reference (`schema/wasim-engine-semantics.md`, v0.9.6),
  the v2 schema (`schema/wasim-schema-v2.json`), and the Rust engine
  (`engine/src/*.rs`), builtins enumerated from `engine/src/eval.rs`. Status is
  **FULL / PARTIAL / ABSENT** against current code.

**One-line answer.** For the *arithmetic + probabilistic core* — distributions,
array algebra over named indices, reductions, interpolation, and self-referential
time recurrence — WASiM covers Analytica **well** (the cyberrisk model is ~90%
representable today). The friction is concentrated in three places that recur across
the corpus: **(1)** the Monte-Carlo sample as a *first-class array axis* usable
mid-expression, **(2)** *runtime-dynamic index construction*, and **(3)** the
*non-numeric layers* (metaprogramming, GUI/form logic). These are paradigm
differences, not missing builtins.

---

## 0. The paradigm mismatch (read this first)

The two tools solve overlapping but differently-centered problems:

| | **Analytica** | **WASiM** |
|---|---|---|
| Core paradigm | Static **Intelligent Arrays** + Monte-Carlo, with an *optional* `Time` dimension bolted on via `Dynamic[]` | **Time-stepping simulation** (stocks/flows/dt/calendars) with arrays and sampling as substrate |
| The sample dimension | Just another array axis (`Run`), reducible **anywhere** in an expression (`Probability(X>t)`, `Mean(X)`, `ProbBands(X)`) | An **analysis step at the output boundary** (A3 layer, `results_spec`), not a mid-graph value |
| Array broadcasting | **Automatic** across any shared named index (Intelligent Arrays) | **Explicit** comprehension (`vector_map` + `index`/`index_ref`) |
| Index membership | Can be **computed at runtime** (`subset`, `sortIndex`, `shuffle`, `1..n` where n is scalar) | **Pre-declared** `dimensions[]` with fixed `size` |
| Time | Optional; ~⅜ of corpus models don't use it | Central; the engine always steps |

WASiM *can* run a dynamics-free 1-step model, so Analytica's ~⅜ static models are
not disqualified — but a static model uses maybe 15% of what WASiM is for, leaning
entirely on its array + sampling machinery. Conversely, Analytica's `Dynamic[]`
recurrence — its **hardest** feature to bolt onto a static tool — is the one thing
WASiM represents **natively** (it *is* a time-stepping engine).

---

## 1. Worked example: the cyberrisk model, node by node

269 lines, ~15 real nodes. This is the concrete parity answer.

| Analytica construct | WASiM mapping | Status |
|---|---|---|
| `Index Event := 'Event ' & 1..50` | `dimensions[]` entry, `size: 50` | **FULL** (§15) |
| `Table(Event)(0.02, 0.05, …)` | array-valued `fixed`/`expression` output over the `Event` dimension | **FULL** |
| `Chance … := Bernoulli(P_Event)` | `sample` node, `bernoulli` family | **FULL** (§2.3 roster) |
| `Chance … := LogNormal(mean:mu, stddev:sigma1)` | `sample` node, `lognormal` family (formula-valued params `mu`,`sigma1`) | **FULL** (§2.3) |
| `Event_Occurrence * Event_Impact` | elementwise array arithmetic via `vector_map` | **FULL** (§15) |
| `sum(Expected_Impact, Event)` | `sum_array` reduction over the dimension | **FULL** |
| `mu := (Upper-Lower)/2`, `sigma := (Upper-Lower)/3.29` | plain `expression` nodes | **FULL** |
| `cubicinterp(Risk_Tolerance_Loss_, Stated_Risk_Toleranc, Loss_Subset)` | `lookup` node, `spline` (Fritsch-Carlson) interpolation | **FULL** (§2.5) — *see note* |
| `curve[Loss_Subset = Loss_Threshold]` (index-remap subscript) | `index` subscript, or a second `lookup` | **PARTIAL** — label-based reindexing is not 1:1 |
| **`Probability(Expected_Total_Loss > Loss_Threshold)`** | exceedance/CCDF in the **A3 results layer** (`cumulative_prob`, CCDF = 1−CDF) | **PARTIAL** — see §2 |
| `SampleSize := 10K` | `n_realizations: 10000` | **FULL** |
| Nested `Module` grouping (`Indexes`, `Event_Impact_CI_data`) | `SubModel` containers (§12) or flat namespacing | **FULL** |

**Representability of cyberrisk: ~90% today.** The one genuine gap (`Probability(...)`
feeding a downstream node) is bridgeable — see next section. A hand-translation into a
WASiM v2 model doc + a parity test (mirroring the existing `seldm_*` fixtures) would
confirm this end-to-end; that remains the natural next artifact if you want proof over
estimate.

> **cubicinterp note.** Analytica's `cubicinterp` is a natural cubic spline; WASiM's
> `spline` is **monotone** cubic (Fritsch-Carlson, never overshoots). For a
> monotone risk-tolerance curve these agree closely, but they are not bit-identical
> — flag as a numeric-parity caveat, not a capability gap.

---

## 2. The one real semantic gap: sample-as-axis reductions

Analytica's `Probability(Expected_Total_Loss > Loss_Threshold)` produces a
**loss-exceedance curve**: for each of ~200 threshold values, the *fraction of the
10K realizations* exceeding it. Crucially, in Analytica `Inherent_Risk` is an
ordinary variable that **feeds another node** (`Prob__of_Exceeding_L`).

WASiM computes exactly this quantity — **CCDF / exceedance = 1 − CDF** and
`cumulative_prob` — but in the **A3 results/analysis layer** (§A3), a *runtime*
`results_spec` reduction at the **output boundary**. It is not a first-class
expression value a downstream element can consume and keep computing with.

So the reduction *exists*; what's missing is treating "across realizations" as an
**array axis reducible mid-graph**. Analytica exposes `Mean(x)`, `SDeviation(x)`,
`ProbBands(x)`, `GetFract(x, p)`, `Probability(cond)`, `CDF(x)` as ordinary
functions over the `Run` dimension. WASiM has all the underlying machinery
(weighted empirical CDF, percentiles, CTE — see §A3) but wired only to the edge.

**This is the highest-leverage gap.** It appeared as `Probability` (cyberrisk),
`Prob`/`ProbBands`/`CDF` (retirement, TAR), and `Mean`/`SDeviation` mid-model
(time-series-reindexing). A small addition — an across-realization reduction AST
node — would unlock a whole class of Analytica risk/decision models. *(Compare the
GoldSim analysis's "results/analysis engine" gap, now largely closed at the boundary;
this is the same machinery, asked to move one layer inward.)*

---

## 3. Corpus-wide feature frequency (8-model survey)

Distributions & core arithmetic are well-covered. The recurring *challenges* cluster.

### 3.1 Functions that map cleanly (FULL)

| Analytica fn | Models | WASiM equivalent |
|---|---|---|
| `Sum` | 6/8 | `sum_array` |
| `Table` / array literal | 6/8 | array-valued output over dimension |
| `If-Then-Else` | 6/8 | conditional AST |
| `Max`/`Min` | 5/8 | `max`/`min`, `max_array`/`min_array` |
| `Sequence` | 4/8 | dimension with computed size / series |
| `Cumulate` / `cumproduct` | 4/8 | `lag`-based accumulation (§15 stateless recurrence) |
| `Uniform`/`Normal`/`LogNormal`/`Poisson`/`Gamma`/`Beta` | 5/8 | `sample` roster (§2.3) — **all present** |
| `cubicinterp` / `LinearInterp` | 4/8 | `lookup` (spline/linear) |
| `argmin`/`argmax` | 2/8 | `argmin_array`/`argmax_array` (0.9.7) |
| trig/math (`Cos`,`Sin`,`ArcTan2`,`Sqrt`,`Mod`,`exp`,`ln`,`abs`) | 2/8 | engine builtins (confirmed in `eval.rs`) |

### 3.2 The recurring challenges (PARTIAL / ABSENT)

Ranked by how much of the corpus they block:

1. **Positional / local-index arithmetic — 7/8 models. PARTIAL→ABSENT.**
   `@Index`, `[Idx=expr]` label-reindexing, `Slice(x, I, pos±n)`, and especially
   **data-dependent, runtime-sized indices** (`subset`, `sortIndex`, `shuffle`
   producing train/CV splits; `index U := Sequence(u0,uN,incU)` created *inside* a
   `for`/`Dynamic` body). WASiM's `dimensions[]` are pre-declared with fixed `size`;
   it has no notion of an index whose *membership or length is computed at runtime*.
   This is the single most pervasive representability limit. Positional access
   (`get_element`, 1-based `index`) is FULL; **dynamic index construction** is ABSENT.

2. **Optimization coupled to simulation — 2/8, but the hardest. PARTIAL.**
   WASiM *has* optimization (§13) and even *dynamic per-timestep* optimization
   (§13a) and *probabilistic* objectives (reduce across realizations). But the
   corpus shows two patterns at the edge of that: (a) an NLP with a **probabilistic
   constraint** `Prob(decline) ≤ 10%` (TAR model) — WASiM does probabilistic
   *objectives* but constraint-on-a-sample-statistic is unverified; (b) a full NLP
   **solve at every Dynamic step** *and* inside the uncertainty dimension
   (cross-validation boosting) — solver-in-both-loops. Worth a targeted check
   against §13a rather than assuming.

3. **Self-referential `Dynamic()` recurrence — 3/8. FULL (this is WASiM's home turf).**
   `Nt := Dynamic(Pop, Self[Time-1]*Lambda − catch)` maps directly onto a stock /
   `lag`-based expression (§15 "stateless recurrence": `x_t = x_{t-1} + rate·Δt`).
   The one thing hardest to graft onto a *static* tool is native here. Noted as a
   **strength**, not a gap.

4. **Metaprogramming & reflection — 1/8 (Feasible_Sampler). ABSENT, by design.**
   Rewriting a node's definition from a string at runtime (`definition of X := …`),
   `outputs of`, `class of`, button `Script`s that mutate `SampleSize` or resize the
   `Run` dimension. This is authoring-time behavior with no declarative numeric
   equivalent — the same class as GoldSim-analysis "procedural scripting (Tier C1)".
   Out of scope for a simulation engine.

5. **GUI / form / presentation logic — 2/8 prominent. ABSENT, by design.**
   `Checkbox`/`Choice` inputs, `ChangeNodeVisibility`, `MsgBox`, `ShowProgressBar`,
   cell styling (`CellSpan`,`CellFill`,`DetermTable`/`MultiTable` edit-tables). These
   reduce to plain input parameters or are dropped. Explicitly out of scope (no
   authoring front end).

---

## 4. Coverage estimate

Stated three ways, because "how much of Analytica" depends on the target:

| Target | Coverage | Rationale |
|---|---|---|
| **The cyberrisk model specifically** | **~90% today**, ~100% with the §2 across-realization reduction | Only gap is `Probability()` feeding a downstream node |
| **Analytica's array + probabilistic core** (Intelligent Arrays, distributions, sample stats) | **~60–70%** | All the pieces exist, but WASiM lacks automatic multi-index broadcasting and runtime-dynamic indices; arrays are explicit comprehensions |
| **This 8-model corpus** | **~½ near-directly representable**; the other ½ blocked by ≥1 of §3.2's challenges | 3 static/simple + the dynamic-recurrence models map well; metaprogramming/GUI/dynamic-index models don't |
| **Analytica as a whole** (its `Time` paradigm, ~200 builtins, influence-diagram semantics, DiagramGUI) | **not a meaningful target** | Different tool, different purpose — this is scope-mismatch, not a gap to close |

---

## 5. Recommendation

The pragmatic ranking, smallest-intervention-first:

1. **If you want proof, not estimate:** hand-translate cyberrisk into one WASiM v2
   model doc + a parity test (mirrors `seldm_*`). ~an afternoon; validates the §2
   gap concretely. **No new infrastructure.**
2. **Highest-leverage engine addition:** an **across-realization reduction** AST node
   (§2) — reuses the A3 weighted-empirical-CDF machinery, moved one layer inward.
   Unlocks a class of Analytica risk/decision models.
3. **Do NOT** scope a general `.ana → WASiM importer` against this evidence. The
   `.ana` grammar, Intelligent-Array broadcasting, and ~200 functions dwarf what
   this corpus shows; §3.2's dynamic-index and metaprogramming features would gate
   it. If an importer is ever wanted, it needs a dedicated corpus survey first.

**Bottom line.** WASiM's substrate is a *good* fit for Analytica's numeric core and a
*native* fit for its `Dynamic[]` recurrence. The distance between them is not builtins
— it's two architectural choices (sample-as-axis, runtime-dynamic indices) plus the
non-numeric layers WASiM deliberately doesn't have. One small addition (§2) buys a
disproportionate slice of Analytica's risk-modeling use cases.

---

*Companion to: [GOLDSIM_ENGINE_GAP_ANALYSIS.md](GOLDSIM_ENGINE_GAP_ANALYSIS.md).
Evidence: `Hubbard_and_Seiersen_cyberrisk.ana` (full read) + 8-model corpus survey.
WASiM side: `schema/wasim-engine-semantics.md` v0.9.6, `engine/src/*.rs`.*
