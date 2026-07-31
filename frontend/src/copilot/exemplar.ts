/**
 * A known-good, engine-validated WASiM v2 model (the "bathtub": a constant tap fills a stock while
 * a drain leaks in proportion to the level). Injected into the copilot's system prompt as a
 * complete, parse-valid reference — it anchors the LLM to the exact engine-native shapes the
 * authoring guide describes (the `ref` node's `element_id`, stock `inflows`/`outflows` wiring,
 * `simulation_settings`), which the feasibility spike found are the shapes an LLM most often gets
 * wrong without an example.
 */
export const BATHTUB_EXEMPLAR = JSON.stringify(
  {
    wasim_version: '0.1.0',
    simulation_settings: {
      duration: { value: 60, unit: 's' },
      timestep: { value: 1, unit: 's' },
      n_realizations: 1,
      sampling_method: 'monte_carlo',
      seed: 42,
    },
    elements: [
      {
        id: 'tap',
        name: 'Tap',
        primitive: 'node',
        value_rule: 'fixed',
        value: { value: 5, unit: '1' },
        save_results: { time_history: true, final_value: true },
      },
      {
        id: 'drain',
        name: 'Drain',
        primitive: 'node',
        value_rule: 'expression',
        expression: {
          ast: { op: 'multiply', left: { op: 'literal', value: 0.1 }, right: { op: 'ref', element_id: 'water' } },
        },
        inputs: ['water'],
        save_results: { time_history: true, final_value: true },
      },
      {
        id: 'water',
        name: 'Water level',
        primitive: 'stock',
        initial_value: { value: 10, unit: '1' },
        inflows: ['tap'],
        outflows: ['drain'],
        save_results: { time_history: true, final_value: true },
      },
    ],
  },
  null,
  2,
)
