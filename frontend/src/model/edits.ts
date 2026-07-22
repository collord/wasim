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

export function addElement(doc: ModelDoc, el: FlatElement, pos?: NodeView): ModelDoc {
  const next = clone(doc)
  next.elements.push(clone(el))
  if (pos) setPositionInPlace(next, el.id, pos)
  return next
}

/** Delete an element and scrub dangling references to it (from `inputs` lists + view). */
export function deleteElement(doc: ModelDoc, id: string): ModelDoc {
  const next = clone(doc)
  next.elements = next.elements.filter((e) => e.id !== id)
  for (const e of next.elements) {
    if (Array.isArray(e.inputs)) e.inputs = e.inputs.filter((r) => r !== id)
    if (Array.isArray(e.inflows)) e.inflows = e.inflows.filter((r) => r !== id)
    if (Array.isArray(e.outflows)) e.outflows = e.outflows.filter((r) => r !== id)
  }
  if (next.view?.positions) delete next.view.positions[id]
  return next
}

/** Rename an element's id, rewriting every reference (inputs, inflows/outflows, view). */
export function renameId(doc: ModelDoc, oldId: string, newId: string): ModelDoc {
  const next = clone(doc)
  for (const e of next.elements) {
    if (e.id === oldId) e.id = newId
    if (Array.isArray(e.inputs)) e.inputs = e.inputs.map((r) => (r === oldId ? newId : r))
    if (Array.isArray(e.inflows)) e.inflows = e.inflows.map((r) => (r === oldId ? newId : r))
    if (Array.isArray(e.outflows)) e.outflows = e.outflows.map((r) => (r === oldId ? newId : r))
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
  if (merged.size) el.inputs = [...merged]
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
    make: (id, name, fmt) => withKind(
      { id, name, initial_value: q(0), rate: { ast: { op: 'literal', value: 0 }, display: '0' } as any, inflows: [], outflows: [] },
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
