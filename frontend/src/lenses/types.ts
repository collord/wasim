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

/** Canvas glyph shape for an element, chosen by its lens role (Forrester notation: a stock is a
 *  box, an auxiliary a circle, a flow a valve). `default` is the standard node. */
export type NodeShape = 'box' | 'circle' | 'valve' | 'default'

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

/** A canonical starting model for a lens (bathtub, SIR, …) — offered on the empty canvas so a
 *  first-time user never faces a blank graph. `build()` returns a fresh, runnable doc. */
export interface ModelTemplate {
  id: string
  label: string
  description: string
  build: () => ModelDoc
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
  /**
   * Domain label for an element's `lens_role`, shown in the inspector header so the same engine
   * element reads as its domain noun (e.g. an `expression` tagged `flow` reads as "Flow", not
   * "Expression"). Omitted by lenses without roles.
   */
  roleLabels?: Record<string, string>
  /**
   * Canvas glyph shape for an element, chosen by its `lens_role` — so a stock reads as a box, an
   * auxiliary as a circle, etc. Omitted lenses (and unmapped roles) render the default node.
   */
  glyphOf?: (role: string | null | undefined) => NodeShape
  /** Canonical starting models offered on the empty canvas. */
  templates?: ModelTemplate[]
  /**
   * Which result the Results view should open on after a run — for stock-and-flow this is a
   * stock's trajectory, because the empirical finding (thesis §6) is that people cannot integrate
   * a flow into a stock in their heads: the lens must *show* the accumulation, not a final number.
   * Returns an element id present in `outputIds`, or null to keep the default (first output).
   */
  preferredResultId?: (doc: ModelDoc, outputIds: string[]) => string | null
  /**
   * On-canvas "draw a connection" gesture semantics. Dragging from one node onto another calls
   * this; it returns the updated doc, or null if the pair can't be connected. Stock-flow wires a
   * flow → stock as an inflow, a stock → flow as an outflow. Lenses without on-canvas wiring omit
   * it (the general lens connects via expression references, not a drawn edge).
   */
  connect?: (doc: ModelDoc, fromId: string, toId: string) => ModelDoc | null
  // Part D will further extend this for reliability (redundancy gesture, availability preset).
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
