import type { FlatElement, ModelDoc } from '../../model/schema'
import type { LensId } from '../types'
import {
  isStock, isGate, isEvent, isStatus, isPid,
  flowIds, eventsFeedingGates, isOnEventTriggered, isCouplingMatrix, optimizationOf,
} from './structure'

/**
 * Per-lens **signatures** — how strongly a container's interior reads as each modeling paradigm
 * (`EMITTER_LENS_TARGETING.md` §2–§3). Each returns a score in `[0,1]`; `lensHint` takes the
 * argmax. Scores are structural fractions plus targeted boosts for the discriminating construct
 * (a coupling matrix for metapop, gates for reliability, `on_event` chains for discrete-event), so
 * the two disambiguations that matter fall out of scoring: metapop ⊐ stock-flow (needs a coupling
 * matrix) and reliability vs discrete-event (gate aggregation vs `on_event` wiring). Classification
 * is per-container and instance-singular — an element counts toward whichever paradigm its
 * surrounding structure indicates; no element is ever "in two lenses".
 */

export type ScoreFn = (interior: FlatElement[], doc: ModelDoc) => number

const frac = (n: number, total: number) => (total > 0 ? n / total : 0)

/** stock-flow: fraction of the interior that is a stock or a flow (node in a stock's in/outflows). */
const stockFlowScore: ScoreFn = (interior) => {
  const flows = flowIds(interior)
  const n = interior.filter((e) => isStock(e) || flows.has(e.id)).length
  return frac(n, interior.length)
}

/** metapop: 0 unless a square coupling matrix and at least one stock (compartment) are present —
 *  that is what lifts it above plain stock-flow. When present, score high and scale with the
 *  stock-flow substrate density. */
const metapopScore: ScoreFn = (interior, doc) => {
  const hasCoupling = interior.some((e) => isCouplingMatrix(e, doc))
  const hasCompartment = interior.some(isStock)
  if (!hasCoupling || !hasCompartment) return 0
  return 0.6 + 0.4 * stockFlowScore(interior, doc)
}

/** reliability: fault-tree gates plus the events feeding them. A container of events-under-gates
 *  scores here, not discrete-event. */
const reliabilityScore: ScoreFn = (interior) => {
  const feeders = eventsFeedingGates(interior)
  const n = interior.filter((e) => isGate(e) || (isEvent(e) && feeders.has(e.id))).length
  // A standalone failure model (events, no gate) still reads as reliability if events dominate and
  // none are wired as an on_event chain.
  if (n === 0) {
    const events = interior.filter(isEvent)
    if (events.length > 0 && !events.some(isOnEventTriggered)) return frac(events.length, interior.length)
    return 0
  }
  return frac(n, interior.length)
}

/** decision: an optimization study present whose referenced elements live in this interior. */
const decisionScore: ScoreFn = (interior, doc) => {
  const opt = optimizationOf(doc)
  if (!opt) return 0
  const ids = new Set(interior.map((e) => e.id))
  const refs = new Set<string>()
  for (const v of opt.variables ?? []) if (v.element_id) refs.add(v.element_id)
  const obj = (opt.objective as { element_id?: string } | undefined)?.element_id
  if (obj) refs.add(obj)
  if (refs.size === 0) return 0
  const inHere = [...refs].filter((id) => ids.has(id)).length
  return inHere > 0 ? 0.7 + 0.3 * frac(inHere, refs.size) : 0
}

/** discrete-event: events NOT feeding a gate, plus status latches; boosted by `on_event` chains
 *  (the strong signal). */
const discreteEventScore: ScoreFn = (interior) => {
  const feeders = eventsFeedingGates(interior)
  const own = interior.filter((e) => (isEvent(e) && !feeders.has(e.id)) || isStatus(e))
  if (own.length === 0) return 0
  const base = frac(own.length, interior.length)
  const chained = own.some(isOnEventTriggered)
  return Math.min(1, chained ? base + 0.25 : base)
}

/** control-systems: fraction of the interior that is a `pid` controller. A lone controller on a
 *  big plant scores low here (→ stock-flow dominant, control as an overlay); a controller-dense
 *  container scores high. */
const controlScore: ScoreFn = (interior) => frac(interior.filter(isPid).length, interior.length)

/** The six signatures, in tie-break preference order (more specific paradigms first, so an equal
 *  score resolves toward the narrower reading). */
export const SIGNATURES: { lens: LensId; score: ScoreFn }[] = [
  { lens: 'decision', score: decisionScore },
  { lens: 'metapop', score: metapopScore },
  { lens: 'reliability', score: reliabilityScore },
  { lens: 'discrete-event', score: discreteEventScore },
  { lens: 'control-systems', score: controlScore },
  { lens: 'stock-flow', score: stockFlowScore },
]

/** Below this, a container is too weakly matched to hint anything but the General lens. */
export const HINT_FLOOR = 0.25
