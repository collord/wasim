import { describe, it, expect } from 'vitest'
import type { ModelDoc } from '../../model/schema'
import { applyEditPatch } from './applyEdit'

/** A small doc: a constant `tap`, an expression `drain` referencing `water`, and a stock `water`. */
function doc(): ModelDoc {
  return {
    wasim_version: '0.1.0',
    simulation_settings: { duration: { value: 10, unit: 's' }, timestep: { value: 1, unit: 's' } },
    elements: [
      { id: 'tap', name: 'Tap', primitive: 'node', value_rule: 'fixed', value: { value: 5, unit: '1' } },
      { id: 'drain', name: 'Drain', primitive: 'node', value_rule: 'expression', expression: { ast: { op: 'ref', element_id: 'water' } }, inputs: ['water'] },
      { id: 'water', name: 'Water', primitive: 'stock', initial_value: { value: 10, unit: '1' }, inflows: ['tap'], outflows: ['drain'] },
    ],
  } as unknown as ModelDoc
}

describe('applyEditPatch — add', () => {
  it('adds a new element', () => {
    const { doc: next, error } = applyEditPatch(doc(), { op: 'add', element: { id: 'rain', name: 'Rain', primitive: 'node', value_rule: 'fixed', value: { value: 2, unit: '1' } } })
    expect(error).toBeUndefined()
    expect(next.elements.map((e) => e.id)).toContain('rain')
  })

  it('rejects a duplicate id', () => {
    const { error } = applyEditPatch(doc(), { op: 'add', element: { id: 'tap', name: 'X', primitive: 'node' } })
    expect(error).toMatch(/already exists/)
  })

  it('rejects an element with no id', () => {
    const { error } = applyEditPatch(doc(), { op: 'add', element: { name: 'X' } })
    expect(error).toMatch(/needs a string "id"/)
  })
})

describe('applyEditPatch — modify', () => {
  it('shallow-merges the patch', () => {
    const { doc: next, error } = applyEditPatch(doc(), { op: 'modify', id: 'tap', patch: { value: { value: 9, unit: '1' } } })
    expect(error).toBeUndefined()
    expect(next.elements.find((e) => e.id === 'tap')).toMatchObject({ value: { value: 9 } })
  })

  it('errors on a missing id (does not silently no-op)', () => {
    const { error } = applyEditPatch(doc(), { op: 'modify', id: 'ghost', patch: {} })
    expect(error).toMatch(/no element with id "ghost"/)
  })
})

describe('applyEditPatch — delete', () => {
  it('deletes and scrubs the flow ref from the stock', () => {
    const { doc: next, error } = applyEditPatch(doc(), { op: 'delete', id: 'tap' })
    expect(error).toBeUndefined()
    expect(next.elements.map((e) => e.id)).not.toContain('tap')
    // `tap` was a `water` inflow — deleteElement scrubs it.
    expect(next.elements.find((e) => e.id === 'water')!.inflows).not.toContain('tap')
  })

  it('errors on a missing id', () => {
    expect(applyEditPatch(doc(), { op: 'delete', id: 'ghost' }).error).toMatch(/no element/)
  })
})

describe('applyEditPatch — rename', () => {
  it('rewrites references including the expression AST', () => {
    const { doc: next, error } = applyEditPatch(doc(), { op: 'rename', id: 'water', newId: 'lake' })
    expect(error).toBeUndefined()
    expect(next.elements.map((e) => e.id)).toContain('lake')
    // drain's AST ref and inputs now point at `lake`.
    const drain = next.elements.find((e) => e.id === 'drain')!
    expect(drain.inputs).toContain('lake')
    expect(JSON.stringify(drain.expression)).toContain('lake')
  })

  it('rejects renaming onto a taken id', () => {
    expect(applyEditPatch(doc(), { op: 'rename', id: 'water', newId: 'tap' }).error).toMatch(/already taken/)
  })
})

describe('applyEditPatch — robustness (never throws)', () => {
  it('handles a non-object input', () => {
    expect(applyEditPatch(doc(), 'nope').error).toMatch(/must be an object/)
    expect(applyEditPatch(doc(), null).error).toBeTruthy()
  })

  it('handles an unknown op', () => {
    expect(applyEditPatch(doc(), { op: 'frobnicate' }).error).toMatch(/unknown op/)
  })

  it('leaves the doc unchanged on any error', () => {
    const before = doc()
    const { doc: after } = applyEditPatch(before, { op: 'delete', id: 'ghost' })
    expect(after).toBe(before) // same ref — no clone on the error path
  })
})
