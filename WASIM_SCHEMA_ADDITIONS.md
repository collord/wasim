# WASiM schema additions required to fully represent GoldSim models

**Status:** proposal for the schema owner. Authored 2026-06-27 from the `re-gsm` decoder/emitter.
**Schema in question:** originally `model.schema.json` (0.7.0); current target is
`wasim-schema-v2.json` (the primitives schema). **Updated 2026-07-17 against schema 0.9.0.**

> **Progress since authoring (2026-07-17, schema 0.9.0).** Several proposals below have
> **shipped** and are marked ✅ inline:
> - **Gap 1b (`extern_call` escape) — ✅ SHIPPED** (0.8.3 arrays round). Any source function
>   outside the `call.fn` enum now round-trips losslessly as `extern_call` (name + args, refs
>   preserved) instead of collapsing to `0.0`. This resolves the *data-loss* half of Gap 1 for
>   the whole function vocabulary; Gap 1a (below) is now only about which functions the engine
>   *evaluates* vs. leaves opaque.
> - **Gap 2 (arrays: `index_ref` / `index` / `vector_map` + `output_spec.dimensions`) — ✅ SHIPPED**
>   (0.8.3). The largest structural gap by input count (~611) is closed at the schema level; the
>   engine's array executor status is tracked in `wasim-engine-semantics.md` §15.
> - **`gamma` builtin — ✅ SHIPPED** (0.8.5), added to `call.fn` for Weibull/gamma scale derivation.
> - **Dynamic (per-timestep) optimization — ✅ SHIPPED** (0.9.0, §13a) — unrelated to this doc's
>   gaps but relevant to Gap 4 (calibratable convolution kernels feed the same optimization path).
>
> **Update 2026-07-17 (schema 0.9.1): Gaps 4, 5, 1a, 3, 6, 7 all LANDED.** The
> correctness-critical pair (5 mean-reversion, 4 convolution-response-expression) plus 1a
> (function vocabulary, now engine-evaluated) ship with engine execution + tests; 3 (spreadsheet
> value_rule — engine placeholder, emit population of cell ranges deferred to the decoder side),
> 6 (financial `payoff_spec`), and 7 (series calendar/ensemble metadata) ship as schema+emit
> fidelity additions. Per-gap status in the tables below. Nothing in this doc remains open except
> the deferred emit population of spreadsheet cell ranges (Gap 3, decoder-side).

## Context

`emit.py` now emits each formula from GoldSim's **native parsed AST** (the `*SNode` tree the decoder
reads at `input.expr._oc`), not by re-parsing the display string. Coverage across the 283-file corpus:

- **92.0% of formula inputs (8,607 / 9,360) emit a faithful native `ast_node`.** Corpus 283/283 schema-valid.
- The remaining **8.0% (753) fall back to the legacy string path** because the construct has **no
  representation in the current schema** — not because we can't parse it. We decode these constructs
  fully; the schema simply has nowhere to put them.

This document describes the schema additions that would let the remaining 8% (and several already-lossy
cases) be represented faithfully. There are **two real gaps** plus **two minor doc/contract fixes**.

The whole 8% splits cleanly:

| Gap | GoldSim construct | fallback inputs | §  |
|-----|-------------------|----------------:|----|
| 1   | Built-in functions outside `call.fn` enum | ~140 | [§1](#gap-1-builtin-function-vocabulary) |
| 2   | Arrays / vectorized (`X[row]`, `vector()`, `matrix()`) | ~611 | [§2](#gap-2-arrays-and-vectorized-expressions) |

---

## Gap 1: built-in function vocabulary  (1b ✅ SHIPPED; 1a partial)

### The problem
`ast_node`'s `call` branch constrains `fn` to a closed enum of ~35 functions:

```json
"fn": { "type": "string", "enum": ["min","max","abs","sqrt","exp","ln","log","log2",
  "sin","cos","tan","asin","acos","atan","atan2","sinh","cosh","tanh","floor","ceil",
  "round","mod","sign","int","step","sum_array","mean_array","min_array","max_array",
  "size_array","get_element","dot_product","interp_array"] }
```

GoldSim ships many more built-ins. When a formula uses one outside this set, the whole expression
falls back to the (lossy) string path. The functions actually hit in the corpus, by category:

| category | functions (corpus count) |
|----------|--------------------------|
| **Distribution introspection** | `pdf_value` (138), `pdf_mean` (69), `pdf_sd` (64), `pdf_cumprob` (2) |
| **Date/calendar extraction** | `GetYear` (29), `GetMonth` (20), `GetDay` (20) |
| **Linear algebra / matrix** | `matrix` (40), `inv` (5), `trans`, `vvmatrix`, `sdc`, `sdv`, `sort321`, `minr`, `maxc`, `rowmax` (2), `rowmin` (2), `sumr` (3) |
| **Table / array introspection** | `getcolumncount` (5), `tablemin`, `tablemax` |
| **Finance** | `ptof` (3, present→future factor), `ptoa` (annuity factor) |
| **Importance sampling** | `ImpProb`, `ImpWeight` |
| **Special math** | `erf` (2) |
| **Interp/index** | `vindex` (5) |

(`atan2` is already in the enum but missing from the emitter's name map — that one is an emitter fix,
not a schema gap.)

### Proposed addition

Two complementary changes:

**1a. Extend the `call.fn` enum** with the well-defined, engine-implementable functions:

```
erf, erfc,
get_year, get_month, get_day, get_hour, get_minute, get_second,   // date extraction (1 date arg)
table_min, table_max, column_count,                               // table/array introspection
pv_factor, annuity_factor                                          // finance (ptof/ptoa)
```

These have clear, single-valued semantics and concrete arguments — the engine can implement them.

**1b. Add an escape op for source functions the engine does not (yet) implement** — so the call is
preserved losslessly instead of collapsing to `0.0`:  **✅ SHIPPED (0.8.3).**

```json
{
  "description": "A source-model function the engine does not implement. Preserves the call (name +
                  args) for round-tripping and inspection; the engine treats it as opaque/NaN.",
  "properties": {
    "op":   { "const": "extern_call" },
    "fn":   { "type": "string", "description": "Source function name, verbatim (e.g. 'pdf_mean')." },
    "args": { "type": "array", "items": { "$ref": "#/$defs/ast_node" } }
  },
  "required": ["op", "fn", "args"],
  "additionalProperties": false
}
```

This covers the GoldSim-specific families that have no portable engine semantics — distribution
introspection (`pdf_*`), matrix linear algebra (`matrix`/`inv`/`trans`/…), importance sampling,
`vindex`. Their **argument refs still resolve** (the connectivity is preserved), which is the main
value; the engine can warn or yield NaN for the call itself.

> Why not just widen `call.fn` to any string? Because the enum is the engine's *implemented* contract.
> Keeping `call` for implemented functions and `extern_call` for the rest keeps that contract honest
> while never losing data.

---

## Gap 2: arrays and vectorized expressions  (✅ SHIPPED 0.8.3)

> **✅ SHIPPED in 0.8.3.** `index_ref`, `index`, and `vector_map` ast_node ops and
> `output_spec.dimensions` are all in `wasim-schema-v2.json`; top-level `dimensions[]` declares
> the ordinal sets. Engine executor status: see `wasim-engine-semantics.md` §15. The design below
> is retained for reference; it matches what shipped.

### The problem
GoldSim has first-class **arrays** (vectors/matrices) whose dimensions are **Ordinal Sets** (e.g.
`Months` = 12 members). Three AST node families encode array math, and none is representable today:

| node | GoldSim syntax | example (corpus) | count |
|------|----------------|------------------|------:|
| `OffsetSNode` | `Element[index]` | `I_daily[row]`, `ETc[Water]` | 452 |
| `RowSNode` / `ColSNode` | `row` / `col` (implicit loop index) | `vector(MIN(I_daily[row], …))` | 159 |
| (`VectorFuncNode` `vector(…)`) | array comprehension over a set | `vector(if(NW[row] >= 3, …))` | mapped→`array` today, but lossy |

`vector(expr)` builds an array by evaluating `expr` — which references the implicit index `row` — once
per member of an ordinal set. The current schema has only `array` (a fixed list of element ASTs) and
`get_element` (a scalar call). There is no way to express **element access by index**, **the iteration
index itself**, or **the comprehension** that ties them together. So `vector(MIN(I_daily[row], …))`
can only be flattened to `min(I_daily, …)` with the indexing dropped — lossy.

### Proposed addition

Three new `ast_node` ops (smallest set that makes vectorized expressions faithful):

```json
// the implicit iteration index inside a vector()/matrix() body
{ "op": { "const": "index_ref" },
  "axis": { "enum": ["row", "col"], "default": "row" } }

// array element access: array[i] or matrix[i, j]
{ "op": { "const": "index" },
  "array":   { "$ref": "#/$defs/ast_node" },
  "indices": { "type": "array", "items": { "$ref": "#/$defs/ast_node" }, "minItems": 1, "maxItems": 2 } }

// comprehension: evaluate `body` (which may use index_ref) over an ordinal set, producing an array
{ "op": { "const": "vector_map" },
  "over":  { "type": "string", "description": "Ordinal-set element id the result is dimensioned over." },
  "body":  { "$ref": "#/$defs/ast_node" } }
```

With these, `vector(MIN(I_daily[row], ETc[row]))` becomes:

```json
{ "op": "vector_map", "over": "Months",
  "body": { "op": "call", "fn": "min_array", "args": [
      { "op": "index", "array": {"op":"ref","element_id":"I_daily"}, "indices": [{"op":"index_ref","axis":"row"}] },
      { "op": "index", "array": {"op":"ref","element_id":"ETc"},     "indices": [{"op":"index_ref","axis":"row"}] } ] } }
```

The row/column reductions (`rowmax`/`rowmin`/`maxc`/`minr`/`sumr`) then become ordinary `call`s once
added to the enum (§1a) — they operate on the array, not the index.

**Companion (element typing):** to consume arrays, the engine also needs to know an element/output is
array-valued and over which set. `output_spec` currently carries only `{name, unit}`. Suggest an
optional:

```json
"dimensions": { "type": "array", "items": { "type": "string" },
                "description": "Ordinal-set element ids this output is dimensioned over (empty = scalar)." }
```

The decoder already has the data (`ValueType._dim` SI vector + the `OrdinalSet` objects on each port).

### Phasing
Gap 2 is the larger design. If first-class array support is not wanted yet, the **`extern_call` escape
from §1b also covers it losslessly in the interim**: emit `OffsetSNode`/`vector()` as
`extern_call("index", …)` / `extern_call("vector", …)`, preserving structure and refs until the
first-class ops land. Recommend shipping §1 first (small, unblocks ~140 inputs + the reductions) and
treating §2 as a follow-up.

---

## Minor related items (not schema additions)

1. **`connection_edge` description is stale.** It says the integer `from_id`/`to_id` "are NOT
   necessarily resolved to elements … resolving them requires an object_id→element map." The emitter
   now attaches `source_object_id` to every element and container, and the edges are keyed on it — so
   they **are** resolvable within the document. The description should be updated to name
   `source_object_id` as the join key. (Text fix, no structural change.)

2. **Unit strings are GoldSim display units, not a canonical vocabulary.** emit passes through the
   source display unit (`pers/d`, `KAF/yr`, `kpers`). `quantity.unit` is free-form so these validate,
   but they are not normalized to a canonical `units.json` vocabulary. If the engine expects canonical
   units, a mapping layer is needed (separate from the schema); otherwise document that `unit` carries
   source display units and `display_unit` is their presentation form.

---

## Impact summary

Of the 753 fallback inputs (8.0%), the first unmapped node is: array-indexing
(`OffsetSNode` 452 + `RowSNode` 156 + `ColSNode` 3 = **611**), a non-enum function (**140**), or a
`TableRef` arity edge case (**2**).

| Addition | Unblocks | Effort | Status |
|----------|----------|--------|--------|
| §1b `extern_call` escape | ~140 inputs (lossless) | small | ✅ shipped 0.8.3 |
| §1a extend `call.fn` enum | function *evaluation* (else extern_call) | small | open |
| §2 `index_ref` / `index` / `vector_map` + `output_spec.dimensions` | ~611 array-indexing inputs | medium | ✅ shipped 0.8.3 |
| connection_edge description fix | (docs correctness) | trivial | open |

With §1b + §2 shipped, native-AST coverage is essentially complete — every construct either emits
a faithful `ast_node` or a lossless `extern_call`. §1a remains only to promote specific opaque
`extern_call`s to *evaluated* `call`s (the engine computes them rather than yielding NaN).

---

## Gap 3: SSpreadSheet has no v2 primitive (2026-07-17, v2 emit round)

**Schema in question:** `wasim/schema/wasim-schema-v2.json` (the primitives schema — the only
active emit target).

**The problem.** The v2 emit pass now maps every decoded element class to a primitive except
**`SSpreadSheet`** (GoldSim's linked-Excel element — 7 instances across 5 corpus models:
Plume, Plume (1), Reservoir Carryover Realization, Spreadsheet, …). None of the six primitives
(node/stock/link/event/gate/cell) fits: a spreadsheet element is a grid of Excel-range-bound
I/O cells, not a per-timestep value rule. It is left `unmapped` (allowlisted in `emitcheck.py`
as `UNMAPPED_OK`) rather than force-fit.

**What the decoder has** (see `format_spec.md` "SSpreadSheet"): SElement base + a `+0x88`
embedded cell collection — N input cells (each an `SSpreadSheetInput` + an Excel range ref
like `"WGEN Parameters!B5"`) and N output cells (each an `SDataOutput` + range like
`"Observed Stats!B7:B18"`) — plus an optional linked-workbook `ExternalFileLock`. So the full
provenance (which model port maps to which Excel cell, and the source workbook) is recoverable.

**Options for the schema owner (pick one):**

1. **A `spreadsheet` node value_rule** (mirrors the old 0.7.0 `spreadsheet` type): fields
   `cells` (the decoded input/output range map), `external_file` (workbook ref), `links`
   (named I/O ranges). Runnable only when the workbook is present, but round-trips faithfully.
2. **Decompose to per-output `expression` nodes** — one node per output cell whose formula
   is the cell's Excel formula. Runnable where cell formulas resolve to model refs, but loses
   the workbook binding and doesn't handle formulas we can't translate from Excel.
3. **Leave unmapped** (current state) and treat linked-Excel models as out of scope.

**Recommendation:** option 1 — it is the smallest addition that preserves the decoded data,
matches the retired 0.7.0 vocabulary, and keeps the door open for an Excel-evaluation engine
round later. This is a schema-vocabulary decision, not an emit-side blocker; emit will populate
whichever shape is chosen. Until then SSpreadSheet stays the one allowlisted unmapped class.

---

## Gap 4: convolution response is expression-valued (2026-07-17, v2 emit round)

**Schema in question:** `wasim-schema-v2.json`, `node` / `value_rule: "convolution"`, `response`.

**The problem.** The `response` field accepts only a sampled `{times, values}` table or an
element id. But GoldSim's Convolver defines its response as a **formula over the lag variable
`~Lag`** — in all 8 corpus instances (Convolution, Earthquake, GR4J, Warranty_Costs), never a
tabulated series. So emit must **sample the formula at emit time** onto the element's lag grid.

Sampling **bakes any referenced parameter** into the numbers. GR4J's routing unit hydrographs
are `IF(~Lag < X4, (~Lag/X4)^2.5, 1)` where `X4` is a **calibration parameter** — exactly the
kind of value an optimization study varies. Baking `X4` at emit time makes the response static,
which defeats the probabilistic-optimization use case the 0.8.1–0.8.2 rounds were built for.

Emit currently carries the faithful encoding on the node (`additionalProperties:true`):
`response_expression` (the `~Lag` AST — `~Lag` itself is an `extern_call fn:"lag"`),
`response_length_s`, `response_interval_s`, `response_cumulative`. The engine ignores all of it
and reads only the baked `response.times/values`.

**Proposal.** Let `response` also be an expression form the engine evaluates against a lag axis:

```jsonc
"response": {
  "expression": { "ast": { … over an `extern_call fn:"lag"` node … } },
  "length":   { "value": 864000.0, "unit": "s" },   // response support
  "interval": { "value": 86400.0,  "unit": "s" },   // lag sampling step
  "cumulative": true                                 // response is an S-curve; weights = its diffs
}
```

The engine samples it (once, or per-realization if it references varying elements), applying the
`cumulative` differencing emit does today. **Effort: medium.** Unblocks parameter-driven /
optimization-driven convolution kernels; without it, calibratable unit hydrographs are wrong.

---

## Gap 5: `process_spec` is GBM-only and drops mean reversion (2026-07-17, v2 emit round)

**Schema in question:** `wasim-schema-v2.json`, `$defs/process_spec` (used by `node` /
`value_rule: "process"`).

**The problem.** `process_spec` is `{family: "gbm", mean_type, mean, stddev}` with
`additionalProperties:false`. GoldSim's SHistoryGenerator (24 corpus instances — HistoryGenerator,
CashFlowAlternatives, Control Systems, Option, …) is a more general random-walk generator whose
five ports are: drift, volatility, **reversion rate**, initial value, **reversion reference
target**. A non-zero reversion rate makes it a mean-reverting (Ornstein-Uhlenbeck-style) process,
not plain GBM — several corpus instances set it (e.g. HistoryGenerator's `*_Reverts` elements).

Emit maps drift→`mean`, volatility→`stddev`, and `_u100`→`mean_type` (`0` = geometric,
`1` = arithmetic), but `reversion_rate`, `reference_value`, `initial_value`, and
`change_model_code` ride along on the node, unread by the engine. So a mean-reverting walk
currently executes as a non-reverting one — a **results-changing** loss.

**Proposal.** Either add a `mean_reverting` family, or extend `process_spec` with optional
`reversion_rate` (quantity_or_formula), `reference_value` (quantity_or_formula), and
`initial_value` (quantity). Keep `mean_type` for the geometric/arithmetic distinction.
**Effort: small–medium.** Unblocks correct mean-reverting stochastic processes.

---

## Gap 6: financial events have no payoff/effect model (2026-07-17, v2 emit round)

**Schema in question:** `wasim-schema-v2.json`, `event` primitive (`effects`, `effect_spec`).

**The problem.** SInsurance (6) and SOption (5) decode with full port data but emit as
**structural** events — `effects: []` — because there is no decodable effect target and no
schema shape for their conditional payoffs. An option pays off when the underlying crosses the
strike; insurance pays claims above the deductible up to a coverage cap. `effect_spec` models
`{target, change, mode}` additive/multiplicative/replace changes — not a threshold-conditional
payout.

Emit carries the financial parameters on the event node (`additionalProperties:true`):
- SOption: `option_style` (american/european/asian), `option_kind_code`, `strike_input`,
  `maturity_input`, `underlying_input`, `units`.
- SInsurance: `claims`, `deductible_input`, `coverage_limit_input`, `reset_mode`
  (after_each_claim/annual/never), `reset_mode_code`.

All inert — the engine sees a triggerless-payoff event.

**Proposal (optional, lower priority).** A payoff-effect shape, e.g. an `effect_spec` variant
`{kind: "payoff", condition, amount}` or dedicated option/insurance value_rules. Only worth it
if executing financial models is a goal; until then these are faithful-provenance-only.
**Effort: medium** (needs a payoff-semantics spec first).

---

## Gap 7: series metadata — calendar epoch + realization ensembles (2026-07-17, v2 emit round)

**Schema in question:** `wasim-schema-v2.json`, `node` / `value_rule: "series"`.

**The problem.** Two faithful bits of a time series have no formal home and ride on
`additionalProperties:true`:
- **Calendar re-basing.** Calendar-based series store timestamps as seconds since GoldSim's date
  epoch (t[0] ≈ 3e9). Emit re-bases them to elapsed-from-sim-start (t starts at 0) and carries
  `calendar_based` + `calendar_start_seconds` so the absolute axis is recoverable. Without a
  schema field, a consumer can't tell a re-based calendar series from a native elapsed one.
- **Realization ensembles.** Two corpus files store **100 histories per series** (a stochastic
  ensemble); emit keeps the first and carries `n_histories`. Multi-column (array-valued) series
  carry `extra_value_rows`. The engine sees only the first history/column.

**Proposal (minor).** Optional `series` fields: `calendar_epoch_seconds` (null = elapsed) and
either an ensemble representation or an explicit "first-of-N, N carried" contract. **Effort:
small.** Mostly fidelity/round-trip; ensembles matter only if per-realization series playback is
a goal.

---

## Consolidated priority (2026-07-17)

Ranked by whether the carried-but-unread data changes **simulation results** (not just
round-trip fidelity):

| Gap | Class(es) | Instances | Changes results? | Effort | Status |
|-----|-----------|----------:|:---:|--------|--------|
| 4 | SConvolution response expression | 8 | **Yes** — bakes calibration params | medium | ✅ shipped 0.9.1 |
| 5 | `process_spec` mean reversion | 24 | **Yes** — reversion executed as non-reverting | small–med | ✅ shipped 0.9.1 |
| 1a | function vocabulary (evaluation) | ~140 inputs | no (extern_call is lossless) | small | ✅ shipped 0.9.1 |
| 1b | `extern_call` escape | ~140 inputs | — | small | ✅ shipped 0.8.3 |
| 2 | arrays / vectorized | ~611 inputs | — | medium | ✅ shipped 0.8.3 |
| 6 | financial event payoffs | 11 | only if executing financial models | medium | ✅ shipped 0.9.1 (schema; not executed) |
| 7 | series calendar/ensemble metadata | ~110 + 2 | fidelity only | small | ✅ shipped 0.9.1 |
| 3 | SSpreadSheet primitive | 7 | class is unmapped entirely | medium | ◑ 0.9.1 schema+placeholder (emit cell-range population deferred) |

All gaps in this doc have landed. Gaps 4/5 (correctness) and 1a execute in the engine with tests;
6/7 are schema+emit fidelity; 3 has a schema home + a fixed-0 engine placeholder, with only the
decoder-side population of spreadsheet cell ranges still to do.
