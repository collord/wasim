import type { ModelDoc } from '../../model/schema'
import type { LensId } from '../types'
import { interiorOf } from './scope'
import { isPid, isStochastic } from './structure'
import { SIGNATURES, HINT_FLOOR } from './signatures'

/**
 * `lensHint` — the pure, ephemeral lens classifier (`EMITTER_LENS_TARGETING.md`; consensus
 * `WASIM_LENS_MODELING_TYPES.md` §1). Given a container id (null = the model root) and the doc, it
 * returns the container's dominant modeling paradigm plus any co-existing overlays — reading only
 * emitted/authored fields, writing nothing. Total and side-effect-free: same input → same output.
 * The UI calls it on activation and re-calls it when the container's contents change.
 */

/** A non-topology or non-dominant projection layered on the primary lens (a rendering mode, never
 *  a second membership). */
export type OverlayId = 'control' | 'uncertainty' | (string & {})

export interface LensHint {
  /** The container's dominant paradigm (or `general` when nothing matches above the floor). */
  lens: LensId
  /** `0..1`, advisory only — lets the UI soften a weak guess; there is no correctness threshold. */
  confidence: number
  /** Co-existing computed projections (control loops, Monte-Carlo/uncertainty). */
  overlays: OverlayId[]
}

/** The override-map key for a drilled container (null = the model root). Container ids are
 *  model-local, so this map is in-memory and reset on model load (see the store). */
export const ROOT_OVERRIDE_KEY = '__root__'
export type LensOverrides = Record<string, LensId>

/**
 * Resolve the *active* lens id for the drilled container, the single place the priority is decided:
 * a **per-container override** (the user's one-click correction for *this* container) wins first, so
 * a wrong hint on one submodel is fixed without clobbering the others; then an explicit authored
 * whole-doc `view.lens` (so authored docs and the root picker stay non-regressive); otherwise the
 * computed hint for the active container drives it (imports open in their inferred lens, drilling
 * re-lenses per container). Pure — the store hook (`useActiveLens`) wraps this with `resolveLens`.
 */
export function activeLensId(
  doc: ModelDoc | null | undefined,
  activeContainerId: string | null,
  overrides?: LensOverrides,
): LensId {
  const override = overrides?.[activeContainerId ?? ROOT_OVERRIDE_KEY]
  if (override) return override
  const explicit = doc?.view?.lens
  if (explicit) return explicit
  if (!doc) return 'general'
  return lensHint(activeContainerId, doc).lens
}

/** Compute the lens hint for the container the user has drilled into (null = root). */
export function lensHint(containerId: string | null, doc: ModelDoc): LensHint {
  const interior = interiorOf(containerId, doc)
  if (interior.length === 0) return { lens: 'general', confidence: 0, overlays: [] }

  let best: { lens: LensId; score: number } = { lens: 'general', score: 0 }
  for (const sig of SIGNATURES) {
    const score = sig.score(interior, doc)
    if (score > best.score) best = { lens: sig.lens, score }
  }

  const lens: LensId = best.score >= HINT_FLOOR ? best.lens : 'general'

  const overlays: OverlayId[] = []
  // Control surfaces as an overlay whenever a controller is present but control isn't dominant.
  if (lens !== 'control-systems' && interior.some(isPid)) overlays.push('control')
  // Uncertainty highlights stochastic facets (distribution draws / processes), by structure.
  if (interior.some(isStochastic)) overlays.push('uncertainty')

  return { lens, confidence: best.score, overlays }
}
