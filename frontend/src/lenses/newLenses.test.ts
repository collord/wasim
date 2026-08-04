import { describe, it, expect } from 'vitest'
import { resolveLens } from './loader'
import { discreteEventBehavior } from './behaviors/discreteEvent'
import { controlBehavior } from './behaviors/control'
import type { ModelDoc, FlatElement } from '../model/schema'
import type { ModelSummary } from '../types'

const SUMMARY = {} as unknown as ModelSummary
const doc = (elements: FlatElement[]): ModelDoc =>
  ({ wasim_version: '0.1.0', simulation_settings: { duration: { value: 1, unit: 's' }, timestep: { value: 1, unit: 's' } }, elements } as unknown as ModelDoc)
const deInv = (d: ModelDoc) => discreteEventBehavior.invariants!(SUMMARY, d).map((i) => i.message)
const ctrlInv = (d: ModelDoc) => controlBehavior.invariants!(SUMMARY, d).map((i) => i.message)

describe('the two new lenses register and resolve', () => {
  it('discrete-event and control-systems are resolvable with their behaviors attached', () => {
    expect(resolveLens('discrete-event').id).toBe('discrete-event')
    expect(resolveLens('discrete-event').invariants).toBeTruthy()
    expect(resolveLens('control-systems').id).toBe('control-systems')
    expect(resolveLens('control-systems').invariants).toBeTruthy()
  })
})

describe('discrete-event invariants', () => {
  it('warns when a non-empty model has no events or latches', () => {
    expect(deInv(doc([{ id: 'x', name: 'X', primitive: 'node', value_rule: 'fixed' } as FlatElement]))).toEqual([
      'No events — add an Event or a Latch to model a discrete-event process.',
    ])
  })

  it('is clean for a model with events / latches', () => {
    expect(deInv(doc([
      { id: 'gen', name: 'Gen', primitive: 'event', trigger: { mode: 'on_condition' } } as FlatElement,
      { id: 'l', name: 'L', primitive: 'node', value_rule: 'status', set: {}, reset: {} } as FlatElement,
    ]))).toEqual([])
  })

  it('flags a broken on_event chain (dangling source)', () => {
    const msgs = deInv(doc([
      { id: 'eff', name: 'Effect', primitive: 'event', trigger: { mode: 'on_event', source: 'ghost' } } as FlatElement,
    ]))
    expect(msgs.some((m) => m.includes("fires on \"ghost\""))).toBe(true)
  })
})

describe('control-systems invariants', () => {
  it('warns when a non-empty model has no controller', () => {
    expect(ctrlInv(doc([{ id: 's', name: 'S', primitive: 'stock' } as FlatElement]))).toEqual([
      'No controller — add a Controller (PID / on-off) to model feedback control.',
    ])
  })

  it('is clean for a controller whose measured input resolves', () => {
    expect(ctrlInv(doc([
      { id: 'lvl', name: 'Level', primitive: 'stock' } as FlatElement,
      { id: 'c', name: 'Ctrl', primitive: 'node', value_rule: 'pid', input: 'lvl' } as FlatElement,
    ]))).toEqual([])
  })

  it('flags an open loop (controller reads a missing element)', () => {
    const msgs = ctrlInv(doc([{ id: 'c', name: 'Ctrl', primitive: 'node', value_rule: 'pid', input: 'ghost' } as FlatElement]))
    expect(msgs.some((m) => m.includes('reads "ghost"'))).toBe(true)
  })
})

describe('new-lens invariants work on untagged (imported) models via loader injection', () => {
  it('discrete-event invariants run through resolveLens on an untagged model', () => {
    const d = doc([{ id: 'e', name: 'E', primitive: 'event', trigger: { mode: 'on_condition' } } as FlatElement])
    expect(resolveLens('discrete-event').invariants!(SUMMARY, d)).toEqual([])
  })
})
