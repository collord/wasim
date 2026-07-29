// Pure transforms over the canonical `ModelDoc` (spec §13.4: "each edit is a pure
// transform of the canonical document"). Every editor calls one of these; the store wraps
// them in a command stack for undo/redo. None mutate their argument — they return a new doc.

import type { FlatElement, ModelDoc, ModelFormat, NodeView } from './schema'
import { detectFormat } from './schema'
import { printAst, refsOf, type Ast } from './ast'

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

/** Recompute an element's `inputs` from all ASTs it carries (expression + rate). Keeps the
 *  dependency graph honest after an expression edit. */
export function recomputeInputs(el: FlatElement): void {
  const refs = new Set<string>()
  const collect = (ef?: { ast?: Ast } | unknown) => {
    const ast = (ef as { ast?: Ast })?.ast
    if (ast) refsOf(ast, refs)
  }
  collect(el.expression)
  if (el.rate && typeof el.rate === 'object' && 'ast' in el.rate) collect(el.rate)
  // Stocks also list inflows/outflows as inputs.
  const explicit = [...(el.inflows ?? []), ...(el.outflows ?? []), el.input].filter(Boolean) as string[]
  const merged = new Set<string>([...refs, ...explicit])
  el.inputs = [...merged]
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

// ── Simulation settings ──────────────────────────────────────────────────────────

export function updateSettings(doc: ModelDoc, patch: Partial<ModelDoc['simulation_settings']>): ModelDoc {
  const next = clone(doc)
  next.simulation_settings = { ...next.simulation_settings, ...patch }
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
