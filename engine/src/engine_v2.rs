//! v2 Monte-Carlo engine.
//!
//! Operates on the v2 primitive model ([`crate::model_v2::Model`]). For M1 it re-homes
//! the behaviors the v1 engine had — node rules `fixed`/`expression`/`sample`/`process`/
//! `lookup`/`series`/`lag` and `stock` (base + floor + capacity) — reusing the shared AST
//! walker, samplers, and result helpers. The net-new primitives (link/event/gate/cell) and
//! node rules (markov/hysteresis/filter/convolution/gate_logic) are M2+ and currently
//! return `Unsupported`.
//!
//! Sampling orchestration (independent draws, Gaussian-copula correlation groups, AR(1)
//! per-step resampling, GBM processes) mirrors the v1 engine so results match on the
//! unchanged-semantics subset. Lag is the one intentional divergence: v2 `lag` is a strict
//! one-step delay (multi-step delays are chained at import), which fixes a v1 off-by-one.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::engine::{
    cholesky, cholesky_matvec, eval_timeseries, mean, percentile, ElementResults, RunConfig,
    SimulationResults, TimeHistoryStats,
};
use crate::error::EngineError;
use crate::eval::{eval_ast, resolve_distribution, EvalCtx, LookupData, Value};
use crate::graph_v2::ModelGraphV2;
use crate::model::{AstNode, OptDirection, QuantityOrFormula};
use crate::model_v2::{
    ConvResponse, EffectMode, Element, FailureBasis, FilterStat, FixedValue, GateNode, MarkovStart,
    Model, NodeRule, PartitionEntry, Primitive, QuantityExpr, RepairPolicy, TransitionRow,
    TriggerMode, TriggerSpec, WithdrawalSpec,
};
use crate::optimize_v2::SearchBounds;
use crate::sampling;

struct CorrGroup {
    ids: Vec<String>,
    chol_l: Vec<Vec<f64>>,
}

/// Precomputed wiring for a submodel's dynamic (per-timestep) optimization (§13a). The
/// objective is re-minimized each outer step against `objective_ast` evaluated with candidate
/// variable values injected, and the winning values are recorded as the variables' series.
struct DynOpt {
    var_ids: Vec<String>,
    bounds: SearchBounds,
    /// The objective element's AST (must be an expression element). None disables the solve.
    objective_ast: Option<AstNode>,
    direction: OptDirection,
}

/// Per-realization failure_state_machine state for an event.
struct Fsm {
    failed: bool,
    ttf: f64, // time-to-failure remaining (exposure/operating bases)
    ttr: f64, // time-to-repair remaining
}

pub fn run(
    model: &Model,
    graph: &ModelGraphV2,
    config: &RunConfig,
) -> Result<SimulationResults, EngineError> {
    let n_real = config.n_realizations.unwrap_or(model.simulation_settings.n_realizations);
    let seed = config.seed.or(model.simulation_settings.seed).unwrap_or(0);

    // Realization weights (B7): normalized to sum 1 for the weighted stat reductions. Only used
    // when the length matches `n_real`; otherwise empty (unweighted, behavior unchanged).
    let realization_weights: Vec<f64> = {
        let w = &config.realization_weights;
        if w.len() == n_real as usize {
            let sw: f64 = w.iter().sum();
            if sw > 0.0 { w.iter().map(|x| x / sw).collect() } else { Vec::new() }
        } else {
            Vec::new()
        }
    };

    // Strict dimensional analysis (B5): reject a model with any dimensional inconsistency before
    // running. `Warn` (default) leaves the pre-B5 behavior unchanged (warnings are logged in lib.rs).
    if config.units == crate::UnitsMode::Strict {
        let errs = crate::units::check_dimensions(model);
        if !errs.is_empty() {
            return Err(EngineError::InvalidModel(format!(
                "strict dimensional analysis found {} inconsistency(ies):\n  - {}",
                errs.len(),
                errs.join("\n  - ")
            )));
        }
    }

    let dt = config.timestep_override.unwrap_or(model.simulation_settings.timestep.value);
    let duration = config.duration_override.unwrap_or(model.simulation_settings.duration.value);
    if !dt.is_finite() || dt <= 0.0 {
        return Err(EngineError::InvalidModel(format!("timestep must be > 0, got {dt}")));
    }
    if !duration.is_finite() || duration < 0.0 {
        return Err(EngineError::InvalidModel(format!("duration must be >= 0, got {duration}")));
    }
    let dt_unit = model.simulation_settings.timestep.unit.clone();
    // duration and timestep may be authored in different time units (e.g. duration in `s`,
    // timestep in `day`). Reconcile duration into the timestep's unit before dividing;
    // fall back to a raw ratio only when the units are non-convertible (unknown/mismatched).
    let duration_in_dt = crate::units::convert(
        duration,
        &model.simulation_settings.duration.unit,
        &dt_unit,
    )
    .unwrap_or(duration);
    // A duration of 0 (or below half a timestep) is a single-evaluation model: evaluate once
    // at t=start and stop. These are GoldSim driver/instant models (optimization/statistics
    // drivers, static calcs) whose real timeline is a nested submodel run. See semantics §9.
    let n_steps = ((duration_in_dt / dt).round() as usize).max(1);

    let elem_idx: HashMap<&str, usize> =
        model.elements.iter().enumerate().map(|(i, e)| (e.id(), i)).collect();

    // Array-comprehension environment (§15): dimension-size table + shared vector_map
    // index stack, threaded into every EvalCtx via ArrayEnv.
    let dim_sizes: HashMap<String, usize> =
        model.dimensions.iter().map(|d| (d.id.clone(), d.size)).collect();
    let index_stack: RefCell<Vec<usize>> = RefCell::new(Vec::new());
    // Ids of events that fired in the current step (§2, `occurs` builtin). Cleared and
    // repopulated each step by the event pass; shared through ArrayEnv via interior mutability.
    let fired_events: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    // SubModel pre-pass (§12): run each referenced submodel once and collect its output
    // samples, so `submodel_stat` nodes reduce real data instead of degrading to 0.0.
    let submodel_outputs = crate::submodel_v2::run_submodels(model, config)?;
    let arr = ArrayEnv {
        dims: &dim_sizes,
        index_stack: &index_stack,
        submodel_outputs: &submodel_outputs,
        fired_events: &fired_events,
        calendar_start: model.simulation_settings.calendar_start,
    };

    let lookups = lookups_map(model);

    // Reserved global identifiers (semantics §1b): seeded into every outputs map so refs to
    // GoldSim run properties resolve instead of degrading to 0.0 as dangling. Seeded before
    // elements are evaluated, so a model element with the same id shadows the global.
    // Time quantities are SI seconds, matching the SI-normalized values emit produces.
    let dt_seconds = crate::units::convert(dt, &dt_unit, "s").unwrap_or(dt);
    let duration_seconds = crate::units::convert(
        duration,
        &model.simulation_settings.duration.unit,
        "s",
    )
    .unwrap_or(duration);
    let run_globals: [(&str, f64); 3] = [
        ("gee", 9.80665), // standard gravity, m/s²
        ("TimestepLength", dt_seconds),
        ("SimDuration", duration_seconds),
    ];

    let save_final: Vec<&str> =
        model.elements.iter().filter(|e| should_save_final(e)).map(|e| e.id()).collect();
    let save_hist: Vec<&str> =
        model.elements.iter().filter(|e| should_save_history(e)).map(|e| e.id()).collect();

    let stock_ids: Vec<&str> = model
        .elements
        .iter()
        .filter(|e| matches!(e.primitive, Primitive::Stock(_)))
        .map(|e| e.id())
        .collect();
    let process_ids: Vec<&str> = model
        .elements
        .iter()
        .filter(|e| matches!(&e.primitive, Primitive::Node(n) if matches!(n.rule, NodeRule::Process { .. })))
        .map(|e| e.id())
        .collect();
    // sample nodes with autocorrelation re-sample every timestep; others once per realization.
    let per_step_sample_ids: Vec<&str> = model
        .elements
        .iter()
        .filter(|e| matches!(&e.primitive,
            Primitive::Node(n) if matches!(&n.rule, NodeRule::Sample { autocorrelation: Some(_), .. })))
        .map(|e| e.id())
        .collect();
    // sample nodes that redraw on a resampling trigger.
    let resample_ids: Vec<&str> = model
        .elements
        .iter()
        .filter(|e| matches!(&e.primitive,
            Primitive::Node(n) if matches!(&n.rule, NodeRule::Sample { resampling: Some(_), .. })))
        .map(|e| e.id())
        .collect();

    let mut final_store: HashMap<String, Vec<f64>> =
        save_final.iter().map(|&id| (id.to_string(), Vec::with_capacity(n_real as usize))).collect();
    let mut hist_store: HashMap<String, Vec<Vec<f64>>> =
        save_hist.iter().map(|&id| (id.to_string(), vec![Vec::new(); n_steps])).collect();
    for d in &model.time_history_displays {
        final_store.insert(d.id.clone(), Vec::with_capacity(n_real as usize));
        hist_store.insert(d.id.clone(), vec![Vec::new(); n_steps]);
    }

    // Species decay info (id → (half_life, [(daughter, branching)])) + a parents-first order.
    let species_info: HashMap<String, (Option<f64>, Vec<(String, f64)>)> = model.elements.iter()
        .filter_map(|e| match &e.primitive {
            Primitive::Species(s) => Some((e.id().to_string(), (
                s.half_life.as_ref().map(|q| q.value),
                s.decay_products.iter()
                    .map(|d| (d.species.clone(), d.branching_fraction.unwrap_or(1.0)))
                    .collect(),
            ))),
            _ => None,
        })
        .collect();
    let decay_order = build_decay_order(&species_info);

    // Cell media as (medium_id, fraction); medium-less cells get one implicit medium "".
    // Fractions are assumed constant (evaluated structurally).
    let cell_media: HashMap<String, Vec<(String, f64)>> = model.elements.iter().filter_map(|e| {
        if let Primitive::Cell(c) = &e.primitive {
            let media = if c.media.is_empty() {
                vec![(String::new(), 1.0)]
            } else {
                c.media.iter()
                    .map(|m| (m.medium.clone(), m.fraction.as_ref().map(qof_const).unwrap_or(1.0)))
                    .collect()
            };
            Some((e.id().to_string(), media))
        } else {
            None
        }
    }).collect();

    // Result ids: per-(cell, species) total, plus per-medium for multi-medium cells.
    let mut cell_species_ids: Vec<String> = Vec::new();
    for elem in &model.elements {
        if let Primitive::Cell(c) = &elem.primitive {
            if should_save_history(elem) || should_save_final(elem) {
                for sp in &c.species {
                    cell_species_ids.push(format!("{}:{}", elem.id(), sp.species));
                    if !c.media.is_empty() {
                        for m in &c.media {
                            cell_species_ids.push(format!("{}:{}@{}", elem.id(), sp.species, m.medium));
                        }
                    }
                }
            }
        }
    }
    for id in &cell_species_ids {
        final_store.entry(id.clone()).or_insert_with(|| Vec::with_capacity(n_real as usize));
        hist_store.entry(id.clone()).or_insert_with(|| vec![Vec::new(); n_steps]);
    }

    let corr_groups = build_corr_groups(model)?;
    let corr_ids: HashSet<String> = corr_groups.iter().flat_map(|g| g.ids.iter().cloned()).collect();
    let use_lhs = model.simulation_settings.sampling_method == crate::model::SamplingMethod::Lhs;
    // Iman-Conover rank correlation: reorder per-realization draws up front (semantics §8).
    // Under LHS the marginals inside each group are stratified before reordering.
    let ic_samples = iman_conover_samples(model, &corr_groups, n_real, seed, use_lhs, &lookups, dt, &dt_unit, &arr)?;

    // Latin Hypercube pre-pass (semantics §8): stratified columns for the independent,
    // once-per-realization sample nodes (not correlated, not autocorrelated, not resampled).
    // Empty under Monte Carlo, so the independent-sampling loop below is unchanged by default.
    let lhs_ids: Vec<&str> = if use_lhs {
        model
            .elements
            .iter()
            .filter_map(|e| match &e.primitive {
                Primitive::Node(n) => matches!(
                    &n.rule,
                    NodeRule::Sample { autocorrelation: None, resampling: None, .. }
                )
                .then(|| e.id()),
                _ => None,
            })
            .filter(|id| !corr_ids.contains(*id))
            .collect()
    } else {
        Vec::new()
    };
    let lhs_cols = lhs_samples(model, &lhs_ids, n_real, seed, &lookups, dt, &dt_unit, &arr)?;

    // Dynamic (per-timestep) optimization (§13a): if THIS model carries an optimization spec
    // (only a submodel-scoped one reaches engine_v2::run — extract_submodel plants it, the
    // top-level study runs outside via optimize_v2), precompute its wiring once. Each outer
    // step re-solves it against the objective at that step, so the variables become series.
    let dyn_opt = model.optimization.as_ref().filter(|_| model.dynamic_optimization).and_then(|spec| {
        (!spec.variables.is_empty()).then(|| DynOpt {
            var_ids: spec.variables.iter().map(|v| v.element_id.clone()).collect(),
            bounds: crate::optimize_v2::bounds_of(spec),
            objective_ast: model.elements.iter().find(|e| e.id() == spec.objective.element_id)
                .and_then(|e| match &e.primitive {
                    Primitive::Node(n) => match &n.rule {
                        NodeRule::Expression(ef) => Some(ef.ast.clone()),
                        _ => None,
                    },
                    _ => None,
                }),
            direction: spec.objective.direction,
        })
    });

    // Timebase (B1): under EventAccurate, collect the exact scheduled instants from every
    // schedule-typed trigger (event/link/resampling) once, converted into the timestep unit so
    // they share the grid's clock. These + stock bound crossings drive sub-step refinement.
    // Under Fixed (default) the provider yields no split points → bit-identical.
    let use_event_accurate = config.timebase == crate::TimebaseMode::EventAccurate;
    let scheduled_times: Vec<f64> = if use_event_accurate {
        let mut ts: Vec<f64> = Vec::new();
        let mut push_sched = |t: &crate::model_v2::TriggerSpec| {
            for q in &t.schedule {
                // Schedule quantities are absolute times; convert into the timestep unit.
                let v = crate::units::convert(q.value, &q.unit, &dt_unit).unwrap_or(q.value);
                if v.is_finite() && v > 0.0 {
                    ts.push(v);
                }
            }
        };
        for elem in &model.elements {
            match &elem.primitive {
                Primitive::Event(ev) => {
                    if let Some(t) = &ev.trigger { push_sched(t); }
                }
                Primitive::Link(l) => {
                    if let Some(t) = &l.schedule { push_sched(t); }
                }
                Primitive::Node(n) => {
                    if let NodeRule::Sample { resampling: Some(t), .. } = &n.rule { push_sched(t); }
                }
                _ => {}
            }
        }
        crate::timebase::dedup_sorted(&mut ts);
        ts
    } else {
        Vec::new()
    };

    for real_idx in 0..n_real {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        rng.set_stream(real_idx as u64);

        // dist_ctx accumulates scalar values visible to distribution-parameter ASTs:
        // fixed-scalar nodes up front, then each sample draw as it's produced.
        let mut rv_samples: HashMap<String, f64> = HashMap::new();
        let mut dist_ctx: HashMap<String, Value> = HashMap::new();
        for elem in &model.elements {
            if let Some(q) = fixed_scalar(elem) {
                dist_ctx.insert(elem.id().to_string(), Value::Scalar(q));
            }
        }
        let empty_prev: HashMap<String, Value> = HashMap::new();

        // Independent sample nodes (correlated ones handled by the copula below). Under LHS,
        // a node with a stratified column takes this realization's pre-drawn value; all others
        // (Monte Carlo, or LHS distributions with no closed-form ICDF) draw iid here as before.
        for elem in &model.elements {
            if let Primitive::Node(n) = &elem.primitive {
                if let NodeRule::Sample { distribution, .. } = &n.rule {
                    if !corr_ids.contains(elem.id()) {
                        let v = if let Some(col) = lhs_cols.get(elem.id()) {
                            col[real_idx as usize]
                        } else {
                            let ctx = dist_ctx_eval(&lookups, &dist_ctx, &empty_prev, dt, &dt_unit, &arr);
                            let resolved = resolve_distribution(distribution, &ctx)?;
                            sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)?
                        };
                        rv_samples.insert(elem.id().to_string(), v);
                        dist_ctx.insert(elem.id().to_string(), Value::Scalar(v));
                    }
                }
            }
        }

        // Correlated groups: look up this realization's Iman-Conover-reordered draw.
        for group in &corr_groups {
            for id in &group.ids {
                if let Some(col) = ic_samples.get(id) {
                    let v = col[real_idx as usize];
                    rv_samples.insert(id.clone(), v);
                    dist_ctx.insert(id.clone(), Value::Scalar(v));
                }
            }
        }

        // Initial draw for process (GBM) nodes. `sp_level` carries the running level for
        // mean-reverting (OU) processes across steps; `sp_state` is the per-step node value.
        let mut sp_state: HashMap<String, f64> = HashMap::new();
        let mut sp_level: HashMap<String, f64> = HashMap::new();
        for &id in &process_ids {
            if let Primitive::Node(n) = &model.elements[elem_idx[id]].primitive {
                if let NodeRule::Process { process, lower_bound } = &n.rule {
                    if sampling::is_reverting(process) {
                        // Seed the level at initial_value (else reference/drift level); the node's
                        // step-0 value is that level, not a GBM draw.
                        let x0 = process.initial_value.as_ref().map(|q| q.value()).unwrap_or_else(|| {
                            process.reference_value.as_ref().map(|q| q.value())
                                .unwrap_or(process.mean.value)
                        });
                        sp_level.insert(id.to_string(), x0);
                        sp_state.insert(id.to_string(), x0);
                    } else {
                        let v = sampling::sample_gbm(process, lower_bound.as_ref(), dt, &dt_unit, &mut rng)?;
                        sp_state.insert(id.to_string(), v);
                    }
                }
            }
        }

        // AR(1) standard-normal driver state for per-step sample nodes.
        let mut z_state: HashMap<String, f64> = HashMap::new();
        for &id in &per_step_sample_ids {
            let z0: f64 = rng.sample(
                rand_distr::Normal::new(0.0_f64, 1.0_f64)
                    .map_err(|e| EngineError::Sampling(e.to_string()))?,
            );
            z_state.insert(id.to_string(), z0);
        }

        // Per-realization reserved globals (§1b): the run-wide constants plus the 1-based
        // realization index. Inserted first at every outputs-map creation site so real
        // elements (evaluated after) shadow them.
        let global_seed: Vec<(String, Value)> = run_globals
            .iter()
            .map(|(id, v)| (id.to_string(), Value::Scalar(*v)))
            .chain(std::iter::once((
                "Realization".to_string(),
                Value::Scalar((real_idx + 1) as f64),
            )))
            .collect();

        // t=0 snapshot for stock initial_expression evaluation.
        let empty_map: HashMap<String, Value> = HashMap::new();
        let mut init_outputs: HashMap<String, Value> = HashMap::new();
        init_outputs.extend(global_seed.iter().cloned());
        for elem in &model.elements {
            let id = elem.id();
            match &elem.primitive {
                Primitive::Node(n) => match &n.rule {
                    NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => {
                        init_outputs.insert(id.to_string(), Value::Scalar(q.value));
                    }
                    NodeRule::Sample { .. } => {
                        init_outputs.insert(id.to_string(), Value::Scalar(rv_samples[id]));
                    }
                    NodeRule::Process { .. } => {
                        init_outputs.insert(id.to_string(), Value::Scalar(sp_state.get(id).copied().unwrap_or(0.0)));
                    }
                    _ => {}
                },
                Primitive::Stock(s) => {
                    init_outputs.insert(id.to_string(), Value::Scalar(s.initial_value.value));
                }
                _ => {}
            }
        }
        for elem_id in &graph.topo_order {
            let elem = &model.elements[elem_idx[elem_id.as_str()]];
            if let Primitive::Node(n) = &elem.primitive {
                if let NodeRule::Expression(ef) = &n.rule {
                    let ctx = ctx_at(&lookups, &init_outputs, &empty_map, 0.0, dt, &dt_unit, 0, &arr);
                    if let Ok(v) = eval_ast(&ef.ast, &ctx) {
                        init_outputs.insert(elem_id.clone(), v);
                    }
                }
            }
        }

        // Initialize stock state (initial_expression if present, else initial_value).
        let mut stock_state: HashMap<String, Value> = HashMap::new();
        for &id in &stock_ids {
            if let Primitive::Stock(s) = &model.elements[elem_idx[id]].primitive {
                let init = match &s.initial_expression {
                    Some(expr) => {
                        let ctx = ctx_at(&lookups, &init_outputs, &empty_map, 0.0, dt, &dt_unit, 0, &arr);
                        eval_ast(&expr.ast, &ctx)?
                    }
                    None => Value::Scalar(s.initial_value.value),
                };
                stock_state.insert(id.to_string(), init);
            }
        }

        // Cumulative flow totals per stock (§1c, `output_kind: cumulative`): the running total of
        // each flow (addition / withdrawal / overflow / net_change) since the run start, in level
        // units. Incremented by the applied *amount* (rate · sub_dt) each sub-interval, so the sum
        // is correct under B1 sub-stepping and consumes no RNG. Keyed by (stock id, flow name).
        let mut stock_cumulative: HashMap<(String, &'static str), f64> = HashMap::new();

        // Per-realization state for stateful node rules.
        let mut hyst_state: HashMap<String, bool> = HashMap::new();
        let mut filter_buf: HashMap<String, VecDeque<f64>> = HashMap::new();
        let mut filter_ema: HashMap<String, f64> = HashMap::new();
        let mut markov_state: HashMap<String, usize> = HashMap::new();
        let mut conv_buf: HashMap<String, VecDeque<f64>> = HashMap::new();
        // Status latch (§2): id → current latched bool. Milestone (§2): id → first-fire elapsed
        // time (absent until it fires). PID (§2): id → (integral accumulator, previous error).
        let mut status_state: HashMap<String, bool> = HashMap::new();
        let mut milestone_time: HashMap<String, f64> = HashMap::new();
        let mut pid_state: HashMap<String, (f64, f64)> = HashMap::new();
        // Queue (§B3): per queue node, a map of release_step → amount scheduled to exit then.
        let mut queue_buf: HashMap<String, HashMap<usize, f64>> = HashMap::new();
        // Resource (§B3): per-realization balance, seeded from each Resource's initial amount.
        // `borrowed` tracks outstanding borrow so a return event can restore it.
        let mut resource_balance: HashMap<String, f64> = HashMap::new();
        let mut resource_borrowed: HashMap<String, f64> = HashMap::new();
        for elem in &model.elements {
            if let Primitive::Resource(r) = &elem.primitive {
                resource_balance.insert(elem.id().to_string(), r.initial.value);
            }
        }
        // Interrupt (§2): once set, the realization holds its last-computed outputs for all
        // remaining steps instead of recomputing.
        let mut interrupted = false;
        // Transit buffers: per link, a map of release_step → scheduled amount.
        let mut link_buf: HashMap<String, HashMap<usize, f64>> = HashMap::new();
        // failure_state_machine state per event.
        let mut fsm_state: HashMap<String, Fsm> = HashMap::new();
        for elem in &model.elements {
            if let Primitive::Event(ev) = &elem.primitive {
                if let Some(fp) = &ev.failure_process {
                    // Time-based bases draw an initial time-to-failure up front.
                    let ttf = match fp.basis {
                        FailureBasis::ExposureTime | FailureBasis::OperatingTime => fp
                            .time_to_failure
                            .as_ref()
                            .map(|d| sampling::sample(&d.kind, &d.truncation, &mut rng))
                            .transpose()?
                            .unwrap_or(f64::INFINITY),
                        _ => f64::INFINITY,
                    };
                    fsm_state.insert(elem.id().to_string(), Fsm { failed: false, ttf, ttr: 0.0 });
                }
            }
        }
        for elem in &model.elements {
            if let Primitive::Node(n) = &elem.primitive {
                if let NodeRule::Markov { states, initial_state, .. } = &n.rule {
                    let idx = match initial_state {
                        MarkovStart::Index(i) => *i,
                        MarkovStart::Label(l) => states.iter().position(|s| s == l).unwrap_or(0),
                    };
                    markov_state.insert(elem.id().to_string(), idx);
                }
            }
        }

        // Cell mass per (cell, species, medium) and finite source-inventory budgets.
        let mut cell_mass: HashMap<(String, String, String), f64> = HashMap::new();
        let mut source_inv: HashMap<String, f64> = HashMap::new();
        {
            let ctx = ctx_at(&lookups, &init_outputs, &empty_map, 0.0, dt, &dt_unit, 0, &arr);
            for elem in &model.elements {
                if let Primitive::Cell(c) = &elem.primitive {
                    let first_medium = cell_media.get(elem.id())
                        .and_then(|m| m.first()).map(|(id, _)| id.clone()).unwrap_or_default();
                    for sp in &c.species {
                        if let Some(q) = &sp.initial_inventory {
                            cell_mass.insert(
                                (elem.id().to_string(), sp.species.clone(), first_medium.clone()),
                                q.value,
                            );
                        }
                    }
                    if let Some(inv) = &c.inventory {
                        source_inv.insert(elem.id().to_string(), eval_qof_value(inv, &ctx)?.as_scalar());
                    }
                }
            }
        }

        let mut prev_outputs: HashMap<String, Value> = HashMap::new();
        prev_outputs.extend(global_seed.iter().cloned());

        for step_idx in 0..n_steps {
            let elapsed = step_idx as f64 * dt;

            // Interrupt (§2): once a realization is interrupted, every remaining step holds the
            // last-computed values — record them and advance without recomputing anything.
            if interrupted {
                for &id in &save_hist {
                    if let Some(v) = prev_outputs.get(id) {
                        hist_store.get_mut(id).unwrap()[step_idx].push(v.as_scalar());
                    }
                }
                if step_idx == n_steps - 1 {
                    for &id in &save_final {
                        if let Some(v) = prev_outputs.get(id) {
                            final_store.get_mut(id).unwrap().push(v.as_scalar());
                        }
                    }
                }
                for id in &cell_species_ids {
                    if let Some(v) = prev_outputs.get(id) {
                        hist_store.get_mut(id).unwrap()[step_idx].push(v.as_scalar());
                        if step_idx == n_steps - 1 {
                            final_store.get_mut(id).unwrap().push(v.as_scalar());
                        }
                    }
                }
                for d in &model.time_history_displays {
                    let ctx = ctx_at(&lookups, &prev_outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                    let v = eval_ast(&d.expression.ast, &ctx)?.as_scalar();
                    hist_store.get_mut(&d.id).unwrap()[step_idx].push(v);
                    if step_idx == n_steps - 1 {
                        final_store.get_mut(&d.id).unwrap().push(v);
                    }
                }
                continue;
            }

            for &id in &process_ids {
                if let Primitive::Node(n) = &model.elements[elem_idx[id]].primitive {
                    if let NodeRule::Process { process, lower_bound } = &n.rule {
                        if sampling::is_reverting(process) {
                            // Mean-reverting (OU): carry the level across steps, seeded at step 0
                            // from initial_value (else the reference/drift level). §16.
                            let prev = sp_level.get(id).copied().unwrap_or_else(|| {
                                process.initial_value.as_ref().map(|q| q.value()).unwrap_or_else(|| {
                                    process.reference_value.as_ref().map(|q| q.value())
                                        .unwrap_or(process.mean.value)
                                })
                            });
                            let v = sampling::sample_ou_step(process, prev, dt, &dt_unit, &mut rng)?;
                            sp_level.insert(id.to_string(), v);
                            sp_state.insert(id.to_string(), v);
                        } else {
                            let v = sampling::sample_gbm(process, lower_bound.as_ref(), dt, &dt_unit, &mut rng)?;
                            sp_state.insert(id.to_string(), v);
                        }
                    }
                }
            }

            for &id in &per_step_sample_ids {
                if let Primitive::Node(n) = &model.elements[elem_idx[id]].primitive {
                    if let NodeRule::Sample { distribution, autocorrelation, .. } = &n.rule {
                        let rho = autocorrelation.unwrap_or(0.0).clamp(0.0, 1.0);
                        let z_prev = z_state.get(id).copied().unwrap_or(0.0);
                        let (v, z_new) = sampling::sample_autocorr_step(
                            &distribution.kind, &distribution.truncation, rho, z_prev, &mut rng,
                        )?;
                        rv_samples.insert(id.to_string(), v);
                        z_state.insert(id.to_string(), z_new);
                    }
                }
            }

            // Redraw sample nodes whose resampling trigger fires. The trigger is evaluated
            // against the previous step's outputs (current step isn't computed yet).
            for &id in &resample_ids {
                if let Primitive::Node(n) = &model.elements[elem_idx[id]].primitive {
                    if let NodeRule::Sample { distribution, resampling: Some(trig), .. } = &n.rule {
                        let ctx = ctx_at(&lookups, &prev_outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                        if trigger_fires(trig, &ctx, dt, step_idx)? {
                            let resolved = resolve_distribution(distribution, &ctx)?;
                            let v = sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)?;
                            rv_samples.insert(id.to_string(), v);
                        }
                    }
                }
            }

            let mut outputs: HashMap<String, Value> = HashMap::new();
            outputs.extend(global_seed.iter().cloned());
            // Per-step queue levels (§B3), published as each queue's `num_in_queue` secondary port.
            let mut queue_level: HashMap<String, f64> = HashMap::new();

            // ── Sub-interval boundaries for this grid step (B1). Under Fixed timebase this is
            // just [elapsed, elapsed+dt] (one sub-interval, sub_dt == dt) → bit-identical. Under
            // EventAccurate, scheduled instants that fall strictly inside the step split it; stock
            // bound crossings add further splits dynamically inside the loop. The grid step's
            // statistics / state / history are unaffected — only integration is refined. ──
            let grid_end = elapsed + dt;
            let sub_splits: Vec<f64> = if use_event_accurate {
                scheduled_times
                    .iter()
                    .copied()
                    .filter(|&t| t > elapsed + crate::timebase::EPS && t < grid_end - crate::timebase::EPS)
                    .collect()
            } else {
                Vec::new()
            };
            // Ordered list of upcoming scheduled boundaries within this step (ascending).
            let mut pending_splits = sub_splits;
            crate::timebase::dedup_sorted(&mut pending_splits);
            let mut split_iter = pending_splits.into_iter().peekable();

            // Event fired-set pre-pass (§2, for `occurs`): determine which events fire this step
            // BEFORE the node topo eval, so a node reading `occurs(ev)` sees the current step's
            // fire (GoldSim causality ordering). Triggers are evaluated against the previous
            // step's outputs; rate/failure events (whose fire needs current-step draws) are
            // resolved authoritatively in the event pass and reflected there for the next step.
            {
                fired_events.borrow_mut().clear();
                let ctx = ctx_at(&lookups, &prev_outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                for elem in &model.elements {
                    if let Primitive::Event(ev) = &elem.primitive {
                        // Only trigger-driven events are predicted here (rate/failure need draws).
                        if ev.rate.is_none() && ev.failure_process.is_none() {
                            if let Some(t) = &ev.trigger {
                                // `trigger_fires` may itself borrow `fired_events` (an `on_event`
                                // trigger reads the set), so do NOT hold a mutable borrow across
                                // the call — evaluate first, then insert under a short-lived borrow.
                                // A prior event's fire is visible to a later `on_event` in the same
                                // pass (declaration order), which is the documented chaining rule.
                                if trigger_fires(t, &ctx, dt, step_idx)? {
                                    fired_events.borrow_mut().insert(elem.id().to_string());
                                }
                            }
                        }
                    }
                }
            }

            // Sub-interval integration loop (B1). `sub_t` is the current sub-interval start;
            // `sub_dt` its length (== dt under Fixed timebase, one pass). `is_last` gates the
            // grid-only node rules (filter/status/milestone/pid/markov/convolution/hysteresis) so
            // they advance state / consume RNG exactly once per grid step, on the final outputs.
            // Interrupt (§2): an interrupt effect firing in any sub-interval ends the realization
            // after this grid step; declared here so it survives the inner loop.
            let mut interrupt_now = false;
            let mut sub_t = elapsed;
            // Bound-crossing sub-splitting (B1 gap #1). When a bounded stock crosses its
            // floor/capacity strictly inside a sub-interval under `EventAccurate`, we shorten this
            // sub-interval to land exactly on the bound and re-run the body from `sub_t`, so the
            // *next* sub-interval re-evaluates coupled downstream elements at the crossing instant.
            // `forced_sub_end` carries the shortened end across the retry `continue`; the snapshot
            // (below) restores the mutable state the aborted try touched. `splits_this_step` caps
            // the number of crossing subdivisions per grid step (pathological-rate backstop).
            let mut forced_sub_end: Option<f64> = None;
            let mut splits_this_step: usize = 0;
            const MAX_SPLITS_PER_STEP: usize = 64;
            loop {
                // Next scheduled boundary (if any) inside this step, else the grid end. A pending
                // forced crossing end (from an aborted try at this same `sub_t`) takes precedence.
                let next_sched = split_iter.peek().copied().unwrap_or(grid_end);
                let sub_end = match forced_sub_end {
                    Some(fc) => fc,
                    None => next_sched.min(grid_end),
                };
                let sub_dt = (sub_end - sub_t).max(0.0);
                let is_last = sub_end >= grid_end - crate::timebase::EPS;

                // Snapshot the state a crossing re-run must roll back: stock levels, published
                // outputs, cell masses, and drained source inventories. Everything else the body
                // mutates (`link_delta`, `stock_event`, `next_vals`, …) is rebuilt each sub-interval,
                // and grid-only rules / the event pass run only when `is_last` (never on a shortened,
                // crossing-truncated try), so no RNG or grid-only state is captured or replayed.
                // Under Fixed timebase (no crossings possible) these clones are never restored.
                let snap_stock_state = if use_event_accurate { Some(stock_state.clone()) } else { None };
                let snap_outputs = if use_event_accurate { Some(outputs.clone()) } else { None };
                let snap_cell_mass = if use_event_accurate { Some(cell_mass.clone()) } else { None };
                let snap_source_inv = if use_event_accurate { Some(source_inv.clone()) } else { None };
                // Cumulative flow totals are incremented in the publish loop each sub-interval; a
                // crossing re-run must roll them back too, or the aborted try's amounts double-count.
                let snap_stock_cumulative = if use_event_accurate { Some(stock_cumulative.clone()) } else { None };

            for elem_id in &graph.topo_order {
                let elem = &model.elements[elem_idx[elem_id.as_str()]];
                // Grid-only node rules (hysteresis/filter/status/milestone/pid/markov/convolution)
                // advance per-step state and may consume randomness (markov) — evaluate them ONCE
                // per grid step, on the final sub-interval. On non-final sub-intervals they hold
                // their carried value (from this step's outputs if already set, else the previous
                // grid step) without mutating state or drawing RNG (B1 invariant).
                if !is_last && is_grid_only_rule(elem) {
                    let held = outputs
                        .get(elem_id.as_str())
                        .or_else(|| prev_outputs.get(elem_id.as_str()))
                        .cloned()
                        .unwrap_or(Value::Scalar(0.0));
                    outputs.insert(elem_id.clone(), held);
                    continue;
                }
                let value = match &elem.primitive {
                    Primitive::Node(node) => match &node.rule {
                        NodeRule::Hysteresis {
                            input, high_threshold, low_threshold, output_above, output_below,
                        } => {
                            let x = outputs.get(input.as_str()).map(|v| v.as_scalar()).unwrap_or(0.0);
                            let active = match hyst_state.get(elem_id.as_str()) {
                                Some(true) => !(x <= low_threshold.value), // stays active unless x ≤ low
                                Some(false) => x >= high_threshold.value,  // activates when x ≥ high
                                None => x >= high_threshold.value,         // step-0 init
                            };
                            hyst_state.insert(elem_id.clone(), active);
                            Value::Scalar(if active { output_above.value } else { output_below.value })
                        }
                        NodeRule::Filter { input, window, statistic } => {
                            let x = outputs.get(input.as_str()).map(|v| v.as_scalar()).unwrap_or(0.0);
                            let val = match statistic {
                                FilterStat::Ema => {
                                    let alpha = 2.0 / (*window as f64 + 1.0);
                                    let ema = match filter_ema.get(elem_id.as_str()) {
                                        Some(&p) => alpha * x + (1.0 - alpha) * p,
                                        None => x,
                                    };
                                    filter_ema.insert(elem_id.clone(), ema);
                                    ema
                                }
                                _ => {
                                    let buf = filter_buf.entry(elem_id.clone()).or_default();
                                    buf.push_back(x);
                                    while buf.len() > *window {
                                        buf.pop_front();
                                    }
                                    match statistic {
                                        FilterStat::Mean => buf.iter().sum::<f64>() / buf.len() as f64,
                                        FilterStat::Min => buf.iter().cloned().fold(f64::INFINITY, f64::min),
                                        FilterStat::Max => buf.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                                        FilterStat::Sum => buf.iter().sum(),
                                        FilterStat::Ema => unreachable!(),
                                    }
                                }
                            };
                            Value::Scalar(val)
                        }
                        NodeRule::Status { set, reset } => {
                            // Latch: set fires → 1, reset fires → 0; set wins a simultaneous
                            // fire. Triggers evaluated against current-step outputs so far.
                            let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                            let cur = *status_state.get(elem_id.as_str()).unwrap_or(&false);
                            let latched = if trigger_fires(set, &ctx, dt, step_idx)? {
                                true
                            } else if trigger_fires(reset, &ctx, dt, step_idx)? {
                                false
                            } else {
                                cur
                            };
                            status_state.insert(elem_id.clone(), latched);
                            Value::Scalar(if latched { 1.0 } else { 0.0 })
                        }
                        NodeRule::Milestone { trigger } => {
                            // Record the elapsed time of the first fire; output that time. NaN
                            // until it fires (the documented sentinel — an unachieved milestone).
                            let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                            if !milestone_time.contains_key(elem_id.as_str())
                                && trigger_fires(trigger, &ctx, dt, step_idx)?
                            {
                                milestone_time.insert(elem_id.clone(), elapsed);
                            }
                            Value::Scalar(milestone_time.get(elem_id.as_str()).copied().unwrap_or(f64::NAN))
                        }
                        NodeRule::PidController {
                            input, setpoint, kp, ki, kd, output_min, output_max, deadband,
                        } => {
                            let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                            let measured = outputs.get(input.as_str()).map(|v| v.as_scalar()).unwrap_or(0.0);
                            let sp = eval_qof_value(setpoint, &ctx)?.as_scalar();
                            let mut error = sp - measured;
                            // Deadband: treat |error| ≤ deadband as zero to avoid chattering.
                            if error.abs() <= *deadband {
                                error = 0.0;
                            }
                            let (mut integral, prev_error) =
                                pid_state.get(elem_id.as_str()).copied().unwrap_or((0.0, 0.0));
                            integral += error * dt;
                            let derivative = if dt > 0.0 { (error - prev_error) / dt } else { 0.0 };
                            let mut out = kp * error + ki * integral + kd * derivative;
                            if let Some(lo) = output_min {
                                out = out.max(*lo);
                            }
                            if let Some(hi) = output_max {
                                out = out.min(*hi);
                            }
                            pid_state.insert(elem_id.clone(), (integral, error));
                            Value::Scalar(out)
                        }
                        NodeRule::Markov { transition_matrix, output_values, .. } => {
                            let cur = *markov_state.get(elem_id.as_str()).unwrap_or(&0);
                            let out = output_values.get(cur).copied().unwrap_or(0.0);
                            if let Some(row) = transition_matrix.get(cur) {
                                let probs: Vec<f64> = match row {
                                    TransitionRow::Fixed(p) => p.clone(),
                                    TransitionRow::Expr(es) => {
                                        let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                                        es.iter()
                                            .map(|q| eval_qof_value(q, &ctx).map(|v| v.as_scalar()).unwrap_or(0.0))
                                            .collect()
                                    }
                                };
                                let u: f64 = rng.gen();
                                let mut acc = 0.0;
                                let mut next = cur;
                                for (i, &p) in probs.iter().enumerate() {
                                    acc += p;
                                    if u <= acc {
                                        next = i;
                                        break;
                                    }
                                }
                                markov_state.insert(elem_id.clone(), next);
                            }
                            Value::Scalar(out)
                        }
                        NodeRule::Convolution { input, response } => {
                            let x = outputs.get(input.as_str()).map(|v| v.as_scalar()).unwrap_or(0.0);
                            let base_ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                            let weights = conv_weights(response, &lookups, &base_ctx);
                            let n = weights.len().max(1);
                            let buf = conv_buf.entry(elem_id.clone()).or_default();
                            buf.push_front(x);
                            while buf.len() > n {
                                buf.pop_back();
                            }
                            let val: f64 = buf.iter().zip(weights.iter()).map(|(b, w)| b * w).sum();
                            Value::Scalar(val)
                        }
                        NodeRule::Queue { input, delay_time, capacity, .. } => {
                            // Event/discrete-change delay (§B3). Arrivals wait `delay_time` then
                            // exit; `capacity` caps the number waiting (excess arrivals blocked).
                            let arrivals = outputs.get(input.as_str()).map(|v| v.as_scalar()).unwrap_or(0.0).max(0.0);
                            let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                            let delay = eval_qof_value(delay_time, &ctx)?.as_scalar().max(0.0);
                            let delay_steps = (delay / dt).round().max(0.0) as usize;
                            let buf = queue_buf.entry(elem_id.clone()).or_default();
                            // Release what is scheduled to exit this step.
                            let released = buf.remove(&step_idx).unwrap_or(0.0);
                            // Current queue level (amount still waiting) after release.
                            let in_queue: f64 = buf.values().sum();
                            // Capacity: block arrivals that would exceed the cap (dropped this step).
                            let admitted = match capacity {
                                Some(cap) => {
                                    let cap_val = eval_qof_value(cap, &ctx)?.as_scalar().max(0.0);
                                    arrivals.min((cap_val - in_queue).max(0.0))
                                }
                                None => arrivals,
                            };
                            if admitted > 0.0 {
                                *buf.entry(step_idx + delay_steps.max(1)).or_default() += admitted;
                            }
                            // Publish the queue level (post-admit) as the secondary `num_in_queue` port.
                            queue_level.insert(elem_id.clone(), in_queue + admitted);
                            Value::Scalar(released)
                        }
                        NodeRule::GateLogic { root, .. } => {
                            let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                            Value::Scalar(if eval_gate(root, &ctx)? { 1.0 } else { 0.0 })
                        }
                        _ => eval_element(
                            elem, &lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx,
                            &rv_samples, &sp_state, &stock_state, &arr,
                        )?,
                    },
                    Primitive::Gate(g) => {
                        let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                        Value::Scalar(if eval_gate(&g.root, &ctx)? { 1.0 } else { 0.0 })
                    }
                    // A Resource's output is its current balance (§B3); updated in the event pass,
                    // so during topo it reads the prior step's balance (like a stock level).
                    Primitive::Resource(_) => {
                        Value::Scalar(resource_balance.get(elem_id.as_str()).copied().unwrap_or(0.0))
                    }
                    // Links/events/cells are resolved in their own passes; definitions are inert.
                    Primitive::Link(_) | Primitive::Event(_) | Primitive::Cell(_)
                    | Primitive::Species(_) | Primitive::Medium(_) => {
                        prev_outputs.get(elem_id.as_str()).cloned().unwrap_or(Value::Scalar(0.0))
                    }
                    _ => eval_element(
                        elem, &lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx,
                        &rv_samples, &sp_state, &stock_state, &arr,
                    )?,
                };
                outputs.insert(elem_id.clone(), value);
            }

            // Publish queue `num_in_queue` secondary ports (§B3): a queue node's secondary output
            // declaring role `num_in_queue` reports the current queue level under "<id>#<k+1>".
            for (qid, level) in &queue_level {
                let base = &model.elements[elem_idx[qid.as_str()]].base;
                for (k, spec) in base.outputs.iter().enumerate().skip(1) {
                    if spec.role.as_deref() == Some("num_in_queue") {
                        outputs.insert(format!("{qid}#{}", k + 1), Value::Scalar(*level));
                    }
                }
            }

            // ── Link transfers: move quantity source→target, with priority allocation,
            // transit buffering (plug flow), first-order decay, and scheduling. Stocks lose at
            // entry and gain at release; in-transit mass is conserved in the link buffer. ──
            let mut link_delta: HashMap<String, f64> = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut link_reqs: Vec<(String, Option<String>, Option<String>, i64, f64, Option<f64>, Option<f64>, f64)> =
                Vec::new();
            for elem in &model.elements {
                if let Primitive::Link(l) = &elem.primitive {
                    // species_transport links operate on cell mass; handled in the cell pass.
                    if l.species.is_some() {
                        continue;
                    }
                    // Link transfer is integration → sub-interval clock. Its `schedule` trigger
                    // stays grid-quantized in phase 1 (condition/schedule trigger firing is
                    // fenced to the grid; see the timebase semantics §).
                    let ctx = ctx_at(&lookups, &outputs, &prev_outputs, sub_t, sub_dt, &dt_unit, step_idx, &arr);
                    let fires = match &l.schedule {
                        Some(t) => trigger_fires(t, &ctx, dt, step_idx)?,
                        None => true,
                    };
                    let requested = if !fires {
                        0.0
                    } else if let Some(rate) = &l.rate {
                        (eval_qof_value(rate, &ctx)?.as_scalar() * sub_dt).max(0.0)
                    } else if let Some(frac) = &l.fraction {
                        let src_val = l.source.as_ref()
                            .and_then(|s| outputs.get(s)).map(|v| v.as_scalar()).unwrap_or(0.0);
                        // Fraction transfers move a fraction of the source per firing; under
                        // sub-stepping, scale by sub_dt/dt so the per-grid-step fraction is
                        // preserved (N sub-intervals each move frac·(sub_dt/dt) of source).
                        let frac_scale = if dt > 0.0 { sub_dt / dt } else { 1.0 };
                        (eval_qof_value(frac, &ctx)?.as_scalar() * src_val * frac_scale).max(0.0)
                    } else {
                        0.0
                    };
                    let transit_time = l.transit_time.as_ref().map(|q| q.value);
                    let pe = l.dispersion.as_ref().map(|q| q.value);
                    let decay_lambda = match &l.decay_rate {
                        Some(dr) => eval_qof_value(dr, &ctx)?.as_scalar(),
                        None => 0.0,
                    };
                    link_reqs.push((elem.id().to_string(), l.source.clone(), l.target.clone(),
                        l.priority.unwrap_or(i64::MAX), requested, transit_time, pe, decay_lambda));
                }
            }
            // Source availability (stocks only; non-stock sources are treated as unlimited).
            let mut avail: HashMap<String, f64> = HashMap::new();
            for elem in &model.elements {
                if let Primitive::Stock(s) = &elem.primitive {
                    let lo = s.floor.as_ref().map(|q| q.value).unwrap_or(0.0);
                    avail.insert(elem.id().to_string(), (stock_state[elem.id()].as_scalar() - lo).max(0.0));
                }
            }
            // Serve links in (source, priority) order; trait priority_allocation.
            link_reqs.sort_by(|a, b| a.1.cmp(&b.1).then(a.3.cmp(&b.3)));
            let mut link_out: Vec<(String, f64)> = Vec::new();
            for (id, source, target, _prio, requested, transit_time, pe, decay_lambda) in &link_reqs {
                let alloc = match source {
                    Some(src) => match avail.get_mut(src) {
                        Some(a) => {
                            let give = requested.min(*a);
                            *a -= give;
                            *link_delta.entry(src.clone()).or_default() -= give;
                            give
                        }
                        None => *requested,
                    },
                    None => *requested,
                };
                let lambda = *decay_lambda;
                let delivered = if let Some(tt) = *transit_time {
                    let buf = link_buf.entry(id.clone()).or_default();
                    // Trait transit_dispersion: spread the parcel across an Ogata-Banks RTD
                    // kernel (decay applied per residence time). Else plug flow (single slug).
                    if matches!(*pe, Some(p) if p.is_finite() && p > 0.0 && p < 1e6) {
                        let p = pe.unwrap();
                        for (k, &w) in dispersion_kernel(tt, p, dt).iter().enumerate() {
                            let off = k + 1;
                            *buf.entry(step_idx + off).or_default() +=
                                alloc * w * (-lambda * off as f64 * dt).exp();
                        }
                    } else {
                        let steps = (tt / dt).round().max(0.0) as usize;
                        *buf.entry(step_idx + steps).or_default() += alloc * (-lambda * tt).exp();
                    }
                    buf.remove(&step_idx).unwrap_or(0.0)
                } else {
                    alloc
                };
                if let Some(tgt) = target {
                    *link_delta.entry(tgt.clone()).or_default() += delivered;
                }
                link_out.push((id.clone(), delivered));
            }
            for (id, v) in link_out {
                outputs.insert(id, Value::Scalar(v));
            }

            // ── Event pass: fire on trigger (or Poisson rate_generation) and apply effects.
            // Effects on stocks fold into integration; effects on nodes overwrite their output. ──
            let mut stock_event: HashMap<String, (f64, f64, Option<f64>)> = HashMap::new(); // (add, mul, set)
            let mut node_effects: Vec<(String, Value)> = Vec::new();
            let mut event_out: Vec<(String, f64)> = Vec::new();
            // `fired_events` was populated by the pre-pass before node eval (§2); the event pass
            // below additionally records rate/failure fires so `occurs` sees them next step.
            // Interrupt effect (§2): if an event with an interrupt effect fires this step, end
            // the realization after this step completes (remaining steps hold last values).
            //
            // The event pass is GRID-ONLY (B1): event firing consumes randomness (Poisson,
            // failure draws) and advances FSM clocks, all of which must happen once per grid step
            // per the timebase invariant. It runs on the final sub-interval, so its effects fold
            // into that sub-interval's stock/node integration. (Scheduled-event-instant effect
            // timing within a step is a documented phase-1 limitation — see the timebase §.)
            if is_last {
            for elem in &model.elements {
                let Primitive::Event(ev) = &elem.primitive else { continue };
                let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);

                // Output value + effect action. action = Some((reverse, count)).
                let (out_val, action): (f64, Option<(bool, f64)>) = if let Some(fp) = &ev.failure_process {
                    // Trait failure_state_machine: working/failed automaton.
                    let st = fsm_state.get_mut(elem.id()).expect("fsm state initialized");
                    let mut to_failed = false;
                    let mut to_working = false;
                    if !st.failed {
                        let fail_now = match fp.basis {
                            FailureBasis::ExposureTime | FailureBasis::OperatingTime => {
                                st.ttf -= dt;
                                st.ttf <= 0.0
                            }
                            FailureBasis::Condition => match &ev.trigger {
                                Some(t) => trigger_fires(t, &ctx, dt, step_idx)?,
                                None => false,
                            },
                            FailureBasis::Demand => match &ev.trigger {
                                Some(t) if trigger_fires(t, &ctx, dt, step_idx)? => {
                                    let p = fp.time_to_failure.as_ref()
                                        .map(|d| sampling::sample(&d.kind, &d.truncation, &mut rng))
                                        .transpose()?.unwrap_or(0.0);
                                    let u: f64 = rng.gen();
                                    u < p
                                }
                                _ => false,
                            },
                            // Event basis: fail deterministically the step the FSM's triggering
                            // event fires (its `on_event`/condition trigger evaluates true). Uses
                            // the same `fired_events` path as the `on_event` trigger mode.
                            FailureBasis::Event => match &ev.trigger {
                                Some(t) => trigger_fires(t, &ctx, dt, step_idx)?,
                                None => false,
                            },
                            // capacity_demand basis needs demand/capacity fields (schema) — not
                            // yet modeled → never fail. (Deferred S1 follow-up.)
                            FailureBasis::CapacityDemand => false,
                        };
                        if fail_now {
                            st.failed = true;
                            to_failed = true;
                            st.ttr = match fp.repair.as_ref().map(|r| r.policy) {
                                Some(RepairPolicy::Repair) | Some(RepairPolicy::Replace) => fp
                                    .repair.as_ref().and_then(|r| r.time_to_repair.as_ref())
                                    .map(|d| sampling::sample(&d.kind, &d.truncation, &mut rng))
                                    .transpose()?.unwrap_or(f64::INFINITY),
                                _ => f64::INFINITY, // none / preventive_maintenance
                            };
                        }
                    } else {
                        let repair_now = match fp.repair.as_ref().map(|r| r.policy) {
                            Some(RepairPolicy::Repair) | Some(RepairPolicy::Replace) => {
                                st.ttr -= dt;
                                st.ttr <= 0.0
                            }
                            Some(RepairPolicy::PreventiveMaintenance) => match &ev.trigger {
                                Some(t) => trigger_fires(t, &ctx, dt, step_idx)?,
                                None => false,
                            },
                            _ => false, // none → stays failed
                        };
                        if repair_now {
                            st.failed = false;
                            to_working = true;
                            // Returning to working draws a fresh time-to-failure for time-based
                            // bases (replace is as-good-as-new; repair likewise restarts the clock).
                            if matches!(fp.basis, FailureBasis::ExposureTime | FailureBasis::OperatingTime) {
                                st.ttf = fp.time_to_failure.as_ref()
                                    .map(|d| sampling::sample(&d.kind, &d.truncation, &mut rng))
                                    .transpose()?.unwrap_or(f64::INFINITY);
                            }
                        }
                    }
                    let action = if to_failed {
                        Some((false, 1.0))
                    } else if to_working {
                        Some((true, 1.0)) // reverse the effects applied on failure
                    } else {
                        None
                    };
                    (if st.failed { 1.0 } else { 0.0 }, action)
                } else {
                    // Base event: Poisson rate_generation, else a single trigger firing.
                    let count: f64 = if let Some(rate) = &ev.rate {
                        let lambda = (eval_qof_value(rate, &ctx)?.as_scalar() * dt).max(0.0);
                        poisson_count(lambda, &mut rng) as f64
                    } else {
                        match &ev.trigger {
                            Some(t) => if trigger_fires(t, &ctx, dt, step_idx)? { 1.0 } else { 0.0 },
                            None => 0.0,
                        }
                    };
                    (count, if count > 0.0 { Some((false, count)) } else { None })
                };

                event_out.push((elem.id().to_string(), out_val));

                if let Some((reverse, count)) = action {
                    // Record this event as fired this step (for the `occurs` builtin). A reversed
                    // (failure-repair) transition is not a fire.
                    if !reverse {
                        fired_events.borrow_mut().insert(elem.id().to_string());
                    }
                    for effect in &ev.effects {
                        // Interrupt effect (§2): schedule end-of-realization; no target/change.
                        if matches!(effect.mode, EffectMode::Interrupt) {
                            if !reverse {
                                interrupt_now = true;
                            }
                            continue;
                        }
                        let change = match &effect.change {
                            Some(qe) => eval_qexpr(qe, &ctx)?,
                            None => 0.0,
                        };
                        let target = effect.target.clone();
                        let is_stock = elem_idx.get(target.as_str())
                            .map(|&i| matches!(model.elements[i].primitive, Primitive::Stock(_)))
                            .unwrap_or(false);
                        match effect.mode {
                            EffectMode::Additive => {
                                let delta = if reverse { -change } else { change * count };
                                if is_stock {
                                    stock_event.entry(target).or_insert((0.0, 1.0, None)).0 += delta;
                                } else {
                                    let cur = outputs.get(&target).map(|v| v.as_scalar()).unwrap_or(0.0);
                                    node_effects.push((target, Value::Scalar(cur + delta)));
                                }
                            }
                            EffectMode::Multiplicative => {
                                let factor = if reverse {
                                    if change != 0.0 { 1.0 / change } else { 1.0 }
                                } else {
                                    change.powf(count)
                                };
                                if is_stock {
                                    stock_event.entry(target).or_insert((0.0, 1.0, None)).1 *= factor;
                                } else {
                                    let cur = outputs.get(&target).map(|v| v.as_scalar()).unwrap_or(0.0);
                                    node_effects.push((target, Value::Scalar(cur * factor)));
                                }
                            }
                            EffectMode::Replace => {
                                // Replace is not reversible; apply only on the forward transition.
                                if !reverse {
                                    if is_stock {
                                        stock_event.entry(target).or_insert((0.0, 1.0, None)).2 = Some(change);
                                    } else {
                                        node_effects.push((target, Value::Scalar(change)));
                                    }
                                }
                            }
                            // Handled above (scheduled the interrupt, no target write).
                            EffectMode::Interrupt => {}
                            // ── Resource effects (§B3). Adjust the target Resource's balance. ──
                            EffectMode::Spend => {
                                let bal = resource_balance.entry(target.clone()).or_insert(0.0);
                                if reverse {
                                    *bal += change; // reverse a spend = give it back
                                } else {
                                    let want = change * count;
                                    *bal = (*bal - want).max(0.0); // limited to available (partial when short)
                                }
                            }
                            EffectMode::Deposit => {
                                let cap = elem_idx.get(target.as_str()).and_then(|&i| {
                                    if let Primitive::Resource(r) = &model.elements[i].primitive {
                                        r.capacity.as_ref().map(|c| eval_qof_value(c, &ctx).map(|v| v.as_scalar()))
                                    } else { None }
                                });
                                let cap = match cap { Some(Ok(c)) => Some(c), _ => None };
                                let bal = resource_balance.entry(target.clone()).or_insert(0.0);
                                let delta = if reverse { -change } else { change * count };
                                *bal += delta;
                                if let Some(c) = cap { *bal = bal.min(c); }
                                *bal = bal.max(0.0);
                            }
                            EffectMode::Borrow => {
                                let bal = resource_balance.entry(target.clone()).or_insert(0.0);
                                if reverse {
                                    // Return what was borrowed.
                                    let owed = resource_borrowed.get(&target).copied().unwrap_or(0.0).min(change);
                                    *bal += owed;
                                    if let Some(b) = resource_borrowed.get_mut(&target) { *b -= owed; }
                                } else {
                                    let want = change * count;
                                    let got = want.min(*bal);
                                    *bal -= got;
                                    *resource_borrowed.entry(target.clone()).or_insert(0.0) += got;
                                }
                            }
                        }
                    }
                }
            }
            for (id, c) in event_out {
                outputs.insert(id, Value::Scalar(c));
            }
            for (target, v) in node_effects {
                outputs.insert(target, v);
            }
            // Publish updated Resource balances (§B3) so end-of-step recording sees them.
            for elem in &model.elements {
                if let Primitive::Resource(_) = &elem.primitive {
                    let bal = resource_balance.get(elem.id()).copied().unwrap_or(0.0);
                    outputs.insert(elem.id().to_string(), Value::Scalar(bal));
                }
            }
            } // if is_last (grid-only event pass)

            // Stock integration pass. Computes each stock's next level (with traits) but
            // defers writing into `outputs` so a single shared ctx can borrow it.
            let mut next_vals: HashMap<String, Value> = HashMap::new();
            let mut cap_vals: HashMap<String, f64> = HashMap::new();
            let mut overflow_in: HashMap<String, f64> = HashMap::new();
            let mut withdrawal_allocs: Vec<(String, f64)> = Vec::new();
            // Applied per-stock rates this step, for secondary output ports (§1c):
            // (addition_rate, withdrawal_rate, overflow_rate) + the pre-integration level
            // (net_change is derived after the level settles).
            let mut stock_rates: HashMap<String, (f64, f64, f64)> = HashMap::new();
            let mut stock_prev_level: HashMap<String, f64> = HashMap::new();
            // Bound-crossing inputs (B1 gap #1): for each bounded stock, the level at sub-interval
            // start and the net Euler rate over this sub-interval (from the *unclamped* trajectory),
            // so `BoundCrossing` can solve the closed-form crossing time after the pass. Populated
            // only under EventAccurate; empty (and unused) under Fixed.
            let mut stock_bound_views: Vec<crate::timebase::StockBoundView> = Vec::new();
            for &id in &stock_ids {
                let Primitive::Stock(s) = &model.elements[elem_idx[id]].primitive else { continue };
                // Integration uses the SUB-interval clock (sub_t/sub_dt); == elapsed/dt on a
                // fixed grid, so this pass is bit-identical there.
                let ctx = ctx_at(&lookups, &outputs, &prev_outputs, sub_t, sub_dt, &dt_unit, step_idx, &arr);
                let current = stock_state[id].clone();
                let level_start = current.as_scalar();
                stock_prev_level.insert(id.to_string(), level_start);

                // Trait priority_withdrawal: allocate available stock by priority. `request`/
                // `limit` are rates (amount = rate·sub_dt); each target outputs its allocation.
                let mut withdrawal_outflow = 0.0;
                if !s.withdrawals.is_empty() {
                    let floor = s.floor.as_ref().map(|q| q.value).unwrap_or(0.0);
                    let mut available = (current.as_scalar() - floor).max(0.0);
                    let mut ws: Vec<&WithdrawalSpec> = s.withdrawals.iter().collect();
                    ws.sort_by_key(|w| w.priority.unwrap_or(i64::MAX));
                    for w in ws {
                        let mut amount = match &w.request {
                            Some(q) => (eval_qof_value(q, &ctx)?.as_scalar() * sub_dt).max(0.0),
                            None => 0.0,
                        };
                        if let Some(lim) = &w.limit {
                            amount = amount.min((eval_qof_value(lim, &ctx)?.as_scalar() * sub_dt).max(0.0));
                        }
                        let alloc = amount.min(available);
                        available -= alloc;
                        withdrawal_outflow += alloc;
                        withdrawal_allocs.push((w.target.clone(), alloc));
                    }
                }

                // External (non-return) flow: explicit rate, else Σinflows − Σoutflows.
                let (external, add_rate, wd_rate) = match &s.rate {
                    Some(qof) => {
                        let r = eval_qof_value(qof, &ctx)?;
                        let rs = r.as_scalar();
                        (r, rs.max(0.0), (-rs).max(0.0))
                    }
                    None => {
                        let infl: f64 = s.inflows.iter()
                            .map(|i| outputs.get(i).map(|v| v.as_scalar()).unwrap_or(0.0)).sum();
                        let outf: f64 = s.outflows.iter()
                            .map(|o| outputs.get(o).map(|v| v.as_scalar()).unwrap_or(0.0)).sum();
                        (Value::Scalar(infl - outf), infl, outf)
                    }
                };
                // Trait compound_growth: multiplicative self-referential return term.
                let mut next = if let Some(rr_qof) = &s.return_rate {
                    let rr = eval_qof_value(rr_qof, &ctx)?.as_scalar();
                    current.zip_with(external, move |c, e| {
                        let e = if e.is_nan() { 0.0 } else { e };
                        c * (1.0 + rr * sub_dt) + e * sub_dt
                    })
                } else {
                    current.zip_with(external, |c, r| if r.is_nan() { c } else { c + r * sub_dt })
                };
                if withdrawal_outflow != 0.0 {
                    next = next.map(move |v| v - withdrawal_outflow);
                }
                // Link transfers (debit if source, credit if target) — already integrated.
                if let Some(ld) = link_delta.get(id).copied() {
                    next = next.map(move |v| v + ld);
                }
                // Discrete event effects on the stock level (add, then scale, then replace).
                if let Some(&(add, mul, set)) = stock_event.get(id) {
                    next = next.map(move |v| (v + add) * mul);
                    if let Some(s) = set {
                        next = next.map(move |_| s);
                    }
                }
                // Evaluate the capacity bound (if any) before clamping, so the bound-crossing view
                // below and the clamp below share one value. `cap_vals` is consumed by the overflow
                // re-clamp pass.
                let cap_val_opt: Option<f64> = if let Some(cap) = &s.capacity {
                    let cv = eval_qof_value(cap, &ctx)?.as_scalar();
                    cap_vals.insert(id.to_string(), cv);
                    Some(cv)
                } else {
                    None
                };
                // Bound-crossing view (B1 gap #1): record the *unclamped* trajectory (level at
                // sub-interval start, net Euler rate) against this stock's floor/capacity, so the
                // provider can solve the closed-form crossing time. Capture BEFORE the clamps below,
                // since it is the unclamped trajectory that reaches the bound. Scalar stocks only —
                // array stocks are not bound-split in phase 1 (their `as_scalar` is the mean/first).
                if use_event_accurate && sub_dt > 0.0 && (s.floor.is_some() || cap_val_opt.is_some()) {
                    let pre_clamp_next = next.as_scalar();
                    let net_rate = (pre_clamp_next - level_start) / sub_dt;
                    stock_bound_views.push(crate::timebase::StockBoundView {
                        level: level_start,
                        rate: net_rate,
                        floor: s.floor.as_ref().map(|f| f.value),
                        capacity: cap_val_opt,
                    });
                }
                if let Some(floor) = &s.floor {
                    let lo = floor.value;
                    next = next.map(|v| v.max(lo));
                }
                // Trait capacity_clamp (+ overflow_routing): clamp to capacity, route excess.
                let mut ovf_rate = 0.0;
                if let Some(cap_val) = cap_val_opt {
                    let cur = next.as_scalar();
                    if cur > cap_val {
                        let excess = cur - cap_val;
                        ovf_rate = if sub_dt > 0.0 { excess / sub_dt } else { 0.0 };
                        next = next.map(|v| v.min(cap_val));
                        if let Some(target) = &s.overflow_target {
                            *overflow_in.entry(target.clone()).or_default() += excess;
                        }
                    }
                }
                stock_rates.insert(
                    id.to_string(),
                    (add_rate, wd_rate + if sub_dt > 0.0 { withdrawal_outflow / sub_dt } else { 0.0 }, ovf_rate),
                );
                next_vals.insert(id.to_string(), next);
            }

            // Apply routed overflow to target stocks (single level), then re-clamp.
            for &id in &stock_ids {
                let Some(mut next) = next_vals.remove(id) else { continue };
                if let Some(extra) = overflow_in.get(id) {
                    let extra = *extra;
                    next = next.map(move |v| v + extra);
                    if let Some(&cap_val) = cap_vals.get(id) {
                        next = next.map(|v| v.min(cap_val));
                    }
                }
                stock_state.insert(id.to_string(), next);
            }

            // End-of-step: recorded value reflects post-update level; withdrawal targets
            // output their allocation.
            for &id in &stock_ids {
                if let Some(v) = stock_state.get(id) {
                    outputs.insert(id.to_string(), v.clone());
                }
            }
            for (target, alloc) in &withdrawal_allocs {
                outputs.insert(target.clone(), Value::Scalar(*alloc));
            }
            // Publish stock secondary output ports (§1c): outputs[k] (k ≥ 1) that declare a
            // role get this step's applied rate under the key "<id>#<k+1>", resolvable via a
            // `ref` with an output qualifier (same-step consumers see the previous step's
            // value, matching how stock levels are read).
            for &id in &stock_ids {
                let base = &model.elements[elem_idx[id]].base;
                if base.outputs.len() < 2 {
                    continue;
                }
                let (add, wd, ovf) = stock_rates.get(id).copied().unwrap_or((0.0, 0.0, 0.0));
                let level = stock_state.get(id).map(|v| v.as_scalar()).unwrap_or(0.0);
                let net = level - stock_prev_level.get(id).copied().unwrap_or(0.0);
                for (k, spec) in base.outputs.iter().enumerate().skip(1) {
                    let Some(role) = spec.role.as_deref() else { continue };
                    // After parse-normalization `role` is a flow name and `output_kind` is set
                    // (defaulting to "rate"); the per-step `rate` for each flow, plus the flow's
                    // applied amount this sub-interval (rate · sub_dt) for the cumulative total.
                    let (rate, flow): (f64, &'static str) = match role {
                        "addition" => (add, "addition"),
                        "withdrawal" => (wd, "withdrawal"),
                        "overflow" => (ovf, "overflow"),
                        "net_change" => (if sub_dt > 0.0 { net / sub_dt } else { 0.0 }, "net_change"),
                        _ => continue,
                    };
                    let kind = spec.output_kind.as_deref().unwrap_or("rate");
                    let v = match kind {
                        "level" => level,
                        "cumulative" => {
                            // `net_change` is already the level delta over the sub-interval; the
                            // other flows are rates → amount = rate · sub_dt.
                            let amount = if flow == "net_change" { net } else { rate * sub_dt };
                            let entry = stock_cumulative.entry((id.to_string(), flow)).or_default();
                            *entry += amount;
                            *entry
                        }
                        // "rate" (and any unknown kind → rate, back-compat).
                        _ => rate,
                    };
                    outputs.insert(format!("{id}#{}", k + 1), Value::Scalar(v));
                }
            }

            // ── Cell mass transport: source_release, species_transport links, partitioning
            // equilibrium, and decay chains. Mass tracked per (cell, species, medium). ──
            {
                let mut cell_delta: HashMap<(String, String, String), f64> = HashMap::new();
                let mut st_link_out: Vec<(String, f64)> = Vec::new();
                {
                    // Cell transport is integration → sub-interval clock (== grid on a fixed grid).
                    let ctx = ctx_at(&lookups, &outputs, &prev_outputs, sub_t, sub_dt, &dt_unit, step_idx, &arr);
                    let first_medium = |cell: &str| -> String {
                        cell_media.get(cell).and_then(|m| m.first()).map(|(id, _)| id.clone()).unwrap_or_default()
                    };
                    // source_release: emit finite inventory into the target cell's first medium.
                    for elem in &model.elements {
                        if let Primitive::Cell(c) = &elem.primitive {
                            if let (Some(rate), Some(target)) = (&c.release_rate, &c.release_target) {
                                let fires = match &c.release_schedule {
                                    Some(t) => trigger_fires(t, &ctx, dt, step_idx)?,
                                    None => true,
                                };
                                if !fires {
                                    continue;
                                }
                                let want = (eval_qof_value(rate, &ctx)?.as_scalar() * sub_dt).max(0.0);
                                let released = match source_inv.get_mut(elem.id()) {
                                    Some(b) => {
                                        let r = want.min(*b);
                                        *b -= r;
                                        r
                                    }
                                    None => want,
                                };
                                let sp = c.species.first().map(|s| s.species.clone()).unwrap_or_default();
                                *cell_delta.entry((target.clone(), sp, first_medium(target))).or_default() += released;
                            }
                        }
                    }
                    // species_transport links: move a species (rate or fraction) between cells.
                    for elem in &model.elements {
                        if let Primitive::Link(l) = &elem.primitive {
                            let (Some(species), Some(src), Some(tgt)) = (&l.species, &l.source, &l.target) else { continue };
                            let src_medium = l.medium.clone().unwrap_or_else(|| first_medium(src));
                            let tgt_medium = l.medium.clone().unwrap_or_else(|| first_medium(tgt));
                            let src_mass = cell_mass.get(&(src.clone(), species.clone(), src_medium.clone())).copied().unwrap_or(0.0);
                            let want = if let Some(rate) = &l.rate {
                                (eval_qof_value(rate, &ctx)?.as_scalar() * sub_dt).max(0.0)
                            } else if let Some(frac) = &l.fraction {
                                let frac_scale = if dt > 0.0 { sub_dt / dt } else { 1.0 };
                                (eval_qof_value(frac, &ctx)?.as_scalar() * src_mass * frac_scale).max(0.0)
                            } else {
                                0.0
                            };
                            let moved = want.min(src_mass);
                            *cell_delta.entry((src.clone(), species.clone(), src_medium)).or_default() -= moved;
                            *cell_delta.entry((tgt.clone(), species.clone(), tgt_medium)).or_default() += moved;
                            st_link_out.push((elem.id().to_string(), moved));
                        }
                    }
                }
                for (id, v) in st_link_out {
                    outputs.insert(id, Value::Scalar(v));
                }
                for (key, amt) in cell_delta {
                    *cell_mass.entry(key).or_default() += amt;
                }
                // Trait partitioning_equilibrium: redistribute each species across media by Kd.
                {
                    let ctx = ctx_at(&lookups, &outputs, &prev_outputs, sub_t, sub_dt, &dt_unit, step_idx, &arr);
                    for elem in &model.elements {
                        if let Primitive::Cell(c) = &elem.primitive {
                            if c.partitioning.is_empty() {
                                continue;
                            }
                            let media = cell_media.get(elem.id()).cloned().unwrap_or_default();
                            if media.len() < 2 {
                                continue;
                            }
                            let cell = elem.id();
                            let mut species_set: HashSet<String> = c.species.iter().map(|s| s.species.clone()).collect();
                            for p in &c.partitioning {
                                species_set.insert(p.species.clone());
                            }
                            for sp in &species_set {
                                let m_total: f64 = media.iter()
                                    .map(|(med, _)| cell_mass.get(&(cell.to_string(), sp.clone(), med.clone())).copied().unwrap_or(0.0))
                                    .sum();
                                if m_total <= 0.0 {
                                    continue;
                                }
                                let ratios = partition_ratios(sp, &media, &c.partitioning, &ctx)?;
                                let denom: f64 = media.iter().zip(&ratios).map(|((_, f), r)| r * f).sum();
                                if denom <= 0.0 {
                                    continue;
                                }
                                for ((med, f), r) in media.iter().zip(&ratios) {
                                    cell_mass.insert((cell.to_string(), sp.clone(), med.clone()), m_total * (r * f) / denom);
                                }
                            }
                        }
                    }
                }
                // Decay each (species, medium), parents first; daughters ingrow in the same medium.
                for elem in &model.elements {
                    if let Primitive::Cell(_) = &elem.primitive {
                        let cell = elem.id();
                        let media = cell_media.get(cell).cloned().unwrap_or_else(|| vec![(String::new(), 1.0)]);
                        for sp in &decay_order {
                            if let Some((Some(hl), products)) = species_info.get(sp) {
                                let factor = (-std::f64::consts::LN_2 / *hl * sub_dt).exp();
                                for (med, _) in &media {
                                    let key = (cell.to_string(), sp.clone(), med.clone());
                                    let mass = match cell_mass.get(&key) {
                                        Some(&m) if m != 0.0 => m,
                                        _ => continue,
                                    };
                                    let decayed = mass * (1.0 - factor);
                                    cell_mass.insert(key, mass - decayed);
                                    for (daughter, branching) in products {
                                        *cell_mass.entry((cell.to_string(), daughter.clone(), med.clone())).or_default()
                                            += decayed * *branching;
                                    }
                                }
                            }
                        }
                    }
                }
                // Publish cell outputs: cell total, per-species total, and per-medium.
                for elem in &model.elements {
                    if let Primitive::Cell(c) = &elem.primitive {
                        let cell = elem.id();
                        let media = cell_media.get(cell).cloned().unwrap_or_else(|| vec![(String::new(), 1.0)]);
                        for sp in &c.species {
                            let mut sp_total = 0.0;
                            for (med, _) in &media {
                                let m = cell_mass.get(&(cell.to_string(), sp.species.clone(), med.clone())).copied().unwrap_or(0.0);
                                sp_total += m;
                                if !c.media.is_empty() {
                                    outputs.insert(format!("{cell}:{}@{}", sp.species, med), Value::Scalar(m));
                                }
                            }
                            outputs.insert(format!("{cell}:{}", sp.species), Value::Scalar(sp_total));
                        }
                        let total: f64 = cell_mass.iter()
                            .filter(|((cid, _, _), _)| cid == cell)
                            .map(|(_, &m)| m)
                            .sum();
                        outputs.insert(cell.to_string(), Value::Scalar(total));
                    }
                }
            }

                // ── Bound-crossing detection (B1 gap #1). Under EventAccurate, if a bounded stock
                // crossed its floor/capacity strictly inside this sub-interval, roll back this try
                // (restore the snapshot) and re-run shortened to the crossing instant, so the next
                // sub-interval re-evaluates coupled downstream elements against the clamped level.
                // The re-run consumes no RNG and does not touch grid-only state: those run only when
                // `is_last`, and a crossing-truncated interval ends before `grid_end` (never last).
                if use_event_accurate && !stock_bound_views.is_empty() {
                    use crate::timebase::TimebaseProvider as _;
                    let view = crate::timebase::StepView {
                        step_idx,
                        t_start: sub_t,
                        dt: sub_dt,
                        stock_bounds: &stock_bound_views,
                    };
                    let crossing = crate::timebase::BoundCrossing
                        .split_points(&view)
                        .into_iter()
                        .next();
                    if let Some(t_c) = crossing {
                        if t_c > sub_t + crate::timebase::EPS && t_c < sub_end - crate::timebase::EPS {
                            if splits_this_step < MAX_SPLITS_PER_STEP {
                                // Roll back this aborted try and re-run shortened to the crossing.
                                stock_state = snap_stock_state.unwrap();
                                outputs = snap_outputs.unwrap();
                                cell_mass = snap_cell_mass.unwrap();
                                source_inv = snap_source_inv.unwrap();
                                stock_cumulative = snap_stock_cumulative.unwrap();
                                forced_sub_end = Some(t_c);
                                splits_this_step += 1;
                                continue;
                            } else {
                                // Pathological always-crossing rate: cap the subdivisions and let
                                // this (grid-quantized) sub-interval commit so the run completes.
                                eprintln!(
                                    "warn: bound-crossing splits exceeded {MAX_SPLITS_PER_STEP} at \
                                     step {step_idx}; integrating remainder grid-quantized"
                                );
                            }
                        }
                    }
                }
                // Committed for real: clear any forced crossing end so the next sub-interval picks
                // its boundary from the scheduled splits / grid end again.
                forced_sub_end = None;

                // ── End of this sub-interval: advance. Consume the scheduled boundary we just
                // integrated up to, and stop once we reach the grid end.
                if is_last {
                    break;
                }
                if split_iter.peek().map(|&t| t <= sub_end + crate::timebase::EPS).unwrap_or(false) {
                    split_iter.next();
                }
                sub_t = sub_end;
            } // sub-interval loop

            // Dynamic (per-timestep) optimization (§13a): after the topo pass has filled this
            // step's `outputs` (drivers, non-variable elements), re-solve the submodel's
            // optimization against the objective at this step, then overwrite each variable's
            // recorded value with the optimum and re-evaluate the objective at the winner. The
            // objective is evaluated by injecting candidate variable values into a scratch copy
            // of `outputs` (variables' downstream cone is the objective + interface outputs,
            // which are the variables themselves — see §13a).
            if let Some(dopt) = &dyn_opt {
                if let Some(obj_ast) = &dopt.objective_ast {
                    // Score a candidate: inject its variable values into a scratch copy of this
                    // step's outputs, evaluate the objective, apply the maximize→minimize flip.
                    let mut scratch = outputs.clone();
                    let winner = crate::optimize_v2::solve(
                        &dopt.bounds,
                        seed.wrapping_add(step_idx as u64),
                        |point| {
                            for (id, &v) in dopt.var_ids.iter().zip(point) {
                                scratch.insert(id.clone(), Value::Scalar(v));
                            }
                            let ctx = ctx_at(&lookups, &scratch, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                            let val = eval_ast(obj_ast, &ctx).map(|v| v.as_scalar()).unwrap_or(f64::INFINITY);
                            if !val.is_finite() {
                                return f64::INFINITY;
                            }
                            match dopt.direction {
                                OptDirection::Minimize => val,
                                OptDirection::Maximize => -val,
                            }
                        },
                    );
                    // Record the optimum: each variable's output becomes its winning value, then
                    // recompute the objective element at the winner so its series is consistent.
                    for (id, &v) in dopt.var_ids.iter().zip(&winner.point) {
                        outputs.insert(id.clone(), Value::Scalar(v));
                    }
                    if let Some(spec) = &model.optimization {
                        let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                        if let Ok(v) = eval_ast(obj_ast, &ctx) {
                            outputs.insert(spec.objective.element_id.clone(), v);
                        }
                    }
                }
            }

            for d in &model.time_history_displays {
                let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx, &arr);
                let v = eval_ast(&d.expression.ast, &ctx)?.as_scalar();
                hist_store.get_mut(&d.id).unwrap()[step_idx].push(v);
                if step_idx == n_steps - 1 {
                    final_store.get_mut(&d.id).unwrap().push(v);
                }
            }

            for &id in &save_hist {
                if let Some(v) = outputs.get(id) {
                    hist_store.get_mut(id).unwrap()[step_idx].push(v.as_scalar());
                }
            }
            if step_idx == n_steps - 1 {
                for &id in &save_final {
                    if let Some(v) = outputs.get(id) {
                        final_store.get_mut(id).unwrap().push(v.as_scalar());
                    }
                }
            }
            // Per-(cell, species) mass records.
            for id in &cell_species_ids {
                if let Some(v) = outputs.get(id) {
                    hist_store.get_mut(id).unwrap()[step_idx].push(v.as_scalar());
                    if step_idx == n_steps - 1 {
                        final_store.get_mut(id).unwrap().push(v.as_scalar());
                    }
                }
            }

            prev_outputs = outputs;
            // An interrupt fired this step: the current step completed and was recorded above;
            // from the next step on, hold these values.
            if interrupt_now {
                interrupted = true;
            }
        }
    }

    // ── Aggregate ─────────────────────────────────────────────────────────────
    let time_axis: Vec<f64> = (0..n_steps).map(|i| i as f64 * dt).collect();
    let mut results_map: HashMap<String, ElementResults> = HashMap::new();

    for elem in &model.elements {
        let id = elem.id();
        let has_final = save_final.contains(&id);
        let has_hist = save_hist.contains(&id);
        if !has_final && !has_hist {
            continue;
        }
        let final_values = final_store.get(id).cloned().unwrap_or_default();
        let time_history = if has_hist {
            Some(stats(&hist_store[id]))
        } else {
            None
        };
        // A3: richer analysis when a results_spec opts this element in (empty hist if not saved).
        let empty_hist: Vec<Vec<f64>> = Vec::new();
        let hist_ref = if has_hist { &hist_store[id] } else { &empty_hist };
        let analysis = config
            .results_spec
            .as_ref()
            .and_then(|spec| crate::results_spec::compute_analysis(spec, id, &final_values, hist_ref, &realization_weights, dt));
        results_map.insert(id.to_string(), ElementResults {
            label: elem.base.name.clone(),
            unit: primary_unit(elem).to_string(),
            final_values,
            time_history,
            analysis,
        });
    }

    for d in &model.time_history_displays {
        let final_values = final_store.get(&d.id).cloned().unwrap_or_default();
        let analysis = config.results_spec.as_ref().and_then(|spec| {
            crate::results_spec::compute_analysis(spec, &d.id, &final_values, &hist_store[&d.id], &realization_weights, dt)
        });
        results_map.insert(d.id.clone(), ElementResults {
            label: d.name.clone(),
            unit: "1".to_string(),
            final_values,
            time_history: Some(stats(&hist_store[&d.id])),
            analysis,
        });
    }

    // Dynamic (per-timestep) optimization (§13a): run any submodel that carries an optimization
    // over this (parent) clock and merge its element series in (notably the optimized variables'
    // time histories). Gated on `!dynamic_optimization` so a dynamic submodel — which itself runs
    // via this same function — does not recurse into its own dynamic pass.
    if !model.dynamic_optimization {
        let dyn_results = crate::submodel_v2::run_dynamic_submodels(model, config, &model.simulation_settings)?;
        for (id, er) in dyn_results {
            results_map.entry(id).or_insert(er);
        }
    }

    // Display order: time_history_displays, then sinks, then intermediates (topo order).
    let referenced: HashSet<&str> = model.elements.iter()
        .flat_map(|e| {
            let mut v: Vec<&str> = e.base.inputs.iter().map(|s| s.as_str()).collect();
            if let Primitive::Stock(s) = &e.primitive {
                v.extend(s.inflows.iter().map(|x| x.as_str()));
                v.extend(s.outflows.iter().map(|x| x.as_str()));
            }
            v
        })
        .collect();

    let (sinks, intermediates): (Vec<&str>, Vec<&str>) = graph.topo_order.iter()
        .map(String::as_str)
        .filter(|id| results_map.contains_key(*id))
        .partition(|id| !referenced.contains(id));

    // Per-(cell, species) mass result entries.
    for id in &cell_species_ids {
        let final_values = final_store.get(id).cloned().unwrap_or_default();
        let time_history = Some(stats(&hist_store[id]));
        let analysis = config.results_spec.as_ref().and_then(|spec| {
            crate::results_spec::compute_analysis(spec, id, &final_values, &hist_store[id], &realization_weights, dt)
        });
        results_map.insert(id.clone(), ElementResults {
            label: id.clone(),
            unit: "mass".to_string(),
            final_values,
            time_history,
            analysis,
        });
    }

    let display_ids: Vec<String> = model.time_history_displays.iter().map(|d| d.id.clone()).collect();
    let output_ids: Vec<String> = display_ids
        .into_iter()
        .chain(sinks.iter().chain(intermediates.iter()).map(|&s| s.to_string()))
        .chain(cell_species_ids.iter().cloned())
        .collect();

    Ok(SimulationResults { time_axis, time_unit: dt_unit.clone(), elements: results_map, n_realizations: n_real, n_steps, output_ids })
}

// ── per-element evaluation ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn eval_element(
    elem: &Element,
    lookups: &HashMap<String, LookupData>,
    outputs: &HashMap<String, Value>,
    prev_outputs: &HashMap<String, Value>,
    elapsed: f64,
    dt: f64,
    dt_unit: &str,
    step_idx: usize,
    rv_samples: &HashMap<String, f64>,
    sp_state: &HashMap<String, f64>,
    stock_state: &HashMap<String, Value>,
    arr: &ArrayEnv,
) -> Result<Value, EngineError> {
    let id = elem.id();
    match &elem.primitive {
        Primitive::Stock(_) => Ok(stock_state[id].clone()),
        Primitive::Node(node) => match &node.rule {
            NodeRule::Fixed { value, .. } => Ok(match value {
                FixedValue::Scalar(q) => Value::Scalar(q.value),
                FixedValue::Array { values, .. } => Value::Vector(values.clone()),
            }),
            NodeRule::Sample { .. } => Ok(Value::Scalar(rv_samples.get(id).copied().unwrap_or(0.0))),
            NodeRule::Process { .. } => Ok(Value::Scalar(sp_state.get(id).copied().unwrap_or(0.0))),
            // Lookups are invoked via lookup_call/ref; placeholder output here.
            NodeRule::Lookup(_) => Ok(Value::Scalar(0.0)),
            NodeRule::Series { timestamps, values, interpolation, .. } => {
                Ok(Value::Scalar(eval_timeseries(timestamps, values, interpolation, elapsed)?))
            }
            NodeRule::Lag { input, initial } => {
                // Strict one-step delay: read the input's previous-step output.
                // An unwired lag (no input) is a pure initial-value hold.
                let init = initial.as_ref().map(|q| q.value).unwrap_or(0.0);
                let v = input
                    .as_ref()
                    .and_then(|id| prev_outputs.get(id.as_str()))
                    .map(|v| v.as_scalar())
                    .unwrap_or(init);
                Ok(Value::Scalar(v))
            }
            NodeRule::Expression(ef) => {
                let ctx = ctx_at(lookups, outputs, prev_outputs, elapsed, dt, dt_unit, step_idx, arr);
                eval_ast(&ef.ast, &ctx)
            }
            other => Err(EngineError::Unsupported(format!(
                "node rule {} not yet supported in the v2 engine (M2+)",
                rule_name(other)
            ))),
        },
        other => Err(EngineError::Unsupported(format!(
            "primitive {} not yet supported in the v2 engine (M2+)",
            primitive_name(other)
        ))),
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Per-run evaluation environment threaded into every `EvalCtx`. Constant across a run:
/// `dims` is the dimension-size table; `index_stack` is the shared vector_map index stack
/// (interior-mutable); `submodel_outputs` holds each referenced submodel output's
/// per-realization samples (§12). Bundled so the many `ctx_at` call sites take one arg.
pub(crate) struct ArrayEnv<'a> {
    pub dims: &'a HashMap<String, usize>,
    pub index_stack: &'a RefCell<Vec<usize>>,
    pub submodel_outputs: &'a HashMap<(String, String), Vec<f64>>,
    /// Ids of events that fired during the current step (§2, for the `occurs` builtin). The
    /// event pass repopulates this each step via interior mutability.
    pub fired_events: &'a RefCell<HashSet<String>>,
    /// Calendar anchor (B6): model-clock start as seconds since the Unix epoch (None = fixed
    /// 365-day calendar).
    pub calendar_start: Option<f64>,
}

fn ctx_at<'a>(
    lookups: &'a HashMap<String, LookupData>,
    outputs: &'a HashMap<String, Value>,
    prev_outputs: &'a HashMap<String, Value>,
    elapsed: f64,
    dt: f64,
    dt_unit: &'a str,
    step_index: usize,
    arr: &ArrayEnv<'a>,
) -> EvalCtx<'a> {
    EvalCtx {
        lookups, outputs, prev_outputs, elapsed, dt, dt_unit, step_index,
        dimensions: arr.dims, index_stack: arr.index_stack, submodel_outputs: arr.submodel_outputs,
        lag: None, fired_events: arr.fired_events, calendar_start: arr.calendar_start,
    }
}

fn dist_ctx_eval<'a>(
    lookups: &'a HashMap<String, LookupData>,
    outputs: &'a HashMap<String, Value>,
    prev_outputs: &'a HashMap<String, Value>,
    dt: f64,
    dt_unit: &'a str,
    arr: &ArrayEnv<'a>,
) -> EvalCtx<'a> {
    EvalCtx {
        lookups, outputs, prev_outputs, elapsed: 0.0, dt, dt_unit, step_index: 0,
        dimensions: arr.dims, index_stack: arr.index_stack, submodel_outputs: arr.submodel_outputs,
        lag: None, fired_events: arr.fired_events, calendar_start: arr.calendar_start,
    }
}

fn eval_qof_value(qof: &QuantityOrFormula, ctx: &EvalCtx) -> Result<Value, EngineError> {
    match qof {
        QuantityOrFormula::Quantity(q) => Ok(Value::Scalar(q.value)),
        QuantityOrFormula::Expression(ef) => eval_ast(&ef.ast, ctx),
        QuantityOrFormula::Formula(_) => Ok(Value::Scalar(0.0)),
    }
}

/// Evaluate a `quantity_expr` (fixed quantity or bare AST) to a scalar.
fn eval_qexpr(qe: &QuantityExpr, ctx: &EvalCtx) -> Result<f64, EngineError> {
    match qe {
        QuantityExpr::Quantity(q) => Ok(q.value),
        QuantityExpr::Ast(a) => Ok(eval_ast(a, ctx)?.as_scalar()),
    }
}

/// Structural (constant) value of a quantity_or_formula; expressions default to 1.0.
fn qof_const(q: &QuantityOrFormula) -> f64 {
    match q {
        QuantityOrFormula::Quantity(x) => x.value,
        _ => 1.0,
    }
}

/// Per-medium concentration ratios r_m (reference = first medium, r=1) for one species,
/// derived from the partition coefficients (r_to = Kd·r_from). Unconnected media → 1.
/// Equilibrium mass in medium m is then M·(r_m·f_m)/Σ(r_k·f_k).
fn partition_ratios(
    species: &str,
    media: &[(String, f64)],
    entries: &[PartitionEntry],
    ctx: &EvalCtx,
) -> Result<Vec<f64>, EngineError> {
    let n = media.len();
    let idx: HashMap<&str, usize> = media.iter().enumerate().map(|(i, (m, _))| (m.as_str(), i)).collect();
    let mut r = vec![f64::NAN; n];
    r[0] = 1.0;
    for _ in 0..n {
        let mut changed = false;
        for e in entries.iter().filter(|e| e.species == species) {
            let (Some(&fi), Some(&ti)) = (idx.get(e.from_medium.as_str()), idx.get(e.to_medium.as_str())) else {
                continue;
            };
            let kd = eval_qof_value(&e.coefficient, ctx)?.as_scalar();
            if r[fi].is_finite() && !r[ti].is_finite() {
                r[ti] = kd * r[fi];
                changed = true;
            } else if r[ti].is_finite() && !r[fi].is_finite() && kd != 0.0 {
                r[fi] = r[ti] / kd;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for x in &mut r {
        if !x.is_finite() {
            *x = 1.0;
        }
    }
    Ok(r)
}

/// Topologically order species so each parent precedes its decay products (so a chain
/// fully propagates within one step). Cycles are broken by the visited set.
fn build_decay_order(info: &HashMap<String, (Option<f64>, Vec<(String, f64)>)>) -> Vec<String> {
    fn visit(
        id: &str,
        info: &HashMap<String, (Option<f64>, Vec<(String, f64)>)>,
        visited: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) {
        if visited.contains(id) {
            return;
        }
        visited.insert(id.to_string());
        if let Some((_, products)) = info.get(id) {
            for (daughter, _) in products {
                visit(daughter, info, visited, order);
            }
        }
        order.push(id.to_string());
    }
    let mut visited = HashSet::new();
    let mut order = Vec::new();
    for id in info.keys() {
        visit(id, info, &mut visited, &mut order);
    }
    order.reverse(); // post-order reversed → parents before daughters
    order
}

/// Discretized residence-time distribution for link transit_dispersion (V2_SCOPING §11a):
/// the Ogata-Banks / inverse-Gaussian RTD with mean `transit_time` and variance 2T²/Pe,
/// sampled at k·dt for k=1..K and normalized to a unit-sum convolution kernel.
fn dispersion_kernel(transit_time: f64, pe: f64, dt: f64) -> Vec<f64> {
    let kmax = ((10.0 * transit_time / dt).ceil() as usize).clamp(1, 100_000);
    let mut w = Vec::with_capacity(kmax);
    for k in 1..=kmax {
        let t = k as f64 * dt;
        let e = (pe * transit_time / (4.0 * std::f64::consts::PI * t.powi(3))).sqrt()
            * (-pe * (t - transit_time).powi(2) / (4.0 * transit_time * t)).exp();
        w.push(e * dt);
    }
    let sum: f64 = w.iter().sum();
    if sum > 0.0 {
        for x in &mut w {
            *x /= sum;
        }
    }
    w
}

/// Number of Poisson(λ) occurrences this step.
fn poisson_count<R: Rng>(lambda: f64, rng: &mut R) -> u64 {
    if lambda <= 0.0 {
        return 0;
    }
    match rand_distr::Poisson::new(lambda) {
        Ok(p) => rng.sample(p) as u64,
        Err(_) => 0,
    }
}

/// Whether a trigger fires this timestep. `condition` is evaluated against `ctx`
/// (the caller decides whether that holds current- or previous-step outputs).
fn trigger_fires(t: &TriggerSpec, ctx: &EvalCtx, dt: f64, step_idx: usize) -> Result<bool, EngineError> {
    Ok(match infer_mode(t) {
        TriggerMode::Always => true,
        TriggerMode::OnCondition => match &t.condition {
            Some(c) => eval_qof_value(c, ctx)?.as_scalar() != 0.0,
            None => false,
        },
        TriggerMode::Periodic => match &t.period {
            Some(p) => {
                let period_steps = (p.value / dt).round().max(1.0) as usize;
                step_idx > 0 && step_idx % period_steps == 0
            }
            None => false,
        },
        TriggerMode::OnSchedule => {
            t.schedule.iter().any(|q| (q.value / dt).round() as usize == step_idx)
        }
        // Fires when the referenced source event is in this step's fired set (§2, same
        // semantics as the `occurs(ev)` builtin). Causality: `fired_events` holds trigger-driven
        // fires from the pre-pass (same-step) and rate/failure fires from the previous step's
        // event pass (next-step). Chaining two `on_event` events in one step is declaration-order
        // dependent (the pre-pass is a single linear pass, not a fixpoint) — documented in §2.
        TriggerMode::OnEvent => t
            .source
            .as_ref()
            .map(|s| ctx.fired_events.borrow().contains(s))
            .unwrap_or(false),
    })
}

/// Infer a trigger's mode from present fields when `mode` is unspecified.
fn infer_mode(t: &TriggerSpec) -> TriggerMode {
    match t.mode {
        Some(m) => m,
        None => {
            if t.condition.is_some() {
                TriggerMode::OnCondition
            } else if t.period.is_some() {
                TriggerMode::Periodic
            } else if !t.schedule.is_empty() {
                TriggerMode::OnSchedule
            } else if t.source.is_some() {
                TriggerMode::OnEvent
            } else {
                TriggerMode::Always
            }
        }
    }
}

/// Evaluate a boolean gate tree against the current-step outputs.
fn eval_gate(node: &GateNode, ctx: &EvalCtx) -> Result<bool, EngineError> {
    Ok(match node {
        GateNode::And(children) => {
            for ch in children {
                if !eval_gate(ch, ctx)? {
                    return Ok(false);
                }
            }
            true
        }
        GateNode::Or(children) => {
            for ch in children {
                if eval_gate(ch, ctx)? {
                    return Ok(true);
                }
            }
            false
        }
        GateNode::Not(child) => !eval_gate(child, ctx)?,
        GateNode::NVote { threshold, children } => {
            let mut k = 0u32;
            for ch in children {
                if eval_gate(ch, ctx)? {
                    k += 1;
                }
            }
            k >= *threshold
        }
        GateNode::Reference(id) | GateNode::Input(id) => {
            ctx.outputs.get(id.as_str()).map(|v| v.as_scalar()).unwrap_or(0.0) > 0.0
        }
        GateNode::Condition(qof) => eval_qof_value(qof, ctx)?.as_scalar() != 0.0,
    })
}

/// Convolution response weights: inline values, the y-column of a referenced lookup, or — for an
/// expression-valued response (§17) — the `~lag` formula sampled onto the lag grid with any
/// referenced element resolved from `base_ctx` (so calibratable kernels stay live). `base_ctx`
/// supplies the element/time context; each grid point rebinds `lag`.
fn conv_weights(
    response: &ConvResponse,
    lookups: &HashMap<String, LookupData>,
    base_ctx: &EvalCtx,
) -> Vec<f64> {
    match response {
        ConvResponse::Inline { values, .. } => values.clone(),
        ConvResponse::Ref(id) => lookups
            .get(id)
            .map(|l| if !l.y.is_empty() { l.y.clone() } else { l.columns.first().cloned().unwrap_or_default() })
            .unwrap_or_default(),
        ConvResponse::Expr { ast, interval_s, length_s, cumulative } => {
            if *interval_s <= 0.0 || *length_s <= 0.0 {
                return vec![1.0];
            }
            let n = ((length_s / interval_s).round() as usize + 1).min(4096);
            // Sample the response curve at lag τ = i·interval (seconds), binding `lag`.
            let curve: Vec<f64> = (0..n)
                .map(|i| {
                    let tau = i as f64 * interval_s;
                    let ctx = EvalCtx { lag: Some(tau), ..*base_ctx };
                    eval_ast(ast, &ctx).map(|v| v.as_scalar()).unwrap_or(0.0)
                })
                .collect();
            if *cumulative {
                // Cumulative response (S-curve) → weights are its successive differences.
                std::iter::once(curve[0])
                    .chain((1..n).map(|i| curve[i] - curve[i - 1]))
                    .collect()
            } else {
                // Density → weight = value × interval (converting a rate into a per-step mass).
                curve.iter().map(|f| f * interval_s).collect()
            }
        }
    }
}

fn stats(per_step: &[Vec<f64>]) -> TimeHistoryStats {
    TimeHistoryStats {
        mean: per_step.iter().map(|vs| mean(vs)).collect(),
        p05: per_step.iter().map(|vs| percentile(vs, 5.0)).collect(),
        p25: per_step.iter().map(|vs| percentile(vs, 25.0)).collect(),
        p50: per_step.iter().map(|vs| percentile(vs, 50.0)).collect(),
        p75: per_step.iter().map(|vs| percentile(vs, 75.0)).collect(),
        p95: per_step.iter().map(|vs| percentile(vs, 95.0)).collect(),
    }
}

fn lookups_map(model: &Model) -> HashMap<String, LookupData> {
    model.elements.iter().filter_map(|e| {
        if let Primitive::Node(n) = &e.primitive {
            if let NodeRule::Lookup(t) = &n.rule {
                let (columns, extra_axes, nd_values) = decode_table_z(&t.x, &t.z);
                return Some((e.id().to_string(), LookupData {
                    x: t.x.clone(),
                    y: t.y.clone(),
                    columns,
                    extrapolation: t.extrapolation.clone(),
                    interpolation: t.interpolation.clone(),
                    log_result: t.log_result,
                    extra_axes,
                    nd_values,
                }));
            }
        }
        None
    }).collect()
}

/// Decode the emit `table.z` payload (§10). Emit packs an N-D table as
/// `z = [axis2_breakpoints, (axis3_breakpoints)?, flat_values]`, where `flat_values` is
/// row-major over (x, axis2, axis3) and its length equals `|x| · |axis2| · |axis3|`. When `z`
/// matches that shape it is an N-D table → return `(no columns, extra axes, flat values)`.
/// Otherwise `z` is treated as legacy columns-of-y (the pre-0.9.2 behavior) and returned as-is.
fn decode_table_z(x: &[f64], z: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<Vec<f64>>, Vec<f64>) {
    if z.len() >= 2 && !x.is_empty() {
        let (axes, flat) = z.split_at(z.len() - 1);
        let flat = &flat[0];
        let expected: usize = axes.iter().map(|a| a.len()).product::<usize>() * x.len();
        if expected > 0 && flat.len() == expected {
            return (Vec::new(), axes.to_vec(), flat.clone());
        }
    }
    // Not the N-D packing → legacy columns.
    (z.to_vec(), Vec::new(), Vec::new())
}

/// Scalar value of a `fixed`-scalar node (the v1 `constant` analog), else None.
fn fixed_scalar(elem: &Element) -> Option<f64> {
    match &elem.primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => Some(q.value),
            _ => None,
        },
        _ => None,
    }
}

fn is_fixed_scalar(elem: &Element) -> bool {
    fixed_scalar(elem).is_some()
}

/// Default-save everything except fixed-scalar nodes (matches v1: save unless constant).
/// `species`/`medium` are inert definitions and never produce results.
fn is_definition(elem: &Element) -> bool {
    matches!(elem.primitive, Primitive::Species(_) | Primitive::Medium(_))
}
fn should_save_history(elem: &Element) -> bool {
    !is_definition(elem) && elem.base.save_results.time_history.unwrap_or_else(|| !is_fixed_scalar(elem))
}
fn should_save_final(elem: &Element) -> bool {
    !is_definition(elem) && elem.base.save_results.final_value.unwrap_or_else(|| !is_fixed_scalar(elem))
}

fn primary_unit(elem: &Element) -> &str {
    if let Primitive::Node(n) = &elem.primitive {
        match &n.rule {
            NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => return &q.unit,
            NodeRule::Fixed { value: FixedValue::Array { unit, .. }, .. } => return unit,
            _ => {}
        }
    }
    elem.base.outputs.first().map(|o| o.unit.as_str()).unwrap_or("1")
}

/// True when an element is a grid-only stateful node rule (B1): it advances per-grid-step state
/// and/or consumes randomness, so it must evaluate once per grid step (on the final sub-interval),
/// not per integration sub-interval.
fn is_grid_only_rule(elem: &Element) -> bool {
    matches!(
        &elem.primitive,
        Primitive::Node(n) if matches!(
            &n.rule,
            NodeRule::Hysteresis { .. }
                | NodeRule::Filter { .. }
                | NodeRule::Status { .. }
                | NodeRule::Milestone { .. }
                | NodeRule::PidController { .. }
                | NodeRule::Markov { .. }
                | NodeRule::Convolution { .. }
                | NodeRule::Queue { .. }
        )
    )
}

fn rule_name(rule: &NodeRule) -> &'static str {
    match rule {
        NodeRule::Fixed { .. } => "fixed",
        NodeRule::Expression(_) => "expression",
        NodeRule::Sample { .. } => "sample",
        NodeRule::Process { .. } => "process",
        NodeRule::Lookup(_) => "lookup",
        NodeRule::Series { .. } => "series",
        NodeRule::Lag { .. } => "lag",
        NodeRule::Convolution { .. } => "convolution",
        NodeRule::Markov { .. } => "markov",
        NodeRule::Hysteresis { .. } => "hysteresis",
        NodeRule::Filter { .. } => "filter",
        NodeRule::GateLogic { .. } => "gate_logic",
        NodeRule::Status { .. } => "status",
        NodeRule::Milestone { .. } => "milestone",
        NodeRule::PidController { .. } => "pid",
        NodeRule::Queue { .. } => "queue",
    }
}

fn primitive_name(p: &Primitive) -> &'static str {
    match p {
        Primitive::Node(_) => "node",
        Primitive::Stock(_) => "stock",
        Primitive::Link(_) => "link",
        Primitive::Event(_) => "event",
        Primitive::Gate(_) => "gate",
        Primitive::Cell(_) => "cell",
        Primitive::Species(_) => "species",
        Primitive::Medium(_) => "medium",
        Primitive::Resource(_) => "resource",
    }
}

/// Build Gaussian-copula correlation groups from `sample` nodes' `correlations`.
fn build_corr_groups(model: &Model) -> Result<Vec<CorrGroup>, EngineError> {
    let elem_pos: HashMap<&str, usize> =
        model.elements.iter().enumerate().map(|(i, e)| (e.id(), i)).collect();

    let sample_set: HashSet<&str> = model.elements.iter()
        .filter(|e| matches!(&e.primitive, Primitive::Node(n) if matches!(n.rule, NodeRule::Sample { .. })))
        .map(|e| e.id())
        .collect();

    let mut edge_map: HashMap<(String, String), f64> = HashMap::new();
    for elem in &model.elements {
        if let Primitive::Node(n) = &elem.primitive {
            if let NodeRule::Sample { correlations, .. } = &n.rule {
                for pair in correlations {
                    if !sample_set.contains(pair.partner.as_str()) {
                        return Err(EngineError::ElementNotFound(pair.partner.clone()));
                    }
                    let a_pos = elem_pos[elem.id()];
                    let b_pos = elem_pos[pair.partner.as_str()];
                    let (lo, hi) = if a_pos < b_pos {
                        (elem.id().to_string(), pair.partner.clone())
                    } else {
                        (pair.partner.clone(), elem.id().to_string())
                    };
                    edge_map.entry((lo, hi)).or_insert(pair.coefficient);
                }
            }
        }
    }

    if edge_map.is_empty() {
        return Ok(vec![]);
    }

    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for ((a, b), _) in &edge_map {
        adj.entry(a.clone()).or_default().push(b.clone());
        adj.entry(b.clone()).or_default().push(a.clone());
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut components: Vec<Vec<String>> = Vec::new();
    for elem in &model.elements {
        let id = elem.id().to_string();
        if !adj.contains_key(&id) || visited.contains(&id) {
            continue;
        }
        let mut component = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(id.clone());
        visited.insert(id);
        while let Some(cur) = queue.pop_front() {
            component.push(cur.clone());
            if let Some(neighbors) = adj.get(&cur) {
                for nb in neighbors {
                    if !visited.contains(nb) {
                        visited.insert(nb.clone());
                        queue.push_back(nb.clone());
                    }
                }
            }
        }
        component.sort_by_key(|cid| elem_pos.get(cid.as_str()).copied().unwrap_or(usize::MAX));
        components.push(component);
    }

    let mut groups = Vec::new();
    for ids in components {
        let n = ids.len();
        let id_idx: HashMap<&str, usize> =
            ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
        let mut matrix = vec![vec![0.0f64; n]; n];
        for i in 0..n {
            matrix[i][i] = 1.0;
        }
        for ((a, b), &rho) in &edge_map {
            if let (Some(&i), Some(&j)) = (id_idx.get(a.as_str()), id_idx.get(b.as_str())) {
                matrix[i][j] = rho;
                matrix[j][i] = rho;
            }
        }
        let chol_l = cholesky(&matrix).map_err(|_| EngineError::InvalidModel(format!(
            "rank-correlation matrix for [{}] is not positive semi-definite",
            ids.join(", ")
        )))?;
        groups.push(CorrGroup { ids, chol_l });
    }
    Ok(groups)
}

/// Latin Hypercube pre-pass (semantics §8). For each **independent, once-per-realization**
/// sample node, produce a stratified column of `n_real` draws: partition [0,1) into `n_real`
/// equal-probability bins, draw one uniform inside each bin, shuffle the bin order (seeded),
/// and map through the distribution's inverse CDF (truncation-aware). This guarantees the
/// marginal is evenly covered across realizations — LHS's variance-reduction property.
///
/// Scope (matches GoldSim): LHS applies only to once-per-realization draws. Per-step
/// autocorrelated / resampled nodes stay Monte Carlo (their `run`-loop draws are untouched),
/// and correlated groups are stratified separately inside `iman_conover_samples` so LHS and
/// Iman-Conover compose. A distribution with no closed-form inverse CDF (Gamma/Beta/Weibull/
/// Pearson/PERT/StudentT/External) is skipped here and falls back to Monte Carlo in `run`.
///
/// Returns a map from element id → its stratified per-realization column. Only populated when
/// `sampling_method == Lhs`; under Monte Carlo the returned map is empty and `run` samples as
/// before (default behavior bit-identical).
fn lhs_samples(
    model: &Model,
    lhs_ids: &[&str],
    n_real: u32,
    seed: u64,
    lookups: &HashMap<String, LookupData>,
    dt: f64,
    dt_unit: &str,
    arr: &ArrayEnv,
) -> Result<HashMap<String, Vec<f64>>, EngineError> {
    let mut out = HashMap::new();
    let k = n_real as usize;
    if lhs_ids.is_empty() || k == 0 {
        return Ok(out);
    }

    // Dedicated rng stream, disjoint from the realization streams (0..n_real) and from
    // Iman-Conover's stream (u64::MAX). Each variable gets its own sub-stream so adding or
    // removing one variable does not reshuffle the others.
    let elem_idx: HashMap<&str, usize> =
        model.elements.iter().enumerate().map(|(i, e)| (e.id(), i)).collect();
    let mut dist_ctx: HashMap<String, Value> = HashMap::new();
    for elem in &model.elements {
        if let Some(q) = fixed_scalar(elem) {
            dist_ctx.insert(elem.id().to_string(), Value::Scalar(q));
        }
    }
    let empty: HashMap<String, Value> = HashMap::new();

    for (var_i, &id) in lhs_ids.iter().enumerate() {
        let elem = &model.elements[elem_idx[id]];
        let Primitive::Node(node) = &elem.primitive else { continue };
        let NodeRule::Sample { distribution, .. } = &node.rule else { continue };
        let ctx = dist_ctx_eval(lookups, &dist_ctx, &empty, dt, dt_unit, arr);
        let resolved = resolve_distribution(distribution, &ctx)?;

        // Skip (→ MC fallback) any distribution without a closed-form ICDF.
        if !sampling::has_icdf(&resolved.kind) {
            continue;
        }

        // Distinct sub-stream per variable, disjoint from realization/IC streams.
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        rng.set_stream(u64::MAX - 1 - var_i as u64);

        // Stratified uniforms: one per [i/k, (i+1)/k) bin, then permute the bin order.
        let mut strata: Vec<f64> = (0..k)
            .map(|i| (i as f64 + rng.gen::<f64>()) / k as f64)
            .collect();
        shuffle(&mut strata, &mut rng);

        let col: Result<Vec<f64>, EngineError> = strata
            .iter()
            .map(|&u| {
                sampling::icdf_truncated(&resolved.kind, &resolved.truncation, u).ok_or_else(|| {
                    EngineError::Sampling(format!("lhs: no inverse CDF for '{id}'"))
                })
            })
            .collect();
        out.insert(id.to_string(), col?);
    }
    Ok(out)
}

/// Iman-Conover rank correlation. For each group, draw independent marginals for all
/// realizations, build a score matrix with the target rank structure, then reorder each
/// marginal to match — inducing the target rank correlation while preserving marginals.
/// Returns, per correlated element id, its reordered samples (one per realization).
fn iman_conover_samples(
    model: &Model,
    groups: &[CorrGroup],
    n_real: u32,
    seed: u64,
    use_lhs: bool,
    lookups: &HashMap<String, LookupData>,
    dt: f64,
    dt_unit: &str,
    arr: &ArrayEnv,
) -> Result<HashMap<String, Vec<f64>>, EngineError> {
    let mut out = HashMap::new();
    let k = n_real as usize;
    if groups.is_empty() || k == 0 {
        return Ok(out);
    }

    // Dedicated rng stream, disjoint from the realization streams (0..n_real).
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    rng.set_stream(u64::MAX);

    let mut dist_ctx: HashMap<String, Value> = HashMap::new();
    for elem in &model.elements {
        if let Some(q) = fixed_scalar(elem) {
            dist_ctx.insert(elem.id().to_string(), Value::Scalar(q));
        }
    }
    let empty: HashMap<String, Value> = HashMap::new();
    let elem_idx: HashMap<&str, usize> =
        model.elements.iter().enumerate().map(|(i, e)| (e.id(), i)).collect();

    // van der Waerden scores aᵢ = Φ⁻¹(i/(k+1)).
    let scores: Vec<f64> =
        (1..=k).map(|i| sampling::standard_normal_quantile(i as f64 / (k as f64 + 1.0))).collect();

    for group in groups {
        let n = group.ids.len();
        // Independent marginal draws: r_samples[var][realization].
        let mut r_samples: Vec<Vec<f64>> = Vec::with_capacity(n);
        for id in &group.ids {
            let elem = &model.elements[elem_idx[id.as_str()]];
            let Primitive::Node(node) = &elem.primitive else { continue };
            let NodeRule::Sample { distribution, .. } = &node.rule else { continue };
            let ctx = dist_ctx_eval(lookups, &dist_ctx, &empty, dt, dt_unit, arr);
            let resolved = resolve_distribution(distribution, &ctx)?;
            // Under LHS, draw the marginal stratified (evenly covering [0,1)) when the
            // distribution has a closed-form ICDF; Iman-Conover then reorders it to induce
            // the rank correlation without disturbing the (now stratified) marginal — the
            // standard LHS + Iman-Conover pairing. Otherwise fall back to iid draws.
            let col: Result<Vec<f64>, EngineError> = if use_lhs && sampling::has_icdf(&resolved.kind) {
                let mut strata: Vec<f64> = (0..k).map(|i| (i as f64 + rng.gen::<f64>()) / k as f64).collect();
                shuffle(&mut strata, &mut rng);
                strata
                    .iter()
                    .map(|&u| {
                        sampling::icdf_truncated(&resolved.kind, &resolved.truncation, u)
                            .ok_or_else(|| EngineError::Sampling(format!("lhs: no inverse CDF for '{id}'")))
                    })
                    .collect()
            } else {
                (0..k).map(|_| sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)).collect()
            };
            r_samples.push(col?);
        }

        if k < 2 {
            for (j, id) in group.ids.iter().enumerate() {
                out.insert(id.clone(), r_samples[j].clone());
            }
            continue;
        }

        // Score matrix M (n columns, each a permutation of the scores).
        let m_cols: Vec<Vec<f64>> = (0..n)
            .map(|_| {
                let mut s = scores.clone();
                shuffle(&mut s, &mut rng);
                s
            })
            .collect();

        // Decorrelate-then-recorrelate: M* = M · Q⁻ᵀ · Pᵀ, where Q Qᵀ = corr(M), P Pᵀ = C.
        let t = corr_matrix(&m_cols);
        let q = cholesky(&t)
            .map_err(|_| EngineError::InvalidModel("Iman-Conover: score correlation not PSD".into()))?;
        let p = &group.chol_l;
        let mut mstar_cols: Vec<Vec<f64>> = vec![vec![0.0; k]; n];
        for r in 0..k {
            let mrow: Vec<f64> = (0..n).map(|j| m_cols[j][r]).collect();
            let w = forward_solve(&q, &mrow); // Q w = mrow
            let mstar = cholesky_matvec(p, &w); // P w
            for (j, &val) in mstar.iter().enumerate() {
                mstar_cols[j][r] = val;
            }
        }

        // Reorder each marginal so its ranks match the score column's ranks.
        for (j, id) in group.ids.iter().enumerate() {
            let mut sorted = r_samples[j].clone();
            sorted.sort_by(f64::total_cmp);
            let rk = ranks(&mstar_cols[j]);
            let reordered: Vec<f64> = (0..k).map(|r| sorted[rk[r]]).collect();
            out.insert(id.clone(), reordered);
        }
    }
    Ok(out)
}

/// Pearson correlation matrix of N equal-length columns.
fn corr_matrix(cols: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = cols.len();
    let k = cols[0].len() as f64;
    let means: Vec<f64> = cols.iter().map(|c| c.iter().sum::<f64>() / k).collect();
    let mut cov = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            let s: f64 = (0..cols[i].len()).map(|t| (cols[i][t] - means[i]) * (cols[j][t] - means[j])).sum();
            cov[i][j] = s / k;
        }
    }
    let sd: Vec<f64> = (0..n).map(|i| cov[i][i].max(0.0).sqrt()).collect();
    let mut corr = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            corr[i][j] = if sd[i] > 1e-12 && sd[j] > 1e-12 {
                cov[i][j] / (sd[i] * sd[j])
            } else if i == j {
                1.0
            } else {
                0.0
            };
        }
    }
    corr
}

/// Forward substitution: solve L x = b for lower-triangular L.
fn forward_solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let mut x = vec![0.0; n];
    for i in 0..n {
        let mut s = b[i];
        for j in 0..i {
            s -= l[i][j] * x[j];
        }
        x[i] = if l[i][i].abs() > 1e-12 { s / l[i][i] } else { 0.0 };
    }
    x
}

/// In-place Fisher-Yates shuffle.
fn shuffle<R: Rng>(v: &mut [f64], rng: &mut R) {
    for i in (1..v.len()).rev() {
        let j = rng.gen_range(0..=i);
        v.swap(i, j);
    }
}

/// Rank of each element (0 = smallest), ties broken by index via total order.
fn ranks(col: &[f64]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..col.len()).collect();
    idx.sort_by(|&a, &b| col[a].total_cmp(&col[b]));
    let mut r = vec![0usize; col.len()];
    for (rank, &i) in idx.iter().enumerate() {
        r[i] = rank;
    }
    r
}
