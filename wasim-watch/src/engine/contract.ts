/**
 * WaSim → renderer data contract
 * ======================================================================
 * This is the integration boundary. The renderer ("watch it run") is a
 * pure VIEW over the state a completed WaSim run already retains — it does
 * NOT re-execute the simulation. The real WASM engine would produce a
 * `RunResult` shaped exactly like this after a Monte-Carlo sweep finishes;
 * everything in /render and /ui consumes only these types.
 *
 * Design decisions that mirror the engine (see FEATURES_AND_USE_CASES.md):
 *   - Time advances on a FIXED grid (explicit Euler). `grid` is the
 *     statistical / state-machine / reporting lattice. One index = one step.
 *   - The run is a Monte-Carlo loop: state is retained per realization.
 *     A realization is fully reproducible under the `fixed` seed, so any
 *     single one can be replayed exactly.
 *   - Aggregate bands (p05..p95) are what the default results surface emits;
 *     we carry them alongside per-realization traces so the view can show
 *     either "the cloud" or "one story".
 *   - Elements are TYPED. The renderer switches its glyph on `kind`, which
 *     is the same typing the engine uses (stock / flow / gate / state
 *     machine / markov / controller / expression ...).
 *
 * The synthetic generator in ./synthetic.ts fills these structures with
 * realistic values so the UI is exercised without the WASM engine present.
 * Swap that import for a real `wasm.run(model)` returning `RunResult` and
 * nothing in the view layer changes.
 */

/** How the engine typed each element. Drives which glyph the renderer draws. */
export type ElementKind =
  | "stock" // integrates a rate; drawn as a fill gauge
  | "flow" // moves quantity between stocks; drawn as a pulsing link
  | "expression" // live scalar / variable; drawn as a value chip + sparkline
  | "state_machine" // working/failed automaton; drawn as a status lamp
  | "markov" // discrete degradation state; drawn as a state chip
  | "controller" // on/off hysteresis latch; drawn as a toggle
  | "gate"; // boolean logic node; drawn true/false

/** Discrete status a state-bearing element can hold at a step. */
export type DiscreteStatus =
  | "working"
  | "failed"
  | "on"
  | "off"
  | "true"
  | "false"
  | string; // markov states are model-defined names

/** A node in the model graph, with layout + semantics the view needs. */
export interface ElementMeta {
  id: string;
  label: string;
  kind: ElementKind;
  /** Canvas position (model coordinates, 0..1 normalized). */
  x: number;
  y: number;
  units?: string;
  /** For stocks: the capacity used to scale the fill gauge (if bounded). */
  capacity?: number;
  /** For flows: source/target element ids (either may be null = boundary). */
  from?: string | null;
  to?: string | null;
  /** For markov: the ordered set of state names, for stable coloring. */
  states?: string[];
}

/**
 * Per-element time-history for the AGGREGATE view (across all realizations).
 * Arrays are indexed by grid step; length === grid.length.
 * Percentile arrays are present for continuous kinds (stock/flow/expression).
 */
export interface AggregateTrace {
  elementId: string;
  p05?: number[];
  p25?: number[];
  p50?: number[]; // median; also used as the default continuous value
  p75?: number[];
  p95?: number[];
  /**
   * Fraction of realizations in the "active"/"failed"/"on" status at each
   * step, for discrete kinds — e.g. failed-fraction for a state machine,
   * so the aggregate lamp can render as a probability, not a single state.
   */
  activeFraction?: number[];
}

/**
 * Per-element time-history for ONE realization (the "single story" view).
 * Continuous kinds fill `value`; discrete kinds fill `status`. Events, if
 * any fired on this element in this realization, are marked by step index.
 */
export interface RealizationTrace {
  elementId: string;
  value?: number[]; // continuous kinds
  status?: DiscreteStatus[]; // discrete kinds
}

/** An instantaneous event (Poisson occurrence, interrupt, threshold cross). */
export interface EventMark {
  step: number;
  elementId: string;
  type: "occurrence" | "interrupt" | "threshold" | "repair" | "failure";
  label?: string;
}

/** One Monte-Carlo realization's full retained state. */
export interface Realization {
  index: number;
  /** Seed lineage, so a single realization is exactly reproducible. */
  seed: number;
  traces: Record<string, RealizationTrace>;
  events: EventMark[];
  /** Step at which an interrupt ended this realization, if any (else null). */
  terminatedAt: number | null;
}

/** The complete result of one WaSim run, everything the view can draw. */
export interface RunResult {
  modelName: string;
  /** The fixed time grid. `grid[i]` is the elapsed time at step i. */
  grid: number[];
  timeUnit: string;
  /** Timebase the run used — affects how cleanly bounds are hit on screen. */
  timebase: "fixed" | "event_accurate";
  seedRoot: number;
  elements: ElementMeta[];
  aggregate: Record<string, AggregateTrace>;
  realizations: Realization[];
  /**
   * Convenience index: which realization is the p95 "worst case" on a chosen
   * element, so the UI can offer "show me the run behind the 95th percentile".
   */
  exemplars?: { label: string; realizationIndex: number }[];
}

/** The interface a real engine binding would satisfy. */
export interface WasimEngine {
  /** Run a model to completion and return retained state for viewing. */
  run(): RunResult;
}
