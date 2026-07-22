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

export interface InterfaceBinding {
  input: string
  from: string | null
}

export interface ContainerInterface {
  inputs: InterfaceBinding[]
  outputs: string[]
}

export interface ContainerDef {
  id: string
  name: string
  parent: string | null
  children: string[]
  /** Interior element ids (convenience; element.container is authoritative). */
  elements?: string[]
  /** Structural role. `submodel` is a nested run; others are organizational. */
  kind?: 'container' | 'group' | 'submodel'
  /** For a submodel: the nested run's settings (null = inherit parent). */
  simulation_settings?: SimulationSettings | null
  /** For a submodel: boundary inputs (parent driver → interior consumer) + outputs. */
  interface?: ContainerInterface | null
}

export interface Distribution {
  family: string
  parameters: Record<string, Quantity | number>
  truncation?: { min?: number; max?: number } | null
  correlation_group?: string | null
}

/** Top-level metadata, parsed once on load for sim-settings (format-agnostic; v1 or v2). */
export interface ModelJson {
  wasim_version: string
  simulation_settings: SimulationSettings
  containers?: ContainerDef[]
}

// ── Model summary (from WasmEngine.model_summary()) — the FE's model of record ──
//
// The engine emits a legacy `type` (original v1 type for imports, else mapped from the
// primitive/value_rule) plus the v2 fields. The frontend renders + edits from this and
// never parses the model schema itself.

export interface ElementSummary {
  id: string
  name: string
  /** Legacy v1-ish type (back-compat label). */
  type: string
  /** v2 primitive: node | stock | link | event | gate | cell | species | medium. */
  primitive: string
  /** Node value_rule (fixed/expression/sample/…); null for non-node primitives. */
  value_rule: string | null
  /** Active traits, derived from field presence (e.g. capacity_clamp, transit_buffer). */
  traits: string[]
  container: string | null
  editable: boolean
  /** Canonical unit the engine computes in. */
  unit: string
  /** Preferred display unit + affine map (`display = value·factor + offset`), when a valid
   * conversion exists; else absent and the canonical `unit` is shown. */
  display_unit?: string | null
  display_factor?: number
  display_offset?: number
  /** Current value for an editable `fixed` node (in canonical units). */
  value: number | null
  bounds?: { min?: number | null; max?: number | null } | null
  /** Distribution (family + parameters) for a `sample` node. */
  dist?: Distribution | null
  /** Readable formula for an `expression` node. */
  formula?: string | null
  /** Interpolation data for a `lookup`/`series` node. */
  table?: {
    x: number[]
    y: number[]
    columns: number[][]
    x_unit?: string | null
    y_unit?: string | null
  } | null
  inputs: string[]
  description: string | null
}

/** Display mapping for a time quantity (display = value·factor + offset). */
export interface QtyDisplay {
  unit: string
  factor: number
  offset: number
}

export interface ModelSummary {
  element_count: number
  elements: ElementSummary[]
  containers: ContainerDef[]
  simulation_settings: SimulationSettings
  time_display: { duration: QtyDisplay; timestep: QtyDisplay }
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
  /** Unit label for time_axis (display unit if the timestep declared one, else canonical). */
  time_unit: string
  elements: Record<string, ElementResults>
  n_realizations: number
  n_steps: number
  /** Sink elements first (unreferenced outputs), then intermediates — all in topo order. */
  output_ids: string[]
}

// ── Sensitivity analysis (runtime-configured, from WasmEngine.sensitivity_json()) ─
// Spec is transient UI state, never persisted in the model. Input values are canonical;
// result values arrive in the target element's display unit.

/** A Monte-Carlo reduction of the target element (matches engine ObjectiveStatKind). */
export type SensitivityStatKind = 'mean' | 'percentile' | 'peak' | 'valley' | 'sum'

export interface SensitivityStatistic {
  kind: SensitivityStatKind
  /** Percentile in [0,100], required when kind = 'percentile'. */
  p?: number
}

export interface SensitivityResultRef {
  element_id: string
  statistic?: SensitivityStatistic | null
}

export interface SweepVar {
  element_id: string
  lower: number
  upper: number
  base: number
  /** Sweep points for one-at-a-time (≥ 2); ignored by tornado. */
  steps: number
}

export type SensitivityMethod = 'one_at_a_time' | 'tornado'

export interface SensitivitySpec {
  result: SensitivityResultRef
  variables: SweepVar[]
  method: SensitivityMethod
}

export interface CurvePoint {
  input: number
  result: number
}

export interface VarCurve {
  element_id: string
  points: CurvePoint[]
}

export interface TornadoBar {
  element_id: string
  low: number
  high: number
  swing: number
}

export interface SensitivityResults {
  base_result: number
  /** One curve per variable (one-at-a-time); empty for tornado. */
  curves: VarCurve[]
  /** One bar per variable, sorted by descending swing (tornado); empty for one-at-a-time. */
  tornado: TornadoBar[]
}

// ── Optimization (runtime-configured, from WasmEngine.optimize_json()) ─────────
// Spec is transient UI state (never persisted). Mirrors the engine's OptimizationSpec /
// StudyResults (snake_case discriminants).

export type OptDirection = 'maximize' | 'minimize'
export type ObjectiveStatKind = 'mean' | 'percentile' | 'peak' | 'valley' | 'sum'

export interface ObjectiveStatistic {
  kind: ObjectiveStatKind
  /** Percentile in [0,100], required when kind = 'percentile'. */
  p?: number
}

export interface OptObjective {
  element_id: string
  direction: OptDirection
  /** Present for a probabilistic objective; omitted = deterministic (single value). */
  statistic?: ObjectiveStatistic | null
}

export interface OptVariable {
  element_id: string
  lower: Quantity
  upper: Quantity
  initial: Quantity
  integer?: boolean
}

export interface OptimizationSpec {
  objective: OptObjective
  variables: OptVariable[]
  constraints?: { condition: unknown; label?: string }[]
}

export interface VariableResult {
  element_id: string
  value: number
}

export interface StudyResults {
  variables: VariableResult[]
  objective: number
  evaluations: number
  converged: boolean
}
