// ── Model JSON shape (mirrors Rust structs) ───────────────────────────────────

export interface Quantity {
  value: number
  unit: string
  display_unit?: string | null
}

export interface SimulationSettings {
  duration: Quantity
  timestep: Quantity
  n_realizations: number
  sampling_method: 'monte_carlo' | 'lhs'
  seed: number | null
}

export interface ContainerDef {
  id: string
  name: string
  parent: string | null
  children: string[]
}

export interface Distribution {
  family: string
  parameters: Record<string, Quantity | number>
  truncation?: { min?: number; max?: number } | null
  correlation_group?: string | null
}

export type ModelElement =
  | ConstantElement
  | RandomVariableElement
  | ExpressionElement
  | AccumulatorElement
  | OtherElement

export interface ConstantElement {
  id: string
  name: string
  type: 'constant'
  container: string | null
  description?: string | null
  value: Quantity
  editable: boolean
  bounds?: { min?: number | null; max?: number | null } | null
  save_results?: { final_value: boolean; time_history: boolean }
}

export interface RandomVariableElement {
  id: string
  name: string
  type: 'random_variable'
  container: string | null
  description?: string | null
  distribution: Distribution
  save_results?: { final_value: boolean; time_history: boolean }
}

export interface ExpressionElement {
  id: string
  name: string
  type: 'expression'
  container: string | null
  description?: string | null
  save_results?: { final_value: boolean; time_history: boolean }
}

export interface AccumulatorElement {
  id: string
  name: string
  type: 'accumulator'
  container: string | null
  description?: string | null
  save_results?: { final_value: boolean; time_history: boolean }
}

export interface OtherElement {
  id: string
  name: string
  type: string
  container: string | null
}

export interface ModelJson {
  wasim_version: string
  source?: {
    generator?: string | null
    notes?: string | null
  } | null
  simulation_settings: SimulationSettings
  containers: ContainerDef[]
  elements: ModelElement[]
}

// ── Model summary (from WasmEngine.model_summary()) ──────────────────────────

export interface ElementSummary {
  id: string
  name: string
  type: string
  container: string | null
  editable: boolean
  unit: string
}

export interface ModelSummary {
  element_count: number
  elements: ElementSummary[]
  containers: ContainerDef[]
  simulation_settings: SimulationSettings
}

// ── Simulation results (from WasmEngine.run_json()) ──────────────────────────

export interface TimeHistoryStats {
  mean: number[]
  p05: number[]
  p25: number[]
  p50: number[]
  p75: number[]
  p95: number[]
}

export interface ElementResults {
  label: string
  unit: string
  final_values: number[]
  time_history: TimeHistoryStats | null
}

export interface SimulationResults {
  time_axis: number[]
  elements: Record<string, ElementResults>
  n_realizations: number
  n_steps: number
  /** Sink elements first (unreferenced outputs), then intermediates — all in topo order. */
  output_ids: string[]
}
