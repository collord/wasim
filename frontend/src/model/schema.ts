// The canonical, editable model document (spec §13.1). This is the flat v2-native /
// v1 schema JSON the engine parses directly — NOT the engine's `model_json()` serialize
// output (which is externally-tagged and not reparseable). The store holds one of these
// as the source of truth; every editor is a pure transform over it and it serializes
// back to a diffable `model.json`.

import type { Ast } from './ast'
import type { ContainerDef, Distribution, Quantity, SimulationSettings } from '../types'

export interface ExpressionField {
  ast: Ast
  display?: string | null
  source?: unknown
}

/** A flat element object. Fields are activated by presence (the v2 "traits" idea). Only
 *  the fields the authoring tool reads/writes are typed; anything else is preserved. */
export interface FlatElement {
  id: string
  name: string
  description?: string | null
  container?: string | null
  /** v1 discriminator. */
  type?: string
  /** v2 discriminators. */
  primitive?: string
  value_rule?: string
  // node payloads
  value?: Quantity
  values?: number[]
  unit?: string
  editable?: boolean
  bounds?: { min?: number | null; max?: number | null } | null
  expression?: ExpressionField
  distribution?: Distribution
  /** Sample redraw trigger. Absent = once per realization; `mode: 'always'` = every timestep. */
  resampling?: { mode?: string; period?: Quantity } | null
  table?: {
    x: number[]
    y: number[]
    z?: number[][]
    x_unit?: string | null
    y_unit?: string | null
    z_unit?: string | null
    interpolation?: string
    extrapolation?: string
    log_result?: boolean
  }
  interpolation?: string
  timestamps?: number[]
  time_unit?: string | null
  input?: string | null
  initial?: Quantity
  window?: number
  statistic?: string
  // stock payloads
  initial_value?: Quantity
  rate?: ExpressionField | Quantity
  /** Compound-growth rate applied to the current level each step (trait: compound_growth);
   *  composes additively with inflows/outflows. e.g. interest on a balance. */
  return_rate?: Quantity
  inflows?: string[]
  outflows?: string[]
  floor?: Quantity
  min_value?: number
  capacity?: Quantity
  overflow_target?: string
  // pid
  setpoint?: Quantity | ExpressionField
  kp?: number; ki?: number; kd?: number
  output_min?: number | null; output_max?: number | null; deadband?: number
  // hysteresis
  high_threshold?: Quantity; low_threshold?: Quantity
  output_above?: Quantity; output_below?: Quantity
  // status / milestone triggers
  set?: unknown; reset?: unknown; trigger?: unknown
  // bookkeeping the engine ignores but we round-trip
  inputs?: string[]
  save_results?: { time_history?: boolean; final_value?: boolean }
  // catch-all so unknown fields survive edits
  [k: string]: unknown
}

/** Node position + collapse state, kept in the model's `view` block (engine ignores it, §13.3). */
export interface NodeView { x: number; y: number }

export interface ViewBlock {
  positions?: Record<string, NodeView>
  collapsed?: string[]
  /** Marks the document as authored/edited with this tool (provenance, §17.4). */
  authored?: boolean
  [k: string]: unknown
}

/** The whole model document as edited. Extra top-level fields (source, dimensions, …) are
 *  preserved via the index signature so nothing is lost on save. */
export interface ModelDoc {
  wasim_version: string
  simulation_settings: SimulationSettings
  containers?: ContainerDef[]
  elements: FlatElement[]
  view?: ViewBlock
  [k: string]: unknown
}

export type ModelFormat = 'v1' | 'v2'

export function detectFormat(doc: ModelDoc): ModelFormat {
  const first = doc.elements?.[0]
  // An empty document is a new/authored model → default to the v2-native flat schema.
  if (!first) return 'v2'
  return first.primitive !== undefined ? 'v2' : 'v1'
}

/** Human title for an element's kind, from summary primitive/value_rule or flat fields.
 *  Accepts both the flat element and the engine `ElementSummary` (value_rule may be null). */
export function kindLabel(el: { primitive?: string | null; value_rule?: string | null; type?: string | null }): string {
  if (el.primitive) return el.value_rule ? `${el.primitive}/${el.value_rule}` : el.primitive
  return el.type ?? 'element'
}
