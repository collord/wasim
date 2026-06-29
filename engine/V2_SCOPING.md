# Engine v2 Schema — Implementation Scope

Branch: `engine-v2-schema`
Target schema: `schema/wasim-schema-v2.json` (`$id …/model/0.8.0`)
Behavioral contract: `schema/wasim-engine-semantics.md`

## Decisions (locked)

1. **Internal v1→v2 normalizer.** The engine's internal model is rebuilt around the
   six v2 primitives + traits. A `v1_import` shim lifts the existing fixed-taxonomy
   elements into primitives at load time, so the 162-example corpus keeps running as
   regression coverage. The engine core sees **only** v2 primitives.
2. **Full v2 parity in one plan.** Every primitive, trait, node `value_rule`,
   distribution family, and cross-cutting facility in the schema + semantics doc is
   in scope. A recommended dependency-ordered build sequence is given in §8 so the
   work can land incrementally even though the target is complete parity.

### Spec sync — semantics/schema 0.8.0 update

The semantics doc + schema were revised in response to this scope and now resolve three
of the §11 risks (and adopt this doc's M1–M5 milestone numbering):

- **R1 (dispersion)** — `dispersion` is now defined as a **Péclet number** (dimensionless,
  unit `"1"`; Pe = v·L/D, higher = less spread). Two candidate transfer functions are
  named (Ogata-Banks analytical RTD; tanks-in-series). The *choice* + edge cases remain a
  deferred pre-M4 appendix. Downgraded from blocking to "approach selection pending."
- **R2 (cycle policy)** — resolved. Semantics §9 now mandates a version-discriminated
  policy: **reject** for v2-native models, **warn + implicit-lag** (previous-timestep
  values for back-edges) for v1-imported. Now canonical, not a proposal.
- **R3 (multi-step delay)** — resolved. Semantics §2.7: `lag` is strictly one-step;
  multi-step delays map to chained lag nodes / `convolution` / `filter`.

### Implementation progress

**M1 landed (branch `engine-v2-schema`).** Built as additive modules so the v1 engine
stays as the golden reference throughout — no big-bang rewrite, no flip of `run()` yet.

- `model_v2.rs` — complete internal v2 model (all primitives/traits/specs).
- `v1_import.rs` — `normalize(&WasimModel) -> Model`; all 162 corpus models normalize.
- `eval.rs` — decoupled from `WasimModel` (lookup-map + `dt_unit`); AST walker now shared.
- `graph_v2.rs` — dependency graph over v2 primitives (same skip-cycle policy as v1 for now).
- `engine_v2.rs` — Monte-Carlo loop re-homing node `fixed`/`expression`/`sample`/`process`/
  `lookup`/`series`/`lag` + `stock` (base/floor/capacity), reusing eval/sampling/result helpers.
- Exposed as `run_v2`; v1 `run` untouched.

**Verification:** v2 output is **bit-for-bit identical to v1 on the 96 corpus models with
unchanged semantics** (whole-corpus equivalence test). 27 models are run-only (delay/cycle —
intentional divergence: chained-lag fixes a v1 off-by-one; cycle policy is M-later), 39 are
v1-rejected (the duration-0 / non-finite corpus data bug). Inline equivalence tests cover
constant/expression, accumulator integration, seeded RNG, and the exact-delay chained lag.

**Deferred within M1 (deliberate):** flip `run()` to default to the v2 core + delete the v1
engine (do after the new cycle policy lands so cyclic models re-baseline together); the
version-discriminated cycle policy (semantics §9) itself; SI unit normalization (M5).

**M2 complete.** Everything driven by hand-authored v2-native fixtures (no v1 equivalent).

- `v2_parse.rs` (`parse_v2`) — v2-native JSON → `Model` via a raw-DTO + lowering layer
  (node/stock/gate/species/medium; link/event/cell error until M3/M4).
- Net-new node rules **`hysteresis`, `filter` (incl. EMA), `markov`, `convolution`,
  `gate_logic`** + the **`gate` primitive**. Per-realization state (flag/ring-buffer/EMA/
  state-index) + gate-tree + AST dep extraction in the graph.
- Stock traits **`compound_growth`, `overflow_routing` (single-level), `priority_withdrawal`**
  (request/limit as rates; targets output their allocation).
- **`trigger_spec`** evaluation (always/on_condition/periodic/on_schedule; on_event→M3) +
  sample **`resampling`**.
- **+7 distribution families** (pert, pareto, extreme_value, student_t, cumulative, sampled,
  external) + 4-parameter beta; closed-form `icdf` for pareto/extreme_value/cumulative/sampled.
- **Iman-Conover** rank correlation (precomputed across realizations; van der Waerden scores +
  decorrelate/recorrelate via Cholesky), replacing the Gaussian copula in the v2 path.
- Tests: v2_parse(3), engine_v2_rules(8), distributions_v2(8), triggers_v2(2),
  stock_traits_v2(2), iman_conover_v2(1) — all green; v1-import equivalence unaffected.
- **Known limits (noted for later):** overflow cascades are single-level; `convolution`
  `response` Ref resolves only lookups (not series); log_linear/spline interpolation map to
  linear/cubic; `external` distribution degrades to 0.0.

**M3 in progress.**

- **`link` primitive** — rate/fraction transfer source→target with mass conservation;
  traits `priority_allocation` (supply served in priority order), `transit_buffer` (plug-flow
  FIFO delay), `transit_decay` (first-order loss in transit), `scheduled_flow`. Stocks lose at
  entry / gain at release; in-transit mass lives in the link buffer. Folds into the stock
  integration pass as a per-stock delta. (`species_transport` dispersion → M4.)
- **`event` primitive** — base trigger + effects (additive/multiplicative/replace on stock
  levels and node outputs); trait `rate_generation` (Poisson occurrences, effects scaled by
  count). Event pass runs after the link pass; stock effects fold into integration.
- Parser lowers `link` + `event`; `cell` still deferred to M4.
- Tests: links_v2 (5), events_v2 (4) — all green; v1-import path unaffected.
- **Remaining M3:** `event` trait **`failure_state_machine`** (working/failed automaton:
  bases exposure_time/operating_time/demand/capacity_demand/event/condition; repair policies
  none/repair/replace/preventive_maintenance; effect application on transition).

Then M4 mass transport, M5 units + flip `run()` to the v2 core.

## 1. What v2 changes (summary)

v1 is a **fixed-type taxonomy**: `ElementKind` is an internally-tagged enum
(`constant`, `random_variable`, `expression`, `accumulator`, `timeseries`, `lookup`,
`stochastic_process`, `delay`, `script`, `array`). Those are the only `type` values the
corpus uses.

v2 is **6 primitives + 2 definitions + traits by field presence**:

| Primitive | Replaces (v1) | Net-new behavior |
|---|---|---|
| `node` (11 `value_rule`s) | constant, random_variable, expression, stochastic_process, lookup, timeseries, delay | `markov`, `hysteresis`, `filter`, `convolution`, `gate_logic` |
| `stock` | accumulator | traits: capacity_clamp, overflow_routing, compound_growth, priority_withdrawal |
| `link` | — (none) | **entirely new**: rate/fraction transfer, priority allocation, transit buffer, dispersion, transit decay, scheduled flow, species transport |
| `event` | — (none) | **entirely new**: trigger+effects, Poisson rate generation, failure state machine + repair |
| `gate` | — (none) | **entirely new**: recursive boolean tree (and/or/not/n_vote/reference/condition/input) |
| `cell` | — (none) | **entirely new**: mass per species per medium, partitioning equilibrium, decay-chain propagation, source release |
| `species` / `medium` | — | definitions (no behavior; feed cell traits) |

Plus cross-cutting: Iman-Conover correlation (engine does Gaussian copula today),
+7 distribution families, `trigger_spec` evaluation, `reporting_periods`, and SI unit
normalization at load (absent today).

**Magnitude:** the existing 9 behaviors mostly *re-home* into `node`/`stock` with light
change. `link`, `event`, `gate`, `cell`, and 5 of the node value_rules are net-new
stateful machinery. Roughly 60–70% of v2 by surface area is new code.

## 2. Target module architecture

```
engine/src/
  model.rs        REWRITE  → primitive structs + trait fields (the new internal model)
  v1_import.rs    NEW      → v1 ElementKind → v2 primitive normalizer (regression bridge)
  units.rs        NEW      → unit-string → SI factor (consumes openvsim/wasim/units.json)
  graph.rs        REWORK   → derive deps from primitive-specific fields; lag back-edges;
                            overflow-routing ordering; cycle diagnostics
  eval.rs         EXTEND   → +4 AST builtins; generalize resolve_qof to all qof sites
  sampling.rs     EXTEND   → +7 distribution families; beta 4-param
  correlate.rs    NEW      → Iman-Conover rank correlation (replaces copula in engine.rs)
  trigger.rs      NEW      → trigger_spec evaluation (shared by resampling, scheduled_flow,
                            source_release, event triggers)
  state.rs        NEW      → per-realization mutable state for stateful primitives
  primitives/     NEW      → one module per primitive's step logic:
    node.rs, stock.rs, link.rs, event.rs, gate.rs, cell.rs
  engine.rs       REWORK   → orchestration: per-realization state alloc, per-step
                            evaluation dispatch by primitive, results assembly
  params.rs       TOUCH    → parameter overrides keyed by new model shape
  wasm.rs         TOUCH    → editable-field setters keyed by new model shape
```

The single largest structural change is **per-realization mutable state** (§7). v1 is
nearly stateless (accumulators + delay carry forward a scalar). v2 has eight stateful
constructs (process, markov, hysteresis, filter, convolution, lag nodes; stocks; link
transit buffers; event FSMs; cell inventories), each needing allocated, reset-per-
realization state. This forces splitting "model" (immutable, parsed once) from "runtime
state" (mutable, per realization) — a clean refactor the v1 code only half-does today.

## 3. Internal model representation (sketch)

```rust
pub enum Primitive {
    Node(NodeElem),     // discriminated further by NodeRule
    Stock(StockElem),
    Link(LinkElem),
    Event(EventElem),
    Gate(GateElem),
    Cell(CellElem),
    Species(SpeciesDef),
    Medium(MediumDef),
}

pub struct ElementBase { id, name, container, outputs, save_results, inputs, ... }

pub struct NodeElem { base, rule: NodeRule }
pub enum NodeRule {
    Fixed { value/values, unit, editable, bounds },
    Expression(ExpressionField),
    Sample { distribution, resampling: Option<TriggerSpec>, autocorrelation, correlations },
    Process { process: ProcessSpec, lower_bound },
    Lookup { table, interpolation },         // invoked, not self-evaluating
    Series { timestamps, values, time_unit, interpolation },
    Lag { input, initial },
    Convolution { input, response },
    Markov { states, initial_state, transition_matrix, output_values },
    Hysteresis { input, high_threshold, low_threshold, output_above, output_below },
    Filter { input, window, statistic },
    GateLogic { root: GateNode, semantics },
}

pub struct StockElem { base, initial_value, rate|inflows/outflows, floor,
    capacity, overflow_target, return_rate, withdrawals }   // Option fields = traits
// link/event/gate/cell similarly: required fields + Option trait fields
```

Traits are **field presence**, so they map to `Option<…>` on the struct; a `trait flags`
helper derives the active set (and runs the §9 validation that a trait's prerequisite is
present, e.g. `overflow_target` requires `capacity`).

## 4. v1 → v2 normalizer (`v1_import.rs`)

Loader dispatch: if any element carries `primitive`, parse as v2; else parse as v1
(`WasimModel`/`ElementKind`) and normalize. Mapping:

| v1 `type` | v2 primitive | Notes |
|---|---|---|
| `constant` | node/`fixed` | `value`/`values`+`unit`, `editable`, `bounds` carry over 1:1 |
| `random_variable` | node/`sample` | `distribution`, `autocorrelation`, `correlations`; v1 `trigger` provenance → `resampling` if present |
| `expression` | node/`expression` | `expression` + `inputs` 1:1 |
| `stochastic_process` | node/`process` | `process`, `lower_bound` 1:1 |
| `lookup` | node/`lookup` | `x/y/columns`→`table.x/y/z`; `extrapolation`→handled in lookup eval |
| `timeseries` | node/`series` | `times`→`timestamps`, `values`, `times_unit`→`time_unit`, `interpolation` |
| `delay` | node/`lag` (1-step) or chained lags | `input`, `initial`; multi-step v1 `lag` → **N chained lag nodes** (exact N-step delay) per semantics §2.7 (R3 resolved). Convolution is the fallback for fractional/large N |
| `accumulator` | `stock` | `initial_value`, `rate` (ExpressionField → qof), `min_value`→`floor`, `capacity`→`capacity` trait |
| `array` | node/`fixed` (constant mode) or node/`expression` (expression mode) | per `mode` sub-discriminator |
| `script` | node/`expression` per recovered expression, or unsupported-warn | scripts already partially handled; keep current behavior |

The normalizer is also the home for v1 quirks already encoded in the engine: dangling
`inputs` (time-property names), self-referencing expressions, and cycle skipping stay
working because the corpus flows through this path unchanged.

**Coverage check:** the corpus only exercises 9 v1 types, all mapped above. The 29
never-built v1 0.7.0 types (pool, reservoir, event_generator, transport_*, …) are *not*
in the corpus and not in the engine; v2 supersedes them directly — no normalizer rows
needed.

## 5. Expression / AST (`eval.rs`)

Mostly reusable. Concrete deltas:

- **+4 builtins:** `log2`, `sinh`, `cosh`, `max_array` (v1 has `min_array`/`mean_array`
  but not `max_array`; v1 has `tanh` but not `sinh`/`cosh`/`log2`). Mechanical.
- **Generalize `resolve_qof`** (already exists at `eval.rs:273`, used only for
  distribution params) into a first-class `eval_qof(qof, ctx) -> f64` used everywhere v2
  takes `quantity_or_formula`: stock `rate`/`capacity`/`return_rate`, link
  `rate`/`fraction`/`decay_rate`, withdrawal `request`/`limit`, effect `change`, gate
  `condition`, cell `volume`/`inventory`/`release_rate`, flux `rate`/`coefficient`,
  expression-valued Markov transition rows. This is the single most *pervasive* change —
  v2 puts expressions in dozens of slots v1 kept scalar. The `Formula` (raw-string)
  variant continues to degrade to 0.0 + warn (parse is the transpiler's job).
- `time_ref` properties, `lookup_call` (incl. `input2`/2-D), `if`, `array` ops: unchanged.

## 6. Distributions & correlation

- **`sampling.rs` +7 families:** `pert`, `pareto`, `extreme_value` (Gumbel),
  `student_t`, `cumulative` (empirical CDF table → inverse-CDF sample), `sampled`
  (weighted empirical), `external` (stub/warn). Plus `beta` gains optional `min/max`
  (4-param affine scaling). 14 existing families + DiscreteUniform/Bernoulli stay for
  the v1 path. Each family also needs an **inverse-CDF** arm for LHS + Iman-Conover
  (some already have one at `sampling.rs:280+`; gamma/beta/weibull/pearson currently
  return `None` there — see R6).
- **Iman-Conover (`correlate.rs`):** semantics §8 mandates Iman-Conover rank correlation
  with achieved-vs-target diagnostics in output metadata. The engine currently does a
  Gaussian copula (`build_corr_groups` in `engine.rs:66`). Replace with Iman-Conover:
  per group, generate independent marginals, build a target-rank score matrix via
  Cholesky of the (Spearman) correlation, reorder marginals to match ranks, report the
  achieved matrix. Reuses the existing group-discovery/Cholesky code.

## 7. Per-realization state (`state.rs`) — the cross-cutting refactor

Split the model (immutable) from runtime state (mutable, reset each realization). One
state slot per stateful element:

| Construct | State |
|---|---|
| node/`process` | running value `V` |
| node/`markov` | current state index |
| node/`hysteresis` | active/inactive flag |
| node/`filter` | ring buffer (or prev-EMA) |
| node/`convolution` | ring buffer length N |
| node/`lag` | previous-step value (already via `prev_outputs`) |
| `stock` | current level |
| `link` transit_buffer | FIFO/ring of (entry_time, amount) parcels |
| `event` failure FSM | state + TTF/TTR countdowns |
| `cell` | mass[species][medium] inventory |

Initialization order matters (e.g. hysteresis initial state from input's first-step
value; stock from `initial_value`/`initial_expression`). The orchestrator allocates this
vector once and `.reset()`s per realization rather than reallocating.

## 8. Recommended build sequence (milestones)

Even targeting full parity, build in dependency order so each milestone is testable:

- **M1 — Skeleton + bridge.** New `model.rs` primitive types; `v1_import.rs`; loader
  dispatch; `graph.rs` rework; `eval.rs` builtins + `eval_qof`. Re-home node/`fixed`,
  `expression`, `sample`, `process`, `lookup`, `series`, `lag` and `stock` (base + floor
  + capacity). **Exit:** all 162 v1 examples pass through the v2 core unchanged.
- **M2 — Cheap node rules + stock traits + gate.** `markov`, `hysteresis`, `filter`,
  `convolution`, `gate_logic`; stock traits overflow_routing / compound_growth /
  priority_withdrawal; `gate` primitive; `trigger.rs`; resampling triggers; +7
  distributions; Iman-Conover. **Exit:** hand-authored fixtures per rule/trait pass.
- **M3 — Links & events.** `link` (rate/fraction, priority_allocation, transit_buffer,
  transit_decay, scheduled_flow); `event` (trigger+effects, rate_generation Poisson,
  failure_state_machine). **Exit:** reliability + flow fixtures pass.
- **M4 — Mass transport.** `species`/`medium` defs; `cell` (base mass balance,
  partitioning_equilibrium, decay_chain_propagation, source_release); link
  species_transport + `transit_dispersion` (ADE). **Exit:** transport fixtures pass.
- **M5 — Polish.** SI unit normalization (`units.rs`); `reporting_periods`; correlation
  diagnostics in output; multi-output/markov-state results; full validation pass (§9).

M1 carries the most refactor risk (state split, model rewrite) but is the safest to
verify — the corpus is a golden regression. M4 carries the most *algorithmic* risk.

## 9. Validation (load-time, per semantics §10)

- Trait prerequisites: `overflow_target`⇒`capacity`; `decay_rate`/`dispersion`⇒
  `transit_time`.
- `hysteresis`: `low_threshold < high_threshold`.
- `markov`: transition rows sum to 1 (within tol); `output_values` length = states.
- Dependency cycle not broken by a `lag` node ⇒ **version-discriminated** (semantics §9):
  reject for v2-native models; for v1-imported, warn and evaluate back-edges with
  previous-timestep values (implicit lag). Note this *replaces* the current engine's
  warn-and-**skip** behavior — the ~2 cyclic corpus models will re-baseline (R2).
- `n_vote`: `threshold ≤ children count`.

## 10. Testing strategy

1. **Regression:** all 162 v1 examples run through `v1_import` → v2 core; assert outputs
   match the current engine's (snapshot the v1 engine's results before the rewrite as
   golden values).
2. **v2 fixtures:** hand-author a minimal model per new primitive/trait/value_rule (no
   transpiler emits v2 yet). One focused fixture each: a stock per trait, a link per
   trait, an event FSM, a gate tree, a two-phase cell with partitioning, a decay chain,
   a markov/hysteresis/filter/convolution node. These double as executable docs.
3. **Property tests:** distribution moments (mean/var within CI), Iman-Conover achieved
   vs target correlation, mass conservation in cells/links (no decay → inventory
   constant), stock capacity/floor invariants.

## 11. Risks & open technical questions

- **R1 — ADE dispersion (link `transit_dispersion`).** *Parameterization resolved:*
  `dispersion` is a Péclet number (Pe = v·L/D). Semantics §4 now names two candidate
  RTDs — Ogata-Banks analytical kernel vs. tanks-in-series (N = Pe/2). *Still open:*
  approach selection, Pe<1 / Pe→∞ edge cases, and transit_decay interaction under
  dispersed flow — a dedicated pre-M4 appendix. **Recommendation:** the analytical RTD
  is the better fit because it reuses the M2 `convolution` machinery (the kernel is just
  a response function), so M4 adds little new code beyond kernel derivation; tanks-in-
  series needs a separate sub-cell cascade state. Still the highest *algorithmic* risk.
- **R2 — Cycle policy. (RESOLVED — semantics §9.)** Version-discriminated: reject for
  v2-native, warn + implicit-lag (previous-timestep back-edges) for v1-imported.
  *Implementation notes:* (a) need a v2-native vs v1-imported discriminator
  (`wasim_version >= 0.8.0` AND non-legacy `source.generator`); (b) the v1 path's new
  *implicit-lag* behavior differs from the engine's current *skip* — so the cyclic corpus
  models (~2: self-referencing expressions) change outputs and must be re-baselined, not
  snapshot-matched against today's engine.
- **R3 — Multi-step delay. (RESOLVED — semantics §2.7.)** `lag` is strictly one-step.
  Normalizer maps a v1 `delay` of `k·dt` to **k chained lag nodes** (exact k-step delay);
  fractional or very large k falls back to a `convolution` with an offset unit-impulse
  response. Synthesizing the chained nodes (stable ids, graph back-edges) is M1 work.
- **R4 — Cell multi-phase equilibrium.** Two-phase is closed-form; ≥3 phases needs a
  small linear solve (semantics §7). Need a tiny dense linear solver (or pull in
  `nalgebra` — currently no linear-algebra dep). Scope before M4.
- **R5 — Effect reversibility.** Event failure FSM "reverses effects on repair (if
  reversible)" — reversibility isn't defined in the schema. Need a rule (e.g. additive/
  multiplicative reversible, replace not).
- **R6 — Inverse-CDF coverage for LHS/Iman-Conover.** gamma/beta/weibull/pearson return
  `None` from the current quantile fn; Iman-Conover and LHS need quantiles for *all*
  families. Adds numerical-inverse work (e.g. Acklam/Newton) for those marginals.
- **R7 — Units registry.** SI normalization needs `units.json` (at
  `openvsim/wasim/units.json`, 7 KB). Confirm it covers every unit string the corpus +
  v2 fixtures use; decide load-time hard-fail vs warn-and-passthrough for unknown units.
- **R8 — `quantity_or_formula` raw-`Formula` strings.** Still degrade to 0.0 + warn
  (parsing is the transpiler's job). Confirm acceptable for v2 (v2 fixtures should use
  parsed `ast`, never raw strings).

## 12. Sizing (rough)

| Milestone | Relative size | Risk |
|---|---|---|
| M1 skeleton + bridge + re-home | L | refactor risk (state split) |
| M2 node rules + stock traits + gate + distros + Iman-Conover | L | medium |
| M3 links + events | M | medium |
| M4 mass transport (cell/dispersion) | M–L | **high (R1, R4)** |
| M5 units + polish + validation | S–M | low |

The honest cost driver is **M1's model/state refactor** (touches every file) and **M4's
algorithmic unknowns** (R1, R4). M2–M3 are largely additive against the M1 skeleton.
```
