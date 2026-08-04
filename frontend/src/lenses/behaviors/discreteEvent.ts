import type { ModelDoc } from '../../model/schema'
import type { ModelSummary } from '../../types'
import type { Issue } from '../../worker/protocol'
import type { LensBehavior } from '../manifest-types'
import { isEvent, isStatus, triggerOf } from '../hint/structure'

/**
 * Discrete-event behavior (`EMITTER_LENS_TARGETING.md` §3; `WASIM_LENS_MODELING_TYPES.md` §3):
 * events firing (at a rate, on a schedule, or on a condition) plus set-reset latches. Checks are
 * structural — keyed off `primitive`/`value_rule` and the `trigger` sub-object — so they hold on
 * imported models with no stored roles, and (unlike reliability) they do **not** claim events that
 * feed a fault-tree gate: that is the reliability paradigm's, and its own lens scores those. The
 * `on_event` edge lives in `trigger.source`, not in `connections[]`, so the dangling-source check
 * reads the field directly.
 */
function discreteEventInvariants(_summary: ModelSummary, doc: ModelDoc): Issue[] {
  const issues: Issue[] = []
  const els = doc.elements
  if (els.length > 0 && !els.some((e) => isEvent(e) || isStatus(e))) {
    issues.push({ severity: 'warning', message: 'No events — add an Event or a Latch to model a discrete-event process.' })
  }
  const ids = new Set(els.map((e) => e.id))
  for (const e of els) {
    const t = triggerOf(e)
    if (t?.mode === 'on_event' && t.source && !ids.has(t.source)) {
      issues.push({
        severity: 'warning',
        message: `Event "${e.name}" fires on "${t.source}", which isn't in the model — the on_event chain is broken.`,
        element_id: e.id,
      })
    }
  }
  return issues
}

export const discreteEventBehavior: LensBehavior = {
  id: 'discrete-event',
  invariants: discreteEventInvariants,
}
