import { describe, it, expect, beforeEach, vi } from 'vitest'
import { registerManifest, unregisterManifest, resolveLens, listLenses, LENSES } from './loader'
import { importManifest, removeManifest, parseManifest, registerPersisted } from './customLenses'
import type { LensManifest } from './manifest-types'

/**
 * P6: user-imported custom lenses (§7). A manifest is pure data — it re-themes over the built-in
 * registry and may only name a built-in behavior. These tests cover the loader's dynamic layer
 * (register/override/unregister with stable refs) and the import/persist round-trip.
 */

// A minimal in-memory localStorage for the persistence tests (node has none).
function installStorage() {
  const map = new Map<string, string>()
  const mock = {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
    key: () => null,
    length: 0,
  }
  vi.stubGlobal('localStorage', mock)
  return map
}

const relabelLens: LensManifest = {
  version: '1.0.0',
  id: 'my-relabel',
  label: 'My Relabel',
  tagline: 'A relabel-only custom lens (no behavior).',
  palette: [{ section: 'Things', items: [{ ref: 'constant', label: 'Widget', role: 'widget' }] }],
  roleLabels: { widget: 'Widget' },
}

beforeEach(() => {
  unregisterManifest('my-relabel')
  vi.unstubAllGlobals()
})

describe('loader — custom lens registration', () => {
  it('registers a relabel-only lens and it resolves + lists', () => {
    const spec = registerManifest(relabelLens)
    expect(resolveLens('my-relabel')).toBe(spec)
    expect(spec.label).toBe('My Relabel')
    expect(spec.invariants).toBeUndefined() // no behavior → declarative only
    expect(listLenses().map((l) => l.id)).toContain('my-relabel')
    // Palette projection uses the registry entry's iconType for `constant`.
    const groups = spec.palette([])
    expect(groups).toEqual([
      { label: 'Things', advanced: undefined, items: [{ key: 'constant', label: 'Widget', iconType: 'constant', lensRole: 'widget', editor: 'structured' }] },
    ])
  })

  it('returns a stable reference until re-registered', () => {
    const a = registerManifest(relabelLens)
    expect(resolveLens('my-relabel')).toBe(a)
    expect(resolveLens('my-relabel')).toBe(a) // stable across calls
  })

  it('a custom lens may name a built-in behavior', () => {
    const withBehavior = { ...relabelLens, id: 'my-sfc', behavior: 'stock-flow' as const }
    const spec = registerManifest(withBehavior)
    expect(spec.invariants).toBeTypeOf('function')
    expect(spec.connect).toBeTypeOf('function')
    unregisterManifest('my-sfc')
  })

  it('rejects an unknown behavior (never registers broken code)', () => {
    expect(() => registerManifest({ ...relabelLens, id: 'bad', behavior: 'nope' })).toThrow(/behavior/i)
    expect(LENSES['bad']).toBeUndefined()
  })

  it('rejects a dangling palette ref', () => {
    expect(() =>
      registerManifest({ ...relabelLens, id: 'bad2', palette: [{ section: 'X', items: [{ ref: 'no_such_key' }] }] }),
    ).toThrow(/no_such_key/)
  })

  it('unregister removes a custom lens; built-ins are protected', () => {
    registerManifest(relabelLens)
    unregisterManifest('my-relabel')
    expect(resolveLens('my-relabel').id).toBe('general') // falls back
    unregisterManifest('general') // no-op on a built-in
    expect(resolveLens('general').id).toBe('general')
  })

  it('a custom lens can override a built-in by id, then restore is not automatic', () => {
    const override = { ...relabelLens, id: 'stock-flow', label: 'My Stock-Flow' }
    registerManifest(override)
    expect(resolveLens('stock-flow').label).toBe('My Stock-Flow')
    // Listing does not duplicate the overridden id.
    expect(listLenses().filter((l) => l.id === 'stock-flow')).toHaveLength(1)
    // Cleanup: unregister does NOT restore the built-in (documented limitation) — re-register it.
    unregisterManifest('stock-flow')
  })
})

describe('customLenses — import + persist round-trip', () => {
  it('parseManifest validates structure and defaults version/tagline', () => {
    const m = parseManifest(JSON.stringify({ id: 'x', label: 'X', palette: '@registry' }))
    expect(m.version).toBe('1.0.0')
    expect(m.tagline).toBe('X')
    expect(() => parseManifest('{ not json')).toThrow(/JSON/)
    expect(() => parseManifest(JSON.stringify({ label: 'no id', palette: '@registry' }))).toThrow(/id/)
    expect(() => parseManifest(JSON.stringify({ id: 'y', label: 'Y' }))).toThrow(/palette/)
  })

  it('imports, persists, and re-registers from storage', () => {
    const store = installStorage()
    const { manifest, next } = importManifest(JSON.stringify(relabelLens), [])
    expect(manifest.id).toBe('my-relabel')
    expect(next).toHaveLength(1)
    expect(store.get('wasim.customLenses.v1')).toContain('my-relabel')
    expect(resolveLens('my-relabel').label).toBe('My Relabel')

    // Simulate a fresh load: unregister from memory, then registerPersisted() reads storage back.
    unregisterManifest('my-relabel')
    expect(resolveLens('my-relabel').id).toBe('general')
    const restored = registerPersisted()
    expect(restored.map((m) => m.id)).toContain('my-relabel')
    expect(resolveLens('my-relabel').label).toBe('My Relabel')

    // Remove clears storage + registration.
    const after = removeManifest('my-relabel', next)
    expect(after).toHaveLength(0)
    expect(resolveLens('my-relabel').id).toBe('general')
    unregisterManifest('my-relabel')
  })
})
