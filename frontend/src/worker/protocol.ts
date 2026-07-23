import type { ModelSummary, OptimizationSpec, ResultsSpec, SensitivityResults, SensitivitySpec, SimulationResults, StudyResults } from '../types'

// ── Validation diagnostics (from WASM validate_json + reconcile) ────────────────

export type IssueSeverity = 'error' | 'warning'

export interface Issue {
  severity: IssueSeverity
  message: string
  /** Element the issue jumps to on click, when the engine names one. */
  element_id?: string | null
}

export interface Validation {
  ok: boolean
  issues: Issue[]
  topo: string[]
}

// ── Main → Worker ─────────────────────────────────────────────────────────────

export type MainToWorker =
  | { type: 'load_model'; payload: string }
  | { type: 'set_constant'; element_id: string; value: number }
  | { type: 'set_rv_param'; element_id: string; param_name: string; value: number }
  | { type: 'run'; config: { n_realizations?: number; seed?: number; duration_override?: number; timestep_override?: number; results_spec?: ResultsSpec } }
  | { type: 'run_sensitivity'; spec: SensitivitySpec }
  | { type: 'run_optimization'; spec: OptimizationSpec }
  // Authoring additions (spec §13.5): a structural edit rebuilds the engine from the whole
  // model; `validate` runs parse + dimensional + graph checks without rebuilding the run engine.
  | { type: 'reconcile'; model: string; token: number }
  | { type: 'validate'; model: string; token: number }
  // Silent validation for the copilot loop — validates a candidate without touching the main
  // issues panel (§17.3).
  | { type: 'llm_validate'; model: string; token: number }

// ── Worker → Main ─────────────────────────────────────────────────────────────

export type WorkerToMain =
  | { type: 'model_loaded'; summary: ModelSummary }
  | { type: 'complete'; results: SimulationResults }
  | { type: 'sensitivity_complete'; results: SensitivityResults }
  | { type: 'optimization_complete'; results: StudyResults }
  | { type: 'error'; message: string }
  // `reconciled` carries the fresh summary (render/edit source) + validation (issues panel)
  // + topo (causality view). `token` lets the store drop stale (out-of-order) responses.
  | { type: 'reconciled'; summary: ModelSummary | null; validation: Validation; token: number }
  | { type: 'validated'; validation: Validation; token: number }
  | { type: 'llm_validated'; validation: Validation; token: number }
