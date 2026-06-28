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

use std::collections::{HashMap, HashSet};

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
use crate::model_v2::{Element, FixedValue, Model, NodeRule, Primitive};
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

        // Gaussian copula for correlated groups.
        for group in &corr_groups {
            let n = group.ids.len();
            let std_normal = rand_distr::Normal::new(0.0_f64, 1.0_f64)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            let z_iid: Vec<f64> = (0..n).map(|_| rng.sample(std_normal)).collect();
            let z_corr = cholesky_matvec(&group.chol_l, &z_iid);
            for (i, id) in group.ids.iter().enumerate() {
                let elem = &model.elements[elem_idx[id.as_str()]];
                if let Primitive::Node(node) = &elem.primitive {
                    if let NodeRule::Sample { distribution, .. } = &node.rule {
                        let ctx = dist_ctx_eval(&lookups, &dist_ctx, &empty_prev, dt, &dt_unit);
                        let resolved = resolve_distribution(distribution, &ctx)?;
                        let u = sampling::standard_normal_cdf(z_corr[i]);
                        let v = match sampling::icdf(&resolved.kind, u) {
                            Some(raw) => {
                                let lo = resolved.truncation.as_ref().and_then(|t| t.min);
                                let hi = resolved.truncation.as_ref().and_then(|t| t.max);
                                raw.max(lo.unwrap_or(f64::NEG_INFINITY)).min(hi.unwrap_or(f64::INFINITY))
                            }
                            None => sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)?,
                        };
                        rv_samples.insert(id.clone(), v);
                        dist_ctx.insert(id.clone(), Value::Scalar(v));
                    }
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

            let mut outputs: HashMap<String, Value> = HashMap::new();

            for elem_id in &graph.topo_order {
                let elem = &model.elements[elem_idx[elem_id.as_str()]];
                let value = eval_element(
                    elem, &lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx,
                    &rv_samples, &sp_state, &stock_state,
                )?;
                outputs.insert(elem_id.clone(), value);
            }

            // Stock integration: S_{t+1} = clamp(S_t + net_rate * dt).
            for &id in &stock_ids {
                if let Primitive::Stock(s) = &model.elements[elem_idx[id]].primitive {
                    let ctx = ctx_at(&lookups, &outputs, &prev_outputs, elapsed, dt, &dt_unit, step_idx);
                    let rate_val = match &s.rate {
                        Some(qof) => eval_qof_value(qof, &ctx)?,
                        None => {
                            let infl: f64 = s.inflows.iter()
                                .map(|i| outputs.get(i).map(|v| v.as_scalar()).unwrap_or(0.0)).sum();
                            let outf: f64 = s.outflows.iter()
                                .map(|o| outputs.get(o).map(|v| v.as_scalar()).unwrap_or(0.0)).sum();
                            Value::Scalar(infl - outf)
                        }
                    };
                    let current = stock_state[id].clone();
                    let mut next = current.zip_with(rate_val, |c, r| if r.is_nan() { c } else { c + r * dt });
                    if let Some(floor) = &s.floor {
                        let lo = floor.value;
                        next = next.map(|v| v.max(lo));
                    }
                    if let Some(cap) = &s.capacity {
                        let cap_val = eval_qof_value(cap, &ctx)?.as_scalar();
                        next = next.map(|v| v.min(cap_val));
                    }
                    stock_state.insert(id.to_string(), next);
                }
            }
            // End-of-step semantics: recorded value reflects the post-update level.
            for &id in &stock_ids {
                if let Some(v) = stock_state.get(id) {
                    outputs.insert(id.to_string(), v.clone());
                }
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
