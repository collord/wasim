import type { PaletteEntry } from '../model/edits'

/**
 * A **lens** is a thin, domain-specific authoring surface projected onto the one general engine
 * (see `WASIM_VALUE_PROP_THESIS.md` and `WASIM_LENS_IMPLEMENTATION_PLAN.md`). It reprograms *what
 * nouns you author* — and, in later phases, how they're drawn, labelled, and validated — while the
 * engine, the store, and the on-disk JSON stay unchanged.
 *
 * The `general` lens is the least-restricted spec (palette = the full union of every entry), so
 * "no lens selected" is not a special case — it is just the widest lens.
 *
 * This is Phase 0: only the palette projection is wired. The commented fields below are the
 * surfaces later phases attach (glyphs, inspector labels, author-time invariants, result presets,
 * templates, round-trip role tags). They are intentionally omitted from the type until built, so
 * the type never promises behavior that isn't yet connected.
 */
export type LensId = 'general' | 'stock-flow' | 'reliability' | 'decision'

/** One labelled section of the palette, in the active lens's vocabulary and order. */
export interface PaletteGroup {
  label: string
  entries: PaletteEntry[]
}

export interface LensSpec {
  id: LensId
  label: string
  /** One-line description shown on the lens picker (Phase A7). */
  tagline: string
  /** Which palette entries this lens exposes, grouped and ordered in the lens's terms. */
  palette: (all: PaletteEntry[]) => PaletteGroup[]
  // Phase A4–A7 / Parts B & D will extend this interface:
  //   roleOf(el): LensRole | null            — reads the `lens_role` round-trip tag
  //   glyphOf(el, role): IconType            — canvas / inspector glyph
  //   inspectorLabels: Partial<Record<LensRole, FieldLabelMap>>
  //   invariants(summary, doc): Issue[]      — FE author-time governance checks
  //   resultPreset, templates, primaryVerbs
}

/** Group palette entries by their `group`, preserving first-appearance order — the identity
 *  projection that reproduces the pre-lens palette exactly. */
export function groupInOrder(all: PaletteEntry[]): PaletteGroup[] {
  const order: string[] = []
  const byGroup = new Map<string, PaletteEntry[]>()
  for (const e of all) {
    let bucket = byGroup.get(e.group)
    if (!bucket) {
      bucket = []
      byGroup.set(e.group, bucket)
      order.push(e.group)
    }
    bucket.push(e)
  }
  return order.map((label) => ({ label, entries: byGroup.get(label)! }))
}
