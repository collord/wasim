import type { FlatElement, ModelDoc } from '../model/schema'
import type { ModelSummary, SimulationResults } from '../types'
import type { Issue } from '../worker/protocol'
import type { LensReadout, NodeShape } from './types'

/**
 * The **data layer** of the lens system (spec `WASIM_LENS_MANIFEST_SPEC.md` §4–§8): the
 * declarative surface of a lens — palette taxonomy, per-element relabeling, glyphs, roles,
 * templates — expressed as JSON over a canonical **element registry**. The *behavioral*
 * surface (invariants / connect / result readouts) stays first-party code, keyed by a
 * `behavior` id (`LensBehavior` below). The loader (`loader.ts`) compiles a manifest + the
 * registry + a behavior plugin into the runtime `LensSpec` the app already consumes.
 *
 * These interfaces are the TS shapes the bundled `.json` artifacts are validated against at
 * build time (JSON Schemas live under `schema/` for editor tooling; runtime ajv validation is
 * deferred to a later phase).
 */

// ── Element registry (element-registry.json, §4) ─────────────────────────────────

/** A palette section: a labelled group of constructs, in taxonomy order. */
export interface RegistrySection {
  id: string
  label: string
  /** Collapsed by default in the palette (rarely-used constructs). */
  advanced?: boolean
}

/** One buildable construct — the General-lens superset; every lens `ref`s a subset. */
export interface RegistryEntry {
  /** Stable id; manifests reference this and it maps 1:1 to a `PALETTE` key (`edits.ts`). */
  key: string
  /** Default section id (a lens may re-section by placing the ref elsewhere). */
  section: string
  /** Default palette label (a lens may relabel). */
  label: string
  /** Engine target: a `Primitive`, a node `value_rule`, or (future) a `ContainerDef.kind`. */
  construct: string
  /** Default canvas glyph. */
  glyph?: NodeShape
  /** `structured` = a real inspector editor exists; `raw` = edited via the JSON escape hatch. */
  editor: 'structured' | 'raw'
  /** One-line description (tooltip / registry reference). */
  doc?: string
}

export interface ElementRegistry {
  version: string
  sections: RegistrySection[]
  entries: RegistryEntry[]
}

// ── Lens manifest (lenses/*.json, §6) ────────────────────────────────────────────

/** One palette control in a lens's vocabulary: a `ref` into the registry + overrides. */
export interface ManifestPaletteItem {
  /** A registry `entry.key` (⇒ which `PALETTE.make()` scaffolds the element). */
  ref: string
  /** Lens-facing label (defaults to the entry's label). */
  label?: string
  /** Stamped onto the inserted element's `lens_role` (the round-trip tag). */
  role?: string
  /** Canvas glyph override for this item (future canvas; not used by the current glyphOf path). */
  glyph?: NodeShape
  /** Collapse this item's section by default. */
  advanced?: boolean
}

/** A labelled palette section in the lens's terms and order. */
export interface ManifestPaletteSection {
  section: string
  items: ManifestPaletteItem[]
}

export interface LensManifest {
  version: string
  id: string
  label: string
  tagline: string
  /** Base manifest id to inherit + override (usually `general`). Not yet used by the loader. */
  extends?: string
  /** Code-plugin id (`LensBehavior.id`). Absent ⇒ a purely declarative lens. */
  behavior?: string
  /**
   * The palette projection. Either the `@registry` sentinel (the General identity projection —
   * surface every registry entry grouped by its section, taxonomy order) or explicit sections.
   */
  palette: '@registry' | ManifestPaletteSection[]
  /** `lens_role` → inspector header label. */
  roleLabels?: Record<string, string>
  /** `lens_role` → `NodeShape` (compiles to the runtime `glyphOf`). */
  glyphByRole?: Record<string, NodeShape>
  /** Template ids into the template registry (bound by import in the loader). */
  templates?: string[]
  /** Ordered roles; the first whose element is an output opens in Results. */
  preferredResult?: { roleOrder: string[] }
}

// ── Behavior plugin (behaviors/*.ts, §7.1) ───────────────────────────────────────

/**
 * The code half of a lens: the parts that do not reduce to static JSON (graph traversal,
 * arithmetic, doc transforms). Registered by id in `behaviors/`; a manifest may only *name* a
 * built-in behavior, never supply new code — the trust boundary that keeps manifests safe to
 * load from any source.
 */
export interface LensBehavior {
  id: string
  invariants?: (summary: ModelSummary, doc: ModelDoc) => Issue[]
  connect?: (doc: ModelDoc, fromId: string, toId: string) => ModelDoc | null
  resultReadouts?: (results: SimulationResults, doc: ModelDoc) => LensReadout[]
}

/** Re-exported for behavior modules that build elements. */
export type { FlatElement }
