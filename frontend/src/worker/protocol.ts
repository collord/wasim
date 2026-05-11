import type { ModelSummary, SimulationResults } from '../types'

// ── Main → Worker ─────────────────────────────────────────────────────────────

export type MainToWorker =
  | { type: 'load_model'; payload: string }
  | { type: 'set_constant'; element_id: string; value: number }
  | { type: 'set_rv_param'; element_id: string; param_name: string; value: number }
  | { type: 'run'; config: { n_realizations?: number; seed?: number; duration_override?: number; timestep_override?: number } }

// ── Worker → Main ─────────────────────────────────────────────────────────────

export type WorkerToMain =
  | { type: 'model_loaded'; summary: ModelSummary }
  | { type: 'complete'; results: SimulationResults }
  | { type: 'error'; message: string }
