import { describe, it, expect } from 'vitest'
import { resolveLens } from './loader'
import { metapopBehavior } from './behaviors/metapop'
import { METAPOP_TEMPLATES } from './metapopTemplates'
import type { ModelDoc, FlatElement } from '../model/schema'
import type { ModelSummary, SimulationResults } from '../types'

/**
 * Metapopulation lens (ABM_ON_WASIM.md §5.3): the behavior code that a pure-data manifest cannot
 * ship — conservation + coupling invariants, the transition↔compartment wiring gesture, and the
 * epidemic-curve readouts — plus the two canonical templates.
 */

const SUMMARY = {} as unknown as ModelSummary
const inv = (doc: ModelDoc) => metapopBehavior.invariants!(SUMMARY, doc)
const msgs = (doc: ModelDoc) => inv(doc).map((i) => i.message)
const doc = (elements: FlatElement[], dimensions?: ModelDoc['dimensions']): ModelDoc =>
  ({ wasim_version: '0.1.0', simulation_settings: { duration: { value: 1, unit: 'd' }, timestep: { value: 1, unit: 'd' } }, dimensions, elements } as unknown as ModelDoc)

describe('metapop templates', () => {
  it('both templates are registered on the lens by id', () => {
    expect(resolveLens('metapop').templates?.map((t) => t.id)).toEqual(['two-patch-sir', 'sir-on-network'])
  })

  it.each(METAPOP_TEMPLATES)('template "$id" builds and opens warning-clean under the invariants', (tpl) => {
    const built = tpl.build()
    expect(built.view?.lens).toBe('metapop')
    // Every template element carries a role the lens knows, and there is at least one compartment.
    expect(built.elements.some((e) => e.lens_role === 'compartment')).toBe(true)
    expect(inv(built)).toEqual([]) // zero warnings — the point of a canonical starting doc
  })

  it('two-patch tags compartments/transitions/coupling/mixing; network is dimensioned', () => {
    const two = METAPOP_TEMPLATES.find((t) => t.id === 'two-patch-sir')!.build()
    const roles = new Set(two.elements.map((e) => e.lens_role))
    expect(roles).toContain('compartment')
    expect(roles).toContain('transition')
    expect(roles).toContain('coupling')
    expect(roles).toContain('mixing')
    const net = METAPOP_TEMPLATES.find((t) => t.id === 'sir-on-network')!.build()
    expect(net.dimensions?.map((d) => d.id)).toEqual(['Node', 'NodeB'])
  })
})

describe('metapop invariants', () => {
  it('warns when a model has elements but no compartment', () => {
    expect(msgs(doc([{ id: 't', name: 'T', lens_role: 'transition' } as FlatElement]))).toEqual(
      expect.arrayContaining([expect.stringMatching(/No compartments/)]),
    )
  })

  it('flags a leak: a transition that adds to a compartment without draining another', () => {
    const d = doc([
      { id: 'birth', name: 'Birth', lens_role: 'transition' } as FlatElement,
      { id: 'I', name: 'Infected', lens_role: 'compartment', inflows: ['birth'], outflows: [] } as FlatElement,
    ])
    expect(msgs(d)).toEqual(expect.arrayContaining([expect.stringMatching(/leaks in/)]))
  })

  it('flags a transition connected to no compartment', () => {
    const d = doc([
      { id: 'flow', name: 'Flow', lens_role: 'transition' } as FlatElement,
      { id: 'S', name: 'S', lens_role: 'compartment', inflows: [], outflows: [] } as FlatElement,
    ])
    expect(msgs(d)).toEqual(expect.arrayContaining([expect.stringMatching(/isn't connected to any compartment/)]))
  })

  it('a conserving S→I→R chain is clean', () => {
    const d = doc([
      { id: 'inf', name: 'Infection', lens_role: 'transition' } as FlatElement,
      { id: 'rec', name: 'Recovery', lens_role: 'transition' } as FlatElement,
      { id: 'S', name: 'S', lens_role: 'compartment', inflows: [], outflows: ['inf'] } as FlatElement,
      { id: 'I', name: 'I', lens_role: 'compartment', inflows: ['inf'], outflows: ['rec'] } as FlatElement,
      { id: 'R', name: 'R', lens_role: 'compartment', inflows: ['rec'], outflows: [] } as FlatElement,
    ])
    expect(inv(d)).toEqual([])
  })

  it('warns when a mixing term references no coupling / no compartment (mean-field)', () => {
    const d = doc([
      { id: 'C', name: 'C', lens_role: 'compartment', inflows: [], outflows: [] } as FlatElement,
      { id: 'lam', name: 'Force', lens_role: 'mixing', inputs: [] } as FlatElement,
    ])
    expect(msgs(d)).toEqual(expect.arrayContaining([expect.stringMatching(/mean-field/)]))
  })

  it('a mixing term reaching a coupling and a compartment (transitively) is accepted', () => {
    const d = doc([
      { id: 'W', name: 'W', lens_role: 'coupling' } as FlatElement,
      { id: 'I', name: 'I', lens_role: 'compartment', inflows: [], outflows: [] } as FlatElement,
      { id: 'prod', name: 'prod', inputs: ['W', 'I'] } as FlatElement, // intermediate
      { id: 'lam', name: 'Force', lens_role: 'mixing', inputs: ['prod'] } as FlatElement,
    ])
    expect(msgs(d)).not.toEqual(expect.arrayContaining([expect.stringMatching(/mean-field/)]))
  })

  it('warns on a non-square coupling matrix', () => {
    const d = doc(
      [{ id: 'W', name: 'W', lens_role: 'coupling', outputs: [{ name: 'W', dimensions: ['Node', 'NodeB'] }] } as unknown as FlatElement,
       { id: 'C', name: 'C', lens_role: 'compartment', inflows: [], outflows: [] } as FlatElement],
      [{ id: 'Node', name: 'Node', size: 6 }, { id: 'NodeB', name: 'NodeB', size: 3 }],
    )
    expect(msgs(d)).toEqual(expect.arrayContaining([expect.stringMatching(/must be square/)]))
  })

  it('warns on a transposed reduction (mixing left on the neighbour axis)', () => {
    const d = doc(
      [
        { id: 'W', name: 'W', lens_role: 'coupling', outputs: [{ name: 'W', dimensions: ['Node', 'NodeB'] }] } as unknown as FlatElement,
        { id: 'I', name: 'I', lens_role: 'compartment', inflows: [], outflows: [] } as FlatElement,
        // mixing reduced the HOME axis, so it still carries NodeB (the neighbour axis) — the bug.
        { id: 'lam', name: 'Force', lens_role: 'mixing', inputs: ['W', 'I'], outputs: [{ name: 'lam', dimensions: ['NodeB'] }] } as unknown as FlatElement,
      ],
      [{ id: 'Node', name: 'Node', size: 6 }, { id: 'NodeB', name: 'NodeB', size: 6 }],
    )
    expect(msgs(d)).toEqual(expect.arrayContaining([expect.stringMatching(/transposed reduction/)]))
  })
})

describe('metapop connect gesture', () => {
  const base = doc([
    { id: 'inf', name: 'Infection', lens_role: 'transition' } as FlatElement,
    { id: 'S', name: 'S', lens_role: 'compartment', inflows: [], outflows: [] } as FlatElement,
  ])
  it('transition → compartment wires an inflow', () => {
    const next = metapopBehavior.connect!(base, 'inf', 'S')
    expect(next?.elements.find((e) => e.id === 'S')?.inflows).toEqual(['inf'])
  })
  it('compartment → transition wires an outflow', () => {
    const next = metapopBehavior.connect!(base, 'S', 'inf')
    expect(next?.elements.find((e) => e.id === 'S')?.outflows).toEqual(['inf'])
  })
  it('an unconnectable pair returns null', () => {
    expect(metapopBehavior.connect!(base, 'inf', 'inf')).toBeNull()
  })
})

describe('metapop readouts', () => {
  const d = doc([
    { id: 'beta', name: 'Transmission β', lens_role: 'parameter', value: { value: 0.4, unit: '1' } } as unknown as FlatElement,
    { id: 'gamma', name: 'Recovery γ', lens_role: 'parameter', value: { value: 0.1, unit: '1' } } as unknown as FlatElement,
    { id: 'S', name: 'Susceptible', lens_role: 'compartment' } as FlatElement,
    { id: 'I', name: 'Infected', lens_role: 'compartment' } as FlatElement,
    { id: 'R', name: 'Recovered', lens_role: 'compartment' } as FlatElement,
  ])
  const results = {
    time_axis: [0, 1, 2, 3, 4],
    time_unit: 'd',
    elements: {
      S: { label: 'Susceptible', unit: '1', final_values: [], time_history: { mean: [100, 80, 40, 20, 10] } },
      I: { label: 'Infected', unit: '1', final_values: [], time_history: { mean: [0, 20, 55, 30, 10] } },
      R: { label: 'Recovered', unit: '1', final_values: [], time_history: { mean: [0, 0, 5, 50, 80] } },
    },
  } as unknown as SimulationResults

  it('summarises the epidemic: peak prevalence, time-to-peak, attack rate, R₀', () => {
    const out = metapopBehavior.resultReadouts!(results, d)
    const summary = out.find((r) => r.id === 'metapop:summary')!
    const m = Object.fromEntries(summary.metrics.map((x) => [x.name, x.value]))
    expect(m['Peak prevalence']).toBe('55') // max of I over time
    expect(m['Time to peak']).toBe('2 d') // I peaks at index 2 → t=2
    expect(m['Attack rate']).toBe('80.0%') // final R (80) / initial total (100)
    expect(m['R₀ (β/γ)']).toBe('4') // 0.4 / 0.1
  })

  it('emits a per-compartment peak row for each compartment', () => {
    const out = metapopBehavior.resultReadouts!(results, d)
    expect(out.filter((r) => r.id !== 'metapop:summary').map((r) => r.id).sort()).toEqual(['I', 'R', 'S'])
  })
})
