import type { ModelDoc, FlatElement } from '../model/schema'
import type { ModelTemplate } from './types'
import { parseExpr, printAst, refsOf } from '../model/ast'

// Canonical stock-and-flow starting models. Each is a small, runnable, stock-flow-consistent doc
// tagged with the lens and per-element lens_role, so opening one lands directly in the lens with
// zero warnings. Expressions are built through the real parser so their ASTs / inputs are valid.

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

/** A flow/auxiliary defined by an expression, with `inputs` derived from the parsed refs. */
function exprEl(id: string, name: string, src: string, lensRole: string): FlatElement {
  const ast = parseExpr(src)
  return {
    id, name, primitive: 'node', value_rule: 'expression',
    expression: { ast, display: printAst(ast) },
    inputs: [...refsOf(ast)],
    lens_role: lensRole,
    save_results: SAVE,
  }
}

function constEl(id: string, name: string, value: number, lensRole: string, bounds?: { min: number; max: number }): FlatElement {
  return {
    id, name, primitive: 'node', value_rule: 'fixed',
    value: { value, unit: '1' },
    editable: true, ...(bounds ? { bounds } : {}),
    lens_role: lensRole, save_results: SAVE,
  }
}

function stockEl(id: string, name: string, initial: number, inflows: string[], outflows: string[]): FlatElement {
  return {
    id, name, primitive: 'stock',
    initial_value: { value: initial, unit: '1' },
    inflows, outflows,
    lens_role: 'stock', save_results: SAVE,
  }
}

/** Bathtub: a constant tap fills the tub while the drain leaks in proportion to the level — the
 *  canonical "flow into a stock" model. The level rises and asymptotes to tap/leak (50). */
function bathtub(): ModelDoc {
  return {
    wasim_version: '0.1.0',
    simulation_settings: settings(60),
    elements: [
      constEl('tap', 'Tap', 5, 'flow', { min: 0, max: 20 }),
      exprEl('drain', 'Drain', '0.1 * water', 'flow'),
      stockEl('water', 'Water level', 10, ['tap'], ['drain']),
    ],
    view: {
      lens: 'stock-flow', authored: true,
      positions: { tap: { x: 90, y: 80 }, water: { x: 340, y: 170 }, drain: { x: 90, y: 260 } },
    },
  }
}

/** Population growth: births flow in proportional to the population — exponential accumulation,
 *  the classic reinforcing loop. */
function population(): ModelDoc {
  return {
    wasim_version: '0.1.0',
    simulation_settings: settings(100),
    elements: [
      constEl('birth_rate', 'Birth rate', 0.03, 'auxiliary', { min: 0, max: 0.2 }),
      exprEl('births', 'Births', 'birth_rate * population', 'flow'),
      stockEl('population', 'Population', 100, ['births'], []),
    ],
    view: {
      lens: 'stock-flow', authored: true,
      positions: { birth_rate: { x: 90, y: 80 }, births: { x: 90, y: 220 }, population: { x: 360, y: 150 } },
    },
  }
}

export const STOCK_FLOW_TEMPLATES: ModelTemplate[] = [
  { id: 'bathtub', label: 'Bathtub', description: 'A tap fills a tub; the drain leaks with the level. Accumulation, asymptotic.', build: bathtub },
  { id: 'population', label: 'Population growth', description: 'Births feed back into population — an exponential reinforcing loop.', build: population },
]
