import { mutateElement } from '../../model/edits'
import type { FlatElement, ModelDoc } from '../../model/schema'
import type { ModelSummary, SimulationResults } from '../../types'
import type { Issue } from '../../worker/protocol'
import type { LensReadout } from '../types'
import type { LensBehavior } from '../manifest-types'
import { fmtNum, pct } from './format'

/**
 * Metapopulation lens behavior — the network-epidemiology twin of the stock-flow SFC checks
 * (ABM_ON_WASIM.md §5.3). Compartments are stocks over a patch axis; transitions are flows; a
 * coupling matrix `W` and a neighbour-mediated force of infection add the two nouns that make it a
 * network model rather than a well-mixed one. All checks key off `lens_role`, `inflows`/`outflows`,
 * `inputs`, and (for array shape) `outputs[].dimensions` — structural fields, no expression eval.
 */

type Roled = FlatElement & { lens_role?: string }
const roleIs = (e: FlatElement, r: string) => (e as Roled).lens_role === r

/** A dimensioned element's primary-output axis ids (empty/undefined ⇒ scalar). */
function outDims(e: FlatElement): string[] | undefined {
  const outs = (e as { outputs?: { dimensions?: string[] }[] }).outputs
  return outs?.[0]?.dimensions
}
const dimSize = (doc: ModelDoc, id: string) => doc.dimensions?.find((d) => d.id === id)?.size

/** Direct dependencies of an element: expression inputs, a lag's input, and stock inflows/outflows. */
function deps(e: FlatElement): string[] {
  const d = [...(e.inputs ?? []), ...(e.inflows ?? []), ...(e.outflows ?? [])]
  if (typeof e.input === 'string') d.push(e.input)
  return d
}

/** Does the dependency closure of `startId` contain an element satisfying `pred`? (cycle-safe BFS —
 *  the metapop graph closes cycles through `lag`, so a visited set is required.) */
function reaches(doc: ModelDoc, startId: string, pred: (e: FlatElement) => boolean): boolean {
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

function metapopInvariants(_summary: ModelSummary, doc: ModelDoc): Issue[] {
  const issues: Issue[] = []
  const compartments = doc.elements.filter((e) => roleIs(e, 'compartment'))
  if (doc.elements.length > 0 && compartments.length === 0) {
    issues.push({ severity: 'warning', message: 'No compartments — add a Compartment (S / I / R) to model a metapopulation.' })
  }

  // Population conservation: a transition must move population between compartments; an add with no
  // matching subtract is a leak.
  const inflowOf = new Set<string>()
  const outflowOf = new Set<string>()
  for (const e of doc.elements) {
    for (const f of e.inflows ?? []) inflowOf.add(f)
    for (const f of e.outflows ?? []) outflowOf.add(f)
  }
  for (const t of doc.elements) {
    if (!roleIs(t, 'transition')) continue
    const isIn = inflowOf.has(t.id)
    const isOut = outflowOf.has(t.id)
    if (!isIn && !isOut) {
      issues.push({ severity: 'warning', message: `Transition "${t.name}" isn't connected to any compartment — a transition must move population between compartments (conservation).`, element_id: t.id })
    } else if (isIn && !isOut) {
      issues.push({ severity: 'warning', message: `Transition "${t.name}" adds to a compartment without draining another — population leaks in. Wire it out of a source compartment too.`, element_id: t.id })
    }
  }

  // A force-of-infection (mixing) term must combine a coupling network with a prevalence
  // compartment — else it is mean-field, not a network model.
  for (const m of doc.elements) {
    if (!roleIs(m, 'mixing')) continue
    const hasCoupling = reaches(doc, m.id, (e) => roleIs(e, 'coupling'))
    const hasPrevalence = reaches(doc, m.id, (e) => roleIs(e, 'compartment'))
    if (!hasCoupling || !hasPrevalence) {
      issues.push({ severity: 'warning', message: `Force of infection "${m.name}" must combine a coupling network with a prevalence compartment — otherwise it's mean-field, not a network model.`, element_id: m.id })
    }
  }

  // Coupling well-formedness: W is a square matrix over two axes of the same patch set.
  for (const c of doc.elements) {
    if (!roleIs(c, 'coupling')) continue
    const d = outDims(c)
    if (!d || d.length === 0) continue // scalar / unshaped coupling — nothing to check
    if (d.length !== 2) {
      issues.push({ severity: 'warning', message: `Coupling network "${c.name}" must be a 2-D matrix over two axes of the patch set (${d.length === 1 ? 'it is a vector' : `it has ${d.length} axes`}).`, element_id: c.id })
      continue
    }
    const [a, b] = [dimSize(doc, d[0]), dimSize(doc, d[1])]
    if (a != null && b != null && a !== b) {
      issues.push({ severity: 'warning', message: `Coupling network "${c.name}" must be square (both axes the same patch set): got ${a}×${b}.`, element_id: c.id })
    }
  }

  // Transposed reduction: the force of infection must be reduced over the neighbour axis (leaving one
  // value per home patch). By the engine's axis order the coupling's first axis is "home", the
  // second "neighbour"; a mixing term still carrying the neighbour axis summed the wrong one.
  for (const m of doc.elements) {
    if (!roleIs(m, 'mixing')) continue
    const md = outDims(m)
    if (!md || md.length === 0) continue
    const coupling = doc.elements.find(
      (x) => roleIs(x, 'coupling') && outDims(x)?.length === 2 && reaches(doc, m.id, (y) => y.id === x.id),
    )
    if (!coupling) continue
    const [home, neighbour] = outDims(coupling)!
    if (md.includes(neighbour) && !md.includes(home)) {
      issues.push({ severity: 'warning', message: `Force of infection "${m.name}" reduced the patch axis, not the neighbour axis (a transposed reduction) — sum W × prevalence over neighbours to get one value per home patch.`, element_id: m.id })
    } else if (md.includes(home) && md.includes(neighbour)) {
      issues.push({ severity: 'warning', message: `Force of infection "${m.name}" isn't reduced over the neighbour axis — sum W × prevalence over the neighbour axis ("${neighbour}").`, element_id: m.id })
    }
  }

  return issues
}

/** Append a transition id to a compartment's inflow/outflow list (mirrors stock-flow's wireFlow;
 *  the engine recomputes influence edges on the next reconcile). */
function wire(doc: ModelDoc, compartmentId: string, field: 'inflows' | 'outflows', transitionId: string): ModelDoc {
  return mutateElement(doc, compartmentId, (e) => {
    const arr = (e[field] as string[] | undefined) ?? []
    if (!arr.includes(transitionId)) e[field] = [...arr, transitionId]
  })
}

const metapopConnect: LensBehavior['connect'] = (doc, fromId, toId) => {
  const from = doc.elements.find((e) => e.id === fromId)
  const to = doc.elements.find((e) => e.id === toId)
  if (!from || !to || fromId === toId) return null
  // transition → compartment: the transition flows into the compartment.
  if (roleIs(from, 'transition') && roleIs(to, 'compartment')) {
    if (to.inflows?.includes(fromId)) return null
    return wire(doc, toId, 'inflows', fromId)
  }
  // compartment → transition: the compartment drains out via that transition.
  if (roleIs(from, 'compartment') && roleIs(to, 'transition')) {
    if (from.outflows?.includes(toId)) return null
    return wire(doc, fromId, 'outflows', toId)
  }
  return null
}

// ── readouts ─────────────────────────────────────────────────────────────────────────────────
const norm = (s?: string) => (s ?? '').toLowerCase()
const isInfected = (e: FlatElement) => /infect|prevalen/.test(norm(e.id) + ' ' + norm(e.name)) || norm(e.id) === 'i' || norm(e.id).startsWith('i_')
const isRecovered = (e: FlatElement) => /recover|removed/.test(norm(e.id) + ' ' + norm(e.name)) || norm(e.id) === 'r' || norm(e.id).startsWith('r_')
const seriesOf = (results: SimulationResults, id: string) => results.elements[id]?.time_history?.mean

function findParam(doc: ModelDoc, rx: RegExp): number | undefined {
  const e = doc.elements.find((x) => roleIs(x, 'parameter') && rx.test(x.id + ' ' + x.name))
  const v = (e?.value as { value?: number } | undefined)?.value
  return typeof v === 'number' ? v : undefined
}

/** Metapop readouts (no engine change): the epidemic curve's shape plus the standard summary
 *  numbers — peak prevalence & time-to-peak, epidemic duration, attack rate, and an R₀ estimate. */
function metapopReadouts(results: SimulationResults, doc: ModelDoc): LensReadout[] {
  const out: LensReadout[] = []
  const t = results.time_axis
  const unit = results.time_unit || ''
  const compartments = doc.elements.filter((e) => roleIs(e, 'compartment'))

  // Per-compartment peak + time-to-peak (like reliability's per-component rows).
  for (const c of compartments) {
    const mean = seriesOf(results, c.id)
    if (!mean || mean.length === 0) continue
    let peak = -Infinity
    let ip = 0
    for (let k = 0; k < mean.length; k++) if (mean[k] > peak) { peak = mean[k]; ip = k }
    out.push({
      id: c.id,
      label: results.elements[c.id]?.label ?? c.name,
      metrics: [
        { name: 'Peak', value: fmtNum(peak) },
        { name: 'Time to peak', value: `${fmtNum(t[ip] ?? ip)} ${unit}`.trim() },
      ],
    })
  }

  // Epidemic summary from the aggregate infected curve (Σ infected compartments per step).
  const metrics: { name: string; value: string }[] = []
  const infectedSeries = compartments.filter(isInfected).map((c) => seriesOf(results, c.id)).filter((s): s is number[] => !!s && s.length > 0)
  if (infectedSeries.length) {
    const L = Math.min(...infectedSeries.map((s) => s.length))
    const total = (k: number) => infectedSeries.reduce((a, s) => a + s[k], 0)
    let peak = -Infinity
    let ip = 0
    for (let k = 0; k < L; k++) { const s = total(k); if (s > peak) { peak = s; ip = k } }
    metrics.push({ name: 'Peak prevalence', value: fmtNum(peak) })
    metrics.push({ name: 'Time to peak', value: `${fmtNum(t[ip] ?? ip)} ${unit}`.trim() })
    const eps = Math.max(peak * 0.01, 1e-9)
    let first = -1
    let last = -1
    for (let k = 0; k < L; k++) { if (total(k) > eps) { if (first < 0) first = k; last = k } }
    if (first >= 0 && last > first) {
      metrics.push({ name: 'Duration', value: `${fmtNum((t[last] ?? last) - (t[first] ?? first))} ${unit}`.trim() })
    }
  }

  // Attack rate = final recovered / initial total population. A ratio over one consistent aggregate,
  // so it is correct regardless of whether a dimensioned result reports sum or mean across patches.
  const recovered = compartments.filter(isRecovered)
  const initTotal = compartments.reduce((a, c) => a + (seriesOf(results, c.id)?.[0] ?? 0), 0)
  if (recovered.length && initTotal > 0) {
    const recFinal = recovered.reduce((a, c) => { const s = seriesOf(results, c.id); return a + (s ? s[s.length - 1] : 0) }, 0)
    metrics.push({ name: 'Attack rate', value: pct(recFinal / initTotal) })
  }

  // R₀ (well-mixed estimate) = β / γ, when both are exposed as parameters.
  const beta = findParam(doc, /(^|[^a-z])beta([^a-z]|$)|β|transmiss/i)
  const gamma = findParam(doc, /(^|[^a-z])gamma([^a-z]|$)|γ|recover/i)
  if (beta != null && gamma != null && gamma > 0) metrics.push({ name: 'R₀ (β/γ)', value: fmtNum(beta / gamma) })

  if (metrics.length) out.unshift({ id: 'metapop:summary', label: 'Epidemic', metrics })
  return out
}

/** Metapopulation / network-epidemiology behavior: conservation + coupling invariants, the
 *  transition↔compartment wiring gesture, and epidemic-curve readouts. */
export const metapopBehavior: LensBehavior = {
  id: 'metapop',
  invariants: metapopInvariants,
  connect: metapopConnect,
  resultReadouts: metapopReadouts,
}
