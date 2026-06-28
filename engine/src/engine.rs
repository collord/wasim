use std::collections::{HashMap, VecDeque};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::error::EngineError;
use crate::eval::{eval_ast, eval_ast_scalar, resolve_distribution, EvalCtx, Value};
use crate::graph::ModelGraph;
use crate::model::{ElementKind, InterpolationMethod, WasimModel};
use crate::sampling;

// ── Run config ────────────────────────────────────────────────────────────────

pub struct RunConfig {
    /// Override model's n_realizations.
    pub n_realizations: Option<u32>,
    /// Override model's seed. If neither is set, defaults to 0.
    pub seed: Option<u64>,
    /// Override model's simulation duration (in the model's declared duration unit).
    pub duration_override: Option<f64>,
    /// Override model's timestep (in the model's declared timestep unit).
    pub timestep_override: Option<f64>,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig { n_realizations: None, seed: None, duration_override: None, timestep_override: None }
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

// ── Rank-correlation (Gaussian copula) ───────────────────────────────────────

struct CorrGroup {
    /// Element IDs ordered by their position in model.elements.
    ids: Vec<String>,
    /// Lower-triangular Cholesky factor of the group's correlation matrix (n × n).
    chol_l: Vec<Vec<f64>>,
}

/// Parse all `correlations` entries from RandomVariable elements, find connected
/// components, build a correlation matrix per component, and Cholesky-decompose it.
fn build_corr_groups(model: &WasimModel) -> Result<Vec<CorrGroup>, EngineError> {
    let elem_pos: HashMap<&str, usize> = model.elements.iter()
        .enumerate()
        .map(|(i, e)| (e.id.as_str(), i))
        .collect();

    let rv_set: std::collections::HashSet<&str> = model.elements.iter()
        .filter(|e| matches!(e.kind, ElementKind::RandomVariable { .. }))
        .map(|e| e.id.as_str())
        .collect();

    // Canonical edge map: key = (model-order-first id, model-order-second id) → Spearman ρ.
    // If both directions are specified, the first one encountered (by model order) wins.
    let mut edge_map: HashMap<(String, String), f64> = HashMap::new();
    for elem in &model.elements {
        if let ElementKind::RandomVariable { correlations, .. } = &elem.kind {
            for pair in correlations {
                if !rv_set.contains(pair.partner.as_str()) {
                    return Err(EngineError::ElementNotFound(pair.partner.clone()));
                }
                let a_pos = elem_pos[elem.id.as_str()];
                let b_pos = elem_pos[pair.partner.as_str()];
                let (lo, hi) = if a_pos < b_pos {
                    (elem.id.clone(), pair.partner.clone())
                } else {
                    (pair.partner.clone(), elem.id.clone())
                };
                edge_map.entry((lo, hi)).or_insert(pair.coefficient);
            }
        }
    }

    if edge_map.is_empty() {
        return Ok(vec![]);
    }

    // BFS to find connected components; seed order follows model element order.
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for ((a, b), _) in &edge_map {
        adj.entry(a.clone()).or_default().push(b.clone());
        adj.entry(b.clone()).or_default().push(a.clone());
    }

    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut components: Vec<Vec<String>> = Vec::new();

    for elem in &model.elements {
        let id = &elem.id;
        if !adj.contains_key(id.as_str()) || visited.contains(id) { continue; }
        let mut component = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(id.clone());
        visited.insert(id.clone());
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
        let id_idx: HashMap<&str, usize> = ids.iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();

        let mut matrix = vec![vec![0.0f64; n]; n];
        for i in 0..n { matrix[i][i] = 1.0; }
        for ((a, b), &rho) in &edge_map {
            if let (Some(&i), Some(&j)) = (id_idx.get(a.as_str()), id_idx.get(b.as_str())) {
                matrix[i][j] = rho;
                matrix[j][i] = rho;
            }
        }

        let chol_l = cholesky(&matrix).map_err(|_| EngineError::InvalidModel(format!(
            "rank-correlation matrix for [{}] is not positive semi-definite \
             (check that coefficients are mutually consistent)",
            ids.join(", ")
        )))?;
        groups.push(CorrGroup { ids, chol_l });
    }
    Ok(groups)
}

/// Cholesky–Banachiewicz decomposition: returns lower-triangular L such that A = L Lᵀ.
/// Returns Err if A is not positive semi-definite.
pub(crate) fn cholesky(matrix: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, ()> {
    let n = matrix.len();
    let mut l = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let sum: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                let d = matrix[i][i] - sum;
                if d < -1e-10 { return Err(()); }
                l[i][j] = d.max(0.0).sqrt();
            } else if l[j][j].abs() > 1e-12 {
                l[i][j] = (matrix[i][j] - sum) / l[j][j];
            }
        }
    }
    Ok(l)
}

/// Multiply lower-triangular L by vector z: out = L z.
pub(crate) fn cholesky_matvec(l: &[Vec<f64>], z: &[f64]) -> Vec<f64> {
    let n = l.len();
    let mut out = vec![0.0f64; n];
    for i in 0..n {
        for j in 0..=i {
            out[i] += l[i][j] * z[j];
        }
    }
    out
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

    let dt = config.timestep_override.unwrap_or(model.simulation_settings.timestep.value);
    let duration = config.duration_override.unwrap_or(model.simulation_settings.duration.value);
    if !dt.is_finite() || dt <= 0.0 {
        return Err(EngineError::InvalidModel(format!("timestep must be > 0, got {dt}")));
    }
    if !duration.is_finite() || duration <= 0.0 {
        return Err(EngineError::InvalidModel(format!("duration must be > 0, got {duration}")));
    }
    let n_steps = (duration / dt).round() as usize;

    // Lookup tables, extracted once for the AST walker (decoupled from WasimModel).
    let dt_unit = model.simulation_settings.timestep.unit.clone();
    let lookups: HashMap<String, crate::eval::LookupData> = model.elements.iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Lookup { x, y, columns, extrapolation, .. } => Some((
                e.id.clone(),
                crate::eval::LookupData {
                    x: x.clone(),
                    y: y.clone(),
                    columns: columns.clone(),
                    extrapolation: extrapolation.clone(),
                },
            )),
            _ => None,
        })
        .collect();

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

    // Stochastic process elements (re-sampled every timestep)
    let sp_ids: Vec<&str> = model.elements.iter()
        .filter(|e| matches!(e.kind, ElementKind::StochasticProcess { .. }))
        .map(|e| e.id.as_str())
        .collect();

    // Random variables with autocorrelation set are re-sampled every timestep
    // (one-shot RVs are sampled once at the start of each realization).
    let per_step_rv_ids: Vec<&str> = model.elements.iter()
        .filter(|e| matches!(&e.kind, ElementKind::RandomVariable { autocorrelation: Some(_), .. }))
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

    // time_history_displays piggyback on the same stores (always saved as full history).
    for d in &model.time_history_displays {
        final_store.insert(d.id.clone(), Vec::with_capacity(n_real as usize));
        hist_store.insert(d.id.clone(), vec![Vec::new(); n_steps]);
    }

    // Build rank-correlation groups once; IDs in these groups bypass independent sampling.
    let corr_groups = build_corr_groups(model)?;
    let corr_rv_ids: std::collections::HashSet<String> = corr_groups.iter()
        .flat_map(|g| g.ids.iter().cloned())
        .collect();

    // ── Realization loop ──────────────────────────────────────────────────────
    for real_idx in 0..n_real {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        rng.set_stream(real_idx as u64);

        // Sample independent random variables once per realization.
        // Correlated variables are handled below via the Gaussian copula.
        // `dist_ctx` accumulates scalar values visible to distribution-parameter ASTs:
        // constants up front, plus each RV's draw as soon as it's available, so later
        // RV params can reference earlier ones (document order).
        let mut rv_samples: HashMap<String, f64> = HashMap::new();
        let mut dist_ctx: HashMap<String, Value> = HashMap::new();
        for elem in &model.elements {
            if let ElementKind::Constant { value, .. } = &elem.kind {
                dist_ctx.insert(elem.id.clone(), Value::Scalar(value.value));
            }
        }
        let empty_prev: HashMap<String, Value> = HashMap::new();

        for elem in &model.elements {
            if let ElementKind::RandomVariable { distribution, .. } = &elem.kind {
                if !corr_rv_ids.contains(&elem.id) {
                    let ctx = EvalCtx { lookups: &lookups, dt_unit: &dt_unit, outputs: &dist_ctx, prev_outputs: &empty_prev, elapsed: 0.0, dt, step_index: 0 };
                    let resolved = resolve_distribution(distribution, &ctx)?;
                    let v = sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)?;
                    rv_samples.insert(elem.id.clone(), v);
                    dist_ctx.insert(elem.id.clone(), Value::Scalar(v));
                }
            }
        }

        // Gaussian copula for rank-correlated groups:
        //   1. Draw z_iid ~ N(0, I);  2. z_corr = L z_iid;
        //   3. u_i = Φ(z_corr[i]);   4. x_i = F_i⁻¹(u_i).
        // Distributions without a closed-form inverse CDF fall back to iid for that variable.
        for group in &corr_groups {
            let n = group.ids.len();
            let std_normal = rand_distr::Normal::new(0.0_f64, 1.0_f64)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            let z_iid: Vec<f64> = (0..n).map(|_| rng.sample(std_normal)).collect();
            let z_corr = cholesky_matvec(&group.chol_l, &z_iid);
            for (i, id) in group.ids.iter().enumerate() {
                let elem = &model.elements[elem_idx[id.as_str()]];
                if let ElementKind::RandomVariable { distribution, .. } = &elem.kind {
                    let ctx = EvalCtx { lookups: &lookups, dt_unit: &dt_unit, outputs: &dist_ctx, prev_outputs: &empty_prev, elapsed: 0.0, dt, step_index: 0 };
                    let resolved = resolve_distribution(distribution, &ctx)?;
                    let u = sampling::standard_normal_cdf(z_corr[i]);
                    let v = match sampling::icdf(&resolved.kind, u) {
                        Some(raw) => {
                            let lo = resolved.truncation.as_ref().and_then(|t| t.min);
                            let hi = resolved.truncation.as_ref().and_then(|t| t.max);
                            raw.max(lo.unwrap_or(f64::NEG_INFINITY))
                               .min(hi.unwrap_or(f64::INFINITY))
                        }
                        None => sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)?,
                    };
                    rv_samples.insert(id.clone(), v);
                    dist_ctx.insert(id.clone(), Value::Scalar(v));
                }
            }
        }

        // Initial draw for stochastic process elements (step 0 value).
        let mut sp_state: HashMap<String, f64> = HashMap::new();
        for &id in &sp_ids {
            let elem = &model.elements[elem_idx[id]];
            if let ElementKind::StochasticProcess { process, lower_bound } = &elem.kind {
                let v = sampling::sample_gbm(process, lower_bound.as_ref(), dt, &model.simulation_settings.timestep.unit, &mut rng)?;
                sp_state.insert(id.to_string(), v);
            }
        }

        // AR(1) standard-normal driver state for per-step random_variable elements.
        let mut z_state: HashMap<String, f64> = HashMap::new();
        for &id in &per_step_rv_ids {
            let z0: f64 = rng.sample(rand_distr::Normal::new(0.0_f64, 1.0_f64)
                .map_err(|e| crate::error::EngineError::Sampling(e.to_string()))?);
            z_state.insert(id.to_string(), z0);
        }

        // Build a t=0 snapshot for initial_expression evaluation:
        // seed with constants and RV samples, then evaluate expressions in topo order.
        let empty_map: HashMap<String, Value> = HashMap::new();
        let mut init_ctx_outputs: HashMap<String, Value> = HashMap::new();
        for elem in &model.elements {
            match &elem.kind {
                ElementKind::Constant { value, .. } => {
                    init_ctx_outputs.insert(elem.id.clone(), Value::Scalar(value.value));
                }
                ElementKind::RandomVariable { .. } => {
                    init_ctx_outputs.insert(elem.id.clone(), Value::Scalar(rv_samples[&elem.id]));
                }
                ElementKind::StochasticProcess { .. } => {
                    init_ctx_outputs.insert(elem.id.clone(), Value::Scalar(sp_state.get(&elem.id).copied().unwrap_or(0.0)));
                }
                ElementKind::Accumulator { initial_value, .. } => {
                    init_ctx_outputs.insert(elem.id.clone(), Value::Scalar(initial_value.value));
                }
                _ => {}
            }
        }
        for elem_id in &graph.topo_order {
            let elem = &model.elements[elem_idx[elem_id.as_str()]];
            if let ElementKind::Expression { expression, .. } = &elem.kind {
                let ctx = EvalCtx { lookups: &lookups, dt_unit: &dt_unit, outputs: &init_ctx_outputs, prev_outputs: &empty_map, elapsed: 0.0, dt, step_index: 0 };
                if let Ok(v) = eval_ast(&expression.ast, &ctx) {
                    init_ctx_outputs.insert(elem_id.clone(), v);
                }
            }
        }

        // Initialize accumulator states (use initial_expression if present, else scalar initial_value)
        let mut acc_state: HashMap<String, Value> = HashMap::new();
        for &id in &acc_ids {
            let elem = &model.elements[elem_idx[id]];
            if let ElementKind::Accumulator { initial_value, initial_expression, .. } = &elem.kind {
                let init = match initial_expression {
                    Some(expr) => {
                        let ctx = EvalCtx { lookups: &lookups, dt_unit: &dt_unit, outputs: &init_ctx_outputs, prev_outputs: &empty_map, elapsed: 0.0, dt, step_index: 0 };
                        eval_ast(&expr.ast, &ctx)?
                    }
                    None => Value::Scalar(initial_value.value),
                };
                acc_state.insert(id.to_string(), init);
            }
        }

        // Initialize delay buffers
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

        let mut prev_outputs: HashMap<String, Value> = HashMap::new();

        // ── Timestep loop ─────────────────────────────────────────────────────
        for step_idx in 0..n_steps {
            let elapsed = step_idx as f64 * dt;

            // Re-draw stochastic process elements for this timestep.
            for &id in &sp_ids {
                let elem = &model.elements[elem_idx[id]];
                if let ElementKind::StochasticProcess { process, lower_bound } = &elem.kind {
                    let v = sampling::sample_gbm(process, lower_bound.as_ref(), dt, &model.simulation_settings.timestep.unit, &mut rng)?;
                    sp_state.insert(id.to_string(), v);
                }
            }

            // Re-draw random_variable elements that opted into per-timestep sampling.
            for &id in &per_step_rv_ids {
                let elem = &model.elements[elem_idx[id]];
                if let ElementKind::RandomVariable { distribution, autocorrelation, .. } = &elem.kind {
                    let rho = autocorrelation.unwrap_or(0.0).clamp(0.0, 1.0);
                    let z_prev = z_state.get(id).copied().unwrap_or(0.0);
                    let (v, z_new) = sampling::sample_autocorr_step(
                        &distribution.kind, &distribution.truncation, rho, z_prev, &mut rng,
                    )?;
                    rv_samples.insert(id.to_string(), v);
                    z_state.insert(id.to_string(), z_new);
                }
            }

            let mut outputs: HashMap<String, Value> = HashMap::new();

            // Evaluate elements in topological order
            for elem_id in &graph.topo_order {
                let elem = &model.elements[elem_idx[elem_id.as_str()]];

                let value: Value = match &elem.kind {
                    ElementKind::Constant { value, .. } => Value::Scalar(value.value),

                    ElementKind::RandomVariable { .. } => Value::Scalar(rv_samples[elem_id]),

                    ElementKind::StochasticProcess { .. } => Value::Scalar(sp_state[elem_id]),

                    ElementKind::Accumulator { .. } => {
                        acc_state[elem_id].clone()
                    }

                    ElementKind::Timeseries { times, values, interpolation, .. } => {
                        Value::Scalar(eval_timeseries(times, values, interpolation, elapsed)?)
                    }

                    ElementKind::Lookup { .. } => {
                        // Lookup elements are accessed via LookupCall or Ref (which reads
                        // elem.kind directly in eval_ast). Placeholder value only.
                        Value::Scalar(0.0)
                    }

                    ElementKind::Delay { .. } => {
                        Value::Scalar(delay_buf.get(elem_id)
                            .and_then(|buf| buf.back().copied())
                            .unwrap_or(0.0))
                    }

                    ElementKind::Expression { expression, .. } => {
                        let ctx = EvalCtx {
                            lookups: &lookups, dt_unit: &dt_unit,
                            outputs: &outputs,
                            prev_outputs: &prev_outputs,
                            elapsed,
                            dt,
                            step_index: step_idx,
                        };
                        eval_ast(&expression.ast, &ctx)?
                    }

                    ElementKind::Script { expressions, procedural, .. } => {
                        match expressions.first() {
                            None => Value::Scalar(0.0),
                            Some(ef) => {
                                if *procedural {
                                    eprintln!("warn: {elem_id} has procedural control flow; only expressions[0] evaluated");
                                }
                                let ctx = EvalCtx { lookups: &lookups, dt_unit: &dt_unit, outputs: &outputs, prev_outputs: &prev_outputs, elapsed, dt, step_index: step_idx };
                                eval_ast(&ef.ast, &ctx)?
                            }
                        }
                    }

                    ElementKind::Array { mode, expressions, values, .. } => {
                        // Branch on the 0.2.0 sub-discriminator; fall back to a field-presence
                        // heuristic for pre-0.2.0 models that lack `mode`.
                        let is_expression = match mode {
                            Some(crate::model::ArrayMode::Expression) => true,
                            Some(crate::model::ArrayMode::Constant) => false,
                            None => !expressions.is_empty(),
                        };
                        if is_expression {
                            let ctx = EvalCtx {
                                lookups: &lookups, dt_unit: &dt_unit,
                                outputs: &outputs,
                                prev_outputs: &prev_outputs,
                                elapsed,
                                dt,
                                step_index: step_idx,
                            };
                            let vals: Result<Vec<f64>, _> = expressions.iter()
                                .map(|expr| eval_ast_scalar(&expr.ast, &ctx))
                                .collect();
                            Value::Vector(vals?)
                        } else {
                            // Constant-values form (or extraction_pending — empty vec is fine).
                            Value::Vector(values.clone())
                        }
                    }
                };

                outputs.insert(elem_id.clone(), value);
            }

            // Update accumulator states: state[t+1] = clamp(state[t] + rate * dt)
            for &id in &acc_ids {
                let elem = &model.elements[elem_idx[id]];
                if let ElementKind::Accumulator { rate, min_value, capacity, .. } = &elem.kind {
                    let ctx = EvalCtx {
                        lookups: &lookups, dt_unit: &dt_unit,
                        outputs: &outputs,
                        prev_outputs: &prev_outputs,
                        elapsed,
                        dt,
                        step_index: step_idx,
                    };
                    let rate_val = eval_ast(&rate.ast, &ctx)?;
                    let current = acc_state[id].clone();
                    // NaN rate → no change this step; otherwise euler step.
                    let mut next = current.zip_with(rate_val, |c, r| if r.is_nan() { c } else { c + r * dt });
                    if let Some(lo) = min_value {
                        let lo = *lo;
                        next = next.map(|v| v.max(lo));
                    }
                    if let Some(cap) = capacity {
                        let cap_val = cap.value;
                        next = next.map(|v| v.min(cap_val));
                    }
                    acc_state.insert(id.to_string(), next);
                }
            }

            // Propagate updated accumulator states back into outputs so that recorded
            // values reflect the post-update state (end-of-step semantics).
            for &id in &acc_ids {
                if let Some(v) = acc_state.get(id) {
                    outputs.insert(id.to_string(), v.clone());
                }
            }

            // Advance delay buffers
            for &id in &delay_ids {
                let elem = &model.elements[elem_idx[id]];
                if let ElementKind::Delay { input, lag, .. } = &elem.kind {
                    let v = outputs.get(input.as_str()).map(|v| v.as_scalar()).unwrap_or(0.0);
                    let buf = delay_buf.entry(id.to_string()).or_default();
                    buf.push_front(v);
                    let lag_steps = (lag.value / dt).round() as usize;
                    while buf.len() > lag_steps + 1 {
                        buf.pop_back();
                    }
                }
            }

            // Evaluate time_history_displays against the finalized step outputs.
            for d in &model.time_history_displays {
                let ctx = EvalCtx { lookups: &lookups, dt_unit: &dt_unit, outputs: &outputs, prev_outputs: &prev_outputs, elapsed, dt, step_index: step_idx };
                let v = eval_ast(&d.expression.ast, &ctx)?.as_scalar();
                hist_store.get_mut(&d.id).unwrap()[step_idx].push(v);
                if step_idx == n_steps - 1 {
                    final_store.get_mut(&d.id).unwrap().push(v);
                }
            }

            // Record time histories (post-update); collapse vectors to scalar.
            for &id in &save_hist {
                if let Some(v) = outputs.get(id) {
                    hist_store.get_mut(id).unwrap()[step_idx].push(v.as_scalar());
                }
            }

            // Capture final-step values (last step, post-update)
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

    // Surface time_history_displays as result entries (full history + final values).
    for d in &model.time_history_displays {
        let final_values = final_store.get(&d.id).cloned().unwrap_or_default();
        let per_step = &hist_store[&d.id];
        let time_history = Some(TimeHistoryStats {
            mean: per_step.iter().map(|vs| mean(vs)).collect(),
            p05: per_step.iter().map(|vs| percentile(vs, 5.0)).collect(),
            p25: per_step.iter().map(|vs| percentile(vs, 25.0)).collect(),
            p50: per_step.iter().map(|vs| percentile(vs, 50.0)).collect(),
            p75: per_step.iter().map(|vs| percentile(vs, 75.0)).collect(),
            p95: per_step.iter().map(|vs| percentile(vs, 95.0)).collect(),
        });
        results_map.insert(d.id.clone(), ElementResults {
            label: d.name.clone(),
            unit: "1".to_string(),
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

    // time_history_displays come first (primary user-visible outputs), then sinks, then intermediates.
    let display_ids: Vec<String> = model.time_history_displays.iter().map(|d| d.id.clone()).collect();
    let output_ids: Vec<String> = display_ids.into_iter()
        .chain(sinks.iter().chain(intermediates.iter()).map(|&s| s.to_string()))
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

pub(crate) fn eval_timeseries(
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

pub(crate) fn mean(vs: &[f64]) -> f64 {
    if vs.is_empty() { return 0.0; }
    vs.iter().sum::<f64>() / vs.len() as f64
}

pub(crate) fn percentile(vs: &[f64], p: f64) -> f64 {
    if vs.is_empty() { return 0.0; }
    let mut sorted = vs.to_vec();
    // total_cmp gives a total order over all f64 (including NaN), so a diverging
    // realization that produced NaN can't panic the sort.
    sorted.sort_by(f64::total_cmp);
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
