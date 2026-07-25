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
