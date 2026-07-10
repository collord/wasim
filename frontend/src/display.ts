// Display-unit helpers. The engine computes in canonical units; the summary carries an
// optional display unit + affine map (display = value·factor + offset). These honor it.

import type { ElementSummary } from './types'

export interface Disp {
  unit: string
  factor: number
  offset: number
}

/** Resolve an element's display mapping (falls back to the canonical unit, identity map). */
export function dispOf(e: ElementSummary): Disp {
  return e.display_unit
    ? { unit: e.display_unit, factor: e.display_factor ?? 1, offset: e.display_offset ?? 0 }
    : { unit: e.unit, factor: 1, offset: 0 }
}

/** Unit label to show for an element (display unit if present, else canonical). */
export function unitLabel(e: ElementSummary): string {
  return e.display_unit ?? e.unit
}

export const toDisplay = (v: number, d: Disp): number => v * d.factor + d.offset
export const fromDisplay = (v: number, d: Disp): number => (v - d.offset) / d.factor
