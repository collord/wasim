# Phase 0 — Poll Boundary API Sketch

*The plumbing the chunk-merge spike deliberately skipped. Concrete `start`/`poll`/`cancel`
signatures grounded in the **actual** `wasm.rs`, `engine.rs::SimulationResults`, and the TS
`streaming.ts` contract — not invented shapes. Companion to
`WORKPLAN_SWEEP_COMPOSITION.md` Phase 0.*

---

## 0. What the existing code already gives us (and constrains us to)

Read before designing — three facts from the real types change the API:

1. **The TS contract already has the flags we need.** `PartialRunResult extends RunResult`
   with `final`, `convergence`, `retainedIndices`, **`bandsApproximate`** (`streaming.ts:163`).
   That last one is the pre-built home for the spike's sketch-vs-exact tradeoff: percentile
   bands from an O(1) sketch set `bandsApproximate: true`; exact bands (sample retention) set
   it `false`. No new UI concept needed.

2. **`SimulationResults` is the batch shape** (`engine.rs:78`): `time_axis`, `time_unit`,
   `elements: HashMap<String, ElementResults>`, `n_realizations`, `n_steps`, `output_ids`.
   `ElementResults` carries `final_values`, `time_history: Option<TimeHistoryStats>`, and the
   A3 `analysis`. `TimeHistoryStats` is exactly `{mean, p05, p25, p50, p75, p95}` — six
   `Vec<f64>` over steps. **The partial snapshot must be a `SimulationResults` plus streaming
   metadata**, so the existing consumers (and the TS `RunResult`) keep working unchanged.

3. **Display-unit conversion currently runs *after* the batch run** (`wasm.rs:99–128`): it
   mutates `final_values` and the six band arrays by `v·f + o`, and rewrites the time axis.
   A streaming `poll()` returns partials *repeatedly* — so this conversion can't be a
   one-shot post-run step. It must either run per-poll (cheap; the band arrays are small,
   O(steps)) or be pushed to the JS caller. **Decision: run it per-poll inside the WASM
   boundary**, so `poll()` always returns display-ready results exactly as `run_json` does
   today. Keeps the display boundary in one place; the cost is trivial (bands are O(steps),
   not O(realizations)).

---

## 1. The Rust-side handle (engine crate, no wasm_bindgen)

Keep the streaming state machine in the plain engine crate so it's unit-testable without a
WASM toolchain (the spike proved the numerics this way; the handle should follow).

```rust
// engine/src/stream.rs  (new)

/// A resumable Monte-Carlo run. Owns the accumulators and the next realization
/// index; advancing it is the only way state changes. Deterministic: the result
/// after advancing through realization k is a pure function of
/// (seed_root, sweep_id, k) — never of how the advances were chunked.
pub struct RunStream<'a> {
    model: &'a Model,
    graph: &'a ModelGraphV2,
    config: RunConfig,
    seed_root: u64,
    sweep_id: SweepId,          // Phase 1: stable, content-derived (NOT positional)
    n_total: u32,               // resolved n_realizations
    next_real: u32,             // realizations [0, next_real) are folded in
    accums: StreamAccums,       // per-element, per-step (see §2)
}

impl<'a> RunStream<'a> {
    /// Construct with zero realizations folded. No work done yet.
    pub fn start(model: &'a Model, graph: &'a ModelGraphV2, config: RunConfig)
        -> Result<Self, EngineError>;

    /// Fold realizations [next_real, min(next_real + count, n_total)) into the
    /// accumulators. Returns the number actually advanced (0 at completion).
    /// This is `run_chunk` — the caller picks `count` (the chunk size) freely;
    /// the merge is associative so any sequence of counts yields the same state.
    pub fn advance(&mut self, count: u32) -> Result<u32, EngineError>;

    /// True once every realization is folded in.
    pub fn is_complete(&self) -> bool { self.next_real >= self.n_total }

    /// Reduce the current accumulators to a snapshot. Cheap, non-destructive —
    /// callable after any number of advances. `final` = is_complete().
    pub fn snapshot(&self) -> PartialResults;

    /// Realizations folded so far — the `(…, realizationsCompleted)` coordinate
    /// the determinism invariant is stated in terms of.
    pub fn completed(&self) -> u32 { self.next_real }
}
```

**Why `advance(count)` and not an internal chunk size:** the caller (WASM poll, or a test)
owns cadence. A test folds `advance(1)` 10 000 times; the UI folds `advance(~200)` per frame;
CI folds two different chunkings and asserts equal snapshots. The engine has no opinion on
chunk size — that's the whole point of chunk-invariance.

---

## 2. The accumulator (resolves the spike's open decision)

The spike proved the merge is associative and surfaced the real choice: **exact percentiles
cost O(realizations) memory; the memory win requires an approximate sketch.** Encode both and
let config pick — don't hardcode.

```rust
struct StreamAccums {
    // Per element id → per step. Mean/sd stream for free (Chan merge).
    moments: HashMap<String, Vec<RunningMoments>>,   // O(elements × steps)
    // Percentile bands: ONE of these per the config's `bands` mode.
    bands: BandAccum,
    final_moments: HashMap<String, RunningMoments>,  // for final_values stats
}

enum BandAccum {
    /// Exact: retain per-step samples. Batch-identical bands, O(realizations × elements × steps)
    /// memory — i.e. NO memory win. Sets `bands_approximate = false`. This is the default until
    /// the sketch is validated, because it's the safe/correct anchor.
    Exact(HashMap<String, Vec<Vec<f64>>>),
    /// Sketch: bounded per-step percentile sketch (t-digest/binned). O(elements × steps × k)
    /// memory, k = sketch size. Approximate bands, sets `bands_approximate = true`. The memory
    /// win — opt-in via config until CI bounds its error against the exact anchor.
    Sketch(HashMap<String, Vec<PercentileSketch>>),
}
```

`RunningMoments` is the spike's `RunningMean` extended with M2 (Welford) for sd — both
associatively mergeable. `BandAccum::merge` concatenates samples (Exact) or merges sketches
(Sketch); both associative, which is what makes `advance` order-free.

**Sequencing note:** ship `Exact` first (batch-parity, provable), add `Sketch` behind a config
flag once a CI test bounds `|sketch_band − exact_band|` on the corpus. This matches the spike's
verdict: prove correctness first, buy memory second, never conflate them.

---

## 3. The snapshot type

A `SimulationResults` plus the four streaming fields the TS contract already declares. Reuse,
don't reinvent.

```rust
#[derive(serde::Serialize)]
pub struct PartialResults {
    /// Every field of the batch shape, so a PartialResults IS-A RunResult on the JS side
    /// (mirrors `PartialRunResult extends RunResult`). Bands are display-converted (§0.3).
    #[serde(flatten)]
    pub results: SimulationResults,
    /// false while converging, true on the last snapshot. Drives `isStreaming`.
    pub final_: bool,                       // serde rename to "final"
    /// realizations folded / total → progress + convergence status.
    pub completed: u32,
    pub total: u32,
    /// True when bands came from a sketch (§2). The UI's existing `bandsApproximate`.
    pub bands_approximate: bool,
    /// Optional convergence metrics (running standard error of the mean, band stability).
    /// Absent until we define the CI target; the field exists so the shape is stable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence: Option<ConvergenceStatus>,
}
```

The `#[serde(flatten)]` means the JSON is a `RunResult` with extra keys — exactly what
`PartialRunResult extends RunResult` expects. No TS-side type surgery.

---

## 4. The WASM boundary (thin wrapper over §1)

`wasm_bindgen` can't hand a borrowed `RunStream<'a>` to JS, so the handle owns its model/graph
(or an Rc to the engine's). Keep it a thin translation layer — all logic lives in §1.

```rust
// engine/src/wasm.rs  (added to WasmEngine)

#[wasm_bindgen]
pub struct RunHandle { /* owns a RunStream with 'static model/graph via Rc */ }

#[wasm_bindgen]
impl WasmEngine {
    /// Begin a streaming run. Returns a handle; no realizations folded yet.
    /// Same config JSON as `run_json` — one config path, not two.
    pub fn start(&self, config_json: &str) -> Result<RunHandle, JsError>;
}

#[wasm_bindgen]
impl RunHandle {
    /// Fold up to `count` more realizations; return a display-ready PartialResults
    /// JSON string (display-unit conversion applied per §0.3). The UI calls this
    /// per animation frame with its chosen `count`. Cheap when already complete.
    pub fn poll(&mut self, count: u32) -> Result<String, JsError>;

    /// realizations folded so far / total — for a progress bar without a full poll.
    pub fn progress(&self) -> u32;
    pub fn total(&self) -> u32;

    /// Abandon the run. The last valid snapshot remains pollable once more (partial
    /// state is valid), preserving "cancelled → still a correct partial result".
    pub fn cancel(&mut self);
}
```

**Why `poll(count)` carries the chunk size** rather than `start` fixing it: identical reason
to §1 — cadence is the caller's. rAF-driven UI passes a `count` tuned to frame budget; it does
not change results, only latency-to-first-pixel.

**Cancellation semantics** (matches the streaming design's determinism promise): `cancel`
stops further advances; the accumulators hold realizations `[0, completed)`, which is a
*valid* run of `completed` realizations seeded identically to the first `completed` of a full
run. Poll once more for the final partial. "Cancelled and resumed or run straight through →
identical results" holds because realization k's stream is `(seed_root, sweep_id, k)`,
independent of whether the run was interrupted.

---

## 5. What this preserves, and the one thing it changes

**Preserved:**
- `run_json` stays — it's now expressible as `start` + one `advance(n_total)` + `snapshot`,
  so the batch path is a special case of streaming, not a fork. (Keep the standalone
  `run_json` too, for callers that want one shot.)
- The display-unit boundary stays in `wasm.rs`, just moves from post-run to per-poll (§0.3).
- `SimulationResults` / `TimeHistoryStats` are unchanged; `PartialResults` wraps, not replaces.
- Determinism: realization k's stream is `(seed_root, sweep_id, k)`; chunking is null.

**Changed (and it's the spike's finding, load-bearing):**
- CI asserts **chunk-vs-chunk** snapshot equality, not chunk-vs-batch (FP non-associativity,
  ≤89 ULP measured). The legacy `run_json` batch path and the streamed path will differ by
  tens of ULP on the mean *by construction* — if exact byte-parity with the old batch output
  is required for some downstream consumer, that's a separate stricter constraint that pins
  the accumulator's summation order, and must be decided explicitly.

---

## 6. Build order within Phase 0

1. `engine/src/stream.rs`: `RunStream` + `RunningMoments` + `BandAccum::Exact`. Unit-test
   against `run_v2` batch output (chunk-vs-chunk equality; mean within measured ULP of batch).
   *No WASM yet* — same discipline as the spike.
2. Wire `RunStream` under the existing `run_v2` so the batch path becomes
   `start + advance(all) + snapshot`, proving the special-case claim and catching drift.
3. `BandAccum::Sketch` behind a config flag; CI test bounding sketch-vs-exact band error.
4. `wasm.rs`: `start`/`RunHandle`/`poll`/`cancel` + per-poll display conversion.
5. (Phase 5) drive `poll` from wasim-watch per rAF; render `PartialResults`.

Steps 1–2 are pure Rust and retire all remaining Phase-0 numerical risk. 3 is the memory win.
4–5 are plumbing over a proven core.
