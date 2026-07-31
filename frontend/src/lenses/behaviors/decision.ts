import type { Issue } from '../../worker/protocol'
import type { LensBehavior } from '../manifest-types'
import { fmtNum } from './format'

/** Decision / value-of-information behavior: needs-decision/objective invariants + the objective
 *  readout (expected value + P05–P95 band under uncertainty). */
export const decisionBehavior: LensBehavior = {
  id: 'decision',
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
