import type { ModelDoc, FlatElement } from '../model/schema'
import type { ModelTemplate } from './types'
import { parseExpr, printAst, refsOf } from '../model/ast'

// Canonical reliability starting models. These mirror the proven single-truck RAM pattern
// (a damage state accumulates; a condition-basis failure FSM trips when it crosses a threshold),
// tagged with the lens + per-element lens_role so they open in-lens.

const SAVE = { time_history: true, final_value: true }

function settings(duration: number) {
  return {
    duration: { value: duration, unit: 's' },
    timestep: { value: 1, unit: 's' },
    n_realizations: 1,
    sampling_method: 'monte_carlo' as const,
    seed: 42,
  }
}

/** Run-to-failure component: damage accrues at a constant wear rate; the component fails when
 *  cumulative damage reaches 1 (output 0 = operating, 1 = failed). The simplest reliability
 *  model that runs on the current engine (STAGE1_RAM_DEMO). */
function runToFailure(): ModelDoc {
  const cond = parseExpr('damage >= 1')
  const component: FlatElement = {
    id: 'component', name: 'Component', primitive: 'event',
    trigger: { mode: 'on_condition', condition: { ast: cond, display: printAst(cond) } },
    failure_process: { basis: 'condition', repair: { policy: 'none' } },
    effects: [], inputs: [...refsOf(cond)],
    lens_role: 'component', save_results: SAVE,
  }
  return {
    wasim_version: '0.1.0',
    simulation_settings: settings(20),
    elements: [
      { id: 'wear', name: 'Wear rate', primitive: 'node', value_rule: 'fixed', value: { value: 0.1, unit: '1' }, editable: true, bounds: { min: 0, max: 1 }, lens_role: 'parameter', save_results: SAVE },
      { id: 'damage', name: 'Damage', primitive: 'stock', initial_value: { value: 0, unit: '1' }, inflows: ['wear'], outflows: [], lens_role: 'state', save_results: SAVE },
      component,
    ],
    view: {
      lens: 'reliability', authored: true,
      positions: { wear: { x: 90, y: 90 }, damage: { x: 340, y: 160 }, component: { x: 600, y: 160 } },
    },
  }
}

/** A component = wear parameter + damage state + condition-basis failure FSM. */
function component(suffix: string, label: string, wearRate: number): FlatElement[] {
  const cond = parseExpr(`damage_${suffix} >= 1`)
  return [
    { id: `wear_${suffix}`, name: `Wear ${label}`, primitive: 'node', value_rule: 'fixed', value: { value: wearRate, unit: '1' }, editable: true, bounds: { min: 0, max: 1 }, lens_role: 'parameter', save_results: SAVE },
    { id: `damage_${suffix}`, name: `Damage ${label}`, primitive: 'stock', initial_value: { value: 0, unit: '1' }, inflows: [`wear_${suffix}`], outflows: [], lens_role: 'state', save_results: SAVE },
    {
      id: `fsm_${suffix}`, name: `Component ${label}`, primitive: 'event',
      trigger: { mode: 'on_condition', condition: { ast: cond, display: printAst(cond) } },
      failure_process: { basis: 'condition', repair: { policy: 'none' } },
      effects: [], inputs: [...refsOf(cond)], lens_role: 'component', save_results: SAVE,
    },
  ]
}

/** 1-of-2 redundant system: two components with different wear rates, and a gate that fails only
 *  when BOTH have failed (AND over their failure states) — parallel redundancy. The system
 *  survives the first failure and trips when the second occurs. */
function redundant(): ModelDoc {
  return {
    wasim_version: '0.1.0',
    simulation_settings: settings(20),
    elements: [
      ...component('a', 'A', 0.15),
      ...component('b', 'B', 0.1),
      {
        id: 'system', name: 'System (1-of-2)', primitive: 'gate', semantics: 'success',
        root: { op: 'and', children: [{ op: 'reference', reference: 'fsm_a' }, { op: 'reference', reference: 'fsm_b' }] },
        inputs: ['fsm_a', 'fsm_b'], lens_role: 'redundancy', save_results: SAVE,
      },
    ],
    view: {
      lens: 'reliability', authored: true,
      positions: {
        wear_a: { x: 60, y: 70 }, damage_a: { x: 290, y: 70 }, fsm_a: { x: 520, y: 70 },
        wear_b: { x: 60, y: 290 }, damage_b: { x: 290, y: 290 }, fsm_b: { x: 520, y: 290 },
        system: { x: 760, y: 180 },
      },
    },
  }
}

export const RELIABILITY_TEMPLATES: ModelTemplate[] = [
  { id: 'run-to-failure', label: 'Repairable component', description: 'Damage accrues to a failure threshold — a condition-basis failure FSM. Run-to-failure.', build: runToFailure },
  { id: 'redundant', label: '1-of-2 redundant system', description: 'Two components + a gate that trips only when both fail — parallel redundancy.', build: redundant },
]
