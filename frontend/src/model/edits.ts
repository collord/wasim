// Pure transforms over the canonical `ModelDoc` (spec §13.4: "each edit is a pure
// transform of the canonical document"). Every editor calls one of these; the store wraps
// them in a command stack for undo/redo. None mutate their argument — they return a new doc.

import type { FlatElement, ModelDoc, ModelFormat, NodeView } from './schema'
import { detectFormat } from './schema'
import { printAst, type Ast } from './ast'

// ── Clone / lookup ──────────────────────────────────────────────────────────────

const clone = <T>(v: T): T => (typeof structuredClone === 'function' ? structuredClone(v) : JSON.parse(JSON.stringify(v)))

export function findElement(doc: ModelDoc, id: string): FlatElement | undefined {
  return doc.elements.find((e) => e.id === id)
}

// ── Id generation (slugified, unique) ───────────────────────────────────────────

export function slugify(name: string): string {
  const s = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '')
  return s || 'element'
}

export function uniqueId(doc: ModelDoc, base: string): string {
  const taken = new Set(doc.elements.map((e) => e.id))
  ;(doc.containers ?? []).forEach((c) => taken.add(c.id))
  if (!taken.has(base)) return base
  let i = 2
  while (taken.has(`${base}_${i}`)) i++
  return `${base}_${i}`
}

// ── Element mutations ────────────────────────────────────────────────────────────

/** Replace element `id` with a patched copy (shallow-merge of `patch`). */
export function updateElement(doc: ModelDoc, id: string, patch: Partial<FlatElement>): ModelDoc {
  const next = clone(doc)
  const el = next.elements.find((e) => e.id === id)
  if (!el) return doc
  Object.assign(el, patch)
  return next
}

/** Apply a mutator to element `id` (for edits that need the current element). */
export function mutateElement(doc: ModelDoc, id: string, fn: (el: FlatElement) => void): ModelDoc {
  const next = clone(doc)
  const el = next.elements.find((e) => e.id === id)
  if (!el) return doc
  fn(el)
  return next
}

/** Replace element `id` wholesale with a new element object (the raw-JSON editor escape hatch,
 *  §14). Unlike `updateElement` (shallow-merge) this can also *remove* fields. */
export function replaceElement(doc: ModelDoc, id: string, el: FlatElement): ModelDoc {
  const idx = doc.elements.findIndex((e) => e.id === id)
  if (idx === -1) return doc
  const next = clone(doc)
  next.elements[idx] = clone(el)
  return next
}

export function addElement(doc: ModelDoc, el: FlatElement, pos?: NodeView): ModelDoc {
  const next = clone(doc)
  next.elements.push(clone(el))
  if (pos) setPositionInPlace(next, el.id, pos)
  return next
}

// The element fields that carry a single element id (not in an AST): the lag `input`, an
// event's `source` (on_event trigger). `effects[].target` is handled separately (nested).
const ID_SCALAR_FIELDS = ['input', 'source'] as const
const ID_LIST_FIELDS = ['inputs', 'inflows', 'outflows'] as const

/** Rewrite every AST `ref` node's `element_id` (oldId → newId) anywhere inside `obj` — the
 *  expression/rate ASTs, an event's trigger/set/reset conditions, effect `change` ASTs, etc.
 *  Deep, but only mutates `{ op: 'ref', element_id }` nodes, so it never mangles unrelated
 *  strings (names, units, display text). Returns whether anything changed. Mutates in place. */
function renameAstRefs(obj: unknown, oldId: string, newId: string): boolean {
  if (!obj || typeof obj !== 'object') return false
  if (Array.isArray(obj)) {
    let changed = false
    for (const v of obj) changed = renameAstRefs(v, oldId, newId) || changed
    return changed
  }
  const rec = obj as Record<string, unknown>
  let changed = false
  if (rec.op === 'ref' && rec.element_id === oldId) { rec.element_id = newId; changed = true }
  for (const v of Object.values(rec)) changed = renameAstRefs(v, oldId, newId) || changed
  return changed
}

/** Re-derive the cached `display` string of an expression/rate field after its AST changed. */
function refreshDisplay(ef: unknown): void {
  const e = ef as { ast?: Ast; display?: string }
  if (e && typeof e === 'object' && e.ast) e.display = printAst(e.ast)
}

/** Delete an element and scrub dangling references to it (id lists, scalar id fields, event
 *  effect targets, view). References buried in ASTs are left in place — they become dangling
 *  refs that the reconcile/validate loop reports, which is the honest signal (you cannot
 *  meaningfully rewrite `a + deleted` on delete). */
export function deleteElement(doc: ModelDoc, id: string): ModelDoc {
  const next = clone(doc)
  next.elements = next.elements.filter((e) => e.id !== id)
  for (const e of next.elements) {
    const anyE = e as Record<string, unknown>
    for (const key of ID_LIST_FIELDS) {
      if (Array.isArray(anyE[key])) anyE[key] = (anyE[key] as string[]).filter((r) => r !== id)
    }
    for (const key of ID_SCALAR_FIELDS) {
      if (anyE[key] === id) anyE[key] = null
    }
    if (Array.isArray(anyE.effects)) {
      anyE.effects = (anyE.effects as Array<{ target?: string }>).filter((eff) => !eff || eff.target !== id)
    }
  }
  if (next.view?.positions) delete next.view.positions[id]
  return next
}

/** Rename an element's id, rewriting every reference: id lists (inputs/inflows/outflows),
 *  scalar id fields (lag `input`, event `source`), event `effects[].target`, references buried
 *  in ASTs (expression, rate, trigger/condition, effect changes), and the view positions. */
export function renameId(doc: ModelDoc, oldId: string, newId: string): ModelDoc {
  const next = clone(doc)
  for (const e of next.elements) {
    const anyE = e as Record<string, unknown>
    if (e.id === oldId) e.id = newId
    for (const key of ID_LIST_FIELDS) {
      if (Array.isArray(anyE[key])) anyE[key] = (anyE[key] as string[]).map((r) => (r === oldId ? newId : r))
    }
    for (const key of ID_SCALAR_FIELDS) {
      if (anyE[key] === oldId) anyE[key] = newId
    }
    if (Array.isArray(anyE.effects)) {
      for (const eff of anyE.effects as Array<{ target?: string }>) {
        if (eff && eff.target === oldId) eff.target = newId
      }
    }
    // References inside any AST the element carries (expression, rate, trigger, effects…).
    renameAstRefs(e, oldId, newId)
    // Keep the cached pretty-printed formulas honest.
    refreshDisplay(anyE.expression)
    refreshDisplay(anyE.rate)
  }
  if (next.view?.positions?.[oldId]) {
    next.view.positions[newId] = next.view.positions[oldId]
    delete next.view.positions[oldId]
  }
  return next
}

/** Re-parent an element into a container (or null for root) — `container` is authoritative. */
export function setContainer(doc: ModelDoc, id: string, container: string | null): ModelDoc {
  return updateElement(doc, id, { container })
}

/** Duplicate an element as a plain copy (parallel to GoldSim Clone-as-copy, §2.4). The copy
 *  gets a fresh unique id and a `(copy)` name; its position is offset so it doesn't overlap.
 *  Returns [nextDoc, newId]. */
export function duplicateElement(doc: ModelDoc, id: string): [ModelDoc, string] {
  const src = findElement(doc, id)
  if (!src) return [doc, id]
  const newId = uniqueId(doc, `${id}_copy`)
  const copy: FlatElement = { ...clone(src), id: newId, name: `${src.name} (copy)` }
  let next = addElement(doc, copy)
  const p = doc.view?.positions?.[id]
  if (p) next = setPosition(next, newId, { x: p.x + 40, y: p.y + 40 })
  return [next, newId]
}

// ── Expression edits (write ast + display + recompute inputs) ────────────────────

/** Which downstream reference fields an element carries, given its kind, so we can keep
 *  `inputs` in sync with the expression's refs (the influence graph, §2.2). */
export function setExpression(doc: ModelDoc, id: string, ast: Ast, field: 'expression' | 'rate' = 'expression'): ModelDoc {
  return mutateElement(doc, id, (el) => {
    const ef = { ast, display: printAst(ast) }
    if (field === 'rate') el.rate = ef
    else el.expression = ef
    recomputeInputs(el)
  })
}

/** Deep-collect every AST `ref` element_id inside `obj`, walking all nested objects/arrays
 *  (not just AST-shaped ones), so it reaches wrapped ASTs like `trigger.condition.ast` and
 *  `effects[].change.ast`. Only reads `{ op: 'ref', element_id }` nodes — plain id strings
 *  (effect targets) are not treated as inputs. */
function collectRefsDeep(obj: unknown, acc: Set<string>): void {
  if (!obj || typeof obj !== 'object') return
  if (Array.isArray(obj)) { for (const v of obj) collectRefsDeep(v, acc); return }
  const rec = obj as Record<string, unknown>
  if (rec.op === 'ref' && typeof rec.element_id === 'string') acc.add(rec.element_id)
  for (const v of Object.values(rec)) collectRefsDeep(v, acc)
}

/** Recompute an element's `inputs` from every AST it carries — expression, stock rate, event
 *  trigger/condition, status set/reset, and effect `change` values — plus explicit
 *  flow/lag fields. Keeps the dependency graph + influence arrows honest after any edit. */
export function recomputeInputs(el: FlatElement): void {
  const refs = new Set<string>()
  for (const field of [el.expression, el.rate, el.trigger, el.set, el.reset, el.effects]) {
    collectRefsDeep(field, refs)
  }
  const explicit = [...(el.inflows ?? []), ...(el.outflows ?? []), el.input].filter(Boolean) as string[]
  el.inputs = [...new Set<string>([...refs, ...explicit])]
}

// ── View block (positions / collapse) ────────────────────────────────────────────

function ensureView(doc: ModelDoc): ModelDoc['view'] & object {
  if (!doc.view) doc.view = {}
  if (!doc.view.positions) doc.view.positions = {}
  doc.view.authored = true
  return doc.view
}

function setPositionInPlace(doc: ModelDoc, id: string, pos: NodeView): void {
  const v = ensureView(doc)
  v.positions![id] = pos
}

export function setPosition(doc: ModelDoc, id: string, pos: NodeView): ModelDoc {
  const next = clone(doc)
  setPositionInPlace(next, id, pos)
  return next
}

/** Bulk-write positions (used by "Tidy layout" after Dagre). */
export function setPositions(doc: ModelDoc, positions: Record<string, NodeView>): ModelDoc {
  const next = clone(doc)
  const v = ensureView(next)
  v.positions = { ...v.positions, ...positions }
  return next
}

/** Set the active lens tag on the view block (engine-ignored, like positions). A pure view edit
 *  — no reconcile needed. See `WASIM_LENS_IMPLEMENTATION_PLAN.md` Part A. */
export function setLens(doc: ModelDoc, lens: string): ModelDoc {
  const next = clone(doc)
  ensureView(next).lens = lens
  return next
}

// ── Simulation settings ──────────────────────────────────────────────────────────

export function updateSettings(doc: ModelDoc, patch: Partial<ModelDoc['simulation_settings']>): ModelDoc {
  const next = clone(doc)
  next.simulation_settings = { ...next.simulation_settings, ...patch }
  return next
}

// ── Dimensions (declared array axes, spec §7) ─────────────────────────────────────

export function setDimensions(doc: ModelDoc, dimensions: ModelDoc['dimensions']): ModelDoc {
  const next = clone(doc)
  if (dimensions && dimensions.length) next.dimensions = dimensions
  else delete next.dimensions
  return next
}

// ── Element scaffolds (palette insert, spec §3) ──────────────────────────────────

export interface PaletteEntry {
  key: string
  label: string
  group: string
  /** Legacy type shown by the canvas/icons. */
  iconType: string
  make: (id: string, name: string, fmt: ModelFormat) => FlatElement
}

const q = (value: number, unit = '1') => ({ value, unit })

/** Tag an element with the right discriminator for the document's format. */
function withKind(el: FlatElement, fmt: ModelFormat, v1Type: string, primitive: string, valueRule?: string): FlatElement {
  if (fmt === 'v2') {
    el.primitive = primitive
    if (valueRule) el.value_rule = valueRule
  } else {
    el.type = v1Type
  }
  return el
}

export const PALETTE: PaletteEntry[] = [
  {
    key: 'constant', label: 'Constant', group: 'Inputs', iconType: 'constant',
    make: (id, name, fmt) => withKind(
      { id, name, value: q(0), editable: true, bounds: { min: 0, max: 1 } }, fmt, 'constant', 'node', 'fixed'),
  },
  {
    key: 'stochastic', label: 'Stochastic', group: 'Inputs', iconType: 'random_variable',
    make: (id, name, fmt) => withKind(
      { id, name, distribution: { family: 'normal', parameters: { mean: q(0), stddev: q(1) } } as any },
      fmt, 'random_variable', 'node', 'sample'),
  },
  {
    key: 'timeseries', label: 'Time Series', group: 'Inputs', iconType: 'timeseries',
    make: (id, name, fmt) => withKind(
      { id, name, timestamps: [0, 1], values: [0, 0], time_unit: 's', interpolation: 'linear' },
      fmt, 'timeseries', 'node', 'series'),
  },
  {
    key: 'lookup', label: 'Lookup Table', group: 'Inputs', iconType: 'lookup',
    make: (id, name, fmt) => withKind(
      { id, name, table: { x: [0, 1], y: [0, 1], interpolation: 'linear' } },
      fmt, 'lookup', 'node', 'lookup'),
  },
  {
    key: 'expression', label: 'Expression', group: 'Functions', iconType: 'expression',
    make: (id, name, fmt) => withKind(
      { id, name, expression: { ast: { op: 'literal', value: 0 }, display: '0' }, inputs: [] },
      fmt, 'expression', 'node', 'expression'),
  },
  {
    key: 'lag', label: 'Previous Value', group: 'Functions', iconType: 'delay',
    make: (id, name, fmt) => withKind(
      { id, name, input: null, initial: q(0) }, fmt, 'delay', 'node', 'lag'),
  },
  {
    key: 'stock', label: 'Stock / Reservoir', group: 'Stocks', iconType: 'accumulator',
    // No seeded `rate`: the engine treats rate and inflows/outflows as either-or (a present
    // `rate` shadows flows, engine_v2 §net-rate). Defaulting to the flow path lets a wired
    // inflow drive the stock; typing a net-rate expression in the inspector switches paths.
    make: (id, name, fmt) => withKind(
      { id, name, initial_value: q(0), inflows: [], outflows: [] },
      fmt, 'accumulator', 'stock'),
  },
  {
    // v2-native failure state machine (spec §5.5). Scaffolds a `condition`-basis FSM — fails
    // when its condition (e.g. a damage stock ≥ threshold) becomes true; run-to-failure by
    // default. Output is 0 = operating, 1 = failed. (Events are v2-only.)
    key: 'event', label: 'Failure / Event', group: 'Reliability', iconType: 'event',
    make: (id, name) => ({
      id, name, primitive: 'event',
      trigger: { mode: 'on_condition', condition: { ast: { op: 'literal', value: 0 }, display: '0' } },
      failure_process: { basis: 'condition', repair: { policy: 'none' } },
      effects: [],
      inputs: [],
      save_results: { time_history: true, final_value: true },
    }),
  },
  {
    // v2-native boolean logic gate (spec §gate). Composes referenced elements' states with
    // AND / OR / at-least-k (NVote); a leaf is "active" when its value > 0. Output 0/1.
    key: 'gate', label: 'Logic Gate', group: 'Reliability', iconType: 'event',
    make: (id, name) => ({
      id, name, primitive: 'gate', semantics: 'success',
      root: { op: 'n_vote', threshold: 1, children: [] },
      inputs: [],
      save_results: { time_history: true, final_value: true },
    }),
  },

  // ── v2-only constructs surfaced by the General lens's §5 taxonomy (manifest spec §5). ──
  // Each is a node value_rule or a primitive whose minimal scaffold parses standalone (verified
  // against the v2 parser). No structured inspector editor yet — they route to the raw-JSON
  // escape hatch (registry `editor: "raw"`). v2-only, so no `withKind`/v1 type.
  {
    // Rolling-window statistic over an input signal (mean/min/max/sum/ema). `input` defaults to
    // an empty (0.0) signal until wired; `window: 0` is an expanding window.
    key: 'filter', label: 'Smoothing / Filter', group: 'Functions', iconType: 'expression',
    make: (id, name) => ({ id, name, primitive: 'node', value_rule: 'filter', statistic: 'mean' }),
  },
  {
    // Proportional-integral-derivative controller. `input` is the controlled signal (wire it in
    // the inspector); `setpoint` is a scalar target by default.
    key: 'pid', label: 'PID Controller', group: 'Functions', iconType: 'expression',
    make: (id, name) => ({
      id, name, primitive: 'node', value_rule: 'pid',
      input: '', setpoint: { value: 0, unit: '1' },
    }),
  },
  {
    // A value evaluated once at the end of the run (a terminal reduction).
    key: 'terminal_expression', label: 'Terminal Value', group: 'Functions', iconType: 'expression',
    make: (id, name) => ({
      id, name, primitive: 'node', value_rule: 'terminal_expression',
      expression: { ast: { op: 'literal', value: 0 }, display: '0' },
    }),
  },
  {
    // Convolves an input with an impulse response (inline response by default).
    key: 'convolution', label: 'Convolution', group: 'accumulate', iconType: 'delay',
    make: (id, name) => ({
      id, name, primitive: 'node', value_rule: 'convolution',
      input: '', response: { times: [0], values: [0] },
    }),
  },
  {
    // A material-delay queue: an input flows in and emerges after `delay_time` (conveyor by default).
    key: 'queue', label: 'Queue', group: 'accumulate', iconType: 'delay',
    make: (id, name) => ({
      id, name, primitive: 'node', value_rule: 'queue',
      input: '', delay_time: { value: 1, unit: '1' },
    }),
  },
  {
    // A latch: set/reset triggers flip it on/off. Empty triggers never fire until configured.
    key: 'status', label: 'Status (latch)', group: 'events', iconType: 'event',
    make: (id, name) => ({ id, name, primitive: 'node', value_rule: 'status', set: {}, reset: {} }),
  },
  {
    // Fires once when its trigger condition first becomes true (a one-shot marker).
    key: 'milestone', label: 'Milestone', group: 'events', iconType: 'event',
    make: (id, name) => ({ id, name, primitive: 'node', value_rule: 'milestone', trigger: {} }),
  },
  {
    // Schmitt-trigger hysteresis: switches between two outputs at high/low thresholds.
    key: 'hysteresis', label: 'Hysteresis', group: 'events', iconType: 'event',
    make: (id, name) => ({
      id, name, primitive: 'node', value_rule: 'hysteresis',
      input: '',
      high_threshold: { value: 1, unit: '1' }, low_threshold: { value: 0, unit: '1' },
      output_above: { value: 1, unit: '1' }, output_below: { value: 0, unit: '1' },
    }),
  },
  {
    // Markov chain: states, an initial state, a row-stochastic transition matrix, per-state outputs.
    key: 'markov', label: 'Markov Chain', group: 'processes', iconType: 'random_variable',
    make: (id, name) => ({
      id, name, primitive: 'node', value_rule: 'markov',
      states: ['ok', 'bad'], initial_state: 'ok',
      transition_matrix: [[0.9, 0.1], [0, 1]], output_values: [0, 1],
    }),
  },
  {
    // Geometric Brownian motion / mean-reverting process (arithmetic-drift GBM by default).
    key: 'process', label: 'Stochastic Process (GBM/OU)', group: 'processes', iconType: 'random_variable',
    make: (id, name) => ({
      id, name, primitive: 'node', value_rule: 'process',
      process: { family: 'gbm', mean_type: 'arithmetic', mean: { value: 0, unit: '1' }, stddev: { value: 0, unit: '1' } },
    }),
  },
  {
    // A finite pool consumed/replenished by links (initial scalar level).
    key: 'resource', label: 'Resource', group: 'transport', iconType: 'accumulator',
    make: (id, name) => ({ id, name, primitive: 'resource', initial_value: { value: 0, unit: '1' } }),
  },
  {
    // A well-mixed compartment holding species in media; links transfer mass between cells.
    key: 'cell', label: 'Cell', group: 'transport', iconType: 'accumulator',
    make: (id, name) => ({ id, name, primitive: 'cell', volume: { value: 1, unit: 'm^3' } }),
  },
  {
    // A transported species (optionally with decay / molecular weight).
    key: 'species', label: 'Species', group: 'transport', iconType: 'random_variable',
    make: (id, name) => ({ id, name, primitive: 'species', molecular_weight: { value: 0.001, unit: 'kg/mol' } }),
  },
  {
    // A medium (phase) that species partition into. `phase` is required.
    key: 'medium', label: 'Medium', group: 'transport', iconType: 'constant',
    make: (id, name) => ({ id, name, primitive: 'medium', phase: 'fluid' }),
  },
  {
    // A transfer link between cells/resources. Source/target wired in the inspector.
    key: 'link', label: 'Link (transfer)', group: 'transport', iconType: 'valve',
    make: (id, name) => ({ id, name, primitive: 'link' }),
  },
]

// ── New / blank model scaffold (spec §13.4) ──────────────────────────────────────

export function blankModel(): ModelDoc {
  return {
    wasim_version: '0.1.0',
    simulation_settings: {
      duration: { value: 100, unit: 's' },
      timestep: { value: 1, unit: 's' },
      n_realizations: 1,
      sampling_method: 'monte_carlo',
      seed: 42,
    },
    containers: [],
    elements: [],
    view: { authored: true, positions: {} },
  }
}

// ── Serialization (Save, §13.4) ──────────────────────────────────────────────────

/** Pretty-print for saving. The `view` block is kept (engine ignores it, §13.3). */
export function serializeModel(doc: ModelDoc): string {
  return JSON.stringify(doc, null, 2)
}

export { detectFormat }
