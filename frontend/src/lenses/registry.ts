import type { LensId, LensSpec } from './types'
import { groupInOrder } from './types'

/**
 * The lens registry. Phase 0 ships only the `general` lens (the identity/baseline spec whose
 * palette is the full union of every entry, reproducing the pre-lens behavior exactly). The
 * domain lenses — stock-flow, reliability, decision — are added by their respective phases; until
 * a lens is registered, `resolveLens` falls back to `general`, so a model tagged with a
 * not-yet-built lens opens safely rather than breaking.
 */

/** General (advanced): no domain vocabulary — the raw substrate. Full palette, in file order. */
const generalLens: LensSpec = {
  id: 'general',
  label: 'General',
  tagline: 'The full engine, unfiltered — every element type, no domain vocabulary.',
  palette: (all) => groupInOrder(all),
}

export const LENSES: Partial<Record<LensId, LensSpec>> = {
  general: generalLens,
}

export const DEFAULT_LENS: LensSpec = generalLens

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
