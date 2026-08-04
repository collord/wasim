import { describe, it, expect } from 'vitest'
import type { ModelDoc, FlatElement } from '../../model/schema'
import type { ModelSummary } from '../../types'
import { lensHint, activeLensId, ROOT_OVERRIDE_KEY } from './lensHint'
import { computeRolesForLens, withComputedRoles } from './roles'
import { SIGNATURES } from './signatures'

import { stockFlowBehavior } from '../behaviors/stockFlow'
import { metapopBehavior } from '../behaviors/metapop'
import { reliabilityBehavior } from '../behaviors/reliability'
import { decisionBehavior } from '../behaviors/decision'
import { STOCK_FLOW_TEMPLATES } from '../stockFlowTemplates'
import { METAPOP_TEMPLATES } from '../metapopTemplates'
import { RELIABILITY_TEMPLATES } from '../reliabilityTemplates'
import { DECISION_TEMPLATES } from '../decisionTemplates'

const SUMMARY = {} as unknown as ModelSummary
const doc = (elements: FlatElement[], extra?: Partial<ModelDoc>): ModelDoc =>
  ({
    wasim_version: '0.1.0',
    simulation_settings: { duration: { value: 1, unit: 'd' }, timestep: { value: 1, unit: 'd' } },
    elements,
    ...extra,
  } as unknown as ModelDoc)

const stock = (id: string, inflows: string[] = [], outflows: string[] = []): FlatElement =>
  ({ id, name: id, primitive: 'stock', initial_value: { value: 0, unit: '1' }, inflows, outflows } as FlatElement)
const node = (id: string, value_rule = 'expression', extra: Partial<FlatElement> = {}): FlatElement =>
  ({ id, name: id, primitive: 'node', value_rule, ...extra } as FlatElement)
const event = (id: string, extra: Partial<FlatElement> = {}): FlatElement =>
  ({ id, name: id, primitive: 'event', ...extra } as FlatElement)

describe('lensHint — dominant paradigm per container', () => {
  it('classifies a stock/flow graph as stock-flow', () => {
    const d = doc([stock('water', ['tap'], ['drain']), node('tap', 'fixed'), node('drain')])
    expect(lensHint(null, d).lens).toBe('stock-flow')
  })

  it('classifies a fault-tree (events under a gate) as reliability, not discrete-event', () => {
    const d = doc([
      event('fa'), event('fb'),
      { id: 'sys', name: 'sys', primitive: 'gate', root: { op: 'and', children: [{ op: 'reference', reference: 'fa' }, { op: 'reference', reference: 'fb' }] }, inputs: ['fa', 'fb'] } as FlatElement,
    ])
    const h = lensHint(null, d)
    expect(h.lens).toBe('reliability')
  })

  it('classifies an on_event chain of events/latches as discrete-event, not reliability', () => {
    const d = doc([
      event('gen', { trigger: { mode: 'on_condition' } }),
      event('eff', { trigger: { mode: 'on_event', source: 'gen' } }),
      node('latch', 'status', { set: {}, reset: {} }),
    ])
    const h = lensHint(null, d)
    expect(h.lens).toBe('discrete-event')
  })

  it('lifts a coupled network above plain stock-flow (metapop)', () => {
    const d = doc(
      [
        stock('S', [], ['inf']), stock('I', ['inf'], []),
        node('inf'),
        node('W', 'expression', { outputs: [{ dimensions: ['patch', 'patch'] }] }),
        node('lam', 'expression', { inputs: ['W', 'I'] }),
      ],
      { dimensions: [{ id: 'patch', name: 'patch', size: 3 }] },
    )
    expect(lensHint(null, d).lens).toBe('metapop')
  })

  it('classifies an optimization study as decision', () => {
    const d = doc([node('x', 'fixed'), node('obj')], {
      optimization: { variables: [{ element_id: 'x' }], objective: { element_id: 'obj' } },
    } as Partial<ModelDoc>)
    expect(lensHint(null, d).lens).toBe('decision')
  })

  it('a controller-dense container is control-systems', () => {
    const d = doc([node('c1', 'pid'), node('c2', 'pid')])
    expect(lensHint(null, d).lens).toBe('control-systems')
  })

  it('empty / unmatched containers fall back to general', () => {
    expect(lensHint(null, doc([])).lens).toBe('general')
    expect(lensHint(null, doc([node('a', 'fixed'), node('b', 'fixed')])).lens).toBe('general')
  })

  it('scopes to the drilled container (interior membership)', () => {
    const d = doc([
      stock('water', ['tap'], []), node('tap', 'fixed'),
      { ...node('c1', 'pid'), container: 'sub' } as FlatElement,
      { ...node('c2', 'pid'), container: 'sub' } as FlatElement,
    ])
    expect(lensHint(null, d).lens).toBe('stock-flow')
    expect(lensHint('sub', d).lens).toBe('control-systems')
  })
})

describe('overlays — computed views, never a second membership', () => {
  it('surfaces control as an overlay when a lone controller sits on a stock-flow plant', () => {
    const d = doc([stock('lvl', ['fill'], ['drain']), node('fill', 'fixed'), node('drain'), node('ctrl', 'pid', { input: 'lvl' })])
    const h = lensHint(null, d)
    expect(h.lens).toBe('stock-flow')
    expect(h.overlays).toContain('control')
  })

  it('surfaces uncertainty when stochastic facets are present', () => {
    const d = doc([stock('x', ['g'], []), node('g', 'fixed'), node('rv', 'sample')])
    expect(lensHint(null, d).overlays).toContain('uncertainty')
  })
})

// ── Migration parity gate: computed roles must never CONTRADICT stored roles, and each migrated
//    lens must stay warning-clean on its own templates under `computed ?? stored`. ──────────────
const INVARIANTS = {
  'stock-flow': stockFlowBehavior.invariants!,
  metapop: metapopBehavior.invariants!,
  reliability: reliabilityBehavior.invariants!,
  decision: decisionBehavior.invariants!,
} as const

const TEMPLATE_SETS: { lens: keyof typeof INVARIANTS; templates: { id: string; build: () => ModelDoc }[] }[] = [
  { lens: 'stock-flow', templates: STOCK_FLOW_TEMPLATES },
  { lens: 'metapop', templates: METAPOP_TEMPLATES },
  { lens: 'reliability', templates: RELIABILITY_TEMPLATES },
  { lens: 'decision', templates: DECISION_TEMPLATES },
]

describe('migration parity — computed roles are safe against the shipped templates', () => {
  for (const { lens, templates } of TEMPLATE_SETS) {
    for (const tpl of templates) {
      it(`[${lens}] "${tpl.id}": computed roles never contradict stamped roles`, () => {
        const built = tpl.build()
        const computed = computeRolesForLens(built, lens)
        for (const e of built.elements) {
          const c = computed.get(e.id)
          // Where structure computes a role AND the element was stamped, they must agree.
          if (c != null && e.lens_role != null) expect(c).toBe(e.lens_role)
        }
      })

      it(`[${lens}] "${tpl.id}": stays warning-clean under computed ?? stored roles`, () => {
        const built = tpl.build()
        const roled = withComputedRoles(built, lens)
        expect(INVARIANTS[lens](SUMMARY, roled)).toEqual([])
      })
    }
  }
})

describe('activeLensId — priority: explicit view.lens > hint > general', () => {
  it('honors an explicit authored/picked view.lens (non-regressive)', () => {
    const d = doc([node('c1', 'pid')], { view: { lens: 'reliability' } } as Partial<ModelDoc>)
    expect(activeLensId(d, null)).toBe('reliability') // explicit wins over the pid hint
  })

  it('falls back to the computed hint when no view.lens (imported model)', () => {
    const d = doc([stock('water', ['tap'], []), node('tap', 'fixed')])
    expect(activeLensId(d, null)).toBe('stock-flow')
  })

  it('re-lenses to the drilled submodel container', () => {
    const d = doc([
      stock('water', ['tap'], []), node('tap', 'fixed'),
      { ...node('c1', 'pid'), container: 'sub' } as FlatElement,
      { ...node('c2', 'pid'), container: 'sub' } as FlatElement,
    ])
    expect(activeLensId(d, null)).toBe('stock-flow')
    expect(activeLensId(d, 'sub')).toBe('control-systems')
  })

  it('returns general for a null doc', () => {
    expect(activeLensId(null, null)).toBe('general')
  })
})

describe('activeLensId — per-container overrides win over view.lens and the hint', () => {
  const drilled = () => doc([
    stock('water', ['tap'], []), node('tap', 'fixed'),
    { ...node('c1', 'pid'), container: 'sub' } as FlatElement,
    { ...node('c2', 'pid'), container: 'sub' } as FlatElement,
  ])

  it('a container override beats the computed hint for that container only', () => {
    const d = drilled()
    // The `sub` container hints control-systems; override it to reliability for `sub` alone.
    expect(activeLensId(d, 'sub', { sub: 'reliability' })).toBe('reliability')
    // Root is untouched by the `sub` override — still its own hint.
    expect(activeLensId(d, null, { sub: 'reliability' })).toBe('stock-flow')
  })

  it('a container override beats an authored whole-doc view.lens', () => {
    const d = doc([node('c1', 'pid')], { view: { lens: 'reliability' } } as Partial<ModelDoc>)
    expect(activeLensId(d, null, {})).toBe('reliability') // no override → view.lens
    expect(activeLensId(d, 'sub', { sub: 'metapop' })).toBe('metapop') // override wins
  })

  it('an absent override key falls through to the normal resolution', () => {
    const d = drilled()
    expect(activeLensId(d, 'sub', { other: 'reliability' })).toBe('control-systems')
  })

  it('the root override key is honored (defensive; the picker routes root to view.lens)', () => {
    const d = doc([stock('water', ['tap'], []), node('tap', 'fixed')])
    expect(activeLensId(d, null, { [ROOT_OVERRIDE_KEY]: 'decision' })).toBe('decision')
  })
})

describe('withComputedRoles bridge', () => {
  it('fills in roles on a fully-untagged (imported-style) doc', () => {
    const d = doc([stock('water', ['tap'], []), node('tap', 'fixed')])
    const roled = withComputedRoles(d, 'stock-flow')
    expect(roled.elements.find((e) => e.id === 'water')?.lens_role).toBe('stock')
    expect(roled.elements.find((e) => e.id === 'tap')?.lens_role).toBe('flow')
  })

  it('leaves an already-tagged (authored) doc untouched — stored roles authoritative', () => {
    const tagged = doc([{ ...stock('water', ['tap'], []), lens_role: 'stock' } as FlatElement, node('tap', 'fixed')])
    expect(withComputedRoles(tagged, 'stock-flow')).toBe(tagged) // same reference: no clone
  })
})

describe('metapop role precision — the scalar-coupling ambiguity is left to stored roles', () => {
  const dims: ModelDoc['dimensions'] = [{ id: 'Node', name: 'Node', size: 2 }, { id: 'NodeB', name: 'NodeB', size: 2 }]
  const arr = (id: string, over: string[]): FlatElement =>
    ({ id, name: id, primitive: 'node', value_rule: 'expression', outputs: [{ name: id, unit: '1', dimensions: over }] } as unknown as FlatElement)

  it('a square 2-D array is computed as `coupling`; a scalar weight is NOT (would collide with parameter)', () => {
    // Untagged import: a matrix coupling is unambiguous, a scalar mixing weight is indistinguishable
    // from β/γ — so the matrix gets `coupling` and the scalar is deliberately left unroled.
    const d = doc([
      stock('I', [], []),
      arr('W', ['Node', 'NodeB']),        // square matrix → coupling
      node('mix', 'fixed'),                // scalar weight → left to stored, not guessed
      node('beta', 'fixed'),               // a plain parameter, also a scalar `fixed`
    ], { dimensions: dims } as Partial<ModelDoc>)
    const roles = computeRolesForLens(d, 'metapop')
    expect(roles.get('W')).toBe('coupling')
    expect(roles.get('I')).toBe('compartment')
    expect(roles.has('mix')).toBe(false)   // structurally identical to `beta` — cannot be guessed
    expect(roles.has('beta')).toBe(false)
  })
})

describe('signatures are pure and bounded', () => {
  it('every score is in [0,1]', () => {
    const d = doc([stock('s', ['f'], []), node('f', 'fixed'), event('e'), node('p', 'pid')])
    for (const sig of SIGNATURES) {
      const s = sig.score(d.elements, d)
      expect(s).toBeGreaterThanOrEqual(0)
      expect(s).toBeLessThanOrEqual(1)
    }
  })
})
