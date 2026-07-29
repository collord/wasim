# Stage-3f — Axis-selective reduction (engine feature)

This closes the one engine gap the dispatch arc kept bumping into. It is an engine change,
not a model — proven by two tests, not a demo run.

## What shipped

The array reducers now take an **optional 1-based axis-position argument** and collapse **one**
axis of an n-d array, keeping the rest. Before this, every array reducer collapsed *all* axes to
a scalar; there was no way to say "sum over `Subsystem`, keep `Fleet`."

Affected builtins (all in `engine/src/eval.rs`):

| builtin | no axis arg (unchanged) | with axis arg `n` (new) |
|---|---|---|
| `sum_array(x)` / `sum_array(x, n)` | scalar sum of all cells | sum over axis `n`, keep the rest |
| `mean_array(x, n)` | scalar mean | mean over axis `n` |
| `min_array(x, n)` / `max_array(x, n)` | scalar extreme | extreme over axis `n` |
| `argmin_array(x, n)` / `argmax_array(x, n)` | 1-based flat index | 1-based index **along** axis `n`, per remaining cell |

`n` is the **position** of the axis in canonical order — for arrays built by align-by-name, that
is the dimension ids **sorted alphabetically** (so in a `[A, B]` grid, `A` = axis 1, `B` = axis 2).
It is 1-based to match the existing `argmin/argmax` 1-based member indexing.

Reducing a 1-D array over its only axis, or calling any reducer with no axis argument, is
**bit-identical** to the historical behavior (the no-axis path is unchanged code). Existing
models are untouched.

## Why position, not a dimension-id string

`WASIM_ARRAY_LANGUAGE_SCOPE.md` deferred this feature because it looked like it "needs a
dimension-id argument," which would mean a new AST node and threading a label through eval. In
fact `collect_ast_refs` walks reducer `args` generically, so an axis argument that is just another
`args[1]` expression needs **zero** new match arms — a new AST node would have cascaded across ~6
sites. Position-in-canonical-order is unambiguous (align-by-name already fixes the order), keeps
the reducers ordinary calls, and is the smallest correct change. A named-axis sugar can be layered
on top later without touching the executor.

## Implementation (all in `engine/src/eval.rs`)

- `NamedArray::strides()` — row-major strides over the array's own axes.
- `NamedArray::reduce_axis(axis_ix, init, fold)` — folds one axis away in **ascending coordinate
  order** (the §6 bit-identity rule, same as `reduce_data`); returns the (n-1)-d array, or a
  `Scalar` when only one axis remains.
- `NamedArray::argreduce_axis(axis_ix, want_min)` — 1-based index of the extreme along the axis,
  per remaining cell; ties resolve to the lowest index; NaN never wins (strict comparison).
- `reduce_value(v, axis, init, fold, is_mean)` — dispatches to `reduce_axis` when an axis is given
  *and* the value is a multi-axis array; otherwise full collapse (unchanged path). `is_mean`
  divides by the reduced axis length.
- `axis_of(args, ctx)` — reads the optional 1-based axis from the reducer's second argument.

The six reducer arms became one-liners over `reduce_value` / `argreduce_axis`.

## Proof

Two tests, both passing, no demo hand-waving:

- **`engine/tests/axis_reduction_v2.rs`** — builds `grid[A,B] = va[A] ⊗ vb[B] = [[1,2,3],[2,4,6]]`
  by align-by-name multiply, then asserts exact values for `sum/mean/min/max` over axis 2 (per-`A`),
  `argmin/argmax` over axis 2, `sum` over axis 1 (per-`B`), and — critically — `sum_array(grid)`
  with **no** axis argument still collapses to `18` (back-compat).
- **`engine/tests/nd_recurrence_v2.rs`** — isolates 2-D lag accumulation
  (`acc[A,B] = lag(acc) + rate`) and asserts `[5,10,10,20]` final + `[4,8,12,16,20]` history. This
  proves the *engine's* n-d recurrence machinery is correct, so any n-d modeling difficulty is in
  the model's physics, not the executor.

## What this unblocks

The general multi-mode failure model. Stage-3e (`STAGE3E_SUBSYSTEMS.md`) worked around the missing
reduction by keeping **one array per failure mode** — fine for 3 modes, awkward at 8. With
axis-selective reduction a `Fleet × Subsystem` damage matrix can be reduced over `Subsystem`
directly:

- fleet availability = product/`min` over the `Subsystem` axis, keeping `Fleet`;
- most-critical mode per truck = `argmax_array(damage, subsystem_axis)`, keeping `Fleet` — exactly
  the "protect the truck whose *critical* mode is most worn" dispatch metric Stage-3e named as the
  next model.

The 2-D `Fleet × Subsystem` model itself is the follow-up: its building blocks (2-D broadcast,
2-D lag recurrence, per-axis reduction) are now each independently proven, so the remaining work is
getting the per-subsystem *physics* right, not fighting the engine.

## Status of the flagged gap

`MARKET_RESEARCH_REHYDRATION_2026-07.md` §3, `ANALYTICA_TRANSLATION_STATUS.md` §9.3, and
`STAGE3E_SUBSYSTEMS.md` all named "current reducers collapse *all* axes to scalar" as one of the
three honest frontier gaps (alongside labels-on-axes, which shipped, and still-static dimension
size). **That reducer gap is now closed.** The remaining frontier item is the static dimension
size.
