/**
 * WaSim streaming contract
 * ======================================================================
 * A diff against ./contract.ts. The governing rule:
 *
 *     PartialRunResult is structurally assignable to RunResult.
 *
 * A partial result is not a different shape — it is the SAME shape with
 * fewer realizations folded in. That means the renderer (Diagram, glyphs,
 * Transport) consumes a partial exactly as it consumes a final result, and
 * `RunResult` is simply the last value of a stream of partials. No view code
 * changes to gain streaming.
 *
 * ----------------------------------------------------------------------
 * WHAT STREAMS AND WHAT DOES NOT
 *
 * WaSim has two nested loops with opposite streaming characteristics:
 *
 *     for realization in 1..N:      ← OUTER: independent, long → STREAM THIS
 *         for step in 1..T:         ← INNER: sequential, sub-ms → DO NOT STREAM
 *             evaluate graph
 *
 * The inner loop is already fast, and streaming it would couple frame rate to
 * solver internals while making backward scrub impossible. The playhead stays
 * decoupled: animation always replays retained history.
 *
 * The outer loop is where streaming pays. Realizations arrive WHOLE, and the
 * uncertainty band visibly tightens as they accumulate (CI shrinking at
 * 1/sqrt(N)). That turns "wait for a spinner" into "watch the answer converge"
 * and enables early termination once the CI on a target metric is tight enough.
 *
 * ----------------------------------------------------------------------
 * WHAT CROSSES THE BOUNDARY
 *
 * Never full traces for every realization. At R realizations x E elements x
 * T steps x 8 bytes, retaining everything is hundreds of MB and postMessage
 * serialization dominates the run. Instead:
 *
 *   1. RUNNING SUFFICIENT STATISTICS per (element, step): count, sum, sumsq.
 *      Exactly incremental, mergeable across workers, gives mean + CI for free.
 *   2. PERCENTILE SKETCHES per (element, step): percentiles are NOT
 *      incrementally computable in constant memory, so use a mergeable sketch
 *      (t-digest / P^2 / fixed bins). Approximate is fine for a DISPLAY band;
 *      compute exact percentiles at the end from retained data only if the
 *      results_spec demands it.
 *   3. A BOUNDED SAMPLE of full traces — the first K realizations plus any the
 *      exemplar logic pins (current worst case, etc.). This is all the
 *      single-realization replay view needs.
 *
 * That split — sketches for the cloud, a bounded sample for the stories — is
 * the load-bearing decision. It should be settled before the results layer
 * hardens, because it determines what the engine retains.
 */

import type {
  ElementMeta,
  Realization,
  AggregateTrace,
  RunResult,
} from "./contract";

/* ======================================================================
 * 1. INCREMENTAL STATISTICS
 * ==================================================================== */

/**
 * Running moments for one (element, step) cell. Exactly incremental and
 * exactly mergeable — parallel workers can each accumulate their own and
 * combine without any loss of precision relative to a single-threaded run.
 *
 * Use Welford/Chan pairwise merging rather than naive sum-of-squares if
 * numerical conditioning matters at large N (it will, at 1e5+ realizations
 * with large-magnitude values).
 */
export interface RunningMoments {
  count: number;
  mean: number;
  /** Sum of squared deviations from the mean (Welford's M2). */
  m2: number;
}

/** Merge two independent accumulations (Chan et al. parallel variance). */
export function mergeMoments(a: RunningMoments, b: RunningMoments): RunningMoments {
  if (a.count === 0) return { ...b };
  if (b.count === 0) return { ...a };
  const count = a.count + b.count;
  const delta = b.mean - a.mean;
  const mean = a.mean + (delta * b.count) / count;
  const m2 = a.m2 + b.m2 + (delta * delta * a.count * b.count) / count;
  return { count, mean, m2 };
}

/** Standard error of the mean — the quantity the convergence view watches. */
export function standardError(m: RunningMoments): number {
  if (m.count < 2) return NaN;
  const variance = m.m2 / (m.count - 1);
  return Math.sqrt(variance / m.count);
}

/**
 * A mergeable percentile sketch. The interface is deliberately abstract so
 * the engine can back it with t-digest (best accuracy/memory tradeoff),
 * P^2 (fixed 5-marker, tiny), or fixed histogram bins (simplest, needs a
 * bounded range). All three satisfy: constant memory, mergeable, streaming.
 */
export interface PercentileSketch {
  /** Approximate value at quantile q in [0,1]. */
  quantile(q: number): number;
  /** Number of samples folded in. */
  readonly count: number;
  /** Serializable form for postMessage / SharedArrayBuffer transfer. */
  serialize(): ArrayBuffer;
}

/**
 * Per-(element, step) streaming accumulator. This is what the engine keeps
 * in memory during a sweep, and it is O(elements x steps), independent of
 * realization count — which is the whole point.
 */
export interface StreamingCell {
  moments: RunningMoments;
  sketch?: PercentileSketch;
  /** For discrete kinds: count of realizations in the active/failed state. */
  activeCount?: number;
}

/* ======================================================================
 * 2. PARTIAL RESULTS
 * ==================================================================== */

/** Convergence diagnostics the UI surfaces while a sweep is in flight. */
export interface ConvergenceStatus {
  /** Realizations folded into the current aggregate. */
  completed: number;
  /** Total requested (may be Infinity for run-until-converged). */
  requested: number;
  /**
   * Half-width of the 95% CI on the mean of the watched metric, as a fraction
   * of the mean. This is the number that should visibly shrink at 1/sqrt(N),
   * and the natural early-termination trigger.
   */
  relativeCiHalfWidth?: number;
  /** Element + step the CI above is measured on, if a watch target is set. */
  watchElementId?: string;
  watchStep?: number;
  /** True once a configured convergence tolerance has been met. */
  converged: boolean;
  /** Wall-clock ms elapsed, for throughput display and ETA. */
  elapsedMs: number;
}

/**
 * A partial result. Structurally assignable to RunResult — every field the
 * renderer reads is present and valid, just computed from `completed`
 * realizations rather than all of them.
 *
 * `aggregate` bands are derived from sketches (approximate while streaming,
 * optionally recomputed exactly at the end). `realizations` holds only the
 * BOUNDED RETAINED SAMPLE, not every realization — which is why the view's
 * realization selector must key off `retainedIndices`, not assume dense
 * indexing into the array.
 */
export interface PartialRunResult extends RunResult {
  /** Discriminator: false while streaming, true on the terminal emission. */
  final: boolean;
  convergence: ConvergenceStatus;
  /**
   * Which realization indices are actually retained in `realizations`.
   * Sparse by design: the first K, plus pinned exemplars. The UI offers
   * single-realization replay only for these.
   */
  retainedIndices: number[];
  /**
   * True if `aggregate` percentile bands came from sketches (approximate)
   * rather than exact order statistics. The UI can mark the band as
   * provisional while this is true — honest, and it disappears on `final`
   * if exact percentiles are recomputed.
   */
  bandsApproximate: boolean;
}

/* ======================================================================
 * 3. RUN CONTROL
 * ==================================================================== */

/** Why a run stopped. Distinguishes user action from convergence from error. */
export type RunTermination =
  | "completed" // ran all requested realizations
  | "converged" // hit the convergence tolerance early
  | "cancelled" // user aborted
  | "error";

export interface ConvergenceTarget {
  elementId: string;
  /** Step to watch; omit to watch the final step. */
  step?: number;
  /** Stop when the 95% CI half-width falls below this fraction of the mean. */
  relativeTolerance: number;
  /** Never stop before this many realizations, regardless of CI. */
  minRealizations: number;
}

export interface StreamRunOptions {
  realizations: number;
  seedRoot: number;
  timebase: "fixed" | "event_accurate";
  /**
   * Realizations per emitted chunk. Tune so one chunk is ~16-50ms of work:
   * small enough to keep the UI live, large enough that per-chunk overhead
   * (serialization, boundary crossings) stays negligible.
   */
  chunkSize: number;
  /** How many full traces to retain for single-realization replay. */
  retainTraces: number;
  /** Optional early-stop rule. */
  convergenceTarget?: ConvergenceTarget;
  /** Compute exact percentiles on the final emission (costs a retained pass). */
  exactFinalPercentiles?: boolean;
}

/**
 * A handle on an in-flight run. Cancellation is cooperative: the engine polls
 * the abort flag BETWEEN realizations (never mid-realization), so a cancelled
 * run always ends on a realization boundary and its partial aggregate stays
 * statistically valid — no half-integrated realization pollutes the stats.
 */
export interface RunHandle {
  /** Resolves with the terminal result once the run ends for any reason. */
  readonly done: Promise<PartialRunResult>;
  /** Request cancellation. Idempotent. */
  cancel(): void;
  /** Why it stopped; null while still running. */
  readonly termination: RunTermination | null;
}

/**
 * The streaming engine interface. Note this SUPERSEDES rather than replaces
 * WasimEngine.run() — a batch run is just a stream whose caller ignores every
 * emission except the final one:
 *
 *     const h = engine.runStreaming(opts, () => {});
 *     const result = await h.done;   // identical to the old run()
 */
export interface StreamingWasimEngine {
  runStreaming(
    options: StreamRunOptions,
    /**
     * Called once per completed chunk on the main thread. Must be cheap —
     * it runs inside the render loop's budget. Do state updates here, not
     * heavy derivation.
     */
    onPartial: (partial: PartialRunResult) => void
  ): RunHandle;
}

/* ======================================================================
 * 4. WORKER PROTOCOL
 * ==================================================================== */

/**
 * Messages main thread → worker.
 * The model is sent once at start; chunks are pulled by the worker itself
 * rather than requested per-chunk, to avoid a round-trip per chunk.
 */
export type WorkerRequest =
  | { type: "start"; model: unknown; options: StreamRunOptions; elements: ElementMeta[] }
  | { type: "cancel" };

/**
 * Messages worker → main thread.
 *
 * `partial` carries the merged aggregate, NOT raw traces — see the transfer
 * note at the top. Retained traces are sent once, when the realization that
 * produced them is first retained, so they are not re-serialized every chunk.
 */
export type WorkerResponse =
  | { type: "partial"; partial: PartialRunResult }
  | { type: "retained"; realization: Realization }
  | { type: "done"; termination: RunTermination; partial: PartialRunResult }
  | { type: "error"; message: string };

/* ======================================================================
 * 5. DETERMINISM INVARIANT  ← the one that protects the brand
 * ==================================================================== */

/**
 * STREAMING AND PARALLELISM MUST NOT CHANGE RESULTS.
 *
 * The `fixed` timebase's bit-identical guarantee has to survive chunking,
 * worker fan-out, and early termination. That requires realization k's random
 * stream to be a pure function of (seedRoot, k) — a counter-based or
 * splittable PRNG (PCG, ChaCha, SplitMix-style jump), NOT one sequential
 * generator advanced across realizations.
 *
 * If realization k's stream depends on k-1 having run first, results become
 * order-dependent, and a run split across 8 workers silently differs from a
 * single-threaded run. That failure mode — reproducible single-threaded,
 * subtly different multi-threaded — is exactly the bug that would destroy
 * trust in a defensibility-focused tool.
 *
 * Get it right and the claim is strong and rare:
 *
 *     Identical results whether run on one core or sixteen,
 *     streamed or batched, cancelled and resumed or run straight through.
 *
 * `event_accurate` sub-steps consume no randomness, so this guarantee extends
 * across both timebase modes. Preserve that property.
 *
 * The invariant below should be asserted in CI: run the same model batched
 * single-threaded and streamed across N workers, hash both aggregates, and
 * require bit-identical output.
 */
export function seedForRealization(seedRoot: number, index: number): number {
  // Placeholder for the engine's real splittable-PRNG seeding. The contract
  // is what matters: pure function of (seedRoot, index), no run-order term.
  let z = (seedRoot ^ (index * 0x9e3779b9)) >>> 0;
  z = Math.imul(z ^ (z >>> 16), 0x21f0aaad) >>> 0;
  z = Math.imul(z ^ (z >>> 15), 0x735a2d97) >>> 0;
  return (z ^ (z >>> 15)) >>> 0;
}

/** Convenience: batch semantics on top of the streaming interface. */
export async function runBatch(
  engine: StreamingWasimEngine,
  options: StreamRunOptions
): Promise<RunResult> {
  const handle = engine.runStreaming(options, () => {});
  return handle.done;
}

/** Type guard for view code that wants to know if a result is still growing. */
export function isStreaming(r: RunResult | PartialRunResult): r is PartialRunResult {
  return "final" in r && !(r as PartialRunResult).final;
}

export type { AggregateTrace, StreamingCell as StreamingAccumulator };
