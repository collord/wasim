# WaSim Engine Semantics — Primitive & Trait Reference

## Version 0.9.6

This document is the behavioral contract between the model schema and the simulation engine. It specifies what the engine **must do** for each primitive type, each node value_rule, and each trait activated by field presence. An implementing agent should be able to build a conforming engine from this document and the accompanying JSON Schema alone.

---

## 1. Primitives

The schema defines six primitive types, discriminated by the `primitive` field on every element. Each primitive has a fixed set of base responsibilities and zero or more optional **traits** activated by the presence of specific fields.

| Primitive | Role |
|-----------|------|
| `node`    | Produces a value at each timestep. Stateless between timesteps (no memory unless value_rule provides it). |
| `stock`   | State variable. Accumulates a rate over time. Carries value between timesteps. |
| `link`    | Directed connection. Moves quantity from a source to a target at a rate or fraction. |
| `event`   | Discrete occurrence. Fires on a trigger and applies effects to other elements. |
| `gate`    | Boolean logic composition. Evaluates a tree of AND/OR/NOT/k-of-n gates to produce a boolean state. |
| `cell`    | Transport compartment. Holds species across media with partitioning, decay, and mass exchange. |

Additionally, three **definition types** carry no simulation behavior:

| Definition | Role |
|------------|------|
| `species`    | Defines a transported substance: half-life, decay products, molecular weight. |
| `medium`     | Defines a material phase: solid/fluid/gas, density, porosity. |
| `container`  | Organizational grouping. No simulation semantics. |

### Element identity and references

Every element carries an `id` that is its **globally-unique handle**: `id` MUST be unique across the entire `elements` list — not merely within a container. Source platforms typically scope element *names* by container, so the same bare name (e.g. `nCells`) recurs across containers and is **not** a valid id; emitters must qualify ids to restore global uniqueness (e.g. a container path such as `Model/CoverLayer/nCells`). The `name` field remains the human-readable label and may collide freely — it is never a reference key.

All string references resolve to an `id` by **exact string equality**. There is **no relative-name lookup and no scope-aware resolution**: a reference is the fully-qualified id of its target, wherever it appears —

- `element_base.inputs[]`,
- AST `ref` / `lookup_call` `element_id` (§2),
- `container.elements[]` and `container.interface.inputs/outputs` (§12),
- `optimization.objective.element_id` and `optimization.variables[].element_id` (§13).

A reference that matches no element `id` is **dangling**; the engine evaluates it as `0.0` and should warn (dangling refs are not an error, to tolerate partially-emitted models, but each should be surfaced). Because ids are globally unique, an engine may (and should) reject a model with duplicate ids at load time rather than silently resolving to an arbitrary one.

### 1b. Reserved global identifiers (0.9.2)

Before the dangling-ref fallback, the engine resolves these reserved names (GoldSim run
properties that emit passes through verbatim). A model element with the same id shadows the
global. Time quantities are SI seconds, matching the SI-normalized values emit produces.

| Name | Value |
|------|-------|
| `gee` | Standard gravity, 9.80665 m/s² |
| `TimestepLength` | The simulation timestep Δt, in seconds |
| `SimDuration` | The total simulation duration, in seconds |
| `Realization` | The current realization index, 1-based |

Additionally, three reserved names select a **table mode** when they appear as the second
argument of a `lookup_call` (they are modes, not values — anywhere else they are dangling):

| Name | Meaning |
|------|---------|
| `TBL_Integral` | ∫y dx from the first knot to the input x (cumulative trapezoid; x clamped into the x-range, so below-range → 0, above-range → the full integral) |
| `TBL_Inverse` | Inverse table: given a y-value, the x that maps to it (y must be monotonic; descending tables are handled) |
| `TBL_Inv_Integral` | Inverse of the integral: given v = ∫y dx, the x where the integral reaches v (the stage-storage pattern — table is stage→area, v is a volume, result is the stage). v is clamped into [0, full integral] |
| `TBL_Derivative` | dy/dx of the interpolated table at x: the slope of the bracketing segment (step interpolation → 0). At or beyond the ends, the nearest interior segment's slope (0.9.3) |

GoldSim calendar-of-day (`Hour`/`Minute`/`Second`) and elapsed-calendar counts
(`EMonth`/`EYear`/`StartTime`) map to **`time_ref` properties** (`hour`/`minute`/`second`/
`elapsed_months`/`elapsed_years`/`start`, 0.9.6 — see §14), not reserved globals. `EDay` lowers
to `elapsed / 86400` (a fixed-period elapsed count).

### 1c. Output-qualified references and stock ports (0.9.2; `output_kind` 0.9.7)

An AST `ref` may carry an `output` field naming a secondary output port (`"<Name>#k"`,
matching the element's `outputs[k-1].name`). The engine publishes secondary values under
the key `"<element_id>#k"`; a qualified ref that finds no published port falls back to the
element's **primary** value (the pre-0.9.2 behavior, so models degrade rather than break).

Stocks publish a port for each secondary output that declares a `role`. **Since 0.9.7 the port
is described by two orthogonal axes** (§ schema `output_spec`): `role` names the **flow**, and
`output_kind` names the **accumulation**. The flow's per-step applied *rate* (stock-units/time) is:

| `role` (flow) | Rate this step |
|---------------|----------------|
| `addition` | Σ inflows (or `max(rate, 0)` for an explicit-rate stock) |
| `withdrawal` | Σ outflows + priority-withdrawal allocations (or `max(-rate, 0)`) |
| `overflow` | Capacity-clamp excess routed (or discarded) this step, per time |
| `net_change` | (level_end − level_start) / Δt, after all traits and events |

`output_kind` selects how that flow is reported:

| `output_kind` | Published value | Units |
|---------------|-----------------|-------|
| `rate` (default) | the flow's per-step rate (table above) | value/time |
| `cumulative` | running total of the flow **since the run start** (Σ of rate·Δt over all steps; for `net_change`, Σ of the level deltas) | value |
| `level` | the stock's own value (its primary output) | value |

The cumulative accumulator is per-realization and is incremented by the flow's applied *amount*
each **sub-interval** (so it is exact under B1 sub-stepping and consumes no RNG; a bound-crossing
re-run rolls it back with the rest of the aborted try's state). When `output_kind` is absent it
defaults to `rate`. **Back-compat:** the 0.9.6 fused role names `addition_rate` / `withdrawal_rate`
/ `overflow_rate` are retained as aliases and normalized at parse into `<flow>` + `output_kind:
rate`, so pre-0.9.7 models are unchanged.

Ports are published at end-of-step, so a same-step consumer reads the **previous** step's
value — the same one-step causality as reading a stock's level. The signal-`input` fields
(`filter.input`, `delay.input`, `convolution.input`) are bare element-id strings and cannot
address a port; emit interposes a companion expression node (the `__signal` shim) that
carries the qualified ref.

---

## 2. Node Value Rules

A `node` element is discriminated by its `value_rule` field. The engine evaluates the node once per timestep (unless otherwise specified) in dependency-graph order.

### 2.1 `fixed`

**Fields:** `value` (quantity) or `values` (number[]) + `unit`.

**Behavior:** Returns the same value every timestep. If `editable: true`, the value may be changed by the user between or during runs; the engine treats it as constant within a timestep.

For array-valued nodes: `values` is an ordered vector of scalars sharing a single `unit`. Consumers reference elements by positional index (1-based by convention).

### 2.2 `expression`

**Fields:** `expression` (expression_field containing AST).

**Behavior:** Evaluate the AST against the current timestep's evaluation context (element output cache, simulation time properties). Returns a scalar or boolean. All upstream dependencies must have been evaluated in the current timestep before this node evaluates.

This value_rule subsumes what were previously separate element types: selector (conditional AST), aggregator (reduction function in AST), controller (PID error computation in AST). These are not special-cased in the engine — they are simply expression ASTs that happen to use specific patterns (nested if/then/else, sum_array, or error-integral formulations).

### 2.3 `sample`

**Fields:** `distribution` (distribution spec).

**Behavior:** At the start of each realization, draw a value from the specified distribution. The value is constant for the duration of the realization unless a `resampling` trigger is present.

**Formula-valued parameters:** a distribution parameter may be a `quantity_or_formula` — a scalar literal, or a reference/expression over other elements (e.g. a Weibull `shape` that references an optimization variable, or a gamma `shape` derived from a referenced mean and sd). The engine resolves each parameter to a scalar (evaluating any expression in the element's context) before drawing. This makes a distribution's parameters responsive to the rest of the model — the basis of probabilistic optimization, where an optimization variable *is* a distribution parameter.

**Distribution families (0.9.3 roster).** In addition to the base families (uniform, normal, lognormal[_moments], triangular, trapezoidal, exponential, gamma, beta, weibull, pearson_iii, pearson_v, pert, pareto, extreme_value, student_t, discrete_uniform, bernoulli, discrete, cumulative, sampled), the engine supports: `log_uniform`/`log_triangular`/`log_cumulative` (sample the base in log space, `exp` the result; real-space params must be > 0); `triangular1090`/`log_triangular1090` (GoldSim's alternate parameterization by 10th/90th percentiles + mode, reparameterized to min/mode/max at resolve time); `binomial`(n, prob), `negative_binomial`(r, prob) (#failures before the r-th success), `poisson`(lambda); `extreme_probability` (the min or max of `n` draws from a nested `base` distribution, via the order-statistic ICDF transform); `beta_success_failure`(successes, failures) → Beta(s+1, f+1), optionally affine-scaled onto [min, max]. The `external` family cannot be sampled by the engine: it degrades to 0.0 with a warning unless an inline `parameters.fallback` `{samples, weights}` empirical table is supplied (then it samples that).

**Optional fields:**
- `resampling` (trigger_spec): When present and the trigger fires, the engine draws a new value from the distribution. Between trigger firings, the value persists.
- `autocorrelation` (number, 0–1): Lag-1 autocorrelation applied across timesteps within a realization. When the node resamples, the new value is `ρ * previous + √(1 - ρ²) * fresh_draw` where ρ is the autocorrelation coefficient.
- `correlations[]`: Specifies rank-correlation partners and coefficients. See §7 Correlated Sampling.

### 2.4 `process`

**Fields:** `process` (process_spec: family, mean_type, mean, stddev).

**Behavior:** At each timestep, draws a new value from a time-correlated stochastic process. Each realization gets independent draws. The engine scales drift linearly with dt and stddev by √(dt/T_ref) where T_ref is the period implied by the stddev unit.

For `family: "gbm"` (geometric Brownian motion): The node maintains a running value V. At each step: `V_{t+1} = V_t * exp((μ - σ²/2) * dt + σ * √dt * Z)` where Z ~ N(0,1), and μ is derived from `mean` according to `mean_type`:
- `geometric`: μ = ln(1 + mean)
- `arithmetic`: μ = ln(1 + mean) - σ²/2
- `log_drift`: μ = mean (no transformation)

**Optional fields:**
- `lower_bound` (quantity): Floor clamped on the per-step value.

**Note:** Unlike `sample`, this node has internal state (the running value V). The engine must allocate per-realization state for process nodes.

### 2.5 `lookup`

**Fields:** `table` (object with `x[]`, `y[]`, optionally `z[][]` for 2D), `interpolation` (enum).

**Behavior:** Not self-evaluating. A lookup node is invoked via `lookup_call` AST nodes in other elements' expressions. When called with an input value, the engine interpolates into the table and returns the result. Interpolation method is specified by the `interpolation` field:

- `linear` (default) — piecewise-linear between knots.
- `step` — piecewise-constant (value of the lower knot).
- `spline` — **monotone cubic** (Fritsch-Carlson) Hermite interpolation: C¹ and **never overshoots** the data (0.9.3; previously a silent downgrade to linear).
- `log_linear` — interpolate `ln(y)` linearly and return `exp` (**log-result interpolation**, 0.9.3). Requires y > 0 at the bracketing knots; falls back to linear where a knot is ≤ 0.

**N-D tables (0.9.3):** emit packs an N-D table as `table.z = [axis2_breakpoints, (axis3_breakpoints)?, flat_values]`, where `flat_values` is row-major over (x, axis2, axis3) with length `|x|·|axis2|·|axis3|`. `lookup_call`'s `input` is the first-axis coordinate and `input2` the second-axis coordinate; the engine does multilinear (2-D bilinear / 3-D trilinear) interpolation, clamping unspecified higher-axis coordinates to their low end. A `z` that does not match this packing is treated as legacy columns-of-y (a numeric `input2` selects a column).

### 2.6 `series`

**Fields:** `timestamps[]`, `values[]`, `time_unit`, `interpolation`.

**Behavior:** At each timestep, the engine evaluates the current simulation time against the timestamp array and returns the interpolated value. If the simulation time falls outside the series bounds, the engine uses the boundary value (constant extrapolation) unless otherwise specified.

### 2.7 `lag`

**Fields:** `input` (element id), `initial` (quantity, optional).

**Behavior:** Returns the value that the referenced input element held at the **previous** timestep. This is strictly a one-timestep delay. At the first timestep, returns `initial` if provided, otherwise 0.

**Purpose:** Break algebraic loops and model one-step feedback. The engine must evaluate this node's input in the previous timestep's output cache, not the current one.

**Multi-step delays:** For delays longer than one timestep, use `value_rule: "convolution"` with a unit-impulse response function offset by the desired lag, or use `value_rule: "filter"` to access the trailing window. Alternatively, chain N lag nodes in series for an exact N-step delay (each lag references the previous one). See the transpiler mapping guide for how source-model multi-step delays are emitted.

### 2.8 `convolution`

**Fields:** `input` (element id), `response` (inline table or element id reference).

**Behavior:** Maintains a rolling history buffer of the input signal's values over the most recent N timesteps, where N is the length of the response function. At each timestep, computes the discrete convolution (inner product) of the history buffer with the response function weights.

**Engine state:** Requires a per-realization circular buffer of length N for each convolution node.

### 2.9 `markov`

**Fields:** `states[]` (string labels), `initial_state` (string or index), `transition_matrix` (number[][] or expression-valued), `output_values` (number[], one per state).

**Behavior:** The node maintains a discrete state from the `states` set. At each timestep, the engine draws a transition from the current state using the row of the transition matrix corresponding to the current state. Each row is a probability vector summing to 1; the engine draws a uniform random number and selects the next state accordingly.

The node's output value is `output_values[current_state_index]`, mapping discrete states to numeric values for use in downstream expressions (e.g., 0.0 for "good", 0.5 for "degraded", 1.0 for "failed").

If `transition_matrix` entries are expression-valued (referencing other elements), the engine evaluates them at each timestep, enabling state-dependent or time-dependent transition rates.

**Engine state:** Requires per-realization current-state tracking.

### 2.10 `hysteresis`

**Fields:** `input` (element id), `high_threshold` (quantity), `low_threshold` (quantity), `output_above` (quantity), `output_below` (quantity).

**Behavior:** The node maintains a binary internal state: `active` or `inactive`. Transitions:
- When `inactive` and `input >= high_threshold`: transition to `active`.
- When `active` and `input <= low_threshold`: transition to `inactive`.
- Otherwise: remain in current state.

Output is `output_above` when active, `output_below` when inactive. Initial state is determined by comparing the input's initial value against the thresholds at the first timestep.

**Invariant:** `low_threshold < high_threshold`. The engine should validate this at model load time.

**Engine state:** Requires per-realization active/inactive flag.

### 2.11 `filter`

**Fields:** `input` (element id; optional — absent or dangling resolves to a 0.0 signal per §1, so a partially-emitted filter loads and runs rather than rejecting the model), `window` (integer, number of timesteps), `statistic` (enum: `mean`, `min`, `max`, `sum`, `ema`).

**Behavior:** Maintains a rolling buffer of the most recent `window` timestep values of the input. At each timestep, computes the selected statistic over the buffer contents. A `window` ≥ the run's step count therefore degenerates to the cumulative statistic since t=0 — the encoding emit uses for running extrema (peak/valley since start).

For `ema` (exponential moving average): `window` is interpreted as the smoothing span. The smoothing factor α = 2 / (window + 1). Update rule: `EMA_t = α * input_t + (1 - α) * EMA_{t-1}`. Initial EMA is the input's value at the first timestep. No buffer needed for EMA — only the previous EMA value.

For all other statistics: before the buffer is fully populated (first `window - 1` timesteps), the statistic is computed over the available values.

**Engine state:** Requires a per-realization circular buffer of length `window` (except for EMA, which requires only the previous value).

### 2.12 `gate_logic`

**Fields:** `root` (gate_node tree), `semantics` (enum: `success` | `failure`).

**Behavior:** Evaluates the recursive gate_node tree to produce a boolean output. See §5 Gate Primitive for gate_node evaluation rules. The `semantics` field determines interpretation: `success` means the tree evaluates the probability of system success (RBD), `failure` means it evaluates system failure (fault tree).

Output value is 1.0 (true) or 0.0 (false).

**Note:** This value_rule makes the separate `gate` primitive optional — a gate can be expressed as a node with `value_rule: "gate_logic"`. The `gate` primitive is retained for conceptual clarity and backward compatibility, but engines may implement both via the same code path.

### 2.13 `status` (0.9.3)

**Fields:** `set` (trigger_spec), `reset` (trigger_spec).

**Behavior:** A latching status flag. When the `set` trigger fires the output latches to 1.0; when the `reset` trigger fires it latches back to 0.0; otherwise it holds its prior value. If both fire on the same step, **set wins**. Distinct from `hysteresis` (which latches on thresholds of an input signal, not on triggers). Per-realization state.

### 2.14 `milestone` (0.9.3)

**Fields:** `trigger` (trigger_spec).

**Behavior:** Records the **elapsed time** at which `trigger` first fires and outputs that time for the rest of the run. Before the first fire the output is **NaN** (the sentinel for an unachieved milestone). GoldSim's achievement probability and mean lag fall out of the results/analysis layer (§A3) applied to the milestone's final-value distribution across realizations. Per-realization state.

### 2.15 `pid` (PID / proportional / on_off controller, 0.9.3; on_off 0.9.7)

**Fields:** `input` (element id of the measured value), `setpoint` (quantity_or_formula), `kp`/`ki`/`kd` (gains, numbers), optional `output_min`/`output_max` (output clamps), optional `deadband` (number). GoldSim controllers have three **modes**, selected by an optional top-level `controller_mode` (`pid` default | `proportional` | `on_off`); the on_off mode adds `output_cap` (quantity_or_formula — the "ON" output value) and `deadband_ref` (quantity_or_formula — the hysteresis band, distinct from the numeric `deadband`).

**`pid` / `proportional` behavior:** Euler-discretized PID control. Each step, `error = setpoint − input`; if `|error| ≤ deadband` the error is treated as 0 (anti-chatter). The controller carries an integral accumulator and the previous error per realization:

    integral += error · dt
    derivative = (error − prev_error) / dt
    output = kp·error + ki·integral + kd·derivative

The output is clamped to `[output_min, output_max]` when set. `proportional` is this same law with `ki = kd = 0` (kp only). Closed feedback loops are closed through a stock (integrator) plant, whose back-edge breaks the dependency cycle.

**`on_off` behavior (bang-bang hysteresis latch).** A stateful two-state latch — **not** a stateless threshold. With band = `deadband_ref` (else `deadband`) and half = band/2:

    state = ON   when input > setpoint + half
            OFF  when input < setpoint − half
            HOLD previous state   otherwise   (inside the deadband)
    output = state ? output_cap : 0     (then clamped to output_min/max if set)

The **HOLD-inside-the-band** is load-bearing: it is why a deadband on_off controller does not chatter at the setpoint (a stateless `input < setpoint` rule would flip every step). The latch state is per-realization and advances once per grid step (grid-only under B1, like the PID integral). `output_cap`/`deadband_ref` are evaluated each step (they may be dynamic refs). When `output_cap` is absent the latch still tracks state but emits 0/1 (a degradation for models emitted before the on_off fields are populated).

### 2.16 Event-effect mode `interrupt` (0.9.3)

An `effect_spec` with `mode: "interrupt"` (no `target`/`change`) ends the **realization** at the end of the step it fires on. That step completes and is recorded normally; every remaining step holds the last-computed values (which the time-history bands and final values then reflect).

### 2.17 Event-predicate builtins `occurs` / `changed` (0.9.3)

Two AST `call` builtins whose single argument is an element `ref` (read as an id, not evaluated):

- `occurs(event_id)` — 1.0 on the step the referenced event fires, else 0.0. Trigger-driven events are predicted before the node evaluation pass (against the previous step's outputs) so a node reading `occurs` sees the current step's fire; rate/failure events are reflected on the following step.
- `changed(ref)` — 1.0 when the referenced element's current value differs from its previous-step value, else 0.0 (0.0 at step 0).

### 2.13' Latin Hypercube sampling (0.9.3, runtime-configured)

`simulation_settings.sampling_method: lhs` applies **Latin Hypercube stratification** to once-per-realization independent sample nodes: [0,1) is partitioned into `n_real` equal-probability bins, one draw per bin, shuffled, mapped through the distribution ICDF (truncation scales the uniform into `[F(lo), F(hi)]`). Per-step/autocorrelated/resampled nodes stay Monte Carlo (matches GoldSim — LHS applies to once-per-realization draws only). Distributions with no closed-form ICDF (gamma/beta/weibull/Pearson/PERT/Student-t/binomial/negative-binomial/Poisson) fall back to Monte Carlo for that node. LHS composes with Iman-Conover: correlated-group marginals are stratified then rank-reordered. Default (`monte_carlo`) behavior is bit-identical.

### 2.13 The `submodel_stat` AST node

An `expression` AST may contain a `submodel_stat` node — a Monte-Carlo statistic of a submodel output, reduced across the submodel's realizations (the source `pdf_*` functions). It is a first-class AST node (like `ref`), not a `call`, because it carries structured references plus a typed statistic:

**Fields:** `submodel_id` (full slash-path id of the submodel container, §12), `output` (full slash-path id of the interface output element being reduced), `statistic` (`mean` | `percentile` | `sd` | `cumulative_prob`), and `arg` (an AST sub-node; required for `percentile` and `cumulative_prob`, omitted for `mean`/`sd`).

**Behavior:** The engine runs the named submodel's nested realization loop (§12), collects the `output` element's per-realization final values, and reduces them:

- `mean` — arithmetic mean.
- `percentile` — nearest-rank percentile at `arg`, where **`arg` is p in [0, 100]** (matching `optimization.objective.statistic.percentile.p`).
- `sd` — sample standard deviation.
- `cumulative_prob` — the empirical CDF at `arg`: the fraction of realizations whose value ≤ `arg` (which may be a unit-bearing threshold, SI-normalized like any quantity).

The reduced value is a scalar constant available at every step of the parent run (the submodel run does not vary per parent timestep). An engine that has not yet implemented submodel execution evaluates `submodel_stat` to `0.0` with a warning, exactly like a dangling reference (§1) — so a model carrying these nodes parses and runs, it just reports the placeholder until nested execution is available. See SUBMODEL_STAT_ENCODING.md for the emit-side lowering rules.

---

## 3. Stock Traits

A `stock` element always has `initial_value` and either `rate` (net rate as quantity_or_formula) or `inflows[]` + `outflows[]` (element ids whose values contribute to the net rate).

**Base behavior (always active):** At each timestep, the engine integrates the net rate:

```
S_{t+1} = S_t + net_rate * dt
```

where `net_rate = rate` (if provided directly) or `net_rate = Σ(inflows) - Σ(outflows)` (if provided as element references). The engine evaluates all inflow/outflow source nodes before evaluating the stock.

### Trait: `capacity_clamp`

**Activated by:** `capacity` field present (quantity_or_formula).

**Behavior:** After integration, if `S_{t+1} > capacity`, clamp to `S_{t+1} = capacity`. The excess `S_{t+1} - capacity` (before clamping) is available to the overflow_routing trait if present. If overflow_routing is not present, the excess is silently discarded (the stock simply stops accumulating).

When the stock is at capacity, the **effective** inflow rate is reduced to balance outflows. Downstream elements that query the stock's inflow acceptance should see the reduced rate, not the attempted rate.

### Trait: `overflow_routing`

**Activated by:** `overflow_target` field present (element id). Requires `capacity` to also be present.

**Behavior:** When integration would push `S_{t+1} > capacity`, the excess quantity `(S_{t+1} - capacity)` is routed to the element identified by `overflow_target`. That element receives the excess as an additional inflow for the current timestep. If the target is itself a stock, the excess is added to its net rate. If the target is a link, the excess flows through it.

**Ordering constraint:** The overflow target must be evaluated after the overflowing stock within the same timestep.

### Trait: `compound_growth`

**Activated by:** `return_rate` field present (quantity_or_formula).

**Behavior:** The rate of change includes a self-referential term:

```
S_{t+1} = S_t * (1 + return_rate * dt) + net_external_flow * dt
```

where `net_external_flow` is the sum of inflows minus outflows excluding the return. The engine must use this multiplicative form, not the additive `S + rate * dt` form, to avoid systematic bias in compound growth calculations.

If `return_rate` is expression-valued (e.g., references a stochastic process node), the engine evaluates it at each timestep.

### Trait: `priority_withdrawal`

**Activated by:** `withdrawals[]` field present, where entries have `priority` values.

**Behavior:** At each timestep, the engine sorts withdrawal demands by priority (lower number = higher priority). It serves each demand sequentially: the first demand receives up to its requested amount (limited by available stock), the second receives up to its request from the remainder, and so on. Each withdrawal element receives its allocated amount as its output value for the timestep.

The stock's net change is `Σ(inflows) - Σ(allocated_withdrawals)`. The stock cannot go below zero (or below a `floor` if specified); withdrawals that would push below floor are curtailed.

---

## 4. Link Traits

A `link` element always has `source` (element id) and `target` (element id) and either `rate` (quantity_or_formula) or `fraction` (quantity_or_formula, 0–1).

**Base behavior (always active):** At each timestep, the link transfers quantity from source to target. If `rate` is specified, the transfer amount is `rate * dt`. If `fraction` is specified, the transfer amount is `fraction * source_output_value` (or `fraction * source_outflow` depending on context).

### Trait: `priority_allocation`

**Activated by:** `priority` field present (integer).

**Behavior:** When multiple links share the same source stock, the engine groups them and serves them in priority order (lower number = higher priority). Each link receives up to its requested rate, limited by remaining supply in the source. This is the link-side counterpart of the stock's `priority_withdrawal` trait; the engine may implement either or both depending on how the model is structured.

### Trait: `transit_buffer`

**Activated by:** `transit_time` field present (quantity).

**Behavior:** Material entering the link is not delivered to the target immediately. Instead, it enters an internal buffer and is released after `transit_time` has elapsed. The buffer represents in-transit material.

If `dispersion` is absent or zero, this is plug flow: a slug entering at time t arrives intact at time t + transit_time. The engine maintains a FIFO queue of (entry_time, amount) pairs and releases each slug when its transit time expires.

If `dispersion` is present, the arrival is spread according to a residence-time distribution rather than delivered as a plug-flow slug.

**Dispersion parameterization:** The `dispersion` field is a **Péclet number** (dimensionless, unit `"1"`). Higher values → less dispersion (more plug-like); lower values → more spread. The Péclet number is defined as Pe = v·L / D where v is the advective velocity, L is the pathway length, and D is the longitudinal dispersion coefficient. The transpiler computes Pe from the source model's geometry and dispersivity.

**⚠ DEFERRED MATH SPEC (required before Milestone 4):** The exact residence-time transfer function per geometry is not yet specified. The engine must implement one of the following approaches, to be selected and documented before M4:

- *Analytical:* For `geometry: "pipe"` and `"aquifer"`, use the 1-D ADE analytical solution for a pulse input in a semi-infinite domain (Ogata-Banks). The normalized RTD is:

  ```
  f(t) = (L / √(4πDt³)) · exp(-(L - vt)² / (4Dt))
  ```

  where L is pathway length (derived from transit_time × velocity), D = vL/Pe, and v = L/transit_time. This is discretized to timestep resolution and used as a convolution kernel.

- *Numerical:* Maintain a mixing-cell cascade of N = Pe/2 equal cells in series (the tanks-in-series approximation). Each cell has volume V/N and passes its contents to the next at the advective rate. This is simpler to implement but less accurate at low Pe.

The choice between these approaches, edge-case handling (Pe < 1, Pe → ∞), and interaction with the transit_decay trait under dispersed flow need specification in a dedicated appendix before M4 implementation.

**Engine state:** Requires a per-realization buffer for each transit-buffered link. For plug flow: a FIFO queue of (entry_time, amount) pairs. For dispersed flow: either a convolution kernel buffer (analytical approach) or a cascade of sub-cell states (tanks-in-series approach).

### Trait: `transit_decay`

**Activated by:** `decay_rate` field present (quantity_or_formula). Requires `transit_time` to also be present.

**Behavior:** Material in transit decays at a first-order rate. For plug flow, the delivered amount is `input * exp(-decay_rate * transit_time)`. For dispersed flow, the decay is applied to each parcel according to its actual residence time in the buffer.

### Trait: `scheduled_flow`

**Activated by:** `schedule` field present (trigger_spec).

**Behavior:** The link transfers quantity only when the schedule trigger fires. Between trigger firings, the transfer rate is zero. When the trigger fires, the transfer occurs at the specified `rate` or `fraction` for that timestep.

### Trait: `species_transport`

**Activated by:** `species` and/or `medium` and/or `fluxes[]` fields present.

**Behavior:** The link carries specific species through a specific medium between cells (not generic stocks). The `fluxes[]` array specifies the transport mechanisms (advective, diffusive, etc.) with their rates and coefficients. The engine applies the appropriate mass-transfer equations for each flux mechanism. See §6 Cell Primitive for how cells interact with species-transport links.

---

## 5. Gate Primitive

A `gate` element has a `root` field containing a recursive `gate_node` tree.

**Gate node types:**

- `and`: True if ALL children evaluate to true.
- `or`: True if ANY child evaluates to true.
- `not`: True if its single child evaluates to false.
- `n_vote`: True if at least `threshold` children evaluate to true (k-out-of-n voting).
- `reference`: Leaf node. Evaluates to the boolean state of the referenced element (truthy if value > 0).
- `condition`: Leaf node. Evaluates a quantity_or_formula as a boolean.
- `input`: Leaf node. Evaluates the state of a basic event / input element.

The engine evaluates the tree bottom-up, resolving leaf nodes first, then composing through gates.

Output: 1.0 (system state is true) or 0.0 (system state is false).

The `semantics` field (`success` or `failure`) does not change the evaluation — it is metadata for interpretation. A fault tree and an RBD use the same logic; the difference is whether "true" means "system has failed" or "system is working."

---

## 6. Event Traits

An `event` element always has a `trigger` (trigger_spec) and `effects[]` (array of target + change pairs).

**Base behavior:** At each timestep, the engine evaluates the trigger condition. If the trigger fires, the engine applies each effect: for each entry in `effects[]`, it modifies the target element's value by the specified change (additive, multiplicative, or replacement — determined by the `mode` field on each effect, defaulting to additive).

**Trigger modes** (`trigger.mode`, or inferred from present fields): `always`; `on_condition` (a boolean `condition` becomes true); `periodic` (`period`); `on_schedule` (`schedule[]` instants); and `on_event` — fires the step its `source` event is in the fired-set, the same signal the `occurs(<event>)` builtin reads (§2). **Causality of `on_event`:** the fired-set is populated by a pre-pass over trigger-driven events (condition/periodic/schedule) evaluated against the previous step's outputs, so an `on_event` trigger sees a *trigger-driven* source fire **same-step** and a *rate/failure* source fire **next-step**. The pre-pass is a single linear pass in element-declaration order, not a fixpoint — so chaining two `on_event` events within one step is **declaration-order dependent** (source must be declared before the follower to chain same-step). Deep same-step event cascades are a documented phase-1 limitation.

### Trait: `rate_generation`

**Activated by:** `rate` field present (quantity_or_formula).

**Behavior:** The event generates occurrences according to a Poisson process with the specified mean rate. At each timestep, the engine draws the number of events from Poisson(rate * dt). For each event generated, the effects are applied. If `event_value` is present, each generated event carries that value, which downstream elements can reference.

### Trait: `failure_state_machine`

**Activated by:** `failure_process` field present.

**Behavior:** The event manages a two-state automaton (working / failed) with stochastic transitions.

**Working → Failed transition:** Determined by `failure_process.basis`:
- `exposure_time`: Engine tracks cumulative elapsed time; draws time-to-failure from `failure_process.time_to_failure` distribution at the start of each operational period.
- `operating_time`: Same as exposure_time but only increments when the component is "operating" (determined by an external signal).
- `demand`: On each trigger firing, the component fails with a per-demand probability drawn from the distribution.
- `capacity_demand`: Fails when a demand value exceeds a capacity value (both may be stochastic). *(Not yet modeled — needs `demand`/`capacity` fields on `failure_process`; currently a no-op that never fails. Deferred.)*
- `event`: Fails the step the FSM's triggering event fires — the FSM's `trigger` (typically an `on_event` trigger naming the source) is evaluated via the same fired-set path as the `on_event` trigger mode above.
- `condition`: Fails when a boolean condition becomes true.

**Failed → Working transition:** Determined by `failure_process.repair`:
- `repair.policy: "none"`: Component stays failed permanently.
- `repair.policy: "repair"`: Engine draws repair duration from `repair.time_to_repair` distribution; component returns to working state after that duration.
- `repair.policy: "replace"`: Same as repair, but the time-to-failure clock resets (as-good-as-new).
- `repair.policy: "preventive_maintenance"`: Component is restored on a scheduled trigger regardless of failure state.

When the component transitions to failed, the event's `effects[]` are applied. When it transitions back to working, effects are reversed (if reversible).

**Engine state:** Requires per-realization state tracking: current state (working/failed), time-to-failure countdown, time-to-repair countdown.

---

## 7. Cell Primitive

A `cell` element represents a well-mixed compartment holding one or more species across one or more media.

**Fields:** `volume` (quantity_or_formula), `media[]` (medium references), `species[]` (species references with optional initial inventory), `inflows[]` (element ids of links/sources delivering mass).

**Base behavior:** At each timestep, the engine:

1. Receives incoming mass from inflow links and transport sources.
2. Applies partitioning equilibrium (if partitioning trait active).
3. Applies first-order decay and decay chain propagation (if species have half-lives/decay products).
4. Computes outgoing mass to connected transport links.
5. Updates the cell's mass inventory.

The cell tracks mass per species per medium. **Outputs:** the primary output is total mass (Σ over species and media); per-(cell, species) mass under result id `"<cell>:<species>"`; and, for multi-medium cells, per-medium mass under `"<cell>:<species>@<medium>"`. When the cell declares a positive bulk `volume`, the engine additionally publishes the derived **concentration** under a parallel result id `"<cell>:<species>@<medium>:C"`:

```
C = mass / (volume · medium_fraction · medium_porosity)
```

`medium_porosity` comes from the referenced `medium.porosity` (default 1.0 for a bulk medium); `medium_fraction` from the cell's `media[].fraction` (default 1.0). A cell with **no `volume`** emits mass only — concentration is undefined and no `:C` id is produced. The mass outputs are unchanged whether or not a volume is present (concentration is purely additive). Phase-1 uses `mass/(volume·fraction·porosity)` uniformly across phases (solid/fluid/gas); a phase-specific volume basis is a documented simplification.

> **Emit dependency.** No corpus model currently exercises cell concentration: GoldSim Contaminant-Transport cells (e.g. SimpleMixing's `Cell_Pond`, which outputs mg/L) are emitted as bare stubs because the structural decoder does not yet extract the SCellElem body (volume / media / species / porosity). The engine concentration path is in place; populating it for real models is gated on that emitter cell-body decode.

### Trait: `partitioning_equilibrium`

**Activated by:** `partitioning[]` field present.

**Behavior:** At each timestep, after mass inflows are applied, the engine redistributes mass between media within the cell according to equilibrium partition coefficients. For each partitioning entry:

```
C_to / C_from = Kd
```

The engine solves for the equilibrium distribution of each species across all media simultaneously. For a two-phase system (solid + fluid), this is a simple algebraic expression. For three or more phases, the engine solves a small linear system.

### Trait: `decay_chain_propagation`

**Activated by:** Any referenced species having `decay_products[]` with nonzero entries.

**Behavior:** At each timestep, the engine applies first-order decay to each species with a half_life:

```
mass_{t+1} = mass_t * exp(-λ * dt)     where λ = ln(2) / half_life
decayed_mass = mass_t - mass_{t+1}
```

For each decay product, the engine adds `decayed_mass * branching_fraction` to the daughter species' inventory in the same cell and medium. Decay chains are processed in parent-first order (topological sort of the decay graph).

### Trait: `source_release`

**Activated by:** Element is typed as a source rather than a cell (has `inventory` and `release_rate` but not `partitioning`).

**Behavior:** The source element releases mass into its target cell at `release_rate` per timestep, drawing from a finite `inventory`. When inventory is exhausted, the release rate drops to zero. If a `schedule` trigger is present, release occurs only when the trigger fires.

---

## 8. Correlated Sampling

When multiple `node` elements with `value_rule: "sample"` specify `correlations[]` entries, the engine must produce correlated samples.

**Method:** Iman-Conover rank correlation. At the start of each realization (or at each resampling trigger):

1. Generate independent samples for all correlated nodes.
2. Compute the rank matrix.
3. Apply the Iman-Conover transformation to induce the target rank-correlation structure.
4. Map the reordered ranks back to the marginal distributions.

**Diagnostics:** The engine must compute and report the achieved correlation matrix alongside the target. This is included in the simulation output metadata, not in per-timestep results.

**Grouping:** Correlations are specified per-pair on individual nodes. The engine collects all nodes participating in any correlation into a single correlation group and builds the full target correlation matrix. Unspecified pairs default to zero correlation.

### 8a. Importance Sampling (0.9.7)

A `sample` node opts into **importance sampling** — a variance-reduction technique for rare events — with a `distribution.importance` block:

```jsonc
"distribution": {
  "family": "normal", "parameters": { "mean": 0, "stddev": 1 },   // f — the TARGET
  "importance": { "bias": {
    "family": "normal", "parameters": { "mean": 4, "stddev": 1 }   // g — SAMPLED FROM
  }}
}
```

The declared distribution is the **target** `f`; `importance.bias` is the **biased** distribution `g`. Each realization the engine draws `x ~ g` (not `f`) and multiplies the **likelihood ratio** `w = pdf_f(x) / pdf_g(x)` into that realization's importance weight (the product across all importance nodes in the realization). Those weights are combined with any user realization weights (§B7), normalized, and applied to the **weighted statistical reductions** (weighted mean / percentile / std / CTE in the A3 analysis layer). This makes `E_g[w·h]` an unbiased estimator of `E_f[h]`: the expectation is recovered while rare-event tails are estimated with far lower variance than plain Monte Carlo. (As with §B7, the weights affect the *analysis* reductions, not the raw `time_history.mean`.)

**PDF roster (phase 1):** the likelihood ratio requires a closed-form PDF for both `f` and `g`. Supported families: **normal, lognormal, uniform, exponential**. Any other family (for either `f` or `g`) is a hard error — never a silent wrong weight, which would bias every estimate. (Lognormal PDF uses log-space `(μ, σ)` parameters, matching the sampler.)

**Interaction with LHS / Iman-Conover (phase 1 limitation):** an importance node draws plain **Monte Carlo** from `g` — LHS stratification and Iman-Conover rank-correlation are **skipped for that node** (other nodes are unaffected). The biased-stratified pairing (draw stratified from `g`, weight by `f/g`) is deferred. The engine requires `g` to have positive density wherever its draws land (else it errors), so `g`'s support must cover the region of interest.

---

## 9. Evaluation Order

The engine evaluates elements within each timestep in **topological order** of the dependency graph. Dependencies are derived from:

- `inputs[]` arrays on nodes and expressions
- `source` / `target` references on links
- `inflows[]` / `outflows[]` on stocks
- `input` references on lag, filter, hysteresis, convolution nodes
- Element references within AST nodes

**Lag nodes** (`value_rule: "lag"`) break dependency cycles by referencing the previous timestep's value. The engine must detect and permit these back-edges in the dependency graph.

**Unbroken cycle policy:** When the dependency graph contains a cycle not broken by a lag node, the engine applies one of two policies based on the model's schema version:

- **v2-native models** (`wasim_version >= "0.8.0"` and `source.generator` is not a legacy transpiler): The engine **rejects** the model at load time with a diagnostic identifying the cycle. v2-native models are expected to be cycle-free by construction.

- **v1-imported models** (`wasim_version < "0.8.0"` or `source.generator` identifies a legacy transpiler): The engine **warns** and evaluates the cycle using the previous timestep's values for back-edge dependencies (effectively inserting implicit lag behavior). This preserves compatibility with corpus models that rely on evaluation-order-dependent cycle resolution. The warning should identify each implicit lag insertion so the model author can add explicit lag nodes if desired.

The engine must report which policy was applied and which cycles (if any) were resolved implicitly.

**Step count and single-evaluation models.** The number of timesteps is
`n_steps = max(1, round(duration / timestep))` (with `duration` reconciled into the
timestep's unit first). Consequences:

- `timestep` must be `> 0`.
- `duration` must be `>= 0`. A **`duration` of 0** (or any duration below half a
  timestep) yields `n_steps = 1`: the engine evaluates the model **once** at
  `t = start` — producing each element's initial/instantaneous value and its
  final value — then stops. Stocks return their `initial_value` (no interval to
  integrate over). These are GoldSim **driver / instant** models: optimization and
  statistics drivers, single-period calculations, and sequence/parameter
  generators whose real timeline is a nested submodel run (§12) or a static
  evaluation, not a top-level trajectory. A submodel (§12) with `duration = 0`
  likewise runs one evaluation per realization — which is exactly the point for a
  Monte-Carlo statistics driver: `n_realizations` samples at a single time point.

---

## 10. Trait Activation Summary

The engine infers active traits from field presence. No `traits` array is required in the schema (though one may be included for documentation/validation). The rules:

| Primitive | Field(s) Present | Trait Activated |
|-----------|------------------|-----------------|
| `stock` | `capacity` | `capacity_clamp` |
| `stock` | `overflow_target` (requires `capacity`) | `overflow_routing` |
| `stock` | `return_rate` | `compound_growth` |
| `stock` | `withdrawals[]` with `priority` | `priority_withdrawal` |
| `link` | `priority` (among siblings sharing source) | `priority_allocation` |
| `link` | `transit_time` | `transit_buffer` |
| `link` | `decay_rate` (requires `transit_time`) | `transit_decay` |
| `link` | `dispersion` (requires `transit_time`) | `transit_dispersion` |
| `link` | `schedule` | `scheduled_flow` |
| `link` | `species` / `medium` / `fluxes[]` | `species_transport` |
| `event` | `rate` | `rate_generation` |
| `event` | `failure_process` | `failure_state_machine` |
| `cell` | `partitioning[]` | `partitioning_equilibrium` |
| `cell` | species with `decay_products[]` | `decay_chain_propagation` |
| `cell` | `inventory` + `release_rate` | `source_release` |

**Validation rule:** When a trait requires another field (e.g., `overflow_routing` requires `capacity`), the engine must reject the model if the dependent field is absent.

---

## 11. Definition Types

### `species`

**Fields:** `half_life` (quantity, optional), `decay_products[]` (species id + branching fraction), `molecular_weight` (quantity, optional), `properties` (free-form object).

**Behavior:** No simulation behavior. Defines properties consumed by `cell` elements and the decay chain propagation trait.

### `medium`

**Fields:** `phase` (solid/fluid/gas/reference_fluid), `density` (quantity_or_formula), `porosity` (quantity_or_formula), `properties` (free-form object).

**Behavior:** No simulation behavior. Defines phase properties consumed by `cell` elements and the partitioning equilibrium trait.

### `container`

**Fields:** `id`, `name`, `parent`, `children[]`, `elements[]`, and (optional) `kind`, `simulation_settings`, `interface`.

**Behavior:** For `kind` of `container` or `group` (the default): organizational grouping only — the engine ignores them. They exist for frontend graph rendering and auto-dashboard section grouping. For `kind: "submodel"` the container is a *nested simulation* and is not ignored — see §12.

---

## 12. SubModels

A container with `kind: "submodel"` is a **nested simulation** embedded in the parent model. It is the only container kind with runtime behavior.

**Fields (in addition to the organizational ones):**

- `simulation_settings` — the nested run's time-stepping and Monte-Carlo settings (`duration`, `timestep`, `n_realizations`, `sampling_method`, `seed`). `null` or absent inherits the parent model's settings.
- `interface` — the boundary the parent sees. `inputs[]` is a list of `{input, from}` bindings: `input` is the interior consumer element the value flows into; `from` is the parent driver element that supplies it (`null` for an engine/dashboard-supplied input with no model driver, e.g. a realization count). `outputs[]` is a list of interior element ids the parent may read back (via `submodel_stat`, §2.13). Element ids may be qualified by the submodel name where the parent references them (e.g. `SubModel1.System`).

**Behavior:** When the parent evaluates a submodel it runs the submodel's own realization loop (§9) under the submodel's `simulation_settings`, drives each `interface.inputs` binding by setting the interior `input` element to the parent `from` element's current value (bindings with `from: null` are left at their authored value), and exposes the `interface.outputs`. Because a submodel runs many realizations, a parent expression reads a **reduced statistic** of a submodel output rather than a single trajectory (e.g. `pdf_mean(SubModel1.total_cost)`, `PDF_Value(SubModel1.System, 0.95)`) — this is precisely how a *probabilistic* objective (§13) is formed. The submodel is the boundary at which "run N realizations and reduce to a statistic" happens.

Membership of a submodel's interior elements follows the same convention as any container: each interior element carries a `container` back-reference (authoritative), and the container may also enumerate them in `elements[]`. The `interface` names only the boundary subset, not the full interior.

**Version gating:** submodel behavior applies to models with `wasim_version >= "0.8.1"`. Earlier v2 models had no `kind` field; such containers are treated as organizational (`container`).

---

## 13. Optimization

The optional top-level `optimization` block defines a **study over the model**: search for the input values that make a result best. It is a wrapper *around* the realization loop of §9 — not a per-timestep behavior (the submodel-scoped exception is **dynamic optimization**, §13a) — so it has no effect on a single simulation run and is consumed only when a study is requested.

**Shape:**

- `objective` — the result being optimized: `element_id` (a result element that must depend on every variable), `direction` (`maximize` | `minimize`), and an optional `statistic`.
  - `statistic: null` (or absent) → **deterministic** objective: the objective element's value from a single run.
  - `statistic: { kind, p? }` → **probabilistic** objective: the objective output reduced across realizations by `kind` (`mean`, `percentile` with `p` in [0,100], `peak`, `valley`, `sum`). Typically the objective element references a submodel output (§12), in which case the reduction happens at the submodel boundary.
- `variables[]` — the inputs the optimizer may adjust. Each references an editable `fixed`-value or `sample` element (`element_id`) and carries SI `lower`, `upper`, and `initial` bounds (quantities, so bounds normalize to SI like every other value; `display_unit` is frontend-only), plus optional `integer` to restrict the search to whole numbers.
- `constraints[]` — optional conditions a valid solution must satisfy, each a `quantity_or_formula` `condition` with an optional `label`. **Enforced (0.9.3):** each candidate's constraint conditions are evaluated against the same run's element values; a candidate that violates any constraint is treated as infeasible (cost +∞) so the search is confined to the feasible region — Box's-complex implicit-constraint handling. A condition is *satisfied* when it evaluates truthy (≥ 0.5, so a comparison AST's 1.0 passes); an unverifiable condition (unparsed formula string, bare quantity, or a ref to an element with no output) is treated as satisfied so the engine never rejects a candidate on a constraint it cannot evaluate.
- `sampling` — for a probabilistic study, `realizations_per_trial` Monte-Carlo runs are performed per candidate solution. `null` (or absent) means deterministic, or that the referenced submodel's own `simulation_settings.n_realizations` governs.

**Behavior:** the engine searches the `variables` space (subject to `constraints`) to optimize the reduced `objective`, evaluating each candidate by setting the variable elements to the candidate values and running the model (or, for a probabilistic study, `sampling.realizations_per_trial` realizations, or the referenced submodel's nested run). The **deliverable** of an optimization run is the optimal variable values plus the objective's achieved value — a study result, not a single time history.

The search algorithm itself (e.g. Box's complex method) and per-trial iteration state are **engine implementation details**, not part of the schema or this contract: the schema carries the *problem definition* only.

**Version gating:** the `optimization` block is honored for models with `wasim_version >= "0.8.1"`. It is absent for the overwhelming majority of (non-study) models.

---

## 13a. Dynamic (per-timestep) optimization

A **submodel** (a `container_def` with `kind: "submodel"`, §12) may carry its own `optimization` block (same shape as §13). Unlike the top-level study, a submodel-scoped optimization is **re-solved at each outer timestep**: as the outer clock advances and the submodel's interface inputs change, the inner search runs again against the objective evaluated at that step. The optimized variables therefore become **per-timestep series** (a time history on each variable element), not a single study result.

This is GoldSim's "Dynamic Optimization": an inner optimization inside a submodel that tracks a time-varying driver. Example: minimize `(Parameter − √Driver)²` where `Driver` oscillates over the run → `Parameter` traces `√Driver(t)` step by step.

**Behavior:**

- The objective and variable `element_id`s are **interior submodel elements** (full slash-path ids under the submodel container).
- At each outer step `t`: freeze the submodel's inputs at their step-`t` values, run the inner search (same algorithm as §13) to set the variable elements, and record each variable's winning value into that step's time history. Advance.
- The variable's series is exposed like any other submodel output (§12 interface `outputs`); the parent reads it through the normal interface path. No study-result deliverable is produced — the deliverable *is* the series.
- A submodel-scoped optimization is **independent of** any top-level `optimization` block; a model may have neither, either, or both (they optimize different scopes).

**Distinction from §13:** top-level `optimization` = a once-only static study whose deliverable is the optimal values (no time history). Submodel `optimization` = a per-timestep dynamic solve whose deliverable is a time series. The locus of the block (top-level vs. submodel container) is the sole signal; there is no separate mode flag. This mirrors the source model, which keeps an optimization on each clock and gives every submodel its own clock.

**Version gating:** submodel-scoped `optimization` is honored for models with `wasim_version >= "0.9.0"`.

---

## 14. Units

All `quantity` values in the schema carry a `unit` field. The engine normalizes all values to SI at model load time. The `display_unit` field (if present) is metadata for the frontend and is ignored by the engine.

The engine must support the unit strings used in the schema and perform correct dimensional conversion. At minimum: time (s, min, hr, d, wk, mo, yr), length (m, cm, mm, km, ft, in, mi), mass (kg, g, mg, µg, lb), volume (m3, L, mL, gal, ft3), concentration (kg/m3, mg/L, µg/L), rate (/s, /d, /yr), dimensionless (1, %, fraction).

The engine does NOT perform runtime dimensional analysis of *values* (numeric behavior uses declared magnitudes as-is). It DOES offer an optional **static dimensional check** (B5, `RunConfig.units`): `warn` (default) logs dimensional inconsistencies and continues (behavior unchanged); `strict` rejects a model with any inconsistency before the run.

The checker infers each expression element's dimension as an exponent vector over the base dimensions {Time, Length, Mass, Volume, Temperature} and compares it to the element's declared output unit. Rules: `+`/`−`/comparisons require equal dimensions; `×`/`÷` compose; integer `pow` scales; `sqrt` halves (must yield integer exponents); transcendentals (`exp`/`ln`/`sin`/…) require a dimensionless argument; `min`/`max`/`abs` preserve the operand dimension; lookups use the table's declared axis units (`TBL_Integral` ×x-dim, `TBL_Derivative` ÷x-dim, `TBL_Inverse` →x-dim). Unknown units, unresolved refs (reserved globals, submodel ports), bare unit-less literals, and unsupported nodes are **exempt** — so a partially-emitted model never yields a false positive and always loads under `warn`.

**Calendar (B6).** By default the `time_ref` calendar properties (`year`/`month`/`day_of_month`/`day_of_year`/`days_in_month`) use a **fixed 365-day** calendar (no leap years), with `year` an offset from the run start. When `simulation_settings.calendar_start` is set (seconds since the Unix epoch, 1970-01-01), those properties instead use a **real proleptic-Gregorian calendar with leap years** anchored there — February has 29 days in a leap year, `year` is the actual calendar year, and day-of-year/month-length track the true calendar. The `get_year(date)`/`get_month(date)`/… builtins (which take an explicit date-in-seconds argument) already use the real calendar regardless of the anchor.

Additional calendar `time_ref` properties (calendar-aware; require an anchor to be meaningful, 0 without one): **`hour`/`minute`/`second`** (clock time-of-day components), **`start`** (the `calendar_start` anchor itself, in epoch seconds), and **`elapsed_months`/`elapsed_years`** (whole calendar month/year *field boundaries crossed* since the start — GoldSim EMonth/EYear; not derivable from `elapsed` because month/year lengths vary). Note that GoldSim's `SimDuration` and `Realization` run properties are provided as **reserved globals** (§1b), not `time_ref` properties, and the `TBL_*` table modes are `lookup_call` reserved names (§1b).

---

## 15. Arrays and Dimensions

Array-valued formulas are built over **dimensions** (ordinal sets) — named, ordered index sets declared once at the top level and iterated by comprehension.

### Dimensions

The top-level `dimensions[]` list declares each ordinal set: `id`, `name`, `size` (member count, e.g. Months = 12), and optional ordered `labels[]`. An element output is marked array-valued by listing the dimension ids it ranges over in `output_spec.dimensions` (empty = scalar). Member *values* (e.g. the numeric percentiles of a `Percentiles` dimension) are not stored on the dimension — they live in whichever element carries them, indexed by position.

### Array AST nodes

Four `ast_node` ops build and access arrays:

- **`vector_map`** — `{over, body}`: evaluates `body` once per member of dimension `over`, producing an array dimensioned over that set. A matrix is nested `vector_map`s (outer = rows, inner = cols).
- **`index_ref`** — `{axis: row|col}`: the current iteration index inside the enclosing `vector_map` body. `row` is the first (array) axis; `col` the matrix second axis.
- **`index`** — `{array, indices[]}`: element access `array[i]` (one index) or `matrix[i, j]` (two). Indices are expressions and may be `index_ref`s. (This is the dimension-indexed subscript; `call get_element` remains the positional-scalar subscript.)
- **`extern_call`** — `{fn, args[]}`: a source-model function the engine does not implement, preserved verbatim (name + args) for round-tripping and connectivity. The engine treats it as opaque (evaluates to 0.0). Use `call` with a real builtin `fn` where one exists — e.g. the gamma function is the builtin `call fn:"gamma"` (Γ(x), used in Weibull scale derivation `scale = mean / Γ(1 + 1/shape)`), **not** `extern_call gamma`, so it evaluates rather than degrading to 0.0.

### Evaluation

A `vector_map` over dimension `d` produces a length-`size(d)` vector; within `body`,
`index_ref` yields the current **1-based** member index (matching `get_element`/`index` and
GoldSim arrays) and `index` selects an array member (1-based; a second index addresses a
matrix column). `extern_call` evaluates to `0.0` with a warning (opaque).

**Engine status: implemented.** The comprehension executor is live — `vector_map` iterates
the named dimension and returns a `Value::Vector`; `index_ref` reads the current member index
off a shared index stack; `index` subscripts (1-based). Only `extern_call` remains a
degrade-to-`0.0` opaque node. (These nodes existed in 0.7.0, were dropped in the 0.8.0
primitives rewrite, restored in 0.8.3, and the executor was wired in 0.9.7.)

**Array-valued element outputs and per-member results.** An element is array-valued when its
primary `output_spec` declares `dimensions` (member count = product of those dimensions'
sizes). Such an element's `Value::Vector` flows through the graph and is read per-member by
`index`. At the results boundary the engine **expands** an array-valued element into
per-member series under `"<id>#k"` (1-based, reusing the stock/queue `#k` port convention),
each a fully labelled (`Name[k]`), unit-bearing `ElementResults` with its own time-history and
final-value distribution — so a fleet's per-member spread falls straight out of the A3
analysis layer. The primary `"<id>"` still records member[0] for back-compat.

**Per-member state (stateless recurrence).** `lag` preserves the `Value` shape, so an
array-valued input lags per member. This gives per-member accumulation without an array-valued
*stock*: an array `expression` reading its own previous-step value through a `lag` (e.g.
`x[i]_t = x[i]_{t-1} + rate[i]·Δt`) integrates each member independently. Array-valued stocks
are therefore not required for multi-unit models.

**Array-valued stateful nodes.** The `status` latch is array-aware: when its primary output
declares `dimensions`, it holds a per-member `Vec<bool>` and evaluates its `set`/`reset`
conditions once per member with that member's index bound (pushed onto the same index stack
`vector_map` uses), so `set: damage[i] >= 1` latches truck *i* independently and HOLDS between
set and reset per member. Other stateful node rules (`pid`, `milestone`, `fsm`,
`convolution`, `filter`, `markov`) remain scalar-state per element in this round; where a
model needs N independent instances of those, use N elements or express the behavior with an
array `status` + array `expression` where possible.

**Array reductions and dispatch.** The array-consuming builtins reduce a vector to a scalar:
`sum_array`, `mean_array`, `min_array`, `max_array`, `size_array`, `dot_product`,
`interp_array`, `get_element`, and (0.9.7) **`argmin_array` / `argmax_array`** — the 1-based
index of the extremum member, ties resolving to the **lowest index** (a determinism / bit-
identity requirement). A *masked* selection (e.g. "least-damaged **available** truck") is
expressed by adding a large penalty to masked-out members before `argmin_array`
(`argmin(damage + BIG·failed)`), which needs no dedicated masked-reduction builtin.

---

## 16. Mean-reverting processes

A `process` node (`process_spec`) with a non-zero `reversion_rate` is **mean-reverting**
(Ornstein-Uhlenbeck), not plain GBM. Per step the level updates as

    x_{t+Δt} = x_t + κ·(θ − x_t)·Δτ + σ·√Δτ·z,   z ~ N(0,1)

where κ = `reversion_rate`, θ = `reference_value` (falling back to the drift `mean` when
absent), σ = `stddev`, and Δτ normalizes the timestep into the volatility's time unit. The
node's output is the **level** series (seeded at `initial_value`, else the reference/drift
level). When `reversion_rate` is absent or zero the process is a non-reverting random walk and
behaves exactly as before (a per-step GBM rate). Version-gated `wasim_version >= "0.9.0"`.

## 17. Expression-valued convolution response

A `convolution` node's `response` may be an **expression** over the local lag variable — an
`extern_call` with `fn: "lag"` (τ ≥ 0, seconds) — instead of a baked `{times, values}` table.
The engine samples the expression onto the lag grid (`interval` … `length`) at run time,
binding `lag` at each grid point and resolving any referenced elements from the current
context. A `cumulative: true` response is an S-curve whose convolution weights are its
successive differences; otherwise weights are `value × interval`. This keeps a parameter
referenced in the response (e.g. a calibratable unit-hydrograph shape parameter) **live** —
it responds to the parameter's value rather than a value frozen at emit time, which is what a
baked table would lose. Version-gated `wasim_version >= "0.9.1"`.

## 18. Series calendar and ensemble metadata

A `series` node may carry: `calendar_based` (the source was calendar-dated; its `timestamps`
are re-based to elapsed-from-start with `calendar_start_seconds` recovering the absolute axis),
`n_histories` (the source stored a stochastic ensemble; `values` is the first history and the
count is carried), and `extra_value_rows` (additional rows of a multi-column/array-valued
series). These are fidelity/round-trip metadata; the engine plays back the first history's
`values`. Version-gated `wasim_version >= "0.9.1"`.

## 19. Financial payoffs

An `event` may carry a `payoff` (`payoff_spec`) describing an **option** (pays when an
underlying crosses a strike) or **insurance** (pays claims above a deductible up to a coverage
cap). Its fields reference the driving inputs by element id. This is a threshold-conditional
payout that the ordinary additive/multiplicative/replace `effects` cannot express. The payoff
is **provenance-complete but not executed** by the engine today (`effects` stays empty);
executing financial payoffs is a future round. Version-gated `wasim_version >= "0.9.1"`.

## 20. Linked-Excel (spreadsheet) elements

A `spreadsheet` node represents a GoldSim linked-Excel element: a set of `cells` binding model
ports to workbook ranges (`direction` input/output, `range`, `port`), plus an optional
`external_file` (the workbook). The workbook is external, so **the engine cannot evaluate the
links** — a `spreadsheet` node loads and runs as a fixed-`0` placeholder, with the cell/workbook
binding preserved for round-trip and inspection. Running a linked-Excel model faithfully needs
an Excel-evaluation round; until then these elements are provenance-only. Version-gated
`wasim_version >= "0.9.1"`.

## B1. Timebase and unscheduled updates (runtime-configured; no schema change)

The engine is a fixed-step explicit-Euler evaluator over a single global `dt` (the **grid**).
A **runtime** `RunConfig.timebase` selects how each grid step is integrated:

- `fixed` (default) — the original fixed-grid evaluator, **bit-identical**.
- `event_accurate` — inserts **unscheduled sub-step updates** inside a grid step to refine
  *integration* at known instants (scheduled event/link/resampling times) and at **stock bound
  crossings** (floor/capacity reached mid-step; closed-form under Euler — see below).

**The invariant (load-bearing).** The grid remains the statistical, state-machine, and reporting
lattice; sub-steps refine **integration only**:

- **Per grid step (once):** all RNG draws (sample redraws, AR(1)/process, Markov transitions,
  Poisson/failure event firing), the stateful node rules (`hysteresis`, `filter`, `status`,
  `milestone`, `pid`, `markov`, `convolution`), the event pass (firing + effects), transit-buffer
  advancement, dynamic optimization (§13a), and history recording (`hist_store` stays
  `n_steps`-shaped — the results contract is unchanged; the frontend / sensitivity / optimizer
  never see sub-steps). Determinism guarantee: **sub-steps consume no randomness**, so seed /
  realization streams are identical to `fixed`.
- **Per sub-interval (with the actual sub-`dt`):** topological expression evaluation, stock
  integration (rate·sub_dt, floor/capacity/overflow/withdrawals), link transfers, and cell
  transport.

**Bound-crossing sub-stepping (`event_accurate`).** When a bounded stock's Euler trajectory would
cross its `floor` or `capacity` strictly inside a sub-interval, the engine solves the crossing time
in closed form (`t_c = t + (bound − level)/rate`, linear under constant-rate Euler), shortens the
sub-interval to end exactly at `t_c`, and re-runs it. The stock lands *on* the bound at `t_c`; the
next sub-interval resumes there, so its topological pass re-evaluates every element that reads the
stock (a rate expression, an `Is_Full`-style gate, a downstream stock's inflow, an `overflow_rate`
port) against the clamped level for the remaining `grid_end − t_c`. This is the coupled
re-evaluation GoldSim performs; single-stock mass was already conserved under Euler + clamp, so the
*payoff is the coupling*, not the mass. Guards: the re-run consumes **no randomness** and touches no
grid-only state (both run only on the final sub-interval, which a crossing-truncated interval never
is — the RNG invariant holds across crossings); and a **max of 64 crossing splits per grid step**
bounds a pathological always-crossing rate, after which the remainder integrates grid-quantized with
a `warn:` to stderr.

**Calendar.** Calendar fields (`get_year`/`get_month`/…) derive from **elapsed time**, so they
are correct at sub-interval instants (identical to the former step-count derivation on a uniform
grid).

**Phase-1 fencing (documented limitations).** (1) The event pass is grid-only, so a scheduled
event's *effect* is applied at the grid step it falls in, not at the exact sub-interval instant —
the sub-interval integration around it is still refined, but effect-at-instant timing is deferred.
(2) Condition-triggered events (arbitrary expressions, not analytic in t) stay grid-quantized.
(3) Fraction transfers under sub-stepping are apportioned linearly (`frac·sub_dt/dt` per
sub-interval). *(Stock bound-crossing splitting is no longer fenced — it is wired into the step loop
under `event_accurate`; see "Bound-crossing sub-stepping" above.)*

## B3. Queues and Resources (0.9.4)

Two discrete-event additions (gap #4).

### `queue` node rule — event / discrete-change delay

**Fields:** `input` (arrival signal), `delay_time` (quantity_or_formula), optional `capacity`
(max amount waiting), `discipline` (`conveyor` default | `fixed_at_entry`).

**Behavior:** each step, the amount arriving via `input` is admitted to the queue (up to
`capacity − current level`; excess is blocked/dropped that step) and scheduled to exit
`round(delay_time/dt)` steps later. The node's **primary output** is this step's **throughput**
(amount exiting); a **secondary output with role `num_in_queue`** reports the current queue level,
read via an output-qualified `ref` (`ref{output:"<id>#k"}`, same convention and one-step read
delay as stock ports §1c). Per-realization queue-schedule state; grid-only (advances once per grid
step, per the B1 invariant). `conveyor` = fixed transit from entry (plug flow); `fixed_at_entry`
fixes the delay at the value evaluated when the entity enters.

### `resource` primitive — Resource / Resource Store

**Fields:** `initial_value` (starting balance), optional `capacity` (upper bound).

**Behavior:** a per-realization scalar **balance**; the element's output is the current balance
(updated in the event pass, so same-step consumers read the prior step's value, like a stock
level). Event effects adjust it:

- `spend` — withdraw `change × count`, **limited to the available balance** (partial when supply
  is short; never negative). Allocation across multiple spending events is in event-pass order.
- `deposit` — add `change × count`, **clamped to `capacity`**.
- `borrow` — spend with a tracked outstanding balance; a **reverse** transition (a repair/return
  event, e.g. `failure_state_machine` returning to working) restores the borrowed amount.

## A3. Results / analysis layer (0.9.3, runtime-configured)

The engine's default result surface is a fixed `mean + p05/p25/p50/p75/p95` time-history band
plus per-realization final values. A **runtime** `RunConfig.results_spec` (not schema — it is a
run option, like the sensitivity sweep) opts selected elements into richer statistics, emitted
as an additive `analysis` object on each `ElementResults` (`skip_serializing_if` keeps default
output byte-identical). The spec unlocks, per element, from the same run's stored samples:

- **custom percentile bands** over the time history (any percentiles in [0, 100]);
- **final-value distribution objects** — PDF (binned density), CDF, and CCDF (exceedance = 1 − CDF);
- **capture-time snapshots** — the distribution of values across realizations at requested elapsed
  times (mean/p05/p50/p95 + the raw per-realization values), snapped to the nearest stored step;
- **final-value summary stats** — the mean's confidence interval (t-interval, normal-quantile
  approximation), sample skewness, excess kurtosis, and the conditional tail expectation (mean of
  the samples beyond a chosen upper percentile).
- **reporting-period aggregation (B4)** — `reporting_period` (a fixed length in the timestep unit;
  true calendar months/years arrive with B6) reduces the per-step mean series into consecutive
  periods, each emitting the requested `reporting_reductions`: **accumulated** (Σ value·dt over the
  period), **average** (mean of the period's steps), **change** (last − first), **rate_of_change**
  (change ÷ period length). An empty reductions list emits all four; the final period may be partial.

A Milestone's "achievement probability / mean lag" (§2.14) is exactly this layer applied to the
milestone's final-value distribution across realizations.

**Realization weights (B7).** `RunConfig.realization_weights` (length = `n_realizations`, normalized
to sum 1) makes every reduction in this layer **weighted** — weighted mean, weighted percentile
(via the weighted empirical CDF), weighted std, and the weighted CTE. Absent/uniform weights
reproduce the unweighted statistics; a mismatched-length vector is ignored. This is the hook for
importance sampling (weight = f/g).

---

*Document version: 0.9.6*
*Companion to: wasim-schema-v2.json*
