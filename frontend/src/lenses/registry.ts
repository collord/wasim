import type { LensId, LensSpec } from './types'
import { groupInOrder } from './types'
import { STOCK_FLOW_TEMPLATES } from './stockFlowTemplates'
import { RELIABILITY_TEMPLATES } from './reliabilityTemplates'
import { DECISION_TEMPLATES } from './decisionTemplates'
import { mutateElement } from '../model/edits'
import type { LensReadout } from './types'
import type { ModelDoc } from '../model/schema'
import type { ModelSummary, SimulationResults } from '../types'
import type { Issue } from '../worker/protocol'

/**
 * The lens registry. Phase 0 shipped the `general` lens (identity/baseline); this adds the
 * **stock-and-flow** lens (Part B) — relabelled palette + `lens_role` round-trip tags + the
 * stock-flow-consistency (SFC) invariants. The reliability and decision lenses follow in their
 * own phases. Until a lens is registered, `resolveLens` falls back to `general`, so a model
 * tagged with a not-yet-built lens opens safely.
 */

/** General (advanced): no domain vocabulary — the raw substrate. Full palette, in file order. */
const generalLens: LensSpec = {
  id: 'general',
  label: 'General',
  tagline: 'The full engine, unfiltered — every element type, no domain vocabulary.',
  palette: (all) => groupInOrder(all),
}

/** Stock-and-flow-consistency checks (Godley–Lavoie accounting invariants, thesis §4.4), surfaced
 *  as author-time warnings. Conservation: a flow must move something into or out of a stock.
 *  Reconciliation: a stock should have at least one flow. Both survive deleting the diagram — the
 *  reason stock-and-flow is a lens, not a view. */
function stockFlowInvariants(_summary: ModelSummary, doc: ModelDoc): Issue[] {
  const issues: Issue[] = []
  const referenced = new Set<string>()
  for (const e of doc.elements) {
    for (const f of e.inflows ?? []) referenced.add(f)
    for (const f of e.outflows ?? []) referenced.add(f)
  }
  for (const e of doc.elements) {
    if (e.lens_role === 'flow' && !referenced.has(e.id)) {
      issues.push({
        severity: 'warning',
        message: `Flow "${e.name}" is not connected to a stock — a flow must move something in or out (conservation).`,
        element_id: e.id,
      })
    }
    if (e.lens_role === 'stock') {
      const flowCount = (e.inflows?.length ?? 0) + (e.outflows?.length ?? 0)
      // A stock is static only if nothing drives it: no flows AND no net-rate / growth-rate.
      const hasDriver = flowCount > 0 || e.rate != null || e.return_rate != null
      if (!hasDriver) {
        issues.push({
          severity: 'warning',
          message: `Stock "${e.name}" has no inflows or outflows — it can never change.`,
          element_id: e.id,
        })
      }
    }
  }
  return issues
}

/** Stock & Flow: the Forrester vocabulary (thesis §4). Stocks accumulate; flows are rates;
 *  auxiliaries are derived. The engine constructs (lag accumulator, rate expression) never
 *  surface — only the domain nouns do. */
const stockFlowLens: LensSpec = {
  id: 'stock-flow',
  label: 'Stock & Flow',
  tagline: 'Forrester stock-and-flow — stocks accumulate, flows are rates, guaranteed consistent.',
  palette: () => [
    { label: 'Stocks', items: [{ key: 'stock', label: 'Stock', iconType: 'accumulator', lensRole: 'stock' }] },
    {
      label: 'Flows',
      items: [{ key: 'expression', label: 'Flow', iconType: 'expression', lensRole: 'flow' }],
    },
    {
      label: 'Auxiliaries',
      items: [
        { key: 'expression', label: 'Auxiliary', iconType: 'expression', lensRole: 'auxiliary' },
        { key: 'constant', label: 'Constant', iconType: 'constant', lensRole: 'auxiliary' },
        { key: 'stochastic', label: 'Uncertain input', iconType: 'random_variable', lensRole: 'auxiliary' },
      ],
    },
  ],
  invariants: stockFlowInvariants,
  roleLabels: { stock: 'Stock', flow: 'Flow', auxiliary: 'Auxiliary' },
  glyphOf: (role) =>
    role === 'stock' ? 'box' : role === 'auxiliary' ? 'circle' : role === 'flow' ? 'valve' : 'default',
  templates: STOCK_FLOW_TEMPLATES,
  // Open results on a stock's trajectory — show the accumulation, don't ask the user to infer it.
  preferredResultId: (doc, outputIds) =>
    doc.elements.find((e) => e.lens_role === 'stock' && outputIds.includes(e.id))?.id ?? null,
  connect: (doc, fromId, toId) => {
    const from = doc.elements.find((e) => e.id === fromId)
    const to = doc.elements.find((e) => e.id === toId)
    if (!from || !to || fromId === toId) return null
    // flow → stock: the flow becomes an inflow of the stock.
    if (from.lens_role === 'flow' && to.lens_role === 'stock') {
      if (to.inflows?.includes(fromId)) return null
      return wireFlow(doc, toId, 'inflows', fromId)
    }
    // stock → flow: the stock drains out via that flow.
    if (from.lens_role === 'stock' && to.lens_role === 'flow') {
      if (from.outflows?.includes(toId)) return null
      return wireFlow(doc, fromId, 'outflows', toId)
    }
    return null
  },
}

/** Append a flow id to a stock's inflow/outflow list (engine recomputes the influence edges on
 *  the next reconcile). */
function wireFlow(doc: ModelDoc, stockId: string, field: 'inflows' | 'outflows', flowId: string): ModelDoc {
  return mutateElement(doc, stockId, (e) => {
    const arr = (e[field] as string[] | undefined) ?? []
    if (!arr.includes(flowId)) e[field] = [...arr, flowId]
  })
}

/** Reliability author-time checks: a non-empty model that models no components can't yield a
 *  reliability answer. (The redundancy/gate well-formedness checks arrive with the gate editor.) */
function reliabilityInvariants(_summary: ModelSummary, doc: ModelDoc): Issue[] {
  const issues: Issue[] = []
  const hasComponent = doc.elements.some((e) => e.lens_role === 'component')
  if (doc.elements.length > 0 && !hasComponent) {
    issues.push({
      severity: 'warning',
      message: 'No components — add a Component (failure FSM) to model reliability.',
    })
  }
  // Gate / redundancy well-formedness.
  for (const e of doc.elements) {
    if (e.primitive !== 'gate') continue
    const root = e.root as { op?: string; threshold?: number; children?: unknown[] } | undefined
    const nChildren = root?.children?.length ?? 0
    if (nChildren === 0) {
      issues.push({ severity: 'warning', message: `Gate "${e.name}" references no inputs.`, element_id: e.id })
    } else if (root?.op === 'n_vote') {
      const k = root.threshold ?? 1
      if (k > nChildren) {
        issues.push({ severity: 'warning', message: `Gate "${e.name}": k=${k} exceeds its ${nChildren} inputs.`, element_id: e.id })
      }
    }
  }
  return issues
}

const clamp01 = (x: number) => Math.max(0, Math.min(1, x))
const pct = (x: number) => `${(x * 100).toFixed(1)}%`
const fmtNum = (x: number) => (Number.isInteger(x) ? String(x) : x.toFixed(1))

/** Reliability readouts derived from the run (no engine change): a component/system's status
 *  time-history is the fraction failed per step, so operating fraction R(t) = 1 − failed(t).
 *  Availability = time-average of R; MTTF = ∫ R dt (area under the survival curve); P(failed) is
 *  the fraction of realizations failed at the end. */
function reliabilityReadouts(results: SimulationResults, doc: ModelDoc): LensReadout[] {
  const out: LensReadout[] = []
  const t = results.time_axis
  for (const e of doc.elements) {
    if (e.lens_role !== 'component' && e.lens_role !== 'redundancy') continue
    const er = results.elements[e.id]
    const status = er?.time_history?.mean
    if (!status || status.length === 0) continue
    const survival = status.map((s) => 1 - clamp01(s))
    const availability = survival.reduce((a, b) => a + b, 0) / survival.length
    let mttf = 0
    for (let i = 1; i < survival.length && i < t.length; i++) {
      mttf += ((survival[i] + survival[i - 1]) / 2) * (t[i] - t[i - 1])
    }
    const fv = er?.final_values ?? []
    const pFailed = fv.length ? fv.filter((v) => v > 0).length / fv.length : clamp01(status[status.length - 1])
    out.push({
      id: e.id,
      label: er?.label ?? e.name,
      metrics: [
        { name: 'Availability', value: pct(availability) },
        { name: 'MTTF', value: `${fmtNum(mttf)} ${results.time_unit || ''}`.trim() },
        { name: 'P(failed)', value: pct(pFailed) },
      ],
    })
  }
  return out
}

/** Reliability / RBD: repairable components (failure FSMs) and the states that drive them. Reuses
 *  the engine's Event primitive and the existing EventEditor wholesale — the lens is a relabel +
 *  glyph + template over already-built authoring (thesis: the second lens is a spec file). */
const reliabilityLens: LensSpec = {
  id: 'reliability',
  label: 'Reliability',
  tagline: 'Repairable components and failure FSMs — simulate-first RAM, not static block arithmetic.',
  palette: () => [
    { label: 'Components', items: [{ key: 'event', label: 'Component', iconType: 'event', lensRole: 'component' }] },
    { label: 'Redundancy', items: [{ key: 'gate', label: 'Redundancy gate', iconType: 'event', lensRole: 'redundancy' }] },
    { label: 'State', items: [{ key: 'stock', label: 'Damage state', iconType: 'accumulator', lensRole: 'state' }] },
    {
      label: 'Inputs',
      items: [
        { key: 'constant', label: 'Parameter', iconType: 'constant', lensRole: 'parameter' },
        { key: 'stochastic', label: 'Uncertain input', iconType: 'random_variable', lensRole: 'parameter' },
      ],
    },
  ],
  invariants: reliabilityInvariants,
  roleLabels: { component: 'Component', redundancy: 'Redundancy', state: 'Damage state', parameter: 'Parameter' },
  glyphOf: (role) => (role === 'component' || role === 'state' || role === 'redundancy' ? 'box' : 'default'),
  templates: RELIABILITY_TEMPLATES,
  // Open results on the system (redundancy) trajectory if present, else a component's status
  // (0 = operating, 1 = failed).
  preferredResultId: (doc, outputIds) => {
    const byRole = (r: string) => doc.elements.find((e) => e.lens_role === r && outputIds.includes(e.id))?.id
    return byRole('redundancy') ?? byRole('component') ?? null
  },
  resultReadouts: reliabilityReadouts,
}

/** Decision / value-of-information: declare decisions, chance inputs, and an objective; the
 *  optimizer picks the best decision under uncertainty and VOI prices learning a chance input
 *  before deciding. Reuses the existing optimizer; VOI is the one piece with new engine work. */
const decisionLens: LensSpec = {
  id: 'decision',
  label: 'Decision',
  tagline: 'Decisions, chance inputs, and an objective — optimize under uncertainty and price information (VOI).',
  palette: () => [
    { label: 'Decisions', items: [{ key: 'constant', label: 'Decision', iconType: 'constant', lensRole: 'decision' }] },
    { label: 'Uncertainty', items: [{ key: 'stochastic', label: 'Chance input', iconType: 'random_variable', lensRole: 'chance' }] },
    { label: 'Objective', items: [{ key: 'expression', label: 'Objective', iconType: 'expression', lensRole: 'objective' }] },
  ],
  invariants: (_summary, doc) => {
    const issues: Issue[] = []
    if (doc.elements.length > 0) {
      if (!doc.elements.some((e) => e.lens_role === 'decision')) {
        issues.push({ severity: 'warning', message: 'No decision — add a Decision variable to optimize.' })
      }
      if (!doc.elements.some((e) => e.lens_role === 'objective')) {
        issues.push({ severity: 'warning', message: 'No objective — add an Objective for the optimizer to target.' })
      }
    }
    return issues
  },
  roleLabels: { decision: 'Decision', chance: 'Chance input', objective: 'Objective' },
  // Influence-diagram notation: decision = box, chance = oval, objective/value = hexagon.
  glyphOf: (role) => (role === 'decision' ? 'box' : role === 'chance' ? 'circle' : role === 'objective' ? 'hex' : 'default'),
  templates: DECISION_TEMPLATES,
  preferredResultId: (doc, outputIds) =>
    doc.elements.find((e) => e.lens_role === 'objective' && outputIds.includes(e.id))?.id ?? null,
  // On a run, surface the objective under uncertainty: its expected value and P05–P95 band
  // (the decision context; the EVPPI for each chance input lives in the Optimization tab).
  resultReadouts: (results, doc) => {
    const obj = doc.elements.find((e) => e.lens_role === 'objective')
    const fv = obj ? results.elements[obj.id]?.final_values : undefined
    if (!obj || !fv || fv.length === 0) return []
    const mean = fv.reduce((a, b) => a + b, 0) / fv.length
    const sorted = [...fv].sort((a, b) => a - b)
    const q = (p: number) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]
    return [{
      id: obj.id,
      label: results.elements[obj.id]?.label ?? obj.name,
      metrics: [
        { name: 'Expected', value: fmtNum(mean) },
        { name: 'P05–P95', value: `${fmtNum(q(0.05))} – ${fmtNum(q(0.95))}` },
      ],
    }]
  },
}

const REGISTERED: LensSpec[] = [stockFlowLens, reliabilityLens, decisionLens, generalLens]

export const LENSES: Partial<Record<LensId, LensSpec>> = Object.fromEntries(
  REGISTERED.map((l) => [l.id, l]),
) as Partial<Record<LensId, LensSpec>>

export const DEFAULT_LENS: LensSpec = generalLens

/** Every registered lens, for the picker (declaration order). */
export function listLenses(): LensSpec[] {
  return REGISTERED
}

/** Resolve a (possibly absent or not-yet-built) lens id to a concrete spec, defaulting to
 *  `general`. Returns a stable reference per id, so it is safe to use directly in a store
 *  selector (no new object per render). */
export function resolveLens(id: string | null | undefined): LensSpec {
  if (id) {
    const spec = LENSES[id as LensId]
    if (spec) return spec
  }
  return DEFAULT_LENS
}
