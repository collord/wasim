# `ana_to_wasim.py` — Analytica → WASiM converter

Converts an Analytica model (`.ana`, plain UTF-8) into a WASiM **v0.1.0**
`model.json` — the format the Rust engine parses and that
`schema_examples_manual/*.json` follow. Stdlib-only; no dependencies.

```bash
python3 tools/ana_to_wasim.py path/to/model.ana -o model.wasim.json
python3 tools/ana_to_wasim.py path/to/model.ana --stdout        # JSON to stdout
```

Conversion warnings (unconvertible constructs, degraded nodes, axis caveats)
are printed to **stderr**; the JSON goes to the file / stdout. The emitted model
is designed to always validate against `oldmodel.schema.json` and to parse in
the engine — unconvertible definitions become **inert stubs**
(`{"op":"literal","value":0.0}`, `source:"inferred"`, original text kept in
`display`) that show up as warnings, never as silent wrong numbers.

## Validate the output

```bash
pip install jsonschema
python3 - <<'PY'
import json, jsonschema
jsonschema.validate(json.load(open("model.wasim.json")),
                    json.load(open("oldmodel.schema.json")))
print("valid")
PY
```

To confirm it runs, feed it through the engine (`wasim_engine::simulate_json`).

## Worked example — EVIU "plane catching" (Max Henrion)

`tools/examples/` contains a real Analytica model end-to-end:

| file | what |
|---|---|
| `EVIU_plane_catching_3.ana` | the source model (Lumina example, 2008) |
| `EVIU_plane_catching.wasim.json` | converter output — **validates against `oldmodel.schema.json` and runs in the engine** |
| `EVIU_plane_catching.warnings.txt` | the conversion warnings |

```bash
python3 tools/ana_to_wasim.py tools/examples/EVIU_plane_catching_3.ana \
    -o tools/examples/EVIU_plane_catching.wasim.json
```

**Converts cleanly (the probabilistic core):** the three uncertain inputs
(`Time_to_drive_to_air`, `Time_from_parking_to` as `Lognormal(median, gsdev)` →
log-space `random_variable`s; `Gate_time_before_dep` as `Triangular`), the
summed `Time_from_home_to_g1`, the `Cost` objective (subtraction + `If … Then …
Else` miss-penalty), `Loss_if_miss_the_pla = 400`, the 26 candidate departure
times, and the `View1` label index (labels preserved). Sample size `2000` is
read from the model's `Samplesize` system variable; the two `Module`s become
nested containers.

**Degrades to inert stubs (with warnings) — the EVIU machinery itself:**
`Prob_of_missing_plan = Probability(…)` (across-realization statistic),
`Best_time_* = ArgMin(Mid/Mean(Cost), …)` (optimize-over-the-decision-axis),
and `Eviu = Cost[Time_i_leave_home = …] − Cost[…]` (label-based subscript
reindex). These are precisely the three constructs
`ANALYTICA_ENGINE_GAP_ANALYSIS.md` flags as the hard parts — sample-as-axis
reductions, decision optimization coupled to the sample, and runtime index
remapping. The arithmetic skeleton is faithful; the value-of-information layer
on top needs the results-layer / optimization features called out there.

Note: `.ana` from this era is classic-Mac **CR-delimited** with `~`/`~~`
line-wrap markers inside long values; the converter normalizes both.

## Stress test — `Platform_2017` (a real 300-node model)

`tools/examples/Platform_2017.ana` is a large real Lumina model — the **PLATFORM
decommissioning decision tool**, a multi-attribute decision-analysis framework
for choosing how to decommission offshore oil & gas platforms (full removal /
partial removal to reef / leave-in-place), scored across attributes (cost, reef
biomass, access, air quality, compliance) with stakeholder swing weights, under
cost uncertainty. 7651 statements, ~470 real nodes.

```bash
python3 tools/ana_to_wasim.py tools/examples/Platform_2017.ana \
    -o tools/examples/Platform_2017.wasim.json
```

The conversion (`Platform_2017.wasim.json`, ~470 elements, 71 containers)
**validates against the schema and runs in the engine** — but the warnings show
how much of a heavily array-and-subscript Analytica model doesn't survive a
scalar translation: `Table`/`Subscript` reindexing (`x[Dim=label]`),
`Choice`/`Checkbox`/`Handle` GUI decisions, and `Subtable`/`Slice` degrade to
inert stubs. (`var … := …; result` local-variable blocks are now `let`-lowered
rather than stubbed — see the mapping table — which moved 34 definitions out of
full-stub and recovered ~35 dependency edges; many still carry a stubbed
subscript inside, the next blocker.) This is the expected outcome
the gap analysis predicts for an Intelligent-Arrays model, and it drove two
robustness fixes worth having anywhere: distributions with formula-valued
parameters (which the engine evaluates to 0 and would crash a `lognormal`
sampler) degrade to stubs, and references to non-emitted ids (system indices
`Time`/`Run`, skipped `Function`s) are rewritten to inert `0.0` so the model
still builds. It is guarded by `engine/tests/platform_conversion_v2.rs`. Its
native counterpart is `platform_decommissioning_native.wasim.json` below.

## Native re-solution — `eviu_plane_catching_native.wasim.json`

The mechanical conversion above stubs the value-of-information layer because it
has no faithful scalar translation. `tools/examples/eviu_plane_catching_native.wasim.json`
is the **companion hand-authored model that actually solves the problem** the
way WASiM is built for — a v2 model (`build_eviu_native.py` regenerates it). It
is guarded by `engine/tests/eviu_native_v2.rs`, so it runs on every `cargo test`.

The idea: the expensive Analytica constructs (`Probability(…)`, `ArgMin` over
the decision axis, `Cost[leave = …]`) all reduce to the engine's own
**sweep-composition** pattern — the same one `loss_exceedance_curve` uses:

- A `Trip` **submodel** samples the three uncertain legs and emits `travel`
  (total home-to-gate time) over its own Monte-Carlo loop.
- `submodel_stat` reduces `travel` **across realizations** — `mean`, `percentile`
  (median), and `exceedance` (`P(travel > t)`, the CCDF).
- The expected-cost decision curve is those reducers swept over the `Depart`
  axis with `vector_map`, using the identity
  `E[Cost(lead)] = lead − mean(travel) + Loss · P(travel > lead)` — no
  per-realization cost array needed, so it composes from scalar reducers with
  the lead time as the reducer `arg`.
- `argmin_array` + `get_element` pull the optimum out of the curve **in-graph**;
  a parallel deterministic curve (travel fixed at its median) gives the naive
  optimum, and `EVIU = E[cost | naive decision] − min E[cost]`.

Result (n = 2000): a U-shaped expected-cost curve; the stochastic optimum leaves
**~130 min** early for a min expected cost of **~40 min**, while ignoring
uncertainty picks **105 min** — which costs **~167 min** in expectation because
you still miss the plane ~40% of the time. **EVIU ≈ 125–130 min**: the expected
minutes saved by accounting for uncertainty. That whole computation — the §2
across-realization reductions plus decision optimization — is exactly what the
mechanical 0.1.0 conversion cannot express, and what the engine does natively.

## Native re-solution — `platform_decommissioning_native.wasim.json`

The companion to the `Platform_2017` stress test: a compact hand-authored v2
model (`build_platform_native.py` regenerates it, `engine/tests/platform_native_v2.rs`
guards it) that distills the real tool's *purpose* — rank decommissioning options
by multi-attribute value under uncertainty — into WASiM-native machinery.

- A `Decision_MC` **submodel** scores three options (full removal / reef-partial /
  leave-in-place) across four swing-weighted attributes (cost, reef habitat,
  access, air quality) **per realization**. Independent per-option cost and
  reef-benefit multipliers (the real model's `Cost_uncertainty` and `Biomass`
  Lognormals) make the *ranking itself* uncertain. A `vector_map` over the Option
  axis builds the min-max value functions and the weighted score; `argmax_array`
  picks the winner.
- The submodel exposes **scalar** outputs (each option's score, the chosen index,
  the chosen cost) — a probe (`submodel_stat` reduces member-0 of a vector output,
  not per-member) confirmed scalar outputs are the way to reduce across
  realizations. The parent then composes the answer natively:
  - `mean` → expected score per option, and `argmax_array` over those → the
    **recommendation**;
  - `cumulative_prob` / `exceedance` **differences** → the **probability each
    option is preferred** (`best_idx ∈ {1,2,3}`, so `P(=j)` is a CDF gap);
  - `exceedance` → the chance the preferred option's cost blows the budget.

Result (n=4000): expected scores Leave 68 > Reef 62 > Full 23, so the tool
**recommends Leave-in-place** — but the probabilistic view shows that's not a
foregone conclusion: **Leave is preferred in ~63% of futures, Reef in ~37%**, and
Full removal essentially never. That "how robust is the recommendation" question —
`Probability(option is best)` over the Monte-Carlo sample — is exactly what the
0.1.0 conversion stubs out and what the engine computes in-graph here.

## What maps (and what doesn't)

Grounded in `ANALYTICA_ENGINE_GAP_ANALYSIS.md`. The two tools are architecturally
different: Analytica centres on static Intelligent Arrays + Monte-Carlo with an
optional `Time` axis; WASiM is a time-stepping engine with arrays/sampling as
substrate. This converter targets the **arithmetic + probabilistic core** that
maps cleanly and produces a scalar / 1-step model.

| Analytica node class | WASiM element |
|---|---|
| `Chance` with a distribution definition | `random_variable` |
| `Decision`, `Constant` (numeric) | `constant` (editable for Decision/Constant) |
| `Variable` / `Determ` / `Objective` (formula) | `expression` |
| `Variable` / `Determ` (bare number) | `constant` |
| `Index` (constant members: `Sequence`, list, `Table`) | `array` (expressions form) |
| `Module` / `Model` / `Library` / `Form` | `container` |
| `Function` | *skipped* (no v0.1.0 equivalent) |

| Analytica definition construct | Mapping |
|---|---|
| `+ - * / ^`, comparisons, `And`/`Or`/`Not` | binary/unary AST ops |
| `If c Then a Else b` **and** `If(c, a, b)` | `if` AST node |
| `Var x := e; … result` local-variable blocks (also `Var … Do …`) | inlined (`let`-lowered): bindings resolved in order and substituted into the return expression; an unparseable binding stubs locally |
| `1M`, `10K` numeric suffixes | expanded (`1000000`, `10000`) |
| `Normal`, `Lognormal`, `Uniform`, `Triangular`, `Beta`, `Gamma`, `Exponential`, `Bernoulli`, `Weibull` | `random_variable` distribution (top-level Chance defs). `Lognormal(median, gsdev)` → log-space `(ln median, ln gsdev)`; `Exponential(rate)` → `mean = 1/rate` |
| `Min`/`Max` (2 args) | scalar `min`/`max` |
| `Sum`/`Mean`/`Min`/`Max`/`Size` (reduction over an index) | `sum_array`/`mean_array`/`min_array`/`max_array`/`size_array` |
| `Abs, Sqrt, Exp, Ln, Log, Sin, Cos, Tan, ArcTan2, Floor, Ceil, Round, Mod, Sign, Sinh, Cosh, Tanh` | engine builtins |
| `Sequence(a, b, c)`, `[a, b, …]`, `Table(I)(…)` | array literal / `array` element |

### Degrades to an inert stub + warning

- **Across-realization sample statistics** — `Probability`, `ProbBands`,
  `GetFract`, `CDF`, `SDeviation`, `Variance`, `Frequency`, `Correlation`,
  `Cumulate`, `Dynamic`, … These are the one genuine semantic gap
  (gap analysis §2): in WASiM they live in the results/analysis layer (A3,
  `results_spec`), not as a mid-graph value. A single-arg `Mean`/`Average` is
  mapped to `mean_array` **with an axis-caveat warning**.
- **Runtime-dynamic / label indices** — `subset`, `sortIndex`, `shuffle`,
  index membership computed at runtime. WASiM dimensions are pre-declared.
- **User `Function`s, metaprogramming, GUI/form logic** — no declarative
  numeric equivalent.
- **Any unrecognized function** — preserved as a stub with its original text in
  `display`, so nothing is silently dropped.

## Known limitations

- **Time.** Static Analytica models become a 1-step run
  (`duration = timestep = 1`). A model whose logic depends on `Dynamic[]`
  recurrence maps naturally onto WASiM stocks, but this converter does **not**
  auto-lift `Dynamic()` into an `accumulator` — review those by hand.
- **Intelligent-Array broadcasting** is not reproduced; arithmetic is scalar.
  Index-dimensioned variables collapse to scalars/array-literals.
- **Units.** `Units:` is carried onto outputs verbatim. Non-SI units (e.g.
  `USD`) parse fine but the engine's unit checker will note they're outside its
  registry — harmless.
- The `.ana` attribute reader keys new attributes off known keyword prefixes;
  an exotic multi-line attribute could confuse it. Definitions that fail to
  parse become inert stubs (with a warning), never a hard error.

Treat the output as a **faithful-where-possible draft**: read the warnings and
review before trusting numbers.

## Fixtures

`tools/fixtures/*_synthetic.ana` are **hand-written** models used to exercise
the converter end-to-end (they are **not** real Analytica corpus files):

- `eviu_plane_catching_synthetic.ana` — mirrors the structure of the EVIU
  "when to leave for the airport" example (decision + uncertain travel time +
  miss-penalty cost).
- `coverage_probe_synthetic.ana` — exercises the mapping table and every
  degradation path.
