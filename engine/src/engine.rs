use std::collections::{HashMap, VecDeque};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::error::EngineError;
use crate::eval::{eval_ast, EvalCtx};
use crate::graph::ModelGraph;
use crate::model::{ElementKind, InterpolationMethod, WasimModel};
use crate::sampling;

// ── Run config ────────────────────────────────────────────────────────────────

pub struct RunConfig {
    /// Override model's n_realizations.
    pub n_realizations: Option<u32>,
    /// Override model's seed. If neither is set, defaults to 0.
    pub seed: Option<u64>,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig { n_realizations: None, seed: None }
    }
}

// ── Results ───────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct SimulationResults {
    /// Time axis in declared timestep units. Length = n_steps.
    pub time_axis: Vec<f64>,
    pub elements: HashMap<String, ElementResults>,
    pub n_realizations: u32,
    pub n_steps: usize,
    /// Element IDs in display order: sinks (unreferenced outputs) first, then
    /// intermediates, all in topological evaluation order.
    pub output_ids: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct ElementResults {
    pub label: String,
    pub unit: String,
    /// One value per realization (saved if save_results.final_value).
    pub final_values: Vec<f64>,
    /// Per-timestep summary stats (saved if save_results.time_history).
    pub time_history: Option<TimeHistoryStats>,
}

#[derive(serde::Serialize)]
pub struct TimeHistoryStats {
    pub mean: Vec<f64>,
    pub p05: Vec<f64>,
    pub p25: Vec<f64>,
    pub p50: Vec<f64>,
    pub p75: Vec<f64>,
    pub p95: Vec<f64>,
}

// ── Main entry point ──────────────────────────────────────────────────────────

pub fn run(
    model: &WasimModel,
    graph: &ModelGraph,
    config: &RunConfig,
) -> Result<SimulationResults, EngineError> {
    let n_real = config.n_realizations.unwrap_or(model.simulation_settings.n_realizations);
    let seed = config.seed
        .or(model.simulation_settings.seed)
        .unwrap_or(0);

    let dt = model.simulation_settings.timestep.value;
    let duration = model.simulation_settings.duration.value;
    let n_steps = (duration / dt).round() as usize;

    // Build lookup from id → element index for fast access
    let elem_idx: HashMap<&str, usize> = model
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.as_str(), i))
        .collect();

    // Identify which elements need saved results
    let save_final: Vec<&str> = model.elements.iter()
        .filter(|e| e.should_save_final())
        .map(|e| e.id.as_str())
        .collect();
    let save_hist: Vec<&str> = model.elements.iter()
        .filter(|e| e.should_save_history())
        .map(|e| e.id.as_str())
        .collect();

    // Accumulators (need state carried across timesteps)
    let acc_ids: Vec<&str> = model.elements.iter()
        .filter(|e| matches!(e.kind, ElementKind::Accumulator { .. }))
        .map(|e| e.id.as_str())
        .collect();

    // Delay elements
    let delay_ids: Vec<&str> = model.elements.iter()
        .filter(|e| matches!(e.kind, ElementKind::Delay { .. }))
        .map(|e| e.id.as_str())
        .collect();

    // Storage: final_values[element_id][realization]
    let mut final_store: HashMap<String, Vec<f64>> = save_final
        .iter()
        .map(|&id| (id.to_string(), Vec::with_capacity(n_real as usize)))
        .collect();

    // Storage: hist_store[element_id][step][realization]
    let mut hist_store: HashMap<String, Vec<Vec<f64>>> = save_hist
        .iter()
        .map(|&id| (id.to_string(), vec![Vec::new(); n_steps]))
        .collect();

    // ── Realization loop ──────────────────────────────────────────────────────
    for real_idx in 0..n_real {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        rng.set_stream(real_idx as u64);

        // Sample all random variables once per realization
        let mut rv_samples: HashMap<String, f64> = HashMap::new();
        for elem in &model.elements {
            if let ElementKind::RandomVariable { distribution } = &elem.kind {
                let v = sampling::sample(&distribution.kind, &distribution.truncation, &mut rng)?;
                rv_samples.insert(elem.id.clone(), v);
            }
        }

        // Build a t=0 snapshot for initial_expression evaluation:
        // seed with constants and RV samples, then evaluate expressions in topo order.
        let empty_map: HashMap<String, f64> = HashMap::new();
        let mut init_ctx_outputs: HashMap<String, f64> = HashMap::new();
        for elem in &model.elements {
            match &elem.kind {
                ElementKind::Constant { value, .. } => { init_ctx_outputs.insert(elem.id.clone(), value.value); }
                ElementKind::RandomVariable { .. } => { init_ctx_outputs.insert(elem.id.clone(), rv_samples[&elem.id]); }
                ElementKind::Accumulator { initial_value, .. } => { init_ctx_outputs.insert(elem.id.clone(), initial_value.value); }
                _ => {}
            }
        }
        for elem_id in &graph.topo_order {
            let elem = &model.elements[elem_idx[elem_id.as_str()]];
            if let ElementKind::Expression { expression, .. } = &elem.kind {
                let ctx = EvalCtx { model, outputs: &init_ctx_outputs, prev_outputs: &empty_map, elapsed: 0.0, dt, step_index: 0 };
                if let Ok(v) = eval_ast(&expression.ast, &ctx) {
                    init_ctx_outputs.insert(elem_id.clone(), v);
                }
            }
        }

        // Initialize accumulator states (use initial_expression if present, else scalar initial_value)
        let mut acc_state: HashMap<String, f64> = HashMap::new();
        for &id in &acc_ids {
            let elem = &model.elements[elem_idx[id]];
            if let ElementKind::Accumulator { initial_value, initial_expression, .. } = &elem.kind {
                let init = match initial_expression {
                    Some(expr) => {
                        let ctx = EvalCtx { model, outputs: &init_ctx_outputs, prev_outputs: &empty_map, elapsed: 0.0, dt, step_index: 0 };
                        eval_ast(&expr.ast, &ctx)?
                    }
                    None => initial_value.value,
                };
                acc_state.insert(id.to_string(), init);
            }
        }

        // Initialize delay buffers: delay_buf[id] = ring of past values, front = most recent
        let mut delay_buf: HashMap<String, VecDeque<f64>> = HashMap::new();
        for &id in &delay_ids {
            let elem = &model.elements[elem_idx[id]];
            if let ElementKind::Delay { lag, initial, .. } = &elem.kind {
                let lag_steps = (lag.value / dt).round() as usize;
                let init_val = initial.as_ref().map(|q| q.value).unwrap_or(0.0);
                let buf: VecDeque<f64> = std::iter::repeat(init_val).take(lag_steps + 1).collect();
                delay_buf.insert(id.to_string(), buf);
            }
        }

        let mut prev_outputs: HashMap<String, f64> = HashMap::new();

        // ── Timestep loop ─────────────────────────────────────────────────────
        for step_idx in 0..n_steps {
            let elapsed = step_idx as f64 * dt;
            let mut outputs: HashMap<String, f64> = HashMap::new();

            // Evaluate elements in topological order
            for elem_id in &graph.topo_order {
                let elem = &model.elements[elem_idx[elem_id.as_str()]];

                let value = match &elem.kind {
                    ElementKind::Constant { value, .. } => value.value,

                    ElementKind::RandomVariable { .. } => rv_samples[elem_id],

                    ElementKind::Accumulator { .. } => {
                        // Provide stored state as current output
                        acc_state[elem_id]
                    }

                    ElementKind::Timeseries { times, values, interpolation, .. } => {
                        eval_timeseries(times, values, interpolation, elapsed)?
                    }

                    ElementKind::Lookup { .. } => {
                        // Lookup elements are not directly evaluated; accessed via lookup_call.
                        0.0
                    }

                    ElementKind::Delay { .. } => {
                        // Return the oldest value in the delay buffer (= lag steps ago)
                        delay_buf.get(elem_id)
                            .and_then(|buf| buf.back().copied())
                            .unwrap_or(0.0)
                    }

                    ElementKind::Expression { expression, .. } => {
                        let ctx = EvalCtx {
                            model,
                            outputs: &outputs,
                            prev_outputs: &prev_outputs,
                            elapsed,
                            dt,
                            step_index: step_idx,
                        };
                        eval_ast(&expression.ast, &ctx)?
                    }

                    ElementKind::Script { .. } => {
                        return Err(EngineError::Unsupported("script".into()));
                    }
                };

                outputs.insert(elem_id.clone(), value);
            }

            // Update accumulator states: state[t+1] = clamp(state[t] + rate * dt)
            for &id in &acc_ids {
                let elem = &model.elements[elem_idx[id]];
                if let ElementKind::Accumulator { rate, min_value, capacity, .. } = &elem.kind {
                    let ctx = EvalCtx {
                        model,
                        outputs: &outputs,
                        prev_outputs: &prev_outputs,
                        elapsed,
                        dt,
                        step_index: step_idx,
                    };
                    let rate_val = eval_ast(&rate.ast, &ctx)?;
                    let current = acc_state[id];
                    // NaN rate (e.g. 0/0 from transpiler unit-label refs) → no change this step.
                    let mut next = if rate_val.is_nan() { current } else { current + rate_val * dt };
                    if let Some(lo) = min_value {
                        next = next.max(*lo);
                    }
                    if let Some(cap) = capacity {
                        next = next.min(cap.value);
                    }
                    acc_state.insert(id.to_string(), next);
                }
            }

            // Propagate updated accumulator states back into outputs so that recorded
            // values reflect the post-update state (end-of-step semantics).
            for &id in &acc_ids {
                if let Some(&v) = acc_state.get(id) {
                    outputs.insert(id.to_string(), v);
                }
            }

            // Advance delay buffers
            for &id in &delay_ids {
                let elem = &model.elements[elem_idx[id]];
                if let ElementKind::Delay { input, lag, .. } = &elem.kind {
                    let v = outputs.get(input.as_str()).copied().unwrap_or(0.0);
                    let buf = delay_buf.entry(id.to_string()).or_default();
                    buf.push_front(v);
                    let lag_steps = (lag.value / dt).round() as usize;
                    while buf.len() > lag_steps + 1 {
                        buf.pop_back();
                    }
                }
            }

            // Record time histories (post-update)
            for &id in &save_hist {
                if let Some(v) = outputs.get(id) {
                    hist_store.get_mut(id).unwrap()[step_idx].push(*v);
                }
            }

            // Capture final-step values (last step, post-update)
            if step_idx == n_steps - 1 {
                for &id in &save_final {
                    if let Some(v) = outputs.get(id) {
                        final_store.get_mut(id).unwrap().push(*v);
                    }
                }
            }

            prev_outputs = outputs;
        }
    }

    // ── Aggregate results ─────────────────────────────────────────────────────
    let time_axis: Vec<f64> = (0..n_steps).map(|i| i as f64 * dt).collect();
    let mut results_map: HashMap<String, ElementResults> = HashMap::new();

    for elem in &model.elements {
        let id = &elem.id;
        let has_final = save_final.contains(&id.as_str());
        let has_hist = save_hist.contains(&id.as_str());
        if !has_final && !has_hist {
            continue;
        }

        let final_values = final_store.get(id).cloned().unwrap_or_default();

        let time_history = if has_hist {
            let per_step = &hist_store[id];
            Some(TimeHistoryStats {
                mean: per_step.iter().map(|vs| mean(vs)).collect(),
                p05: per_step.iter().map(|vs| percentile(vs, 5.0)).collect(),
                p25: per_step.iter().map(|vs| percentile(vs, 25.0)).collect(),
                p50: per_step.iter().map(|vs| percentile(vs, 50.0)).collect(),
                p75: per_step.iter().map(|vs| percentile(vs, 75.0)).collect(),
                p95: per_step.iter().map(|vs| percentile(vs, 95.0)).collect(),
            })
        } else {
            None
        };

        results_map.insert(id.clone(), ElementResults {
            label: elem.name.clone(),
            unit: elem.primary_unit().to_string(),
            final_values,
            time_history,
        });
    }

    // Compute display order: sinks (unreferenced by anyone) first, then the rest,
    // all in topo order, restricted to elements that actually have results.
    let referenced: std::collections::HashSet<&str> = model.elements.iter()
        .flat_map(|e| match &e.kind {
            ElementKind::Expression { inputs, .. } | ElementKind::Accumulator { inputs, .. } => {
                inputs.iter().map(String::as_str).collect::<Vec<_>>()
            }
            _ => vec![],
        })
        .collect();

    let (sinks, intermediates): (Vec<&str>, Vec<&str>) = graph.topo_order.iter()
        .map(String::as_str)
        .filter(|id| results_map.contains_key(*id))
        .partition(|id| !referenced.contains(id));

    let output_ids: Vec<String> = sinks.iter().chain(intermediates.iter())
        .map(|&s| s.to_string())
        .collect();

    Ok(SimulationResults {
        time_axis,
        elements: results_map,
        n_realizations: n_real,
        n_steps,
        output_ids,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn eval_timeseries(
    times: &[f64],
    values: &[f64],
    interpolation: &InterpolationMethod,
    elapsed: f64,
) -> Result<f64, EngineError> {
    if times.is_empty() {
        return Ok(0.0);
    }
    if elapsed <= times[0] {
        return Ok(values[0]);
    }
    if elapsed >= *times.last().unwrap() {
        return Ok(*values.last().unwrap());
    }

    let mut lo = 0;
    let mut hi = times.len() - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if times[mid] <= elapsed { lo = mid; } else { hi = mid; }
    }

    let v = match interpolation {
        InterpolationMethod::Step => values[lo],
        InterpolationMethod::Linear | InterpolationMethod::Cubic => {
            let t = (elapsed - times[lo]) / (times[hi] - times[lo]);
            values[lo] + t * (values[hi] - values[lo])
        }
    };
    Ok(v)
}

fn mean(vs: &[f64]) -> f64 {
    if vs.is_empty() { return 0.0; }
    vs.iter().sum::<f64>() / vs.len() as f64
}

fn percentile(vs: &[f64], p: f64) -> f64 {
    if vs.is_empty() { return 0.0; }
    let mut sorted = vs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
