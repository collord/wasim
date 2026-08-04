import type { FlatElement, ModelDoc } from '../../model/schema'

/**
 * Structural predicates & accessors over the flat model — the raw material the lens *hint*
 * (`lensHint.ts`) and computed roles (`roles.ts`) key off. Everything here reads only fields that
 * exist in the emitted/authored JSON (`primitive`, `value_rule`, `inflows`/`outflows`, `inputs`,
 * gate `root`, event `trigger`, array `outputs[].dimensions`, the top-level `optimization` block).
 * No `lens_role`, no `source_type` — see `EMITTER_LENS_TARGETING.md` §2. FlatElement's index
 * signature carries the untyped fields (`root`, `trigger`, `outputs`, …); the small typed views
 * below localize the casts so the rest of the module stays clean.
 */

export const isStock = (e: FlatElement): boolean => e.primitive === 'stock'
export const isGate = (e: FlatElement): boolean => e.primitive === 'gate'
export const isEvent = (e: FlatElement): boolean => e.primitive === 'event'
export const isStatus = (e: FlatElement): boolean => e.primitive === 'node' && e.value_rule === 'status'
export const isPid = (e: FlatElement): boolean => e.primitive === 'node' && e.value_rule === 'pid'
/** A stochastic facet — a distribution draw or a stochastic process (GBM/OU). */
export const isStochastic = (e: FlatElement): boolean =>
  e.value_rule === 'sample' || e.value_rule === 'process' || e.primitive === 'process'

/** Ids referenced by any stock's `inflows`/`outflows` — the flow/transition set. */
export function flowIds(elements: FlatElement[]): Set<string> {
  const s = new Set<string>()
  for (const e of elements) {
    for (const f of e.inflows ?? []) s.add(f)
    for (const f of e.outflows ?? []) s.add(f)
  }
  return s
}

/** A gate's child references (fault-tree inputs). Reads the reliability gate `root` tree
 *  (`{op, children:[{op:'reference', reference}|…]}`) and the flat `inputs[]` fallback. */
export function gateChildIds(gate: FlatElement): string[] {
  const out: string[] = []
  const root = gate.root as { children?: unknown[] } | undefined
  const walk = (n: unknown): void => {
    if (!n || typeof n !== 'object') return
    const node = n as { op?: string; reference?: string; children?: unknown[] }
    if (node.op === 'reference' && typeof node.reference === 'string') out.push(node.reference)
    for (const c of node.children ?? []) walk(c)
  }
  walk(root)
  for (const id of gate.inputs ?? []) out.push(id)
  return out
}

/** Every element id that fans into a fault-tree gate — the reliability *component* set. This is
 *  the discriminator that separates a reliability container (events under gates) from a
 *  discrete-event container (events in `on_event` chains). */
export function eventsFeedingGates(elements: FlatElement[]): Set<string> {
  const s = new Set<string>()
  for (const e of elements) {
    if (!isGate(e)) continue
    for (const id of gateChildIds(e)) s.add(id)
  }
  return s
}

/** An event/status element's trigger, if any (`{mode, source?, condition?}`). */
export function triggerOf(e: FlatElement): { mode?: string; source?: string } | undefined {
  const t = e.trigger as { mode?: string; source?: string } | undefined
  return t && typeof t === 'object' ? t : undefined
}

/** Does this element fire off *another event firing* (`trigger.mode === 'on_event'`)? That edge —
 *  the discrete-event chain link — lives in the field, not in `connections[]`. */
export function isOnEventTriggered(e: FlatElement): boolean {
  return triggerOf(e)?.mode === 'on_event'
}

/** A dimensioned element's primary-output axis ids (empty/undefined ⇒ scalar). Mirrors
 *  `behaviors/metapop.ts` `outDims`. */
export function outDims(e: FlatElement): string[] | undefined {
  const outs = (e as { outputs?: { dimensions?: string[] }[] }).outputs
  return outs?.[0]?.dimensions
}

/** Is this element a coupling *matrix* — a 2-D array over two equal-size axes (a square adjacency /
 *  mixing network)? The structural signature that lifts a metapop out of plain stock-flow. */
export function isCouplingMatrix(e: FlatElement, doc: ModelDoc): boolean {
  const d = outDims(e)
  if (!d || d.length !== 2) return false
  const size = (id: string) => doc.dimensions?.find((x) => x.id === id)?.size
  const [a, b] = [size(d[0]), size(d[1])]
  return a != null && b != null && a === b
}

/** The top-level optimization study, if present (`{variables:[{element_id}], objective:{...}}`). */
export function optimizationOf(doc: ModelDoc): { variables?: { element_id?: string }[]; objective?: unknown } | undefined {
  const o = (doc as { optimization?: unknown }).optimization
  return o && typeof o === 'object' ? (o as { variables?: { element_id?: string }[]; objective?: unknown }) : undefined
}

/** Direct structural dependencies of an element (expression `inputs`, a lag's `input`, stock
 *  flows). Used for the metapop reachability checks. */
export function deps(e: FlatElement): string[] {
  const d = [...(e.inputs ?? []), ...(e.inflows ?? []), ...(e.outflows ?? [])]
  if (typeof e.input === 'string') d.push(e.input)
  return d
}

/** Does the dependency closure of `startId` contain an element satisfying `pred`? Cycle-safe BFS
 *  (metapop graphs close cycles through `lag`). Mirrors `behaviors/metapop.ts` `reaches`. */
export function reaches(doc: ModelDoc, startId: string, pred: (e: FlatElement) => boolean): boolean {
  const byId = new Map(doc.elements.map((e) => [e.id, e]))
  const seen = new Set<string>([startId])
  const queue = [...deps(byId.get(startId) ?? ({} as FlatElement))]
  while (queue.length) {
    const id = queue.shift()!
    if (seen.has(id)) continue
    seen.add(id)
    const e = byId.get(id)
    if (!e) continue
    if (pred(e)) return true
    queue.push(...deps(e))
  }
  return false
}
