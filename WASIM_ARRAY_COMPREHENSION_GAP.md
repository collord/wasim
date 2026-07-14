# Schema gap: v2 has no array-comprehension / dimension AST nodes

**For:** the WASiM schema/engine owner. **From:** the emit side.
**Ask:** v2 (0.8.x primitives) `ast_node` cannot represent GoldSim's array constructs, so
array-valued formulas — including the last 6 stubbed `pdf_*` objectives — degrade to `literal 0.0`.
This brief describes the gap, the data emit already decodes, the 0.7.0 prior art, and a proposed
node set for you to design against.

## The gap

The 0.8.0 primitives rewrite dropped the array nodes that 0.7.0 had:

| | `ast_node` ops |
|---|---|
| **0.7.0** (`model.schema.json`) | literal, ref, time_ref, call, array, if, lookup_call, `extern_call`, **`index_ref`**, **`index`**, **`vector_map`** |
| **v2 0.8.2** (`wasim-schema-v2.json`) | literal, ref, time_ref, call, array, if, lookup_call, submodel_stat |

So in v2 there is no way to encode: an array built by iterating a dimension, an array subscript,
or the loop index. Emit produces these in the 0.7.0 AST fine; `_v2_sanitize_ast` then stubs each to
`literal 0.0` because v2 has no matching op.

**Impact:** **308 elements across 35 models** use `vector_map`/`index_ref` in the 0.7.0 output —
all lost in v2. This also strands **~295 ordinal-set references** (`Months` ×214, `Zones`,
`Elev_Zones`, `Quantiles`, …) as dangling `ref`s: they're array *dimensions*, not elements, but v2
has no dimension concept to point them at. And it's why 6 `pdf_*` terms still stub (the `submodel_stat`
is correct, but it sits inside a `vector`/`matrix` that has no v2 encoding).

## The GoldSim constructs + what emit decodes (data is all present)

Example (WGEN `Annual_Stats`):
`vector(if(row == 1, pdf_mean(Empirical.Annual_Totals), PDF_Value(Empirical.Annual_Totals, Percentiles[row])))`
decodes to this 0.7.0 AST:

```jsonc
{ "op": "vector_map",
  "over": "Percentiles",                       // the ordinal set / dimension iterated over
  "body": {
    "op": "if",
    "cond": { "op": "eq", "left": {"op": "index_ref", "axis": "row"}, "right": {"op":"literal","value":1.0} },
    "then": { /* submodel_stat (pdf_mean) */ },
    "else": { /* submodel_stat (pdf_value), arg = Percentiles[row] */ } } }
```

The three constructs, and the decoded shapes (from GoldSim's native `*SNode` tree — nothing
inferred):

1. **Comprehension** — `vector(body)` / `matrix(vector(...), …)` →
   `{op: vector_map, over: <ordinal-set name>, body: <ast>}`. `matrix` is nested `vector_map`s.
2. **Loop index** — the current row/col inside a comprehension →
   `{op: index_ref, axis: "row" | "col"}`.
3. **Subscript** — `X[i]` / `X[i,j]` → `{op: index, array: <ast>, indices: [<ast>, …]}`.
   (v2's `call get_element` covers a *positional* subscript, but not one indexed by a dimension.)
4. **Dimension / ordinal-set reference** — a bare `Months` / `Zones` in a formula is a reference
   to an *ordinal set* (a named, ordered dimension: Months = 12 members). Today it emits as a
   `ref{element_id:"Months"}` and dangles, because there's no ordinal-set entity to resolve to.
   (Emit already carries the dimension names on `outputs[].dimensions` in 0.7.0 — `os1`/`os2` —
   but v2 drops those, and there's no top-level ordinal-set declaration.)

## Prior art

0.7.0's `ast_node` already defines `index_ref`, `index`, and `vector_map` — look at their shapes
in `model.schema.json` (`$defs.ast_node.oneOf`). The v2 additions can mirror them (adapted to the
primitives conventions), since emit emits exactly those shapes today.

## Proposed v2 additions (design to taste)

- **`vector_map`** — `{op, over, body}` where `over` is a dimension/ordinal-set id and `body` an
  `ast_node` evaluated per index. (Matrix = nested.)
- **`index_ref`** — `{op, axis: "row"|"col"}` — the current index within the enclosing `vector_map`.
- **`index`** — `{op, array, indices[]}` — subscript, indices possibly `index_ref`s.
- **A first-class dimension / ordinal-set concept** — so `over` and a bare `Months` ref resolve to
  a declared entity. Options: a top-level `dimensions` (or `ordinal_sets`) list `{id, name, members}`,
  and re-expose `outputs[].dimensions`. This is the piece that also clears the ~295 dangling
  dimension refs.

## Emit side once the schema lands

No new decode work — emit already produces `vector_map`/`index_ref`/`index` in the 0.7.0 AST and
resolves the dimension names. When v2 gains these ops, `_v2_sanitize_ast` simply *stops stubbing*
them (and I'll emit the ordinal-set declarations). The 6 `pdf_*`-in-comprehension objectives then
carry their real `submodel_stat` inside a real `vector_map`.

## Priority

Per `EMITTER_HANDOFF.md` §P1-d this was tagged "the bigger array-model question, lowest priority."
That's right for the *dangling-count*, but note it also blocks the 6 array-valued probabilistic
objectives (WGEN/WGEN PAR statistics) from ever being non-constant — so it gates a slice of the
optimization feature, not just cosmetics.

---

## Owner decision (2026-07-13): both restored + dimension entity added — schema 0.8.3

Good brief, and the "regression" framing is right: `vector_map`/`index_ref`/`index` and
`output_spec.dimensions` literally existed in 0.7.0 and were dropped in 0.8.0. Restored all
of them, plus `extern_call` (also dropped, same root cause), plus the new first-class
dimension concept you asked for. **Landed in schema 0.8.3.**

**AST ops** (`ast_node` branches, mirroring 0.7.0 — your decoded shapes match verbatim):
- `vector_map` `{over, body}`, `index_ref` `{axis: row|col}`, `index` `{array, indices[]}`,
  `extern_call` `{fn, args[]}`.

**Dimension concept** (new — 0.7.0 had no top-level entity, only bare `over` strings):
- Top-level **`dimensions[]`** of `{id, name, size, labels?}` — `size` is the member count
  (Months → 12), `labels` optional ordered names. `vector_map.over` and
  `output_spec.dimensions` reference these by `id`. **Member numeric values are NOT stored
  on the dimension** — they stay in whatever element already carries them (e.g. the
  `Percentiles` fixed node), indexed by position, so no duplication. Restored
  `output_spec.dimensions` as a `string[]` of dimension ids too.

**Engine status:** decodes all four nodes and preserves their graph dependencies, but the
dimension-aware **array executor is not yet implemented** — placeholder eval (`vector_map`/
`index_ref`/`extern_call` → 0.0; `index` → its array's scalar view), same degrade-to-zero as
a dangling ref. So models carrying these load and run; array results are placeholders until
the executor round. Documented in semantics §15.

**What emit does now** (as you said — no new decode work):
1. Stop stubbing `vector_map`/`index_ref`/`index`/`extern_call` in `_v2_sanitize_ast` — emit
   the real nodes (your 0.7.0 shapes carry straight over).
2. Emit the top-level `dimensions[]` declarations (`{id, name, size, labels?}`) and re-attach
   `output_spec.dimensions`. This is the piece that lets `over`/`Months`-style refs resolve.
3. The 6 `pdf_*`-in-comprehension objectives then carry their real `submodel_stat` inside a
   real `vector_map`.

**One correction on the numbers, for when you regenerate:** the "~295 ordinal-set dangling
refs (`Months` ×214 …)" figure is from an earlier regeneration and no longer matches the
current corpus — `Months` isn't even in the current top-12 dangling bare refs, which are
mostly non-dimension state variables (`Species`, `UZFWC`, `LZTWC`, `ADIMC`, …). So emitting
the dimension declarations will clear the *genuinely-dimensional* dangles (`Percentiles`,
`Zones`, `Elev_Zones`, `Quantiles`, and `Months` where it appears), but a large residual of
non-dimension dangling refs will remain and is a separate issue (missing interior elements /
local symbols — `EMITTER_HANDOFF.md` §P1-d). Re-measure by bucket after regenerating so we
know what's actually left.
