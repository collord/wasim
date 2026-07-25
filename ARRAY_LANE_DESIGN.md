# Array Lane — dual-mode execution for efficient Analytica compatibility

## Context — why this exists

WASiM's engine is a **scalar-per-realization time-stepping interpreter**: the
Monte-Carlo loop lives *outside* eval —

```
for realization in 0..N { for step in 0..T { walk each element's AST } }
```

— so a value in eval is one realization's scalar, and each expression is
re-interpreted `N × T` times. This is a **natural, near-optimal** shape for
WASiM's differentiator: stocks/flows and rich per-realization state machines
(`markov`, `hysteresis`, `queue`, `pid_controller`, `convolution`, `gate_logic`,
`status`, event `interrupt`, failure FSMs). Each realization is an independent
sequential process with data-dependent branching; realization-outer is how you
want to run that.

Analytica's paradigm is the opposite: a value **is** an array over
`{Run × dims (× Time)}`, and the model is evaluated **once per step, vectorized
over the sample**. That's why Analytica scales to 100K-sample models — and it's
what the NamedArray work ([WASIM_NAMEDARRAY_DESIGN.md](WASIM_NAMEDARRAY_DESIGN.md))
began to enable (values now carry axes; `run_stat` gives mid-graph
across-realization reductions via a bounded two-pass).

**The tension:** making Analytica compat *efficient* wants array-first,
vectorized-over-Run execution. Forcing that execution model onto everything would
degrade WASiM's core, because per-realization state machines don't vectorize
cleanly over Run — and it would break the engine's **bit-identity guarantee**
(vectorized float reductions differ from the scalar loop). So the question isn't
"columnarize the engine" — it's "add a columnar **lane** without making the
native case pay."

## The decision — dual-mode, scalar path untouched

Keep two execution paths:

- **Scalar lane (default, unchanged).** Today's realization-outer loop. Every
  existing model runs on it → **bit-identical across engine versions, same perf,
  same memory** — guaranteed *by construction*, not by tuning. State machines,
  stocks, events, RNG streams: all stay exactly as they are.
- **Array lane (opt-in accelerator).** A vectorized-over-`Run` evaluator for the
  **columnar-eligible subset** of a model (pure arithmetic + sampling + stocks +
  reductions; no per-realization state machine). Selected per-model or
  auto-detected per-subgraph. Scalar-only elements stay on the scalar lane even
  inside an array-lane run (the two lanes exchange values at the boundary).

The array lane is a **scale play** for large-sample, arithmetic-heavy
Analytica-style models — not a new default. Small models don't need it.

## Spike evidence (measured, `scratchpad/spike_*.rs`)

Three spikes drove the design; the numbers are the reason the design is what it is.

### 1. The win is *fusion*, not columnar layout
An 8-op elementwise expression over N = 4,000,000, three ways:

| approach | time | vs scalar |
|---|---|---|
| scalar tree-walk (dispatch per realization) | 0.146 s | 1.0× |
| columnar **materialized** (a temp `Vec` per op) | **0.196 s** | **0.75× — slower** |
| columnar **fused** (the whole expr as one loop) | **0.028 s** | **5.2× faster** |

**Naive columnar is a *loss*** — allocating/streaming a temp `Vec` per operation
eats the interpretation amortization. The 5× only appears when an elementwise
chain is **fused into one tight, SIMD-friendly loop with no intermediates**. This
is the single most important design constraint: **the array lane's core is a
fusing kernel evaluator, not a columnar tree-walker.** A columnar interpreter that
materializes intermediates would be pointless (or negative).

### 2. Determinism forces fixed-order reductions
Summing 1,000,000 values: a **sequential** fold is exactly reproducible; a
chunked/SIMD-tree reduction diverges from it by **1.9e-3** (absolute). So array-lane
reductions must run in a **fixed sequential order**. Crucially, because the array
lane is **opt-in for models that never had a scalar-path result to preserve**, it
needs only *internal* determinism (same answer every run) — it does **not** have
to match the scalar lane's exact ULPs. The bit-identity *guarantee* is about the
default path being stable across versions; two lanes may differ in the last bits
(documented, like a compiler-flag difference).

### 3. The target models are fully eligible
Classifying the four committed example models (arithmetic/sampling/stock/reduction
vs state-machine node rules): **100% columnar-eligible** in every case
(EVIU 13/13, Platform_2017 479/479, both native models fully). The models that
*matter for Analytica compat* are exactly the ones the array lane can take; the
models *at risk* (state-machine-heavy native WASiM) are exactly the ones dual-mode
keeps on the scalar lane. The routing is real, not hypothetical.

## Architecture

The NamedArray substrate is already in place (`Value::Array(Box<NamedArray>)`,
axis-tagged; `eval.rs`). The array lane adds, in `engine_v2`:

1. **Eligibility analysis** — walk the graph; mark an element *array-eligible* iff
   its node rule is arithmetic/sampling/stock/lookup/reduction and all its
   transitive inputs are eligible. State-machine rules and anything downstream of
   them stay scalar. (Prototype: the Spike-3 classifier.)
2. **The `Run` column** — for an array-lane subgraph, each value is a
   `NamedArray` whose axes include `Run` (len = n_realizations). Sampling draws all
   N at once (per-realization content seeds preserved for determinism). Stocks
   integrate as vector ops (`level += rate·dt` over the Run column).
3. **A fusing kernel compiler** — compile each maximal elementwise expression
   subtree to **one loop over the Run column, no temporaries** (Spike-1's fused
   form). This is where the speed is. Start with an interpreted fused evaluator
   (a stack machine over columns that fuses a chain into a single pass); a
   closure/bytecode compiler is a later optimization.
4. **Fixed-order reductions** — `sum_array`/`mean_array`/`run_stat`/`submodel_stat`
   over the `Run` (or any) axis use a sequential fold, reusing `reduce_data`.
   `run_stat` becomes a *native axis reduction* here — no second pass — retiring
   the two-pass hack for array-lane models.
5. **Lane boundary** — a scalar-only element feeding an array-lane subgraph
   contributes a broadcast scalar (or its per-realization vector, gathered from the
   scalar loop); an array-lane result feeding a scalar element is read per-lane at
   the boundary. Results surfacing (`#k`, final_values) is unchanged.

## Phased plan

| Phase | Deliverable | Size | Risk |
|---|---|---|---|
| A | **Eligibility analysis + `--array-lane` opt-in flag** that, when a model is fully eligible, runs it in a *columnar-materialized* evaluator (correctness first, speed later). Golden-diff the scalar lane stays byte-identical; array lane validated against the scalar lane within a documented tolerance. | M | Low — additive, gated |
| B | **Fusing kernel evaluator** (elementwise chains → one pass, no temps) — the Spike-1 win. Re-bench vs scalar. | M–L | Med |
| C | **Native `Run`-axis reductions** (fold `run_stat`/`submodel_stat` onto axis reduction in the lane; drop the two-pass for lane models). | M | Med |
| D | **Auto-routing** (per-subgraph eligibility, scalar/array hybrid in one run) + stock vectorization. | L | Med–High |
| — | Closure/bytecode kernel compiler; runtime-dynamic indices | L | later |

Phase A alone is safe and shippable (opt-in, scalar path untouched). Ship A, then
B is where the measured 5× lands.

## Non-goals / constraints

- **Never touch the scalar lane.** It is the correctness + bit-identity anchor and
  the home of state machines. The array lane is additive and gated.
- **No transparent swap.** Array-lane and scalar-lane float results may differ in
  the last ULPs; the array lane is opt-in and documented as such.
- **wasm/determinism.** No BLAS, no autovectorized reordering in reductions;
  fusion uses fixed-order loops the compiler may SIMD *without* reassociating
  (or with an explicit deterministic-SIMD kernel).
- **Memory.** The `Run` column is O(N) live per array-lane element vs O(1) in the
  scalar loop. Chunk the `Run` axis (process R realizations at a time) to bound
  peak memory for large N — the fused loop is chunk-friendly.

## Verification

- **Scalar lane unchanged:** the cross-version golden-diff harness (used for the
  NamedArray series) must stay byte-identical for every model run without the flag.
- **Array lane determinism:** run an eligible model twice under the flag → identical.
- **Array vs scalar agreement:** eligible models agree within a documented
  floating tolerance (not ULP-exact, by design).
- **Perf:** extend `engine/tests/namedarray_bench.rs` with a scalar-vs-array-lane
  comparison on an array-heavy model; expect the Spike-1-class speedup once Phase B
  fusion lands.

---

*Companion to [WASIM_NAMEDARRAY_DESIGN.md](WASIM_NAMEDARRAY_DESIGN.md) and
[ANALYTICA_TRANSLATION_STATUS.md](ANALYTICA_TRANSLATION_STATUS.md). Spikes:
`scratchpad/spike_col.rs` (fusion), `spike_det.rs` (determinism), and the
eligibility classifier. Nothing here is implemented yet; the scalar engine is
unchanged.*
