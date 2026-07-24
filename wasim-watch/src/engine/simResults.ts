// TS mirror of the Rust engine's SimulationResults (engine/src/engine.rs:78-115) — the shape
// WasmEngine.run_json / RunHandle.poll serialize. Kept deliberately separate from the richer
// contract.ts RunResult: the streaming poll payload is aggregate bands + final values only (no
// per-realization traces, events, exemplars, or layout), and this view consumes exactly that.

export interface TimeHistoryStats {
  mean: number[];
  p05: number[];
  p25: number[];
  p50: number[];
  p75: number[];
  p95: number[];
}

export interface ElementResults {
  label: string;
  unit: string;
  final_values: number[];
  time_history: TimeHistoryStats | null;
  // analysis?: … (A3 layer; not read by this view)
}

export interface SimulationResults {
  time_axis: number[];
  time_unit: string;
  elements: Record<string, ElementResults>;
  n_realizations: number;
  n_steps: number;
  output_ids: string[];
}
