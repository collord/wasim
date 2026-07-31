import { mutateElement } from '../../model/edits'
import type { ModelDoc } from '../../model/schema'
import type { ModelSummary } from '../../types'
import type { Issue } from '../../worker/protocol'
import type { LensBehavior } from '../manifest-types'

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

/** Append a flow id to a stock's inflow/outflow list (engine recomputes the influence edges on
 *  the next reconcile). */
function wireFlow(doc: ModelDoc, stockId: string, field: 'inflows' | 'outflows', flowId: string): ModelDoc {
  return mutateElement(doc, stockId, (e) => {
    const arr = (e[field] as string[] | undefined) ?? []
    if (!arr.includes(flowId)) e[field] = [...arr, flowId]
  })
}

/** Stock & Flow behavior: SFC invariants + the draw-flow connect gesture. */
export const stockFlowBehavior: LensBehavior = {
  id: 'stock-flow',
  invariants: stockFlowInvariants,
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
