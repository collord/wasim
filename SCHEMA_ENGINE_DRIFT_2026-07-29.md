# Schema ↔ Engine Drift Report — 2026-07-29

**Question:** During cloud-container engine work (no schema symlink present), did the
v2 WASiM JSON parser quietly gain support for constructs not declared in
`schema/wasim-schema-v2.json` (0.9.7)?

**Answer: Yes — two distinct classes of drift, both confirmed against engine source.**

## Method note (why raw validation was misleading)

`schema_examples_manual/*.json` are **v1-format fixtures** (`wasim_version: 0.1.0`,
elements keyed by `type:` not `primitive:`). The engine routes them to the *legacy* v1
parser (`is_v2_native()` in `src/lib.rs:81` dispatches on presence of `primitive`).
So validating them against the **v2** schema is a category mismatch — every element
fails `oneOf` for lacking `primitive`, producing thousands of cascade errors that hide
the real signal. The findings below come from diffing the engine's serde types directly
against the schema `$defs`, using the fixtures only to confirm the ops are exercised.

---

## Drift class 1 — AST operations in the engine but not the schema

Engine `AstNode` enum (`src/model.rs:570`, `#[serde(tag="op", rename_all="snake_case")]`)
has **36 ops**; `schema/$defs/ast_node.oneOf` declares **27**. All 27 schema ops exist
in the engine (no phantom schema ops). **9 engine ops are undocumented:**

| op (`"op"` value) | engine variant | exercised by manual fixtures |
|---|---|---|
| `run_stat2` | `RunStat2` | 6 files (Asian, barrier, basket, spread, lookback, efficiency) |
| `lsm` | `Lsm` | 3 files (American LSM) |
| `lsm_dual` | `LsmDual` | 2 files (tight dual) |
| `nested_stat` | `NestedStat` | 2 files (exposure profile, nested VaR) |
| `run_stat` | `RunStat` | 1 file (lookback) |
| `submodel_stat2` | `SubmodelStat2` | — (in engine, not in these fixtures) |
| `subscript` | `Subscript` | — |
| `run_regress` | `RunRegress` | — |
| `run_split_beta` | `RunSplitBeta` | — |

These have dedicated integration tests (`run_stat2_v2.rs`, `run_regress_v2.rs`,
`run_splitbeta_v2.rs`, `nested_stat`/LSM smokes), so they are real, tested engine
features — the schema's `ast_node.oneOf` was simply never extended to include them.

## Drift class 2 — formula-valued process parameters not permitted by schema

Engine `ProcessSpec` (`src/model.rs`) types `mean`, `stddev`, `reversion_rate`,
`reference_value`, `initial_value` as **`QuantityOrFormula`** — accepting the
`{ast, display}` expression shape (`expression_field`). The schema **has** a
`quantity_or_formula` def (`quantity | expression_field | string`) that would allow
this, but `process_spec.mean`/`stddev` are wired to plain **`quantity`** instead.

Fixtures using formula-valued process params (fail v2 validation on this alone):
`asian_option_control_variate`, `barrier_option_down_and_out`, `basket_option`,
`correlated_assets_spread_option`, `lookback_option_floating_strike`,
`worst_of_asian_correlated`, `options_pricing_efficiency`, … (`process.mean = {ast,display}`).

Likely the same wiring gap exists on other quantity fields the engine widened to
`QuantityOrFormula` (e.g. `link.condition` at `model.rs:257`). Worth an audit:
grep engine for `QuantityOrFormula` fields and check each corresponding schema slot
references `quantity_or_formula` rather than bare `quantity`.

---

## Recommended schema fixes (v2, → 0.9.8)

1. Add 9 op branches to `$defs/ast_node.oneOf`:
   `run_stat`, `run_stat2`, `run_regress`, `run_split_beta`, `nested_stat`,
   `submodel_stat2`, `subscript`, `lsm`, `lsm_dual`. Field shapes are in the engine
   variant definitions (`src/model.rs:665–856`).
2. Change `$defs/process_spec` fields `mean`, `stddev`, `reversion_rate`,
   `reference_value`, `initial_value` from `{"$ref":"quantity"}` to
   `{"$ref":"quantity_or_formula"}`.
3. Audit all engine `QuantityOrFormula` fields for the same bare-`quantity` gap.

## Caveat

The manual fixtures are **v1-format**, so they are not a clean conformance corpus for
the v2 schema regardless of these fixes. The AST-op and process-param drift is
schema-generation-independent (same `AstNode`/`QuantityOrFormula` types feed both
parsers), so the fixes above are correct — but if you want manual v2 fixtures that
*validate*, they'd need regenerating with `primitive:`-form elements.
