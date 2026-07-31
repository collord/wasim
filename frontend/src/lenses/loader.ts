import { PALETTE, type PaletteEntry } from '../model/edits'
import type { NodeShape, LensSpec, ModelTemplate, PaletteGroup, PaletteItem } from './types'
import type { ElementRegistry, LensManifest, ManifestPaletteSection } from './manifest-types'
import { resolveBehavior } from './behaviors'
import { STOCK_FLOW_TEMPLATES } from './stockFlowTemplates'
import { RELIABILITY_TEMPLATES } from './reliabilityTemplates'
import { DECISION_TEMPLATES } from './decisionTemplates'

import elementRegistry from './element-registry.json'
import generalManifest from './lenses/general.json'
import stockFlowManifest from './lenses/stock-flow.json'
import reliabilityManifest from './lenses/reliability.json'
import decisionManifest from './lenses/decision.json'

/**
 * The lens **loader** (WASIM_LENS_MANIFEST_SPEC.md §3, §6): compiles a declarative JSON manifest
 * over the element registry — plus a named code behavior — into the runtime `LensSpec` the app
 * already consumes. The `LensSpec` interface is unchanged; only its *source* moved from hand-
 * written TS (the old `registry.ts`) to data + plugins.
 *
 * Stability contract: `useActiveLens` (store.ts) calls `resolveLens` on every render, so a lens
 * must be a **stable object reference per id**. Every manifest is compiled exactly once here, at
 * module load, and cached — `resolveLens` only ever returns those cached objects.
 */

const REGISTRY = elementRegistry as ElementRegistry
const BUILTIN_MANIFESTS: LensManifest[] = [
  stockFlowManifest as LensManifest,
  reliabilityManifest as LensManifest,
  decisionManifest as LensManifest,
  generalManifest as LensManifest,
]

// Template registry: manifest `templates` are ids into this map (bound by import, as before).
const TEMPLATES: Record<string, ModelTemplate> = Object.fromEntries(
  [...STOCK_FLOW_TEMPLATES, ...RELIABILITY_TEMPLATES, ...DECISION_TEMPLATES].map((t) => [t.id, t]),
)

/** A ref's palette entry (for `make()` + `iconType`): manifest refs map 1:1 to PALETTE keys. */
function paletteEntry(ref: string): PaletteEntry {
  const entry = PALETTE.find((p) => p.key === ref)
  if (!entry) throw new Error(`Lens palette ref "${ref}" has no PALETTE entry`)
  return entry
}

/** An explicit manifest palette → a stable `PaletteGroup[]`, deriving `iconType` from PALETTE and
 *  `editor` (structured/raw) from the referenced registry entry. A section is `advanced` if any of
 *  its items are flagged advanced (spec §6 item-level `advanced`). */
function compileProjection(sections: ManifestPaletteSection[], reg: ElementRegistry): PaletteGroup[] {
  return sections.map((sec) => ({
    label: sec.section,
    advanced: sec.items.some((it) => it.advanced) || undefined,
    items: sec.items.map((it): PaletteItem => {
      const entry = paletteEntry(it.ref)
      return {
        key: it.ref, // Palette.tsx maps this to PALETTE.make()
        label: it.label ?? entry.label,
        iconType: entry.iconType, // reproduces accumulator / expression / random_variable icons
        lensRole: it.role,
        editor: editorOf(reg, it.ref),
      }
    }),
  }))
}

/** The `@registry` identity projection (General): every registry entry, grouped by its section in
 *  the registry's declared section order. Replaces the old `groupInOrder(PALETTE)` — driven by the
 *  registry's §5 taxonomy, not PALETTE's legacy `group` field. Carries each section's `advanced`
 *  flag and each item's `editor` so the palette can collapse advanced groups and hint raw items. */
function compileRegistryProjection(reg: ElementRegistry): PaletteGroup[] {
  return reg.sections.map((sec) => ({
    label: sec.label,
    advanced: sec.advanced || undefined,
    items: reg.entries
      .filter((e) => e.section === sec.id)
      .map((e): PaletteItem => {
        const entry = paletteEntry(e.key)
        return { key: e.key, label: e.label, iconType: entry.iconType, editor: e.editor }
      }),
  }))
}

/** The registry entry's editor kind for a ref (`structured` if the ref isn't in the registry). */
function editorOf(reg: ElementRegistry, ref: string): 'structured' | 'raw' {
  return reg.entries.find((e) => e.key === ref)?.editor ?? 'structured'
}

function compile(m: LensManifest, reg: ElementRegistry): LensSpec {
  const behavior = m.behavior ? resolveBehavior(m.behavior) : undefined

  // palette: precompute a stable PaletteGroup[] once, then hand back a closure (the runtime shape
  // is `(all) => PaletteGroup[]`; the projection is fixed, so `all` is ignored).
  const groups = m.palette === '@registry' ? compileRegistryProjection(reg) : compileProjection(m.palette, reg)
  const palette = (_all: PaletteEntry[]) => groups

  const glyphByRole = m.glyphByRole
  const glyphOf = glyphByRole
    ? (role: string | null | undefined): NodeShape => (role && glyphByRole[role]) || 'default'
    : undefined

  const roleOrder = m.preferredResult?.roleOrder
  const preferredResultId = roleOrder
    ? (doc: Parameters<NonNullable<LensSpec['preferredResultId']>>[0], outputIds: string[]) => {
        for (const role of roleOrder) {
          const hit = doc.elements.find((e) => e.lens_role === role && outputIds.includes(e.id))
          if (hit) return hit.id
        }
        return null
      }
    : undefined

  const templates = m.templates?.map((id) => {
    const t = TEMPLATES[id]
    if (!t) throw new Error(`Lens "${m.id}" references unknown template "${id}"`)
    return t
  })

  return {
    id: m.id,
    label: m.label,
    tagline: m.tagline,
    palette,
    roleLabels: m.roleLabels,
    glyphOf,
    templates,
    preferredResultId,
    invariants: behavior?.invariants,
    connect: behavior?.connect,
    resultReadouts: behavior?.resultReadouts,
  }
}

// ── Public surface — identical to the old registry.ts ────────────────────────────

/** Every built-in lens, compiled once, keyed by id. */
export const LENSES: Record<string, LensSpec> = Object.fromEntries(
  BUILTIN_MANIFESTS.map((m) => [m.id, compile(m, REGISTRY)]),
)

export const DEFAULT_LENS: LensSpec = LENSES['general']

/** Every registered lens, for the picker (manifest declaration order). */
export function listLenses(): LensSpec[] {
  return BUILTIN_MANIFESTS.map((m) => LENSES[m.id])
}

/** Resolve a (possibly absent or unknown) lens id to a concrete spec, defaulting to `general`.
 *  Returns a stable reference per id — safe to use directly in a store selector. */
export function resolveLens(id: string | null | undefined): LensSpec {
  if (id && LENSES[id]) return LENSES[id]
  return DEFAULT_LENS
}
