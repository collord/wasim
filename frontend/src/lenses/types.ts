import type { PaletteEntry } from '../model/edits'
import type { ModelDoc } from '../model/schema'
import type { ModelSummary } from '../types'
import type { Issue } from '../worker/protocol'

/**
 * A **lens** is a thin, domain-specific authoring surface projected onto the one general engine
 * (see `WASIM_VALUE_PROP_THESIS.md` and `WASIM_LENS_IMPLEMENTATION_PLAN.md`). It reprograms *what
 * nouns you author* and *how they're validated*, while the engine, the store, and the on-disk JSON
 * stay unchanged.
 *
 * The `general` lens is the least-restricted spec (palette = the full union of every entry, no
 * domain invariants), so "no lens selected" is not a special case — it is just the widest lens.
 */
export type LensId = 'general' | 'stock-flow' | 'reliability' | 'decision'

/** One insertable palette control, in the active lens's vocabulary. `key` selects which
 *  `PALETTE` entry's `make()` scaffolds the element; `lensRole` is stamped onto the created
 *  element's `lens_role` so the lens round-trips (re-opening reconstructs the vocabulary). */
export interface PaletteItem {
  key: string
  label: string
  iconType: string
  lensRole?: string
}

/** One labelled section of the palette, in the lens's terms and order. */
export interface PaletteGroup {
  label: string
  items: PaletteItem[]
}

export interface LensSpec {
  id: LensId
  label: string
  /** One-line description shown on the lens picker. */
  tagline: string
  /** Which controls this lens exposes, grouped and ordered in the lens's terms. */
  palette: (all: PaletteEntry[]) => PaletteGroup[]
  /**
   * Author-time governance checks, surfaced as **warnings** in the status bar alongside the
   * engine's validation (they never block a run — the engine stays the arbiter of runnability).
   * This is what makes a lens a lens and not a view: it changes how the model is validated.
   * Omitted by lenses with no domain invariants (e.g. general).
   */
  invariants?: (summary: ModelSummary, doc: ModelDoc) => Issue[]
  // Phase A4–A5 / Parts B & D will extend this: inspectorLabels, glyphOf, resultPreset, templates.
}

/** Group palette entries by their `group`, preserving first-appearance order — the identity
 *  projection that reproduces the pre-lens palette exactly (used by the general lens). */
export function groupInOrder(all: PaletteEntry[]): PaletteGroup[] {
  const order: string[] = []
  const byGroup = new Map<string, PaletteItem[]>()
  for (const e of all) {
    let bucket = byGroup.get(e.group)
    if (!bucket) {
      bucket = []
      byGroup.set(e.group, bucket)
      order.push(e.group)
    }
    bucket.push({ key: e.key, label: e.label, iconType: e.iconType })
  }
  return order.map((label) => ({ label, items: byGroup.get(label)! }))
}
