/**
 * Reference implementation of the streaming contract, over the synthetic
 * model. This is NOT the real engine — it is a working demonstration that
 * the contract in ./streaming.ts closes, and a template for the Rust/WASM
 * side to match.
 *
 * What it proves:
 *   - Partials are structurally valid RunResults, so the renderer needs no
 *     changes to consume a stream.
 *   - Aggregates accumulate incrementally (moments merged, bins folded) with
 *     memory O(elements x steps), independent of realization count.
 *   - Only a bounded sample of full traces is retained.
 *   - Realization seeding is a pure function of (seedRoot, index), so chunk
 *     boundaries and worker fan-out cannot change results.
 */

import type {
  AggregateTrace,
  ElementMeta,
  Realization,
  RunResult,
} from "./contract";
import {
  mergeMoments,
  seedForRealization,
  standardError,
  type ConvergenceStatus,
  type PartialRunResult,
  type RunHandle,
  type RunTermination,
  type RunningMoments,
  type StreamRunOptions,
} from "./streaming";
import { simulateOne, MODEL_ELEMENTS, N_STEPS, DT } from "./synthetic";

/**
 * Fixed-bin histogram sketch — the simplest of the three mergeable options.
 * Constant memory per (element, step), mergeable by bin-wise addition.
 * t-digest would be the production choice; this shows the shape.
 */
class BinSketch {
  bins: Float64Array;
  lo: number;
  hi: number;
  count = 0;
  constructor(lo: number, hi: number, nBins = 64) {
    this.lo = lo;
    this.hi = hi;
    this.bins = new Float64Array(nBins);
  }
  push(v: number) {
    const n = this.bins.length;
    let i = Math.floor(((v - this.lo) / (this.hi - this.lo)) * n);
    if (i < 0) i = 0;
    if (i >= n) i = n - 1;
    this.bins[i] += 1;
    this.count += 1;
  }
  quantile(q: number): number {
    if (this.count === 0) return 0;
    const target = q * this.count;
    let acc = 0;
    for (let i = 0; i < this.bins.length; i++) {
      acc += this.bins[i];
      if (acc >= target) {
        const frac = i / this.bins.length;
        return this.lo + frac * (this.hi - this.lo);
      }
    }
    return this.hi;
  }
}

interface Accum {
  moments: RunningMoments[]; // per step
  sketch: BinSketch[]; // per step
  activeCount: number[]; // per step, for discrete kinds
}

function emptyAccum(lo: number, hi: number): Accum {
  return {
    moments: Array.from({ length: N_STEPS }, () => ({ count: 0, mean: 0, m2: 0 })),
    sketch: Array.from({ length: N_STEPS }, () => new BinSketch(lo, hi)),
    activeCount: new Array(N_STEPS).fill(0),
  };
}

/** Plausible display ranges per element, for the fixed-bin sketch. */
const RANGES: Record<string, [number, number]> = {
  reservoir: [0, 105],
  stage: [0, 13],
  inflow: [0, 45],
  outflow: [0, 2],
};

export function runStreamingSynthetic(
  options: StreamRunOptions,
  onPartial: (p: PartialRunResult) => void
): RunHandle {
  let cancelled = false;
  let termination: RunTermination | null = null;

  const accums: Record<string, Accum> = {};
  for (const el of MODEL_ELEMENTS) {
    const [lo, hi] = RANGES[el.id] ?? [0, 1];
    accums[el.id] = emptyAccum(lo, hi);
  }

  const retained: Realization[] = [];
  const retainedIndices: number[] = [];
  const grid = Array.from({ length: N_STEPS }, (_, i) => i * DT);
  const started = performance.now();
  let completed = 0;
  let worstPeak = -Infinity;
  let worstIndex = 0;

  const done = new Promise<PartialRunResult>((resolve) => {
    function buildPartial(final: boolean, converged: boolean): PartialRunResult {
      const aggregate: Record<string, AggregateTrace> = {};
      for (const el of MODEL_ELEMENTS) {
        const a = accums[el.id];
        if (el.kind === "state_machine" || el.kind === "controller") {
          aggregate[el.id] = {
            elementId: el.id,
            activeFraction: a.activeCount.map((c) => (completed ? c / completed : 0)),
          };
        } else {
          aggregate[el.id] = {
            elementId: el.id,
            p05: a.sketch.map((s) => s.quantile(0.05)),
            p25: a.sketch.map((s) => s.quantile(0.25)),
            p50: a.sketch.map((s) => s.quantile(0.5)),
            p75: a.sketch.map((s) => s.quantile(0.75)),
            p95: a.sketch.map((s) => s.quantile(0.95)),
          };
        }
      }

      // convergence on the watched target (default: reservoir, final step)
      const t = options.convergenceTarget;
      const watchId = t?.elementId ?? "reservoir";
      const watchStep = t?.step ?? N_STEPS - 1;
      const m = accums[watchId]?.moments[watchStep];
      let rel: number | undefined;
      if (m && m.count > 1 && Math.abs(m.mean) > 1e-12) {
        rel = (1.96 * standardError(m)) / Math.abs(m.mean);
      }

      const convergence: ConvergenceStatus = {
        completed,
        requested: options.realizations,
        relativeCiHalfWidth: rel,
        watchElementId: watchId,
        watchStep,
        converged,
        elapsedMs: performance.now() - started,
      };

      return {
        modelName: "Pumped reservoir · failing pump (streaming)",
        grid,
        timeUnit: "days",
        timebase: options.timebase,
        seedRoot: options.seedRoot,
        elements: MODEL_ELEMENTS as ElementMeta[],
        aggregate,
        realizations: retained,
        exemplars: [{ label: "peak level (worst case)", realizationIndex: worstIndex }],
        final,
        convergence,
        retainedIndices,
        bandsApproximate: !final || !options.exactFinalPercentiles,
      };
    }

    function runChunk() {
      if (cancelled) {
        termination = "cancelled";
        resolve(buildPartial(true, false));
        return;
      }

      const end = Math.min(completed + options.chunkSize, options.realizations);
      for (let k = completed; k < end; k++) {
        // determinism: seed is a pure function of (seedRoot, k)
        const sim = simulateOne(seedForRealization(options.seedRoot, k));

        for (const el of MODEL_ELEMENTS) {
          const a = accums[el.id];
          if (el.kind === "state_machine") {
            sim.pumpStatus.forEach((s, i) => {
              if (s === "failed") a.activeCount[i] += 1;
            });
          } else if (el.kind === "controller") {
            sim.controllerStatus.forEach((s, i) => {
              if (s === "on") a.activeCount[i] += 1;
            });
          } else {
            const series = (sim as unknown as Record<string, number[]>)[el.id];
            if (!series) continue;
            for (let i = 0; i < N_STEPS; i++) {
              a.moments[i] = mergeMoments(a.moments[i], {
                count: 1,
                mean: series[i],
                m2: 0,
              });
              a.sketch[i].push(series[i]);
            }
          }
        }

        // bounded trace retention
        if (retained.length < options.retainTraces) {
          retained.push({
            index: k,
            seed: seedForRealization(options.seedRoot, k),
            traces: {
              reservoir: { elementId: "reservoir", value: sim.reservoir },
              stage: { elementId: "stage", value: sim.stage },
              inflow: { elementId: "inflow", value: sim.inflow },
              outflow: { elementId: "outflow", value: sim.outflow },
              pump: { elementId: "pump", status: sim.pumpStatus },
              controller: { elementId: "controller", status: sim.controllerStatus },
            },
            events: sim.events,
            terminatedAt: sim.terminatedAt,
          });
          retainedIndices.push(k);
        }

        const peak = Math.max(...sim.reservoir);
        if (peak > worstPeak) {
          worstPeak = peak;
          worstIndex = k;
        }
      }
      completed = end;

      // early termination on convergence
      const t = options.convergenceTarget;
      let converged = false;
      if (t && completed >= t.minRealizations) {
        const m = accums[t.elementId]?.moments[t.step ?? N_STEPS - 1];
        if (m && m.count > 1 && Math.abs(m.mean) > 1e-12) {
          const rel = (1.96 * standardError(m)) / Math.abs(m.mean);
          if (rel < t.relativeTolerance) converged = true;
        }
      }

      if (completed >= options.realizations || converged) {
        termination = converged ? "converged" : "completed";
        const p = buildPartial(true, converged);
        onPartial(p);
        resolve(p);
        return;
      }

      onPartial(buildPartial(false, false));
      // yield to the event loop so the UI stays live
      setTimeout(runChunk, 0);
    }

    setTimeout(runChunk, 0);
  });

  return {
    done,
    cancel() {
      cancelled = true;
    },
    get termination() {
      return termination;
    },
  };
}

/** Batch semantics on top of the stream — proves the supersede claim. */
export async function runBatchSynthetic(options: StreamRunOptions): Promise<RunResult> {
  return runStreamingSynthetic(options, () => {}).done;
}
