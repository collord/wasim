import type { ModelDoc } from '../../model/schema'
import type { ModelSummary, SimulationResults } from '../../types'
import type { Issue } from '../../worker/protocol'
import type { LensReadout } from '../types'
import type { LensBehavior } from '../manifest-types'
import { clamp01, pct, fmtNum } from './format'

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

/** Reliability / RBD behavior: component/gate invariants + availability/MTTF readouts. */
export const reliabilityBehavior: LensBehavior = {
  id: 'reliability',
  invariants: reliabilityInvariants,
  resultReadouts: reliabilityReadouts,
}
