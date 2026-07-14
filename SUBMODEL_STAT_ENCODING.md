# Spec: encoding pdf / submodel-statistic operations in the AST

**For:** the re-gsm decode/emit side. **From:** the WASiM schema/engine owner.
**Status:** design decision — implement the emit side against this shape; the engine decode + executor land in parallel.

---

## 1. The problem

Objective and result expressions that read a **Monte-Carlo statistic of a submodel output** — e.g. `pdf_mean(Prob_StormwaterModel.total_cost)`, `PDF_Value(SubModel1.System, 0.95)` — currently emit with the pdf term **thrown away**: the AST is either a pure stub `{"op":"literal","value":0.0}` or a partial tree in which the pdf sub-term was replaced by `literal 0.0`. The real intent survives only in the `expression.display` string.

Consequence: the engine parses these fine but computes the wrong math (a constant `0.0`). Every probabilistic optimization objective is currently a constant, so there is nothing meaningful to optimize. This spec defines a real AST encoding so emit can stop stubbing and the engine can compute the statistic.

Verified in the current corpus (162 files): the pdf operation appears **only** in display strings (`pdf_value` ×12, `PDF_Mean` ×9, `PDF_Value` ×4, `PDF_CumProb` ×2, `pdf_mean` ×2, `PDF_mean` ×1, `PDF_SD` ×1, `PDF_sd` ×1, across 9 files). There is no `op` or `call` `fn` representing it, and no dotted `Submodel.Output` reference in any structured field (0 dots in any `id`, `inputs[]`, or ast `ref.element_id`).

## 2. The new AST node

Add a new `ast_node` variant discriminated by `"op": "submodel_stat"`:

```jsonc
{
  "op": "submodel_stat",
  "submodel_id": "Model/Prob_StormwaterModel",        // full slash-path id of the submodel *container*
  "output":      "Model/Prob_StormwaterModel/total_cost", // full slash-path id of the interface output *element*
  "statistic":   "mean",                              // mean | percentile | sd | cumulative_prob
  "arg":         { "op": "literal", "value": 95.0 }   // see §4; present for percentile + cumulative_prob, omitted for mean/sd
}
```

It is a first-class AST node (like `ref` / `lookup_call`), not a `call` with a
function name — because it needs a *structured* reference (two ids) plus a typed
statistic, which cannot ride through a generic `call`'s positional `args`.

## 3. Display → `statistic` mapping (case-insensitive)

The display function name is raw human text with inconsistent casing. Normalize case-insensitively:

| Display token(s) | `statistic` | `arg` |
|---|---|---|
| `pdf_mean`, `PDF_Mean`, `PDF_mean` | `mean` | omitted |
| `pdf_value`, `PDF_Value` | `percentile` | required — the percentile (see §4) |
| `pdf_sd`, `PDF_SD`, `PDF_sd` | `sd` | omitted |
| `pdf_cumprob`, `PDF_CumProb` | `cumulative_prob` | required — the threshold value (see §4) |

If a new pdf function appears that isn't in this table, do **not** silently stub it — flag it so we extend the encoding.

## 4. The `arg` sub-node

`arg` is a full `ast_node` (so it can be a literal, a `ref`, or an expression — the corpus has `PDF_Value(x, Percentiles[row])` where the percentile is itself an array element).

- **`percentile`**: the percentile, **as p in [0, 100]**. Displays use a fraction (`0.95`); emit multiplies by 100 during lowering → `arg` literal `95.0`. This matches the schema's `optimization.objective.statistic.percentile.p` range `[0,100]`. When the display's percentile is an expression rather than a literal (e.g. `Percentiles[row]`), lower the expression and wrap it so the value it produces is on the 0–100 scale (multiply the lowered expression by 100 if the source is a 0–1 fraction).
- **`cumulative_prob`**: the threshold value the CDF is evaluated at. It may be **unit-bearing** in the display (e.g. `PDF_CumProb(Failed3_Time, 6000 hr)`), so emit `arg` as a `literal` **with its `unit`** (`{"op":"literal","value":6000,"unit":"hr"}`), SI-normalized like every other quantity.
- **`mean` / `sd`**: no `arg`; omit the field.

## 5. Reference lowering (the dotted `Submodel.Output`)

The display's `Submodel.Output` (e.g. `Empirical.Annual_Totals`, `SubModel1.System`) must lower to **two full slash-path ids**:

- `submodel_id` = the submodel *container's* id (e.g. `Model/Prob_StormwaterModel`).
- `output` = the interface output *element's* id (e.g. `Model/Prob_StormwaterModel/total_cost`).

Emit already resolves interface outputs to their producing element, so this resolution exists — it must be written into the node instead of dropped. **Both fields must be full slash-path ids**, per the global id-uniqueness invariant (`wasim-engine-semantics.md` §1). Note interface-output naming is currently inconsistent in the emitted corpus (bare `System` vs full-path `Model/.../total_cost`, sometimes mixed within one submodel's `interface.outputs`) — normalize all of these to full paths.

Also add `submodel_id` to the enclosing element's `inputs[]` (as is already done — the submodel container is listed as an input), so the dependency is visible to the graph.

## 6. Engine evaluation contract

The engine evaluates `submodel_stat` by:

1. Running the named submodel's nested realization loop under its own `simulation_settings` (§12 SubModels).
2. Collecting the `output` element's per-realization final values.
3. Reducing across realizations by `statistic`:
   - `mean` → arithmetic mean.
   - `percentile` → nearest-rank percentile at `arg` (p in [0,100]).
   - `sd` → sample standard deviation.
   - `cumulative_prob` → fraction of realizations whose value ≤ `arg` (the empirical CDF at the threshold).
4. The reduced scalar is a constant available at every step of the parent run.

Until the executor ships, the engine parses `submodel_stat` and evaluates it to `0.0` with a warning (same policy as a dangling reference) — so real ASTs are accepted, not rejected, ahead of full execution.

## 7. What emit needs to do

- Parse the `Submodel.Output` and the pdf function from the display.
- Resolve the two ids (§5), map the function to `statistic` (§3), lower the `arg` (§4).
- Emit the `submodel_stat` node in place of the current `literal 0.0` stub — including when it is a **sub-term** of a larger expression (e.g. `Slope + (PDF_Value(SubModel1.System, 0.95) - 10)^2` must keep the surrounding `add`/`power`/`subtract` and only replace the inner stub).
- Keep `expression.display` as-is (it stays the human-readable form); `source` stays `explicit`.

## 8. Open question

Percentile convention: this spec standardizes on **p in [0, 100]** in `arg` (so a `0.95` display → `95.0`) to match the schema's existing percentile range. If storing the raw 0–1 fraction is strongly preferred on the emit side, say so and we'll align the schema/engine instead — but [0,100] is the recommendation for consistency with `optimization.objective.statistic`.
