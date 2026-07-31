import type { LensManifest } from './manifest-types'
import { registerManifest, unregisterManifest } from './loader'

/**
 * User-imported custom lenses (WASIM_LENS_MANIFEST_SPEC.md §7): a pure-data lens `.json` a user
 * imports via the file picker, merged over the built-ins by id. This is the browser-SPA delivery
 * of the spec's "project/user overlay" — there is no repo filesystem here, so lenses are imported
 * as files and persisted to `localStorage` rather than read from `.wasim/lenses/`.
 *
 * Safety: a manifest is pure data — it re-themes (palette/labels/glyphs/roles) and may only *name*
 * a built-in behavior, never supply code. `registerManifest` (the loader) rejects a manifest with
 * an unknown behavior or a dangling palette ref, so a broken/hostile file fails to load loudly.
 */

const STORAGE_KEY = 'wasim.customLenses.v1'

/** localStorage if available (absent under SSR / node test env), else null. */
function storage(): Storage | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null
  } catch {
    return null // access can throw in sandboxed iframes
  }
}

/** Structural validation before handing to the loader (which does ref/behavior validation). */
export function parseManifest(text: string): LensManifest {
  let obj: unknown
  try {
    obj = JSON.parse(text)
  } catch (e) {
    throw new Error(`Not valid JSON: ${(e as Error).message}`)
  }
  const m = obj as Partial<LensManifest>
  if (typeof m.id !== 'string' || !m.id) throw new Error('Manifest is missing a string "id".')
  if (typeof m.label !== 'string' || !m.label) throw new Error('Manifest is missing a string "label".')
  if (m.palette === undefined) throw new Error('Manifest is missing "palette".')
  if (m.palette !== '@registry' && !Array.isArray(m.palette)) {
    throw new Error('Manifest "palette" must be "@registry" or an array of sections.')
  }
  return {
    ...(m as LensManifest),
    version: typeof m.version === 'string' ? m.version : '1.0.0',
    tagline: typeof m.tagline === 'string' ? m.tagline : m.label,
  }
}

function readStored(): LensManifest[] {
  const s = storage()
  if (!s) return []
  try {
    const raw = s.getItem(STORAGE_KEY)
    if (!raw) return []
    const arr = JSON.parse(raw)
    return Array.isArray(arr) ? (arr as LensManifest[]) : []
  } catch {
    return [] // corrupt storage → start clean rather than crash the app
  }
}

function writeStored(manifests: LensManifest[]): void {
  const s = storage()
  if (!s) return
  try {
    s.setItem(STORAGE_KEY, JSON.stringify(manifests))
  } catch {
    // Storage full / unavailable (private mode) — the lens still works this session, just isn't
    // persisted. Silent: not worth failing the import over.
  }
}

/** The persisted custom manifests (data only), for the store's initial state. */
export function loadPersistedManifests(): LensManifest[] {
  return readStored()
}

/**
 * Register every persisted custom manifest into the loader at startup. Runs synchronously before
 * first render (called from the store's module init), so imported lenses are in the picker on load
 * with no async bootstrap. A manifest that fails to compile is skipped (and logged), not fatal.
 */
export function registerPersisted(): LensManifest[] {
  const ok: LensManifest[] = []
  for (const m of readStored()) {
    try {
      registerManifest(m)
      ok.push(m)
    } catch (e) {
      console.warn(`Skipping invalid stored lens "${m?.id}": ${(e as Error).message}`)
    }
  }
  return ok
}

/** Import a manifest from raw JSON text: validate, register, persist. Returns the manifest.
 *  Throws (before persisting) if invalid, so the caller can surface the error. */
export function importManifest(text: string, existing: LensManifest[]): { manifest: LensManifest; next: LensManifest[] } {
  const manifest = parseManifest(text)
  registerManifest(manifest) // throws on dangling ref / unknown behavior
  const next = [...existing.filter((m) => m.id !== manifest.id), manifest]
  writeStored(next)
  return { manifest, next }
}

/** Remove a custom manifest by id: unregister + persist. */
export function removeManifest(id: string, existing: LensManifest[]): LensManifest[] {
  unregisterManifest(id)
  const next = existing.filter((m) => m.id !== id)
  writeStored(next)
  return next
}
