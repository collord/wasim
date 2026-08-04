import { describe, it, expect } from 'vitest'
import { createSubmodelFromSelection } from './edits'
import type { ModelDoc, FlatElement } from './schema'

const base = (elements: FlatElement[]): ModelDoc => ({
  wasim_version: '0.1.0',
  simulation_settings: { duration: { value: 10, unit: 's' }, timestep: { value: 1, unit: 's' }, n_realizations: 1, sampling_method: 'monte_carlo', seed: 7 },
  elements,
})
const el = (id: string, extra: Partial<FlatElement> = {}): FlatElement => ({ id, name: id, primitive: 'node', value_rule: 'expression', ...extra })

describe('createSubmodelFromSelection (Monte-Carlo loop)', () => {
  it('creates a submodel container with n_realizations and reparents the selection', () => {
    const d = base([el('a'), el('b'), el('c')])
    const next = createSubmodelFromSelection(d, ['a', 'b'], { nRealizations: 500, name: 'MC' })
    const c = next.containers?.[0]
    expect(c?.kind).toBe('submodel')
    expect(c?.simulation_settings?.n_realizations).toBe(500)
    expect(c?.simulation_settings?.duration).toEqual({ value: 10, unit: 's' }) // inherits parent clock
    expect(next.elements.find((e) => e.id === 'a')?.container).toBe(c!.id)
    expect(next.elements.find((e) => e.id === 'b')?.container).toBe(c!.id)
    expect(next.elements.find((e) => e.id === 'c')?.container ?? null).toBeNull()
  })

  it('auto-derives the boundary interface from cross-boundary references', () => {
    // interior {inner} reads exterior `drv`; exterior `sink` reads interior `inner`.
    const d = base([
      el('drv', { value_rule: 'fixed' }),
      el('inner', { inputs: ['drv'] }),
      el('sink', { inputs: ['inner'] }),
    ])
    const next = createSubmodelFromSelection(d, ['inner'])
    const iface = next.containers?.[0].interface
    expect(iface?.inputs).toEqual([{ input: 'inner', from: 'drv' }])
    expect(iface?.outputs).toEqual(['inner'])
  })

  it('yields a null interface for a self-contained selection (bare N-times wrapper)', () => {
    const d = base([el('x', { value_rule: 'fixed' }), el('y', { inputs: ['x'] })])
    const next = createSubmodelFromSelection(d, ['x', 'y'], { nRealizations: 1000 })
    expect(next.containers?.[0].interface).toBeNull()
    expect(next.containers?.[0].simulation_settings?.n_realizations).toBe(1000)
  })

  it('is a no-op (same ref) for an empty / unknown selection, and does not mutate input', () => {
    const d = base([el('a')])
    expect(createSubmodelFromSelection(d, [])).toBe(d)
    expect(createSubmodelFromSelection(d, ['ghost'])).toBe(d)
    createSubmodelFromSelection(d, ['a'])
    expect(d.elements[0].container).toBeUndefined() // input untouched
    expect(d.containers).toBeUndefined()
  })

  it('defaults to 100 realizations when unspecified', () => {
    const next = createSubmodelFromSelection(base([el('a')]), ['a'])
    expect(next.containers?.[0].simulation_settings?.n_realizations).toBe(100)
  })
})
