import type { Ast } from '../model/ast'
import { printAst } from '../model/ast'
import type { FlatElement, ModelDoc } from '../model/schema'
import type { ModelTemplate } from './types'

// Canonical metapopulation starting models — compartments (S/I/R) coupled across patches by a
// network, with a neighbour-mediated force of infection. Both are validated to run on the engine
// with the constructs the `network_sir_v2` proof exercises (adjacency matrix + neighbour-axis
// reduction + `lag`); each element is tagged with the lens + per-element `lens_role` so a template
// opens directly in the metapop lens with zero warnings.
//
// A note on the array forms below. The engine evaluates a real AST (never a formula string), and
// the array primitives it needs here — `index_ref` (a per-member coordinate, keyed by `depth`),
// `index` (positional gather), and axis-selective `sum_array(_, axis)` — are engine ops the text
// parser does not synthesise. So the coupling matrix and the force-of-infection reduction are built
// as explicit AST literals (the "raw" authoring the manifest documents), exactly as the proof does.
// Two facts about the engine's array evaluation shape these models and are load-bearing:
//   • binary operators (+ − × ÷, comparisons) broadcast elementwise over a dimension, but the
//     unary / min / max builtins do NOT map over an array — so the infection term is written with
//     elementwise arithmetic only (β·force·S), with β small enough that S never goes negative,
//     rather than a min(…, S) clamp;
//   • a dimensioned *stock*'s scalar `initial_value` seeds a single cell, and stock+lag+gather
//     loses per-node identity — so the networked compartments are dimensioned *expressions*
//     advanced by `lag` (the proof's pattern), seeded per-node on the first step via `elapsed == 0`.

const SAVE = { time_history: true, final_value: true }

function settings(duration: number) {
  return {
    duration: { value: duration, unit: 'd' },
    timestep: { value: 1, unit: 'd' },
    n_realizations: 1,
    sampling_method: 'monte_carlo' as const,
    seed: 1,
  }
}

// ── tiny AST builders (raw nodes; the engine consumes the AST, `display` is cosmetic) ──────────
const lit = (value: number): Ast => ({ op: 'literal', value })
const ref = (element_id: string): Ast => ({ op: 'ref', element_id })
const bin = (op: string, left: Ast, right: Ast): Ast => ({ op, left, right } as Ast)
const call = (fn: string, ...args: Ast[]): Ast => ({ op: 'call', fn, args })
const vmap = (over: string, body: Ast): Ast => ({ op: 'vector_map', over, body } as Ast)
/** Per-member coordinate at nesting `depth` (engine keying; 0 = innermost axis). */
const idx = (depth: number): Ast => ({ op: 'index_ref', depth } as unknown as Ast)
/** Positional gather `array[indices…]`. */
const gather = (array: Ast, ...indices: Ast[]): Ast => ({ op: 'index', array, indices } as Ast)
const timeRef = (property: string): Ast => ({ op: 'time_ref', property } as Ast)

/** An expression element from an AST (`inputs`/`outputs`/`lens_role` carried through `el`; the
 *  extra `outputs` field rides FlatElement's index signature; `expression` is optional on
 *  FlatElement so callers pass everything but the expression here). */
const expr = (ast: Ast, el: FlatElement): FlatElement =>
  ({ ...el, expression: { ast, display: printAst(ast) } })

// ── Template 1: two coupled cities (the "bathtub" of this lens) ────────────────────────────────
// Scalar SIR in two patches; a between-patch mixing weight `mix` carries the epidemic from the
// seeded patch A to patch B. Every compartment is a real stock and every transition is a flow, so
// the stock-flow conservation the metapop invariants inherit holds exactly (S+I+R = 1000/patch).
function twoPatchSir(): ModelDoc {
  const N = 1000
  // λ_p = β·(I_p + mix·I_other)/N  — a "force of infection" combining own and coupled prevalence.
  const lam = (me: string, other: string): Ast =>
    bin('divide', bin('multiply', ref('beta'), bin('add', ref(me), bin('multiply', ref('mix'), ref(other)))), lit(N))

  const patch = (p: string, other: string, s0: number, i0: number): FlatElement[] => [
    expr(lam(`I_${p}`, `I_${other}`), { id: `lam_${p}`, name: `Force of infection ${p.toUpperCase()}`, primitive: 'node', value_rule: 'expression',
      inputs: ['beta', `I_${p}`, 'mix', `I_${other}`], lens_role: 'mixing', save_results: SAVE }),
    expr(bin('multiply', ref(`lam_${p}`), ref(`S_${p}`)), { id: `inf_${p}`, name: `Infection ${p.toUpperCase()}`, primitive: 'node', value_rule: 'expression',
      inputs: [`lam_${p}`, `S_${p}`], lens_role: 'transition', save_results: SAVE }),
    expr(bin('multiply', ref('gamma'), ref(`I_${p}`)), { id: `rec_${p}`, name: `Recovery ${p.toUpperCase()}`, primitive: 'node', value_rule: 'expression',
      inputs: ['gamma', `I_${p}`], lens_role: 'transition', save_results: SAVE }),
    { id: `S_${p}`, name: `Susceptible ${p.toUpperCase()}`, primitive: 'stock', initial_value: { value: s0, unit: '1' },
      inflows: [], outflows: [`inf_${p}`], lens_role: 'compartment', save_results: SAVE },
    { id: `I_${p}`, name: `Infected ${p.toUpperCase()}`, primitive: 'stock', initial_value: { value: i0, unit: '1' },
      inflows: [`inf_${p}`], outflows: [`rec_${p}`], lens_role: 'compartment', save_results: SAVE },
    { id: `R_${p}`, name: `Recovered ${p.toUpperCase()}`, primitive: 'stock', initial_value: { value: 0, unit: '1' },
      inflows: [`rec_${p}`], outflows: [], lens_role: 'compartment', save_results: SAVE },
  ]

  return {
    wasim_version: '0.1.0',
    simulation_settings: settings(80),
    elements: [
      { id: 'beta', name: 'Transmission β', primitive: 'node', value_rule: 'fixed', value: { value: 0.4, unit: '1' }, editable: true, bounds: { min: 0, max: 2 }, lens_role: 'parameter', save_results: SAVE },
      { id: 'gamma', name: 'Recovery γ', primitive: 'node', value_rule: 'fixed', value: { value: 0.15, unit: '1' }, editable: true, bounds: { min: 0, max: 1 }, lens_role: 'parameter', save_results: SAVE },
      { id: 'mix', name: 'Coupling (A↔B)', primitive: 'node', value_rule: 'fixed', value: { value: 0.3, unit: '1' }, editable: true, bounds: { min: 0, max: 1 }, lens_role: 'coupling', save_results: SAVE },
      ...patch('a', 'b', 990, 10), // patch A seeded with 10 infected
      ...patch('b', 'a', 1000, 0), // patch B initially healthy — infected only via coupling
    ],
    view: {
      lens: 'metapop', authored: true,
      positions: {
        beta: { x: 60, y: 40 }, gamma: { x: 60, y: 130 }, mix: { x: 430, y: 40 },
        lam_a: { x: 240, y: 120 }, inf_a: { x: 240, y: 210 }, rec_a: { x: 560, y: 210 },
        S_a: { x: 120, y: 300 }, I_a: { x: 380, y: 300 }, R_a: { x: 640, y: 300 },
        lam_b: { x: 240, y: 420 }, inf_b: { x: 240, y: 510 }, rec_b: { x: 560, y: 510 },
        S_b: { x: 120, y: 600 }, I_b: { x: 380, y: 600 }, R_b: { x: 640, y: 600 },
      },
    },
  }
}

// ── Template 2: SIR on an explicit contact network ─────────────────────────────────────────────
// The `network_sir_v2` graph (path 1–2–3–4–5–6 + chord 2–5) promoted to a full SIR with recovery.
// Per-node S/I/R vectors advance by `lag`; the force of infection is W×prevalence reduced over the
// neighbour axis. The chord makes node 5 infect early (same wave as node 3), proving the spread
// follows the graph's edges, not linear diffusion.
function sirOnNetwork(): ModelDoc {
  // Adjacency W[i,j]: |i−j| == 1 (path) OR the 2–5 chord. depth 1 = Node (home i), depth 0 = NodeB (j).
  const adj: Ast = bin('or',
    bin('or', bin('eq', bin('subtract', idx(1), idx(0)), lit(1)), bin('eq', bin('subtract', idx(0), idx(1)), lit(1))),
    bin('or', bin('and', bin('eq', idx(1), lit(2)), bin('eq', idx(0), lit(5))),
              bin('and', bin('eq', idx(1), lit(5)), bin('eq', idx(0), lit(2)))))

  // First-step seed gate (elapsed == 0) and per-node masks (node 1 seeded).
  const gate0 = bin('eq', timeRef('elapsed'), lit(0))
  const blend = (seed: Ast, dyn: Ast): Ast =>
    bin('add', bin('multiply', gate0, seed), bin('multiply', bin('subtract', lit(1), gate0), dyn))
  const seedI = vmap('Node', bin('multiply', bin('eq', idx(0), lit(1)), lit(0.01)))
  const seedS = vmap('Node', bin('subtract', lit(1), bin('multiply', bin('eq', idx(0), lit(1)), lit(0.01))))

  // new_inf = β·(foi + own prevalence)·S_prev ; new_rec = γ·I_prev  (elementwise only, so S ≥ 0).
  const newInf = bin('multiply', bin('multiply', ref('beta'), bin('add', ref('foi'), ref('I_prev'))), ref('S_prev'))
  const newRec = bin('multiply', ref('gamma'), ref('I_prev'))
  const dim = (n: string) => [{ name: n, unit: '1', dimensions: ['Node'] }]

  return {
    wasim_version: '0.1.0',
    simulation_settings: settings(60),
    dimensions: [
      { id: 'Node', name: 'Node', size: 6 },
      { id: 'NodeB', name: 'Node (neighbour)', size: 6 },
    ],
    elements: [
      { id: 'beta', name: 'Transmission β', primitive: 'node', value_rule: 'fixed', value: { value: 0.25, unit: '1' }, editable: true, bounds: { min: 0, max: 1 }, lens_role: 'parameter', save_results: SAVE },
      { id: 'gamma', name: 'Recovery γ', primitive: 'node', value_rule: 'fixed', value: { value: 0.08, unit: '1' }, editable: true, bounds: { min: 0, max: 1 }, lens_role: 'parameter', save_results: SAVE },
      expr(vmap('Node', vmap('NodeB', adj)), { id: 'W', name: 'Coupling network W', primitive: 'node', value_rule: 'expression', lens_role: 'coupling',
        outputs: [{ name: 'W', unit: '1', dimensions: ['Node', 'NodeB'] }], save_results: SAVE }),
      // lagged per-node state (scalar initial broadcasts to every node)
      { id: 'S_prev', name: 'S (previous)', primitive: 'node', value_rule: 'lag', input: 'S', initial: { value: 1, unit: '1' }, inputs: ['S'], outputs: dim('S_prev') },
      { id: 'I_prev', name: 'I (previous)', primitive: 'node', value_rule: 'lag', input: 'I', initial: { value: 0, unit: '1' }, inputs: ['I'], outputs: dim('I_prev') },
      { id: 'R_prev', name: 'R (previous)', primitive: 'node', value_rule: 'lag', input: 'R', initial: { value: 0, unit: '1' }, inputs: ['R'], outputs: dim('R_prev') },
      // reindex I_prev onto the neighbour axis, W × prevalence, reduce the neighbour axis (axis 2)
      expr(vmap('NodeB', gather(ref('I_prev'), idx(0))), { id: 'I_prev_B', name: 'I on neighbour axis', primitive: 'node', value_rule: 'expression', inputs: ['I_prev'],
        outputs: [{ name: 'I_prev_B', unit: '1', dimensions: ['NodeB'] }] }),
      expr(bin('multiply', ref('W'), ref('I_prev_B')), { id: 'wprod', name: 'W × prevalence', primitive: 'node', value_rule: 'expression', inputs: ['W', 'I_prev_B'],
        outputs: [{ name: 'wprod', unit: '1', dimensions: ['Node', 'NodeB'] }] }),
      expr(call('sum_array', ref('wprod'), lit(2)), { id: 'foi', name: 'Force of infection', primitive: 'node', value_rule: 'expression', inputs: ['wprod'], lens_role: 'mixing',
        outputs: dim('foi'), save_results: SAVE }),
      // transition rates
      expr(newInf, { id: 'new_inf', name: 'Infection rate', primitive: 'node', value_rule: 'expression', inputs: ['beta', 'foi', 'I_prev', 'S_prev'], outputs: dim('new_inf') }),
      expr(newRec, { id: 'new_rec', name: 'Recovery rate', primitive: 'node', value_rule: 'expression', inputs: ['gamma', 'I_prev'], outputs: dim('new_rec') }),
      // compartments: per-node expressions advanced by lag, seeded on the first step
      expr(blend(seedS, bin('subtract', ref('S_prev'), ref('new_inf'))), { id: 'S', name: 'Susceptible', primitive: 'node', value_rule: 'expression', inputs: ['S_prev', 'new_inf'], lens_role: 'compartment', outputs: dim('S'), save_results: SAVE }),
      expr(blend(seedI, bin('add', ref('I_prev'), bin('subtract', ref('new_inf'), ref('new_rec')))), { id: 'I', name: 'Infected', primitive: 'node', value_rule: 'expression', inputs: ['I_prev', 'new_inf', 'new_rec'], lens_role: 'compartment', outputs: dim('I'), save_results: SAVE }),
      expr(blend(vmap('Node', lit(0)), bin('add', ref('R_prev'), ref('new_rec'))), { id: 'R', name: 'Recovered', primitive: 'node', value_rule: 'expression', inputs: ['R_prev', 'new_rec'], lens_role: 'compartment', outputs: dim('R'), save_results: SAVE }),
      // scalar total (the epidemic curve)
      expr(call('sum_array', ref('I')), { id: 'totI', name: 'Total infected', primitive: 'node', value_rule: 'expression', inputs: ['I'], save_results: SAVE }),
    ],
    view: {
      lens: 'metapop', authored: true,
      positions: {
        beta: { x: 60, y: 40 }, gamma: { x: 60, y: 130 }, W: { x: 320, y: 40 },
        S_prev: { x: 60, y: 260 }, I_prev: { x: 60, y: 350 }, R_prev: { x: 60, y: 440 },
        I_prev_B: { x: 300, y: 350 }, wprod: { x: 480, y: 200 }, foi: { x: 480, y: 350 },
        new_inf: { x: 680, y: 260 }, new_rec: { x: 680, y: 440 },
        S: { x: 900, y: 200 }, I: { x: 900, y: 350 }, R: { x: 900, y: 500 }, totI: { x: 1120, y: 350 },
      },
    },
  }
}

export const METAPOP_TEMPLATES: ModelTemplate[] = [
  { id: 'two-patch-sir', label: 'Two coupled cities', description: 'Scalar SIR in two patches; a coupling weight carries the epidemic from the seeded city to the other. The metapopulation "bathtub".', build: twoPatchSir },
  { id: 'sir-on-network', label: 'SIR on a network', description: 'Per-node SIR on an explicit contact graph (path + chord); the force of infection reduces W × prevalence over neighbours.', build: sirOnNetwork },
]
