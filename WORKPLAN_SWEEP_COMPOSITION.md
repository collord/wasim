# Work Plan — Sweep Composition (de novo)

*Grounded in a code-level audit of the engine as it actually is on 2026-07-23, not the
architecture the design docs assume. Inspired by `SWEEP_COMPOSITION.md` and
`ANALYTICA_GAP_REVIEW.md`, but re-sequenced against verified facts. Where this plan
contradicts those docs, the contradiction is deliberate and cited.*

---

## 0. What changed after reading the code

Three claims load-bearing in the design docs are wrong or stale. The plan is built on the
corrected picture, so the corrections come first.

| Doc claim | Verified reality | Consequence for the plan |
|---|---|---|
| Sample-as-axis needs a mid-graph reduction node; that's step-major and expensive | **True** — engine is realization-major (`engine_v2.rs:430` outer, `:696` inner). Mid-graph reduction genuinely would force a per-timestep barrier. | Sweep composition is the right mechanism. Confirmed, not assumed. |
| Sweep composition "already exists in embryo" as submodels + `submodel_stat` | **Stronger than claimed** — submodels *already run full nested Monte-Carlo loops* (`submodel_v2.rs::run_submodels`, `engine_v2.rs:134`). Not embryo; a working nested sweep. | The engine work is a *generalization of a working feature*, not new machinery. Lowers risk further. |
| The array/dimension executor is "provenance-only; array results return 0.0" | **False.** `vector_map`/`index`/`index_ref` produce real `Value::Vector` (`eval.rs:449–488`); arrays persist per-member as `<id>#k` (`engine_v2.rs:301`). | **The array executor is NOT the gating prerequisite.** The exceedance-curve case is much closer than the docs think. Real array gaps are narrower (see §5). |
| Streaming emits completed realizations; chunk-size invariance asserted in CI (1–200) | **No engine streaming exists.** It's a TS type contract (`streaming.ts`) over a synthetic model; the CI claim is a manual 4-size tsx script over a fake simulator. | Build streaming **in Rust/WASM**, not TS. The realization loop, RNG, and aggregation already live in Rust; streaming belongs there too (§6, now a committed decision, not an open question). |

Two things the docs got right and this plan keeps: the **DAG constraint** is acceptable
(corpus shows no genuine cycles), and the **seeding coordinate** is the thing that's cheap
now and expensive later. But the specific reason is sharper than the docs state:

> **The seeding is already broken — and it's a reproduced bug, not a hazard.** Nested
> submodels receive the *same base seed* as the parent (`submodel_v2.rs:334`:
> `RunConfig { seed: config.seed, .. }`) with no sweep-id offset. The Phase-1 spike
> (`scratchpad/seed_coordinate_spike.rs`) confirmed two different submodels produce
> **byte-identical random streams today**. It's latent only because `submodel_stat` reduces
> each to a scalar — but the moment a downstream node combines two submodels' statistics (the
> two-level-uncertainty case the feature exists for), their spurious correlation corrupts the
> result. Fixing the seed coordinate is step 1 because it's a **correctness bug that ships
> now**, independent of composition — see Phase 1 for the reproduction and the fix.

---

## 1. Objective and non-goals

**Objective.** Make a Monte-Carlo sweep a composable unit: sweep A runs N realizations and
produces a distribution; a boundary reduction turns that distribution into a value; sweep B
consumes it. Deliver the Analytica-critical **exceedance-curve** case (`Probability(loss >
threshold)` over a threshold grid, read downstream) with determinism guarantees that survive
reordering, isolation, and eventual parallelism.

**Non-goals (explicit, to bound scope):**

- No `.ana` importer. (Both analyses agree; unbounded scope.)
- No mid-graph across-realization reduction node. (Would force step-major; §0.)
- No runtime-sized / data-dependent indices (`subset`/`sortIndex`/`shuffle`). That's a
  separate, larger array-language effort; the exceedance curve needs only *static-size,
  dynamic-content* arrays, which already work.
- No metaprogramming / GUI layers.

**Committed architectural decision (§6):** the incremental realization loop, streaming, and
Monte-Carlo aggregation are **owned by Rust/WASM**, exposed to the TS/JS layer through a
tightly-defined **polling** boundary. Determinism stays a property of one Rust function
rather than being split across a language boundary. This is a foundation for the plan, not a
deferred option — see Phase 0.

**Framing for the org:** this is built for WaSim's own reasons — chance-constrained
optimization, value-of-information, RAM policy switching, Bayesian updating are all one
mechanism with different reducers (per `ANALYTICA_GAP_REVIEW.md` §6). Analytica overlap is a
side effect. We are not targeting Analytica's users.

---

## 1.5 Status ledger — what's done, what remains

The committed spine (both branches) is **complete and on `main`**. What remains is either
explicitly gated (waits on a concrete need) or a documented follow-up. Nothing below blocks
anything above it.

### ✅ Done (13 commits; `wasim` unless noted)

| Phase | Commit | What |
|---|---|---|
| 1 | `5db25e1` | Seed coordinate — fixed the shipping identical-stream bug (per-submodel content-derived seeds) |
| 2 | `1eb2b02` | CI invariants as tests: composition-isolation + determinism (reorder-invariance in Phase 1's test) |
| 3 | `78cee75` | Extended `submodel_stat` reducers: exceedance/CCDF, CTE, sum, min, max (+ DRY'd `results_spec` CTE) |
| 4 | `8b682ed` | **Exceedance curve** — `vector_map` + `submodel_stat`, the Analytica-critical capability, zero new engine code |
| 0a | `d7fbf6a` | Streaming accumulator core (`stream_accum.rs`), standalone; caught the Welford chunk-variance bug |
| 0b | `a3bafbc`+`85cc12d` | Resumable `RunState` (mechanical `run()` split) + chunk/partial/cancel invariants |
| 0c | `670f654` | WASM `RunHandle` poll boundary (`start`/`poll`/`cancel`) + `Checkpoint` save/restore |
| — | `5030489` / `cc36b1d` (openvsim) | Two corpus examples (exceedance curve + tail-stat reducers) + rot-guard test |
| 5 | `6819b59` | **Live streaming UI** — `?live` view drives real `RunHandle.poll` per frame; band tightens live |

### ⏳ Remaining — gated (build when a concrete model/need appears)

- **Phase 6 — whole-distribution bindings** (SIPmath-compatible): pass empirical sample arrays
  across a boundary for nonlinear downstream models. Gated on a real nonlinear model.
- **Phase 7 — explicit peer sweep graph**: `sweep_graph` edge list, topo-sorted, content-hash
  cached; **this is where a model/graph `sweepId` field lands** (not Phase 1). Gated on a real
  peering need.
- **Phase 8 — pipelined cross-sweep streaming**: opt-in, discard-and-recompute; *enabled* by
  the Phase 0 substrate. Gated on a user asking + a composed model.

### ⏳ Remaining — deferred follow-ups (no gate; do when convenient)

- **Sketch-mode accumulator**: wire `stream_accum::ElementAccum` (t-digest/binned) into the loop
  for O(elements×steps) memory + `bandsApproximate`. Until then `stream_accum.rs` is unused —
  Exact mode (`hist_store` = the accumulator) carries 0a–0c.
- **Phase 1b — child settings inheritance**: the two submodel sites use `..RunConfig::default()`,
  so a child drops the parent's `timebase`/`units`/`realization_weights`. Latent divergence,
  own regression surface — decide + fix separately from the seed work.
- **Phase 3 leftovers**: fail-loud on an unresolved boundary reduction (behind a flag; default
  keeps `→0.0`); `at_time` selection (needs `run_submodels` to retain per-step history).
- **Rich-Diagram bridge** (Option B/C): make the streamed run drive the *existing* bespoke
  `Diagram`. Needs the **engine** to emit per-element layout + retained per-realization traces +
  events + native convergence — a real Rust-side effort, not frontend glue. Phase 5 shipped the
  honest minimum (a new bands view) instead.
- **`poll` re-prepare cost**: each `poll` re-runs `RunState::new` (re-executes `run_submodels`
  for submodel-heavy models — deterministic, wasteful not wrong). Cache prepared tables if a real
  model measures it.
- **Cyberrisk parity fixture**: hand-translate `Hubbard_and_Seiersen_cyberrisk.ana` into a
  permanent benchmark (the proof-over-estimate artifact §4 calls for). Parallel track, not started.

### ⚠️ Standing infra gap

- **No CI exists** (only `node_modules` workflows). The determinism invariants and the whole
  engine suite run on manual `cargo test` only — the brand-defining property is unprotected at
  the engine level. Standing up a minimal `cargo test` workflow (skipping the pre-existing
  `hydropower_optimization` hang in `--test integration`) is a maintainer decision, flagged not
  taken.

---

## 2. The build sequence

Ordered by dependency and by risk-retired-per-unit-effort. Phase 0 relocates the loop into
WASM with incremental accumulators and a polling boundary; phases 1–3 harden and generalize
what exists; phase 4 is the Analytica-critical capability; phase 5 wires the streamed output
through to the live UI.

### Phase 0 — Rust incremental loop + WASM polling boundary (the substrate) — 0a ✅ (d7fbf6a), 0b ✅ (a3bafbc + 85cc12d)

**Status (Phase 0b, the loop extraction):** DONE. `run()` factored into `RunState { new =
prepare, advance(count) = loop body over a range, assemble = finalize (non-destructive) }` —
public signature unchanged, zero call-site churn, diff 206+/35− on a ~2170-line move (body
recognized as relocated). Gate: 337 tests / 56 binaries green, zero fixture drift. Invariants
(`run_stream_v2.rs`): chunked advance **bit-identical to batch** (1/7/33/all — full
chunk-vs-batch identity, stronger than 0a's chunk-vs-chunk, because stores append in
realization order and reduce once at assemble); partial assemble = valid prefix + resume
still batch-identical; **cancel-at-k = fresh batch of k, bit for bit** (plain MC; LHS caveat
documented — stratification depends on total n_real). Closes Phase 2's deferred invariant #4.
Loop-body finding vs. the inventory: `corr_groups` IS read per-realization (`:637`) — it
became a struct field.

**Status (Phase 0c, the WASM poll boundary):** DONE (`670f654`). `RunState` gains a
serializable `Checkpoint` (the 5-field resume set) + `checkpoint()`/`restore()`; `wasm.rs` gets
`RunHandle` (`start`/`poll(count)`/`cancel`/`completed`/`total`/`is_complete`) that owns its own
model+graph+Checkpoint (so it never holds a borrowing `RunState<'a>` across the JS boundary) and
rebuilds a `RunState` per poll. Shared `parse_js_config`/`apply_display_units` helpers dedupe
`run_json`↔`poll`; `ModelGraphV2` gained `#[derive(Clone)]`. Bug caught in review: restoring an
initial (`next_real==0`) checkpoint must be a no-op or it clobbers the freshly-keyed empty stores
→ `advance` panic; fixed + regression-tested. wasm target compiles; 341 tests/57 binaries green.
**Known cost (documented, deferred):** `poll` re-runs `new()`'s prepare each call — cheap for
plain MC (the streaming UI case), but re-executes `run_submodels` for submodel-heavy models
(deterministic, wasteful not wrong); cache prepared tables if a real model measures it.
**The entire Rust+WASM streaming substrate is now complete.** Remaining: Phase 5 (live UI).

**Status (Phase 0a, the numerical core):** `engine/src/stream_accum.rs` landed — `RunningMoments`
(Welford + Chan merge), `BandAccum::Exact`, `ElementAccum` (`push_series`/`merge`/`to_stats`),
built and tested *standalone*, not yet wired into the `engine_v2` loop (that extraction is
invasive to a ~1650-line hot path — a deliberate separate slice, 0b). **A real determinism bug
the standalone tests caught that the spike missed:** the Welford/Chan mean is associative only in
exact arithmetic, so different chunk groupings give means differing by a few ULP — breaking
chunk-invariance. Fixed by deriving both mean and percentiles from the retained samples in
realization order in `Exact` mode (bit-identical to batch *and* chunk-invariant); `RunningMoments`
kept for a future sketch mode that can't retain samples. 5 tests green. **Remaining (0b):** extract
`RunStream`/`run_chunk` from the realization loop and wire the accumulator in; then the WASM
`start`/`poll`/`cancel` boundary. See `WORKPLAN_SWEEP_PHASE0_API.md`.

Original design:

**Why this is phase 0, not deferred:** the current batch loop retains all realizations then
reduces post-hoc (`stats()`, `engine_v2.rs:2597`) — memory O(realizations × elements × steps),
400 MB at 10k × 500. Incremental accumulators fix that scaling *and* produce streaming from
the same change. And it belongs in Rust: the realization loop, RNG, and aggregation already
live there, so streaming from TS would split the determinism guarantee across a language
boundary — the strictly worse place for the brand-defining property to sit.

1. **Incremental accumulators in Rust.** Replace post-hoc `stats()` with per-element running
   accumulators updated as each realization completes: running moments (Chan/Welford parallel
   merge) for mean/sd, and — for the p05..p95 bands — a decision the spike forced into the
   open (see the spike box below): **either** retain samples per step (exact, batch-identical
   percentiles, but O(realizations) memory — *no memory win*) **or** a bounded percentile
   sketch (t-digest/binned, O(1) memory — but *not* bit-identical to batch bands). Mean/sd are
   O(1)-mergeable regardless; only the percentile bands carry the memory-vs-parity tradeoff.

2. **`run_chunk(from, to)` + mergeable partial in Rust.** Expose the realization loop as a
   resumable range so the engine can run realizations `[from, to)`, fold them into the
   accumulators, and hand back a serializable **partial snapshot**. Chunk boundaries must be
   semantically null — the accumulator merge is associative and the per-realization seed is
   `(seedRoot, sweepId, realizationIndex)` (Phase 1), so *any two chunkings agree with each
   other*. **They do not agree with the batch `sum()`**: incremental accumulation visits
   values in a different order, and FP addition isn't associative (the spike measured up to
   89 ULP on the mean alone). So the invariant is **chunk-vs-chunk agreement, not
   chunk-vs-batch** — this changes the CI assertion (Phase 2) and is the kind of thing that
   looks like a bug in CI when it's actually FP physics.

> **Spike result (de-risked 2026-07-23, `scratchpad/chunk_merge_spike.rs`).** A standalone
> Rust spike modeling the exact `hist_store` aggregation contract proved: (a) the exact
> accumulator merge is **bit-identical to batch and identical across all chunk sizes 1–10k**
> — the merge is provably associative and order-free, chunk-invariance holds; (b) the running
> *mean* diverges from the batch mean by up to **89 ULP** purely from FP non-associativity,
> so no incremental accumulator is bit-identical to batch; (c) exact percentiles require
> sample retention — the O(1)-memory sketch is the whole point of streaming but is
> approximate by construction. **Consequences already folded in:** points 1–2 above and the
> Phase 2 assertion. The spike is the smallest slice that retired the riskiest Phase-0 claim
> before any WASM/handle plumbing.

3. **Polling boundary across WASM.** Extend `wasm.rs` beyond single-shot `run_json` with a
   handle-based API: `start(config) -> RunHandle`, `poll(handle) -> PartialSnapshot`,
   `cancel(handle)`. **Polling (pull), not push callbacks** — Rust owns the loop and advances
   to a yield point; TS calls `poll()` on its own cadence (rAF-driven) and renders whatever is
   ready. The critical invariant: a partial snapshot is a **pure function of
   `(seedRoot, sweepId, realizationsCompleted)`** — never of poll timing, chunk size, or
   scheduling. This makes convergence display live without making results depend on wall-clock.

   *(Push/callback streaming is the alternative and is deliberately rejected: it calls across
   the WASM boundary mid-loop repeatedly and complicates cancellation/backpressure, for a UI
   need that rAF-polling already satisfies.)*

**Deliverable:** a Rust-owned streaming engine with O(elements × steps) memory and a
poll-based WASM API. The `PartialRunResult` shape TS already declares (`streaming.ts:163`,
`extends RunResult`) becomes the real return type of `poll()`, so the existing TS contract is
honored rather than discarded. Depends on Phase 1's seed coordinate for the chunk-invariance
property to hold.

> **Concrete API surface:** `WORKPLAN_SWEEP_PHASE0_API.md` sketches the `start`/`poll`/`cancel`
> signatures grounded in the real `wasm.rs` / `SimulationResults` / `streaming.ts` types,
> resolves where display-unit conversion moves (per-poll, not post-run), and gives the
> `RunStream` + `BandAccum::{Exact, Sketch}` design that encodes the spike's memory-vs-parity
> decision as a config choice rather than a hardcode. Build order: pure-Rust `RunStream` first
> (retires numerical risk without a WASM toolchain), WASM wrapper last.

### Phase 1 — Fix the seed coordinate (correctness, blocks everything) ✅ LANDED (commit 5db25e1)

**Status:** done. `engine/src/sweep_seed.rs` (`sweep_id_fnv1a`/`splitmix64`/`child_seed`), both
sites fixed in `submodel_v2.rs`, regression test `sweep_seed_v2.rs` red→green. Refinements the
implementation forced vs. the plan below: (a) the fix lives entirely at the two `sub_config`
sites — **no model/graph `sweepId` field** was needed (deferred to Phase 7); (b) **no keyspace
carve-out** — the fix is in *seed*-space, so distinct child seeds yield distinct RNGs and can't
alias the IC/LHS `set_stream` reservations (those are stream indices inside one seeded RNG);
(c) the root must be resolved and set as a concrete `Some(child_seed)` because `.or()` fallback
would otherwise re-collapse two children (both inherit the same parent `simulation_settings`).
8/8 risk-surface binaries green, top-level bit-identity untouched. Phase 1b (child dropping
parent `timebase`/`units`/`weights` via `..default()`) flagged, deliberately not fixed here.

**Why first:** it is not a future hazard — it is a **reproduced correctness bug that ships
today.** The spike (`scratchpad/seed_coordinate_spike.rs`, verified 2026-07-23) showed two
different submodels in one model produce **byte-identical random streams**
(`[5592132763777985307, …]` for both). Root cause: `submodel_v2.rs:334` and `:380` pass
`RunConfig { seed: config.seed, .. }`, so every submodel re-seeds from the *same* base seed;
combined with `ChaCha8Rng::seed_from_u64(seed); set_stream(real_idx)` at `engine_v2.rs:431`,
realization *k* of submodel A and realization *k* of submodel B draw the identical stream.

> **Blast radius.** It's latent because `submodel_stat` reduces each submodel to a *scalar*,
> so you don't see the shared stream directly. But when a downstream node combines two
> submodels' statistics — exactly the "two-level uncertainty / inner distribution feeding an
> outer decision" case the feature exists for — their spurious correlation shrinks the
> apparent variance of the combination. Silently wrong, in the feature's primary use case.
> This is a bug to fix regardless of sweep composition; composition just makes it unavoidable.

The fix, each step demonstrated in the spike:

1. Introduce a stable, **content-derived `sweepId`** for every sweep (top-level run and each
   submodel). Derive it from the sweep's declared name (or a content hash of its definition)
   — **never its positional index**. The spike showed name-derived ids are reorder-stable
   while position-derived ids silently change results when the document is reordered — the
   `SWEEP_COMPOSITION.md` §7.4 trap, reproduced.
2. Change nested-sweep seeding from `seed: config.seed` to a splittable derivation:
   `child_seed = split(seedRoot, sweepId)`. Extend the coordinate to `(seedRoot, sweepId,
   realizationIndex)`; use `sweepId` as a seed-split input (or disjoint stream selector), not
   sequential advancement, so adding a sweep never perturbs an existing one's draws (spike [4]
   confirmed: each sweep's seed is a pure function of its own name, so a new sweep can't shift
   any existing stream).
3. Audit the two existing disjoint-stream conventions (Iman–Conover `u64::MAX`,
   LHS `u64::MAX - 1 - var_i`, both at `engine_v2.rs:2889/2933`) so the new `sweepId`-derived
   streams cannot collide with them. Reserve a `sweepId` keyspace disjoint from those.

**Deliverable:** every realization's stream is a pure function of `(seedRoot, sweepId,
realizationIndex)` and nothing else. Touches `submodel_v2.rs:334,380`, `engine_v2.rs:66` seed
resolution, and adds a `sweepId` field to the model/graph. **Regression test:** the spike's
sweep-collision check becomes a Rust test asserting two submodels draw *disjoint* streams —
it fails on today's code (proving the bug) and passes after the fix (red-green).

### Phase 2 — CI invariants (cheap now, painful to retrofit) — invariants ✅ LANDED as tests; ⚠️ no CI to run them

**Status:** the three seed-coordinate invariants testable today are implemented in
`engine/tests/sweep_seed_v2.rs` and green: reorder-invariance, **composition-isolation** (add
submodel C, assert A/B streams bit-unchanged), and determinism-baseline (same model twice →
identical). Chunk-invariance (#4) stays deferred to Phase 0.

> **⚠️ Escalation confirmed (2026-07-24):** there is **no CI in this repo.** The only
> `.github/workflows` files are inside `frontend/node_modules` (third-party). No Actions /
> GitLab / Jenkins / CircleCI. So these invariants — and the entire engine suite — run only on
> manual `cargo test`. The determinism brand is **unprotected at the engine level**, exactly
> the gap §0 predicted. Standing up CI (even a minimal `cargo test` workflow that skips the
> known-hanging `--test integration`, see the pre-existing `hydropower_optimization` hang) is a
> separate infra decision for the maintainer — flagged here, not silently added.

These are the assertions the design docs want but which **did not exist** (§0). Built them
against the *real Rust engine*, not the TS synthetic model.

1. **Reordering invariance** — permute submodel/sweep declaration order in a model doc,
   re-run, require identical aggregate hashes.
2. **Composition isolation** — add an unrelated sweep to the graph, require existing sweeps'
   outputs bit-unchanged.
3. **Determinism baseline** — same model, two runs, identical hashes (guards the seed work).
4. **Chunk-size invariance (now real)** — run via `run_chunk` at chunk sizes 1, 7, 33, 200,
   require the aggregate hashes to be **identical to each other** (chunk-vs-chunk), *not* to
   the batch result. Per the spike, incremental accumulation differs from batch `sum()` by
   tens of ULP due to FP non-associativity — a chunk-vs-batch assertion would fail by
   construction and read as a bug. The correct property is that chunking is *semantically
   null*: every chunking of the same run produces the same streamed aggregate. This guards
   Phase 0's merge associativity and the seed coordinate simultaneously. (If bit-identity to
   the *legacy batch path* is also wanted, that is a separate, stricter test that pins the
   accumulator's summation order to batch's — worth deciding explicitly, not assuming.)

Add these as Rust integration tests under `engine/tests/` (alongside `timebase_v2.rs`), and
wire them into whatever CI actually runs Rust tests. **If no Rust CI exists, that gap is
itself a finding to escalate** — the determinism brand is currently unprotected at the engine
level.

*Note:* phases 1 and 2 are co-dependent — write the tests first (they'll fail on today's
naive seeding), then do phase 1 to make them pass. Standard red-green.

### Phase 3 — Generalize boundary reducers (scalar) — reducers ✅ LANDED

**Status:** the reducer extension is done. `SubmodelStatKind` gains `Exceedance` (P(X>t), CCDF),
`Cte` (conditional tail expectation), `Sum`, `Min`, `Max` (`model.rs`); reducer fns
`exceedance`/`cte`/`weighted_cte`/`sum_of`/`min_of`/`max_of` added to `engine.rs` and wired in
the `eval.rs` match arm; `summary.rs` display labels (`pdf_exceedance`, …) added. **DRY bonus:**
`results_spec.rs`'s inline CTE now calls the shared `weighted_cte` — one implementation, and its
`cte_hand_computation` regression stays green. Tests: 5 reducer unit tests (`engine.rs`,
hand-computed against 1..=10) + end-to-end dispatch test (`submodel_reducers_v2.rs`, constant
submodel). 28 lib + touched binaries green. Kept unweighted at the boundary (matching the
existing arm — submodel samples carry no parent weights); `weighted_cte` exists only because
`results_spec` reuses it.

The two remaining Phase-3 items are **deferred as separate slices** (per scope decision):
- **Fail-loud** on an unresolved boundary reduction (behind a flag; default keeps the `→0.0`
  dangling-ref policy at `eval.rs:426` so SELDM fixtures survive). Not yet done.
- **`at_time` selection** (specific step / final / whole-history). Materially bigger — needs
  `run_submodels` to retain per-step history, not just `final_values`. Phase 4's exceedance
  curve does **not** need it (it reduces `final_values`), so it's correctly deferred.

### Phase 4 — Vector-valued boundary reductions (the exceedance curve) ✅ LANDED (composes, zero new engine code)

**Status:** done, and it needed **no new AST/engine code** — the strongest possible confirmation
of the whole analysis's thesis (generalize existing machinery, don't add architecture). The
exceedance curve is authored as a `vector_map` over a threshold dimension whose body is
`submodel_stat(exceedance, arg = threshold[index_ref])`: the array executor pushes the 1-based
member index, `submodel_stat`'s `arg` reads `threshold[k]` and emits one exceedance per
threshold, and the resulting `Value::Vector` reaches results as `curve#1..#N`. Proven by
`exceedance_curve_v2.rs`: a deterministic constant-submodel case (curve = `[1,0,0]`, exact) and
a real uniform(0,10) distribution case (monotone-decreasing `≈[1.0,0.75,0.5,0.25,0.0]`, within a
few % of analytic at n=2000). The decision to reuse `vector_map` (vs. a dedicated node) was the
right smallest-intervention call — the "producer side" the plan below anticipated turned out to
already exist.

Original plan (what turned out to already be in place):

1. A boundary reduction that applies a reducer **across a declared dimension** (the threshold
   grid) and emits a `Value::Vector`, not a scalar. The reducer runs once per dimension
   member against the full sample vector: `out[k] = exceedance(samples, threshold[k])`.
2. Bind that vector into the consuming sweep as an array-valued input over a matching
   dimension. The consumer already handles `Value::Vector` end-to-end (`vector_map`,
   per-member persistence) — so the new code is the *producer* side, not the consumer side.
3. Cross-grid rule: when a reduced vector-over-time crosses into a sweep with a different
   time grid, require an **explicit** `lookup`/`series` resample — never silent
   interpolation (§4 of the design doc; this protects the defensibility claim).

**Dependency reality-check:** the design docs gate this on "the array executor landing."
The array executor is already landed for the 1-D static-dimension case this needs. The
*genuine* array limitations (no true `[i,j]`, no vector-per-member comprehension —
`eval.rs:460` scalarizes) do **not** block an exceedance curve, which is a 1-D vector over a
1-D threshold dimension. So phase 4 depends only on phases 1–3, not on a large array-language
effort.

### Phase 5 — Wire streamed output to the live wasim-watch UI ✅ LANDED (`6819b59`)

**Status:** DONE, but with a deliberately narrower scope than this section originally planned —
and the divergence is the important part. The plan below assumed the streamed payload could
drive the *existing* `Diagram` by reusing the `contract.ts` `PartialRunResult` types. It can't:
the Rust `poll()` payload (`SimulationResults` = aggregate bands + `final_values`) has **no
per-element layout, no per-realization traces, no events, no exemplars** — all of which the
bespoke `Diagram` consumes. Feeding it those would mean fabricating them.

So Phase 5 shipped **Option A** (the honest minimum): a *new* small view (`StreamApp` +
`BandChart` + `pollRun` rAF driver + `wasm` loader + `simResults` TS mirror) consuming exactly
what `poll()` emits — a p05–p95 band that tightens live + progress + cancel. Additive `?live`
toggle in `main.tsx`; the synthetic `Diagram` path is untouched. Convergence shown as band-width
shrink (no CI — default payload has no stddev). A demo model (`demo_model.json`, a top-level MC
diffusion cone) was needed because the committed submodel examples have top-level `n_real=1`
(their MC is an inner pre-pass — wrong shape to stream). Verified: Node smoke (streamed == batch
bit-for-bit, `max|diff|=0`), prod build bundles the wasm, and a real browser run (Playwright)
streams 0→2000 with the band fanning, cancels to a valid partial, and leaves the default view
intact. **Build gotcha:** `engine/pkg` is gitignored and can go stale — run `engine/build-wasm.sh`
before `npm run dev`.

The originally-planned "drive the rich Diagram" path is **not** done and is now tracked as the
**rich-Diagram bridge** in the status ledger (§1.5, deferred follow-ups) — it needs the *engine*
to emit layout + retained traces + events (Option B/C), a separate effort.

---

*Original plan (what was assumed, retained for context):*
Replace the synthetic placeholder with the real streamed engine end-to-end. Today `App.tsx`
calls `generateRun(60)` — a hand-written TS Euler simulation ([synthetic.ts](wasim-watch/src/engine/synthetic.ts)),
explicitly a placeholder ("in production, `generateRun()` is replaced by a call into" the real
engine). Now it is.

1. **Drive Phase 0's poll API from the UI.** On run start, call `start(config)` across WASM;
   on each animation frame, `poll(handle)` and render the `PartialRunResult` — convergence
   status, running bands, retained exemplars. The `isStreaming`/`final` discriminator already
   in the contract (`streaming.ts:332`) drives the "still converging" vs "done" UI state.
2. **Reuse the existing contract types.** `RunResult` / `PartialRunResult` / `ConvergenceStatus`
   already exist in `contract.ts` / `streaming.ts`; wire them to real data instead of the
   synthetic generator. The diagram/render layer already imports only *types*, so it needs no
   change beyond receiving live values.
3. **Cancellation.** Wire the UI's stop/re-run to Phase 0's `cancel(handle)`; a cancelled run
   leaves the accumulators in a valid partial state (poll one last time for the final partial),
   preserving the "cancelled and resumed or run straight through → identical results"
   guarantee.

**Deliverable:** wasim-watch shows a real, live-converging Monte-Carlo run over the actual
engine, not a fake. This is the visible payoff of Phase 0 and the first end-to-end proof that
the Rust-owned streaming substrate works.

### Phase 6 — Whole-distribution bindings (optional, SIPmath-compatible)

Only if a downstream model is nonlinear in the uncertain quantity (`f(mean(x)) ≠ mean(f(x))`).
Pass the empirical sample array across the boundary and let the consumer draw from it. Adopt a
SIPmath-compatible representation for ecosystem interop (design doc §3.2c — a genuinely good
call). Restrict to point-in-time / reduced-over-time by default; time-history distribution
bindings are `realizations × steps × 8` bytes (400 MB at 10k×500) and must be explicit
opt-in. **Defer until a concrete model needs it.**

### Phase 7 — Explicit peer sweep graph (optional)

Today submodels *nest* (child inside parent). A peer DAG (`sweep A` then `sweep B` as
siblings) is a different relationship and shouldn't be conflated with nesting in the schema
(design doc §10 — right). Add an explicit `sweep_graph` edge list, topologically sorted, with
content-hash caching so an outer optimizer re-running the graph only re-runs changed sweeps.
**Defer until phase 4 proves the boundary works and a real model needs peering rather than
nesting.**

### Phase 8 — Pipelined cross-sweep streaming (optional, novel)

The genuinely novel capability the design docs (§8) flag: because `PartialRunResult` is
assignable to `RunResult`, a downstream sweep can consume a *partial* upstream reduction and
converge on provisional inputs, refining as upstream tightens — end-to-end convergence across
a composed model, with early-stop driven by the terminal quantity. **Opt-in only**, and with
the §8 discipline: downstream realizations computed against stale bindings are discarded and
recomputed once upstream finalizes, or the mode is restricted to already-converged bindings.
Never ship silently-mixed realizations. **Gated on phases 0, 4, and a user asking for it** —
now *enabled* by the Rust streaming substrate rather than blocked by its absence.

---

## 3. Dependency graph of phases

```
Phase 2 (write failing CI invariants incl. chunk-invariance)
   └─▶ Phase 1 (fix seed coordinate → invariants pass)
          ├─▶ Phase 0 (Rust incremental loop + WASM poll boundary)   ◀── streaming substrate
          │      └─▶ Phase 5 (wire streamed output to live wasim-watch UI)   ◀── visible payoff
          │             └─▶ Phase 8 (pipelined cross-sweep streaming)  [gated: opt-in + a user need]
          └─▶ Phase 3 (scalar reducer set + fail-loud + at_time)
                 └─▶ Phase 4 (vector boundary reductions = exceedance curve)  ◀── Analytica-critical
                        ├─▶ Phase 6 (whole-distribution bindings)  [gated on a real nonlinear model]
                        └─▶ Phase 7 (peer sweep graph)             [gated on a real peering need]
```

**Committed spine:** Phases 2 → 1 → {0 → 5, 3 → 4}. The seed coordinate (1) is the shared
root: it makes both the CI invariants pass *and* the chunk-invariance property hold, so it
gates both the streaming branch (0, 5) and the composition branch (3, 4). Phases 6, 7, 8 are
explicitly gated on evidence of need. Note the streaming branch (0, 5) and the composition
branch (3, 4) are independent after Phase 1 and can proceed in parallel.

---

## 4. The parity test — do this in parallel, from day one

Independent of the phases above and endorsed by both analyses: hand-translate the
`Hubbard_and_Seiersen_cyberrisk.ana` model into a WaSim v2 model doc + a parity fixture
(mirroring `seldm_*`). ~an afternoon. Two payoffs:

- Converts the "~90% representable" estimate into proof, and pins down exactly which line the
  exceedance-curve gap falls on.
- Checked into the benchmark suite, it becomes a **permanent regression test** for the array
  + sampling machinery and public evidence for the "why do tools disagree, which can you
  defend" story (`ANALYTICA_GAP_REVIEW.md` §5).

Do this first or concurrently with phase 1 — it de-risks phase 4 by making the target
concrete before building the producer.

---

## 5. Array executor: what's actually true, and what to fix separately

The docs conflate "array executor is broken" with "some array shapes are unsupported." The
audit says the executor works for the common case. For accuracy, the real array gaps —
**tracked separately, not blockers for sweep composition:**

- No true multi-dimensional `[i,j]` (flat `Value::Vector`; second index ignored,
  `eval.rs:479`).
- `vector_map` scalarizes each member (`eval.rs:460`) — no ragged / vector-per-member arrays.
- Dimensions are statically sized (`model.rs:264`); no runtime index construction. This is
  the `subset`/`sortIndex`/`shuffle` gap the gap analysis ranks 7/8 — real, but a distinct
  effort from sweep composition, and only the *runtime-sized* half is expensive (the
  *static-size dynamic-content* half already works).

**Recommendation:** file these as their own work item. The gap analysis §5 should be
corrected to say the array executor is functional for static 1-D dimensions and the blocker is
runtime-sized indices specifically — not "array results return 0.0," which is false.

---

## 6. Streaming ownership: decided — Rust-owned loop, polling boundary

Both design docs assume engine-level streaming with CI-asserted chunk invariants. **It does
not exist in the engine today.** The realization loop and MC aggregation are owned by Rust
(`engine_v2.rs`), batch-only, retaining all realizations then computing percentiles post-hoc
(`stats()`, `:2597`). The TS `streaming.ts` is a design contract; `streamingRef.ts` streams a
*synthetic* model; `App.tsx` uses batch `generateRun`; `wasm.rs` exposes only single-shot
`run_json`.

**Decision: the chunked/streaming realization loop lives in Rust/WASM**, exposed through a
tightly-defined **polling** boundary, *not* driven from the TS/worker layer. Two decisive
reasons:

1. **Determinism must not cross the language boundary.** The RNG streams keyed by
   `(seedRoot, sweepId, realizationIndex)` and the accumulator merge are the brand-defining
   correctness property. If TS owns chunk assignment and merge, that property becomes a
   property of TS glue rather than one Rust function — the strictly worse place for it. Keep
   the loop, the seeds, and the reduction in Rust; TS only *reads* partials.
2. **The memory fix and the streaming capability are the same change.** Incremental Rust
   accumulators drop batch memory from O(realizations × elements × steps) to
   O(elements × steps) *and* yield streaming for free. Driving batch-per-chunk from TS gets
   neither benefit — it still materializes full per-chunk results and re-reduces in JS.

**Boundary shape: polling (pull), not push callbacks.** Rust advances the loop to a yield
point; TS calls `poll()` on its own (rAF) cadence. A partial snapshot is a pure function of
`(seedRoot, sweepId, realizationsCompleted)` — never of poll timing or chunk size. Push
callbacks are rejected: repeated mid-loop calls across the WASM boundary complicate
cancellation/backpressure for a UI need that rAF-polling already satisfies.

This is Phase 0 (substrate) + Phase 5 (UI wiring), both committed. The composition spine
(phases 1, 3, 4) is independent of it and can proceed in parallel after the seed fix.

---

## 7. Documentation corrections to land alongside the code

The three source docs contain factual errors that will mislead the next reader. Fix them:

1. `ANALYTICA_ENGINE_GAP_ANALYSIS.md` §2: "small addition — an across-realization reduction
   AST node" → reframe as generalized sweep composition; note it would force step-major if
   done mid-graph (verified realization-major, `engine_v2.rs:430`).
2. `ANALYTICA_ENGINE_GAP_ANALYSIS.md` §5: promote the 7/8-model dynamic-index finding into the
   recommendation (currently omitted — its own #1 finding), splitting static-size from
   runtime-sized.
3. `SWEEP_COMPOSITION.md` §6 and build table: correct "array executor is provenance-only /
   returns 0.0" — it produces real vectors. Re-gate phase 4 (vector reductions) off phases
   1–3, not off a mythical array-executor milestone.
4. Both docs: qualify every "streaming emits completed realizations / CI asserts chunk
   invariance 1–200" claim — engine streaming does not exist; the invariant is a manual
   4-size TS script over a synthetic model.

---

## 8. Summary

The architecture the design docs propose is **correct** — sweep composition over a mid-graph
reduction node — and the code audit makes it *cheaper* than they estimate, for a reason they
missed: submodels already run nested Monte-Carlo loops and the array executor already works.
The single real blocker they under-weighted is the **seed coordinate**, which is not merely a
future hazard but a present latent bug the moment a second consuming stage exists. Streaming
is settled: it lives in Rust/WASM behind a polling boundary, because that keeps determinism in
one place and fixes the batch memory-scaling problem in the same change (§6). So:

- **Do first:** parity test (proof) + failing CI invariants incl. chunk-invariance +
  seed-coordinate fix (phases 2, 1, §4). The seed fix is the shared root of both branches.
- **Then, in parallel** (independent after the seed fix):
  - *Streaming branch:* Rust incremental loop + WASM poll boundary (phase 0) → wire the live
    wasim-watch UI over the real engine, replacing the synthetic placeholder (phase 5).
  - *Composition branch:* scalar reducer generalization with fail-loud boundaries (phase 3) →
    **the payoff:** vector boundary reductions = the exceedance curve (phase 4), unblocked by
    the array executor already working.
- **Defer with explicit gates:** whole-distribution bindings (6), peer graphs (7), pipelined
  cross-sweep streaming (8) — each waits on a concrete model that needs it. Note phase 8 is now
  *enabled* by the streaming substrate rather than blocked by its absence.
