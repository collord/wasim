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
