// Drive a streaming run over the real WASM engine: fold realizations in chunks per animation
// frame and emit a growing partial SimulationResults each frame. This is the browser side of the
// Rust RunHandle poll boundary (engine/src/wasm.rs) — no synthetic model, no fabricated data.
import type { WasmEngine, RunHandle } from "./wasm";
import type { SimulationResults } from "./simResults";

export interface PollProgress {
  completed: number;
  total: number;
  done: boolean;
}

export interface RunController {
  /** Stop folding further realizations; the last emitted partial stays valid. */
  cancel(): void;
}

/**
 * Start a streaming run. `onPartial` fires once per animation frame with the partial results
 * folded so far (n_realizations grows toward total); the final frame has `done: true`.
 * `chunk` realizations are folded per frame — sized so a full run spans ~1-2s of frames.
 *
 * Determinism (proven bit-identical to a batch run_json): the emitted partial after all frames
 * equals the batch result, and cancelling leaves a valid partial of `completed` realizations.
 */
export function startRun(
  engine: WasmEngine,
  configJson: string,
  onPartial: (results: SimulationResults, progress: PollProgress) => void,
  chunk = 100,
): RunController {
  const handle: RunHandle = engine.start(configJson);
  let cancelled = false;
  let raf = 0;

  const frame = () => {
    if (cancelled) return;
    const json = handle.poll(chunk);
    const results = JSON.parse(json) as SimulationResults;
    const completed = handle.completed();
    const total = handle.total();
    const done = handle.is_complete();
    onPartial(results, { completed, total, done });
    if (!done) {
      raf = requestAnimationFrame(frame);
    }
  };
  raf = requestAnimationFrame(frame);

  return {
    cancel() {
      cancelled = true;
      cancelAnimationFrame(raf);
      handle.cancel();
      // Emit one final partial so the UI settles on the cancelled-but-valid state.
      const results = JSON.parse(handle.poll(0)) as SimulationResults;
      onPartial(results, { completed: handle.completed(), total: handle.total(), done: true });
    },
  };
}
