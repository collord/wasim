import { describe, it, expect } from 'vitest'
import { resolveLens, listLenses, LENSES, DEFAULT_LENS } from './loader'
import { PALETTE } from '../model/edits'
import { LEGACY_LENSES } from './__parity__/legacyLenses'
import elementRegistry from './element-registry.json'
import type { ModelDoc } from '../model/schema'

/**
 * P3 parity gate (WASIM_LENS_MANIFEST_SPEC.md §13): the manifest loader reproduces the four
 * shipped lenses byte-for-byte in behavior. We compare the compiled projections against a frozen
 * snapshot of the old hand-written specs (`__parity__/legacyLenses.ts`). General is exempt from
 * the byte-identical palette check — it deliberately gained the full §5 taxonomy — so it gets its
 * own completeness assertion instead.
 */

const DOMAIN_IDS = ['stock-flow', 'reliability', 'decision'] as const

// A doc exercising every role, so preferredResultId agreement is meaningfully tested.
const fixtureDoc: ModelDoc = {
  wasim_version: '0.1.0',
  simulation_settings: { duration: { value: 1, unit: 's' }, timestep: { value: 1, unit: 's' } },
  elements: [
    { id: 'a', name: 'A', lens_role: 'stock' },
    { id: 'b', name: 'B', lens_role: 'flow' },
    { id: 'c', name: 'C', lens_role: 'auxiliary' },
    { id: 'd', name: 'D', lens_role: 'component' },
    { id: 'e', name: 'E', lens_role: 'redundancy' },
    { id: 'f', name: 'F', lens_role: 'state' },
    { id: 'g', name: 'G', lens_role: 'objective' },
    { id: 'h', name: 'H', lens_role: 'decision' },
    { id: 'i', name: 'I', lens_role: 'chance' },
  ],
} as unknown as ModelDoc
const allIds = fixtureDoc.elements.map((e) => e.id)

describe('manifest loader — parity with the hand-written specs', () => {
  it.each(DOMAIN_IDS)('%s palette projection is byte-identical', (id) => {
    const now = resolveLens(id).palette(PALETTE)
    const was = LEGACY_LENSES[id].palette(PALETTE)
    expect(now).toEqual(was)
  })

  it.each(DOMAIN_IDS)('%s metadata (id/label/tagline/roleLabels) is identical', (id) => {
    const now = resolveLens(id)
    const was = LEGACY_LENSES[id]
    expect(now.id).toBe(was.id)
    expect(now.label).toBe(was.label)
    expect(now.tagline).toBe(was.tagline)
    expect(now.roleLabels).toEqual(was.roleLabels)
  })

  it.each(DOMAIN_IDS)('%s glyphOf agrees for every role and unknowns', (id) => {
    const now = resolveLens(id).glyphOf
    const was = LEGACY_LENSES[id].glyphOf
    const roles = [...Object.keys(resolveLens(id).roleLabels ?? {}), null, undefined, 'nope']
    for (const r of roles) expect(now?.(r)).toBe(was?.(r))
  })

  it.each(DOMAIN_IDS)('%s preferredResultId agrees', (id) => {
    const now = resolveLens(id).preferredResultId
    const was = LEGACY_LENSES[id].preferredResultId
    expect(now?.(fixtureDoc, allIds)).toBe(was?.(fixtureDoc, allIds))
    // Also with a restricted output set (reliability's redundancy→component fallback order).
    expect(now?.(fixtureDoc, ['d'])).toBe(was?.(fixtureDoc, ['d']))
  })

  it.each(DOMAIN_IDS)('%s templates match by id', (id) => {
    expect(resolveLens(id).templates?.map((t) => t.id)).toEqual(LEGACY_LENSES[id].templates?.map((t) => t.id))
  })
})

describe('manifest loader — behavior wiring', () => {
  it('stock-flow has invariants + connect, no readouts', () => {
    const l = resolveLens('stock-flow')
    expect(l.invariants).toBeTypeOf('function')
    expect(l.connect).toBeTypeOf('function')
    expect(l.resultReadouts).toBeUndefined()
  })
  it('reliability has invariants + readouts, no connect', () => {
    const l = resolveLens('reliability')
    expect(l.invariants).toBeTypeOf('function')
    expect(l.resultReadouts).toBeTypeOf('function')
    expect(l.connect).toBeUndefined()
  })
  it('decision has invariants + readouts', () => {
    const l = resolveLens('decision')
    expect(l.invariants).toBeTypeOf('function')
    expect(l.resultReadouts).toBeTypeOf('function')
  })
  it('general has no behavior', () => {
    const l = resolveLens('general')
    expect(l.invariants).toBeUndefined()
    expect(l.connect).toBeUndefined()
    expect(l.resultReadouts).toBeUndefined()
  })
})

describe('manifest loader — reference stability (store-selector contract)', () => {
  it('resolveLens returns the same object per id', () => {
    for (const id of ['general', 'stock-flow', 'reliability', 'decision']) {
      expect(resolveLens(id)).toBe(resolveLens(id))
      expect(resolveLens(id)).toBe(LENSES[id])
    }
  })
  it('unknown / absent ids fall back to the stable general spec', () => {
    expect(resolveLens('nope')).toBe(DEFAULT_LENS)
    expect(resolveLens(null)).toBe(DEFAULT_LENS)
    expect(resolveLens(undefined)).toBe(DEFAULT_LENS)
    expect(DEFAULT_LENS.id).toBe('general')
  })
  it('the palette projection is a stable array (no fresh object per call)', () => {
    const l = resolveLens('stock-flow')
    expect(l.palette(PALETTE)).toBe(l.palette(PALETTE))
  })
})

describe('General lens — §5 completeness', () => {
  it('lists every registry construct exactly once, grouped by section order', () => {
    const groups = resolveLens('general').palette(PALETTE)
    // Section order matches the registry.
    expect(groups.map((g) => g.label)).toEqual(elementRegistry.sections.map((s) => s.label))
    // Every registry entry key appears exactly once across the palette.
    const keys = groups.flatMap((g) => g.items.map((i) => i.key)).sort()
    const expected = elementRegistry.entries.map((e) => e.key).sort()
    expect(keys).toEqual(expected)
  })
  it('every registry entry key resolves to a real PALETTE entry (no dangling refs)', () => {
    for (const e of elementRegistry.entries) {
      expect(PALETTE.find((p) => p.key === e.key), `missing PALETTE entry for "${e.key}"`).toBeTruthy()
    }
  })
})

describe('picker', () => {
  it('lists all four lenses in manifest declaration order', () => {
    expect(listLenses().map((l) => l.id)).toEqual(['stock-flow', 'reliability', 'decision', 'general'])
  })
})
