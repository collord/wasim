import type { LensId, LensSpec } from './types'
import { groupInOrder } from './types'
import type { ModelDoc } from '../model/schema'
import type { ModelSummary } from '../types'
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
      if (flowCount === 0) {
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
}

const REGISTERED: LensSpec[] = [stockFlowLens, generalLens]

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
