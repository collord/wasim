import type { LensBehavior } from '../manifest-types'
import { stockFlowBehavior } from './stockFlow'
import { reliabilityBehavior } from './reliability'
import { decisionBehavior } from './decision'
import { metapopBehavior } from './metapop'

/**
 * The behavior-plugin registry (WASIM_LENS_MANIFEST_SPEC.md §7): the code half of a lens,
 * keyed by id. A manifest's `behavior` field may only *name* one of these — it can never supply
 * new code. This is the trust boundary that keeps user/project manifests safe to load: they
 * re-theme (data), they never execute (code).
 */
export const BEHAVIORS: Record<string, LensBehavior> = {
  'stock-flow': stockFlowBehavior,
  reliability: reliabilityBehavior,
  decision: decisionBehavior,
  metapop: metapopBehavior,
}

/** Resolve a manifest `behavior` id to its plugin; throws on an unknown id (a load error). */
export function resolveBehavior(id: string): LensBehavior {
  const b = BEHAVIORS[id]
  if (!b) throw new Error(`Unknown lens behavior "${id}"`)
  return b
}
