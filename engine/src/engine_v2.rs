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
use crate::model::QuantityOrFormula;
use crate::model_v2::{
    ConvResponse, EffectMode, Element, FilterStat, FixedValue, GateNode, MarkovStart, Model,
    NodeRule, Primitive, QuantityExpr, TransitionRow, TriggerMode, TriggerSpec, WithdrawalSpec,
};
use crate::sampling;

struct CorrGroup {
    ids: Vec<String>,
    chol_l: Vec<Vec<f64>>,
}

pub fn run(
    model: &Model,
    graph: &ModelGraphV2,
    config: &RunConfig,
) -> Result<SimulationResults, EngineError> {
    let n_real = config.n_realizations.unwrap_or(model.simulation_settings.n_realizations);
    let seed = config.seed.or(model.simulation_settings.seed).unwrap_or(0);

    let dt = config.timestep_override.unwrap_or(model.simulation_settings.timestep.value);
    let duration = config.duration_override.unwrap_or(model.simulation_settings.duration.value);
    if !dt.is_finite() || dt <= 0.0 {
        return Err(EngineError::InvalidModel(format!("timestep must be > 0, got {dt}")));
    }
    if !duration.is_finite() || duration <= 0.0 {
        return Err(EngineError::InvalidModel(format!("duration must be > 0, got {duration}")));
    }
    let n_steps = (duration / dt).round() as usize;
    let dt_unit = model.simulation_settings.timestep.unit.clone();

    let elem_idx: HashMap<&str, usize> =
        model.elements.iter().enumerate().map(|(i, e)| (e.id(), i)).collect();

    let lookups = lookups_map(model);

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

    let corr_groups = build_corr_groups(model)?;
    let corr_ids: HashSet<String> = corr_groups.iter().flat_map(|g| g.ids.iter().cloned()).collect();
    // Iman-Conover rank correlation: reorder per-realization draws up front (semantics §8).
    let ic_samples = iman_conover_samples(model, &corr_groups, n_real, seed, &lookups, dt, &dt_unit)?;

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

        // Independent sample nodes (correlated ones handled by the copula below).
        for elem in &model.elements {
            if let Primitive::Node(n) = &elem.primitive {
                if let NodeRule::Sample { distribution, .. } = &n.rule {
                    if !corr_ids.contains(elem.id()) {
                        let ctx = dist_ctx_eval(&lookups, &dist_ctx, &empty_prev, dt, &dt_unit);
                        let resolved = resolve_distribution(distribution, &ctx)?;
                        let v = sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)?;
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

        // Initial draw for process (GBM) nodes.
        let mut sp_state: HashMap<String, f64> = HashMap::new();
        for &id in &process_ids {
            if let Primitive::Node(n) = &model.elements[elem_idx[id]].primitive {
                if let NodeRule::Process { process, lower_bound } = &n.rule {
                    let v = sampling::sample_gbm(process, lower_bound.as_ref(), dt, &dt_unit, &mut rng)?;
                    sp_state.insert(id.to_string(), v);
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

        // t=0 snapshot for stock initial_expression evaluation.
        let empty_map: HashMap<String, Value> = HashMap::new();
        let mut init_outputs: HashMap<String, Value> = HashMap::new();
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
                    let ctx = ctx_at(&lookups, &init_outputs, &empty_map, 0.0, dt, &dt_unit, 0);
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
                        let ctx = ctx_at(&lookups, &init_outputs, &empty_map, 0.0, dt, &dt_unit, 0);
                        eval_ast(&expr.ast, &ctx)?
                    }
                    None => Value::Scalar(s.initial_value.value),
                };
                stock_state.insert(id.to_string(), init);
            }
        }

        // Per-realization state for stateful node rules.
        let mut hyst_state: HashMap<String, bool> = HashMap::new();
        let mut filter_buf: HashMap<String, VecDeque<f64>> = HashMap::new();
        let mut filter_ema: HashMap<String, f64> = HashMap::new();
        let mut markov_state: HashMap<String, usize> = HashMap::new();
        let mut conv_buf: HashMap<String, VecDeque<f64>> = HashMap::new();
        // Transit buffers: per link, a FIFO of (release_step, amount).
        let mut link_buf: HashMap<String, VecDeque<(usize, f64)>> = HashMap::new();
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

        let mut prev_outputs: HashMap<String, Value> = HashMap::new();

        for step_idx in 0..n_steps {
            let elapsed = step_idx as f64 * dt;

            for &id in &process_ids {
                if let Primitive::Node(n) = &model.elements[elem_idx[id]].primitive {
                    if let NodeRule::Process { process, lower_bound } = &n.rule {
                        let v = sampling::sample_gbm(process, lower_bound.as_ref(), dt, &dt_unit, &mut rng)?;
                        sp_state.insert(id.to_string(), v);
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
                        let ctx = ctx_at(&lookups, &prev_outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
                        if trigger_fires(trig, &ctx, dt, step_idx)? {
                            let resolved = resolve_distribution(distribution, &ctx)?;
                            let v = sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)?;
                            rv_samples.insert(id.to_string(), v);
                        }
                    }
                }
            }

            let mut outputs: HashMap<String, Value> = HashMap::new();

            for elem_id in &graph.topo_order {
                let elem = &model.elements[elem_idx[elem_id.as_str()]];
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
                        NodeRule::Markov { transition_matrix, output_values, .. } => {
                            let cur = *markov_state.get(elem_id.as_str()).unwrap_or(&0);
                            let out = output_values.get(cur).copied().unwrap_or(0.0);
                            if let Some(row) = transition_matrix.get(cur) {
                                let probs: Vec<f64> = match row {
                                    TransitionRow::Fixed(p) => p.clone(),
                                    TransitionRow::Expr(es) => {
                                        let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
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
                            let weights = conv_weights(response, &lookups);
                            let n = weights.len().max(1);
                            let buf = conv_buf.entry(elem_id.clone()).or_default();
                            buf.push_front(x);
                            while buf.len() > n {
                                buf.pop_back();
                            }
                            let val: f64 = buf.iter().zip(weights.iter()).map(|(b, w)| b * w).sum();
                            Value::Scalar(val)
                        }
                        NodeRule::GateLogic { root, .. } => {
                            let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
                            Value::Scalar(if eval_gate(root, &ctx)? { 1.0 } else { 0.0 })
                        }
                        _ => eval_element(
                            elem, &lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx,
                            &rv_samples, &sp_state, &stock_state,
                        )?,
                    },
                    Primitive::Gate(g) => {
                        let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
                        Value::Scalar(if eval_gate(&g.root, &ctx)? { 1.0 } else { 0.0 })
                    }
                    // Links and events are resolved in their own passes below; placeholder here.
                    Primitive::Link(_) | Primitive::Event(_) => {
                        prev_outputs.get(elem_id.as_str()).cloned().unwrap_or(Value::Scalar(0.0))
                    }
                    _ => eval_element(
                        elem, &lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx,
                        &rv_samples, &sp_state, &stock_state,
                    )?,
                };
                outputs.insert(elem_id.clone(), value);
            }

            // ── Link transfers: move quantity source→target, with priority allocation,
            // transit buffering (plug flow), first-order decay, and scheduling. Stocks lose at
            // entry and gain at release; in-transit mass is conserved in the link buffer. ──
            let mut link_delta: HashMap<String, f64> = HashMap::new();
            #[allow(clippy::type_complexity)]
            let mut link_reqs: Vec<(String, Option<String>, Option<String>, i64, f64, usize, f64)> =
                Vec::new();
            for elem in &model.elements {
                if let Primitive::Link(l) = &elem.primitive {
                    let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
                    let fires = match &l.schedule {
                        Some(t) => trigger_fires(t, &ctx, dt, step_idx)?,
                        None => true,
                    };
                    let requested = if !fires {
                        0.0
                    } else if let Some(rate) = &l.rate {
                        (eval_qof_value(rate, &ctx)?.as_scalar() * dt).max(0.0)
                    } else if let Some(frac) = &l.fraction {
                        let src_val = l.source.as_ref()
                            .and_then(|s| outputs.get(s)).map(|v| v.as_scalar()).unwrap_or(0.0);
                        (eval_qof_value(frac, &ctx)?.as_scalar() * src_val).max(0.0)
                    } else {
                        0.0
                    };
                    let transit_steps = l.transit_time.as_ref()
                        .map(|q| (q.value / dt).round().max(0.0) as usize).unwrap_or(0);
                    let decay_factor = match (&l.decay_rate, &l.transit_time) {
                        (Some(dr), Some(tt)) => (-eval_qof_value(dr, &ctx)?.as_scalar() * tt.value).exp(),
                        _ => 1.0,
                    };
                    link_reqs.push((elem.id().to_string(), l.source.clone(), l.target.clone(),
                        l.priority.unwrap_or(i64::MAX), requested, transit_steps, decay_factor));
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
            for (id, source, target, _prio, requested, transit_steps, decay_factor) in &link_reqs {
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
                let entered = alloc * decay_factor;
                let delivered = if *transit_steps > 0 {
                    let buf = link_buf.entry(id.clone()).or_default();
                    buf.push_back((step_idx + transit_steps, entered));
                    let mut released = 0.0;
                    while let Some(&(rel, amt)) = buf.front() {
                        if rel <= step_idx {
                            released += amt;
                            buf.pop_front();
                        } else {
                            break;
                        }
                    }
                    released
                } else {
                    entered
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
            for elem in &model.elements {
                if let Primitive::Event(ev) = &elem.primitive {
                    let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
                    // Trait rate_generation: Poisson occurrences; else a single trigger firing.
                    let count: f64 = if let Some(rate) = &ev.rate {
                        let lambda = (eval_qof_value(rate, &ctx)?.as_scalar() * dt).max(0.0);
                        poisson_count(lambda, &mut rng) as f64
                    } else {
                        match &ev.trigger {
                            Some(t) => if trigger_fires(t, &ctx, dt, step_idx)? { 1.0 } else { 0.0 },
                            None => 0.0,
                        }
                    };
                    event_out.push((elem.id().to_string(), count));
                    if count <= 0.0 {
                        continue;
                    }
                    for effect in &ev.effects {
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
                                if is_stock {
                                    stock_event.entry(target).or_insert((0.0, 1.0, None)).0 += change * count;
                                } else {
                                    let cur = outputs.get(&target).map(|v| v.as_scalar()).unwrap_or(0.0);
                                    node_effects.push((target, Value::Scalar(cur + change * count)));
                                }
                            }
                            EffectMode::Multiplicative => {
                                let factor = change.powf(count);
                                if is_stock {
                                    stock_event.entry(target).or_insert((0.0, 1.0, None)).1 *= factor;
                                } else {
                                    let cur = outputs.get(&target).map(|v| v.as_scalar()).unwrap_or(0.0);
                                    node_effects.push((target, Value::Scalar(cur * factor)));
                                }
                            }
                            EffectMode::Replace => {
                                if is_stock {
                                    stock_event.entry(target).or_insert((0.0, 1.0, None)).2 = Some(change);
                                } else {
                                    node_effects.push((target, Value::Scalar(change)));
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

            // Stock integration pass. Computes each stock's next level (with traits) but
            // defers writing into `outputs` so a single shared ctx can borrow it.
            let mut next_vals: HashMap<String, Value> = HashMap::new();
            let mut cap_vals: HashMap<String, f64> = HashMap::new();
            let mut overflow_in: HashMap<String, f64> = HashMap::new();
            let mut withdrawal_allocs: Vec<(String, f64)> = Vec::new();
            for &id in &stock_ids {
                let Primitive::Stock(s) = &model.elements[elem_idx[id]].primitive else { continue };
                let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
                let current = stock_state[id].clone();

                // Trait priority_withdrawal: allocate available stock by priority. `request`/
                // `limit` are rates (amount = rate·dt); each target outputs its allocation.
                let mut withdrawal_outflow = 0.0;
                if !s.withdrawals.is_empty() {
                    let floor = s.floor.as_ref().map(|q| q.value).unwrap_or(0.0);
                    let mut available = (current.as_scalar() - floor).max(0.0);
                    let mut ws: Vec<&WithdrawalSpec> = s.withdrawals.iter().collect();
                    ws.sort_by_key(|w| w.priority.unwrap_or(i64::MAX));
                    for w in ws {
                        let mut amount = match &w.request {
                            Some(q) => (eval_qof_value(q, &ctx)?.as_scalar() * dt).max(0.0),
                            None => 0.0,
                        };
                        if let Some(lim) = &w.limit {
                            amount = amount.min((eval_qof_value(lim, &ctx)?.as_scalar() * dt).max(0.0));
                        }
                        let alloc = amount.min(available);
                        available -= alloc;
                        withdrawal_outflow += alloc;
                        withdrawal_allocs.push((w.target.clone(), alloc));
                    }
                }

                // External (non-return) flow: explicit rate, else Σinflows − Σoutflows.
                let external = match &s.rate {
                    Some(qof) => eval_qof_value(qof, &ctx)?,
                    None => {
                        let infl: f64 = s.inflows.iter()
                            .map(|i| outputs.get(i).map(|v| v.as_scalar()).unwrap_or(0.0)).sum();
                        let outf: f64 = s.outflows.iter()
                            .map(|o| outputs.get(o).map(|v| v.as_scalar()).unwrap_or(0.0)).sum();
                        Value::Scalar(infl - outf)
                    }
                };
                // Trait compound_growth: multiplicative self-referential return term.
                let mut next = if let Some(rr_qof) = &s.return_rate {
                    let rr = eval_qof_value(rr_qof, &ctx)?.as_scalar();
                    current.zip_with(external, move |c, e| {
                        let e = if e.is_nan() { 0.0 } else { e };
                        c * (1.0 + rr * dt) + e * dt
                    })
                } else {
                    current.zip_with(external, |c, r| if r.is_nan() { c } else { c + r * dt })
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
                if let Some(floor) = &s.floor {
                    let lo = floor.value;
                    next = next.map(|v| v.max(lo));
                }
                // Trait capacity_clamp (+ overflow_routing): clamp to capacity, route excess.
                if let Some(cap) = &s.capacity {
                    let cap_val = eval_qof_value(cap, &ctx)?.as_scalar();
                    cap_vals.insert(id.to_string(), cap_val);
                    let cur = next.as_scalar();
                    if cur > cap_val {
                        let excess = cur - cap_val;
                        next = next.map(|v| v.min(cap_val));
                        if let Some(target) = &s.overflow_target {
                            *overflow_in.entry(target.clone()).or_default() += excess;
                        }
                    }
                }
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

            for d in &model.time_history_displays {
                let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
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

            prev_outputs = outputs;
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
        results_map.insert(id.to_string(), ElementResults {
            label: elem.base.name.clone(),
            unit: primary_unit(elem).to_string(),
            final_values,
            time_history,
        });
    }

    for d in &model.time_history_displays {
        let final_values = final_store.get(&d.id).cloned().unwrap_or_default();
        results_map.insert(d.id.clone(), ElementResults {
            label: d.name.clone(),
            unit: "1".to_string(),
            final_values,
            time_history: Some(stats(&hist_store[&d.id])),
        });
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

    let display_ids: Vec<String> = model.time_history_displays.iter().map(|d| d.id.clone()).collect();
    let output_ids: Vec<String> = display_ids
        .into_iter()
        .chain(sinks.iter().chain(intermediates.iter()).map(|&s| s.to_string()))
        .collect();

    Ok(SimulationResults { time_axis, elements: results_map, n_realizations: n_real, n_steps, output_ids })
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
                let v = prev_outputs
                    .get(input.as_str())
                    .map(|v| v.as_scalar())
                    .unwrap_or_else(|| initial.as_ref().map(|q| q.value).unwrap_or(0.0));
                Ok(Value::Scalar(v))
            }
            NodeRule::Expression(ef) => {
                let ctx = ctx_at(lookups, outputs, prev_outputs, elapsed, dt, dt_unit, step_idx);
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

fn ctx_at<'a>(
    lookups: &'a HashMap<String, LookupData>,
    outputs: &'a HashMap<String, Value>,
    prev_outputs: &'a HashMap<String, Value>,
    elapsed: f64,
    dt: f64,
    dt_unit: &'a str,
    step_index: usize,
) -> EvalCtx<'a> {
    EvalCtx { lookups, outputs, prev_outputs, elapsed, dt, dt_unit, step_index }
}

fn dist_ctx_eval<'a>(
    lookups: &'a HashMap<String, LookupData>,
    outputs: &'a HashMap<String, Value>,
    prev_outputs: &'a HashMap<String, Value>,
    dt: f64,
    dt_unit: &'a str,
) -> EvalCtx<'a> {
    EvalCtx { lookups, outputs, prev_outputs, elapsed: 0.0, dt, dt_unit, step_index: 0 }
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
        // External-event triggers require the event primitive (M3).
        TriggerMode::OnEvent => false,
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

/// Convolution response weights: inline values, or the y-column of a referenced lookup.
fn conv_weights(response: &ConvResponse, lookups: &HashMap<String, LookupData>) -> Vec<f64> {
    match response {
        ConvResponse::Inline { values, .. } => values.clone(),
        ConvResponse::Ref(id) => lookups
            .get(id)
            .map(|l| if !l.y.is_empty() { l.y.clone() } else { l.columns.first().cloned().unwrap_or_default() })
            .unwrap_or_default(),
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
                return Some((e.id().to_string(), LookupData {
                    x: t.x.clone(),
                    y: t.y.clone(),
                    columns: t.z.clone(),
                    extrapolation: t.extrapolation.clone(),
                }));
            }
        }
        None
    }).collect()
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
fn should_save_history(elem: &Element) -> bool {
    elem.base.save_results.time_history.unwrap_or_else(|| !is_fixed_scalar(elem))
}
fn should_save_final(elem: &Element) -> bool {
    elem.base.save_results.final_value.unwrap_or_else(|| !is_fixed_scalar(elem))
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

/// Iman-Conover rank correlation. For each group, draw independent marginals for all
/// realizations, build a score matrix with the target rank structure, then reorder each
/// marginal to match — inducing the target rank correlation while preserving marginals.
/// Returns, per correlated element id, its reordered samples (one per realization).
fn iman_conover_samples(
    model: &Model,
    groups: &[CorrGroup],
    n_real: u32,
    seed: u64,
    lookups: &HashMap<String, LookupData>,
    dt: f64,
    dt_unit: &str,
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
            let ctx = dist_ctx_eval(lookups, &dist_ctx, &empty, dt, dt_unit);
            let resolved = resolve_distribution(distribution, &ctx)?;
            let col: Result<Vec<f64>, _> =
                (0..k).map(|_| sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)).collect();
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
