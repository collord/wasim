import type { ModelDoc, FlatElement } from '../model/schema'
import type { ModelTemplate } from './types'
import { parseExpr, printAst, refsOf } from '../model/ast'

// Canonical decision-under-uncertainty starting models. A decision (editable, bounded), one or
// more uncertainties (chance inputs), and an objective expression over them — the shape the
// optimizer and the value-of-information reduction both consume.

const SAVE = { time_history: true, final_value: true }

function settings() {
  return {
    duration: { value: 1, unit: 's' },
    timestep: { value: 1, unit: 's' },
    n_realizations: 80,
    sampling_method: 'monte_carlo' as const,
    seed: 42,
  }
}

/** Capacity choice: pick a capacity to match uncertain demand; the objective is the squared
 *  mismatch (minimize). Knowing demand before choosing is worth something — that's the VOI. */
function capacityChoice(): ModelDoc {
  // (capacity - demand)^2, written as a product to avoid the power operator.
  const ast = parseExpr('(capacity - demand) * (capacity - demand)')
  const objective: FlatElement = {
    id: 'mismatch', name: 'Mismatch cost', primitive: 'node', value_rule: 'expression',
    expression: { ast, display: printAst(ast) }, inputs: [...refsOf(ast)],
    lens_role: 'objective', save_results: SAVE,
  }
  return {
    wasim_version: '0.1.0',
    simulation_settings: settings(),
    elements: [
      { id: 'capacity', name: 'Capacity', primitive: 'node', value_rule: 'fixed', value: { value: 100, unit: '1' }, editable: true, bounds: { min: 0, max: 200 }, lens_role: 'decision', save_results: SAVE },
      { id: 'demand', name: 'Demand', primitive: 'node', value_rule: 'sample', distribution: { family: 'normal', parameters: { mean: { value: 100, unit: '1' }, stddev: { value: 30, unit: '1' } } } as FlatElement['distribution'], lens_role: 'chance', save_results: SAVE },
      objective,
    ],
    view: {
      lens: 'decision', authored: true,
      positions: { capacity: { x: 80, y: 80 }, demand: { x: 80, y: 250 }, mismatch: { x: 400, y: 165 } },
    },
  }
}

export const DECISION_TEMPLATES: ModelTemplate[] = [
  { id: 'capacity-choice', label: 'Capacity choice', description: 'Pick a capacity to match uncertain demand — the objective is the squared mismatch.', build: capacityChoice },
]
