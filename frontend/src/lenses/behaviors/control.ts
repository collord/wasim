import type { ModelDoc } from '../../model/schema'
import type { ModelSummary } from '../../types'
import type { Issue } from '../../worker/protocol'
import type { LensBehavior } from '../manifest-types'
import { isPid } from '../hint/structure'

/**
 * Control-systems behavior (`WASIM_LENS_MODELING_TYPES.md` §4): feedback control — a `pid`
 * controller drives a measured variable toward a setpoint. Detection is `value_rule === 'pid'`
 * (stable across the frontend's `kp/ki/kd` shape and the emitter's `controller` role-map shape),
 * so the checks hold on imported models. The controller reads its measured variable via `input`;
 * a dangling `input` means the loop isn't closed. Plant elements (the stocks/flows it drives) keep
 * their own lens — control surfaces as an overlay there rather than relabeling them.
 */
function controlInvariants(_summary: ModelSummary, doc: ModelDoc): Issue[] {
  const issues: Issue[] = []
  const els = doc.elements
  if (els.length > 0 && !els.some(isPid)) {
    issues.push({ severity: 'warning', message: 'No controller — add a Controller (PID / on-off) to model feedback control.' })
  }
  const ids = new Set(els.map((e) => e.id))
  for (const e of els) {
    if (!isPid(e)) continue
    const input = typeof e.input === 'string' ? e.input : undefined
    if (input && !ids.has(input)) {
      issues.push({
        severity: 'warning',
        message: `Controller "${e.name}" reads "${input}", which isn't in the model — the feedback loop is open.`,
        element_id: e.id,
      })
    }
  }
  return issues
}

export const controlBehavior: LensBehavior = {
  id: 'control',
  invariants: controlInvariants,
}
