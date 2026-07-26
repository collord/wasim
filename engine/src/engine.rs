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
    /// Optional richer results/analysis (A3, gap #3). None = the default fixed summary; when
    /// set, opted-in elements gain custom percentile bands, PDF/CDF/CCDF, capture-time
    /// snapshots, and final-value stats. Additive — default output is byte-identical.
    pub results_spec: Option<crate::results_spec::ResultsSpec>,
    /// Timebase mode (B1, gap #1). `Fixed` (default) = the original fixed-grid Euler evaluator,
    /// bit-identical. `EventAccurate` inserts unscheduled sub-step updates at scheduled event
    /// instants and stock bound crossings to refine integration within each grid step (the grid
    /// stays the statistical/state/reporting lattice — sub-steps consume no randomness and the
    /// results contract is unchanged).
    pub timebase: TimebaseMode,
    /// Dimensional analysis mode (B5, gap #6). `Warn` (default) logs dimensional inconsistencies
    /// and continues (numeric behavior unchanged). `Strict` turns any inconsistency into a hard
    /// load error before the run. Unknown units / unresolved refs are always exempt.
    pub units: UnitsMode,
    /// Per-realization importance weights (B7, gap #5). Empty = every realization weighted
    /// equally (unweighted, behavior unchanged). When set, its length must equal `n_realizations`;
    /// weights are normalized to sum 1 and applied to weighted statistics (mean/percentile/std/CTE
    /// and the A3 analysis layer). Weighted reductions on uniform weights equal the unweighted ones.
    pub realization_weights: Vec<f64>,
    /// Opt-in **array lane** (ARRAY_LANE_DESIGN.md, Phase A). When true and the (v2)
    /// model is array-lane-eligible — flat independent Monte-Carlo: no dimensions,
    /// stocks, submodels, or state-machine node rules; arithmetic/comparison/if +
    /// `run_stat` only — the run is evaluated columnar over the `Run` axis instead of
    /// the scalar realization loop. Falls back to the scalar lane when ineligible, so
    /// the default (false) path is completely untouched.
    pub array_lane: bool,
}

/// Dimensional-analysis strictness for a run (B5). See `RunConfig.units`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum UnitsMode {
    #[default]
    Warn,
    Strict,
}

/// Timebase selection for a run (B1). See `RunConfig.timebase`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TimebaseMode {
    #[default]
    Fixed,
    EventAccurate,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            n_realizations: None,
            seed: None,
            duration_override: None,
            timestep_override: None,
            results_spec: None,
            timebase: TimebaseMode::Fixed,
            units: UnitsMode::Warn,
            realization_weights: Vec::new(),
            array_lane: false,
        }
    }
}

// ── Results ───────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct SimulationResults {
    /// Time axis in declared timestep units. Length = n_steps.
    pub time_axis: Vec<f64>,
    /// Unit label for `time_axis`. The engine emits the canonical timestep unit; the
    /// display boundary (wasm bridge) may rewrite both axis and label to a display unit.
    #[serde(default)]
    pub time_unit: String,
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
    /// Optional richer analysis (A3): present only when a `RunConfig.results_spec` opts this
    /// element in. `skip_serializing_if` keeps default output byte-identical for existing consumers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<crate::results_spec::ElementAnalysis>,
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
    if !duration.is_finite() || duration < 0.0 {
        return Err(EngineError::InvalidModel(format!("duration must be >= 0, got {duration}")));
    }
    // Lookup tables, extracted once for the AST walker (decoupled from WasimModel).
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
    // duration 0 (or below half a timestep) is a single-evaluation model — see engine_v2 + §9.
    let n_steps = ((duration_in_dt / dt).round() as usize).max(1);
    // v1 models have no dimensions/array comprehensions; supply empty array-env state so the
    // shared EvalCtx (used by the v2 array executor) is satisfied.
    let dim_sizes_empty: HashMap<String, usize> = HashMap::new();
    let dim_labels_empty: HashMap<String, Vec<String>> = HashMap::new();
    let run_stats_empty: HashMap<String, f64> = HashMap::new();
    let index_stack_empty: std::cell::RefCell<Vec<usize>> = std::cell::RefCell::new(Vec::new());
    let submodel_outputs_empty: HashMap<(String, String), Vec<f64>> = HashMap::new();
    // The v1 path has no events; supply an always-empty fired-event set for the shared EvalCtx.
    let fired_events_empty: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
    let lookups: HashMap<String, crate::eval::LookupData> = model.elements.iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Lookup { x, y, columns, extrapolation, .. } => Some((
                e.id.clone(),
                crate::eval::LookupData {
                    x: x.clone(),
                    y: y.clone(),
                    columns: columns.clone(),
                    extrapolation: extrapolation.clone(),
                    // v1 tables are 1-D linear; the §10 extras are v2-only.
                    interpolation: crate::model::InterpolationMethod::Linear,
                    log_result: false,
                    extra_axes: Vec::new(),
                    nd_values: Vec::new(),
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
                    let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None, lookups: &lookups, dt_unit: &dt_unit, outputs: &dist_ctx, prev_outputs: &empty_prev, elapsed: 0.0, dt, step_index: 0 };
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
                    let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None, lookups: &lookups, dt_unit: &dt_unit, outputs: &dist_ctx, prev_outputs: &empty_prev, elapsed: 0.0, dt, step_index: 0 };
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
                let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None, lookups: &lookups, dt_unit: &dt_unit, outputs: &init_ctx_outputs, prev_outputs: &empty_map, elapsed: 0.0, dt, step_index: 0 };
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
                        let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None, lookups: &lookups, dt_unit: &dt_unit, outputs: &init_ctx_outputs, prev_outputs: &empty_map, elapsed: 0.0, dt, step_index: 0 };
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
                        let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None,
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
                                let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None, lookups: &lookups, dt_unit: &dt_unit, outputs: &outputs, prev_outputs: &prev_outputs, elapsed, dt, step_index: step_idx };
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
                            let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None,
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
                    let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None,
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
                let ctx = EvalCtx { dimensions: &dim_sizes_empty, dim_labels: &dim_labels_empty, run_stats: &run_stats_empty, index_stack: &index_stack_empty, submodel_outputs: &submodel_outputs_empty, lag: None, fired_events: &fired_events_empty, calendar_start: None, lookups: &lookups, dt_unit: &dt_unit, outputs: &outputs, prev_outputs: &prev_outputs, elapsed, dt, step_index: step_idx };
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
            // The v1 reference engine does not compute the A3 analysis layer.
            analysis: None,
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
            analysis: None,
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
        time_unit: dt_unit.clone(),
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

/// Sample standard deviation (n-1 denominator). 0.0 for fewer than 2 samples.
pub(crate) fn std(vs: &[f64]) -> f64 {
    if vs.len() < 2 { return 0.0; }
    let m = mean(vs);
    let var = vs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (vs.len() - 1) as f64;
    var.sqrt()
}

/// Empirical CDF at `threshold`: fraction of samples ≤ threshold.
pub(crate) fn cumulative_prob(vs: &[f64], threshold: f64) -> f64 {
    if vs.is_empty() { return 0.0; }
    vs.iter().filter(|&&x| x <= threshold).count() as f64 / vs.len() as f64
}

// ── Weighted statistics (B7, importance/realization weights) ──────────────────
//
// Each takes parallel `vs` (samples) and `w` (weights). When `w` is empty or its length differs
// from `vs`, they fall back to the unweighted helpers above — so uniform/absent weights reproduce
// the standard statistics exactly.

/// Weighted mean Σ(wᵢ·xᵢ) / Σwᵢ.
pub(crate) fn weighted_mean(vs: &[f64], w: &[f64]) -> f64 {
    if w.len() != vs.len() || vs.is_empty() {
        return mean(vs);
    }
    let sw: f64 = w.iter().sum();
    if sw <= 0.0 {
        return mean(vs);
    }
    vs.iter().zip(w).map(|(x, wi)| x * wi).sum::<f64>() / sw
}

/// Weighted sample percentile via the weighted empirical CDF (samples sorted; the value whose
/// cumulative weight first reaches `p/100`). Matches `percentile` on equal weights.
pub(crate) fn weighted_percentile(vs: &[f64], w: &[f64], p: f64) -> f64 {
    if w.len() != vs.len() || vs.is_empty() {
        return percentile(vs, p);
    }
    let mut pairs: Vec<(f64, f64)> = vs.iter().copied().zip(w.iter().copied()).collect();
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = pairs.iter().map(|(_, wi)| wi).sum();
    if total <= 0.0 {
        return percentile(vs, p);
    }
    let target = (p / 100.0).clamp(0.0, 1.0) * total;
    let mut cum = 0.0;
    for (x, wi) in &pairs {
        cum += wi;
        if cum >= target - 1e-12 {
            return *x;
        }
    }
    pairs.last().map(|(x, _)| *x).unwrap_or(0.0)
}

/// Weighted (population) standard deviation √(Σwᵢ(xᵢ−μ_w)² / Σwᵢ).
pub(crate) fn weighted_std(vs: &[f64], w: &[f64]) -> f64 {
    if w.len() != vs.len() || vs.len() < 2 {
        return std(vs);
    }
    let sw: f64 = w.iter().sum();
    if sw <= 0.0 {
        return std(vs);
    }
    let m = weighted_mean(vs, w);
    let var = vs.iter().zip(w).map(|(x, wi)| wi * (x - m).powi(2)).sum::<f64>() / sw;
    var.sqrt()
}

// ── Extended reducers (sweep-composition boundary, Phase 3) ────────────────────
//
// The reducers `submodel_stat` can apply at a sweep boundary, beyond mean/percentile/std/
// cumulative_prob. Exceedance is the CCDF complement of `cumulative_prob`; CTE mirrors the
// A3 final-stats computation (results_spec.rs) but as a reusable reducer. Sum/min/max are the
// plain aggregations. Each has a weighted variant that falls back to the unweighted one on
// empty/mismatched weights, matching the convention above.

/// Exceedance probability P(X > threshold) = 1 − CDF (the CCDF). Complement of `cumulative_prob`.
pub(crate) fn exceedance(vs: &[f64], threshold: f64) -> f64 {
    if vs.is_empty() { return 0.0; }
    vs.iter().filter(|&&x| x > threshold).count() as f64 / vs.len() as f64
}

/// Conditional tail expectation: mean of samples at or above the `p`-th percentile (upper tail).
/// Matches the A3 final-stats CTE (results_spec.rs). Empty tail → the threshold itself.
pub(crate) fn cte(vs: &[f64], p: f64) -> f64 {
    if vs.is_empty() { return 0.0; }
    let threshold = percentile(vs, p);
    let tail: Vec<f64> = vs.iter().copied().filter(|x| *x >= threshold).collect();
    if tail.is_empty() { threshold } else { mean(&tail) }
}

/// Weighted conditional tail expectation (upper tail beyond the weighted `p`-th percentile).
pub(crate) fn weighted_cte(vs: &[f64], w: &[f64], p: f64) -> f64 {
    if w.len() != vs.len() || vs.is_empty() {
        return cte(vs, p);
    }
    let threshold = weighted_percentile(vs, w, p);
    let (tail, tail_w): (Vec<f64>, Vec<f64>) = vs
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, x)| *x >= threshold)
        .map(|(i, x)| (x, w.get(i).copied().unwrap_or(1.0)))
        .unzip();
    if tail.is_empty() { threshold } else { weighted_mean(&tail, &tail_w) }
}

/// Sum of samples.
pub(crate) fn sum_of(vs: &[f64]) -> f64 {
    vs.iter().sum()
}

/// Min / max of samples (NaN-safe via total_cmp). Empty → 0.0, matching the reducer convention.
/// Weights don't affect an extremum, so there is no weighted variant.
pub(crate) fn min_of(vs: &[f64]) -> f64 {
    vs.iter().copied().min_by(f64::total_cmp).unwrap_or(0.0)
}

pub(crate) fn max_of(vs: &[f64]) -> f64 {
    vs.iter().copied().max_by(f64::total_cmp).unwrap_or(0.0)
}

/// Sample covariance of two index-aligned series (n−1 denominator, matching `std`).
/// 0.0 for fewer than 2 pairs or a length mismatch (defensive).
pub(crate) fn covariance(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 || ys.len() != n {
        return 0.0;
    }
    let mx = mean(xs);
    let my = mean(ys);
    let s: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    s / (n - 1) as f64
}

/// Pearson correlation Cov(x,y)/(σx·σy) ∈ [−1, 1]. 0.0 when either side has zero variance.
pub(crate) fn correlation(xs: &[f64], ys: &[f64]) -> f64 {
    let denom = std(xs) * std(ys);
    if denom <= 0.0 {
        return 0.0;
    }
    covariance(xs, ys) / denom
}

/// Regression slope Cov(x,y)/Var(x) — the optimal single-control-variate coefficient
/// b* when `x` is the control and `y` the target. 0.0 when the control has zero
/// variance (the control then contributes nothing, keeping the estimator unbiased).
pub(crate) fn beta(xs: &[f64], ys: &[f64]) -> f64 {
    let var_x = {
        let s = std(xs);
        s * s
    };
    if var_x <= 0.0 {
        return 0.0;
    }
    covariance(xs, ys) / var_x
}

/// OLS slope coefficients of regressing `y` on the `controls` across realizations:
/// b = Σ_C⁻¹ σ_Cy, where Σ_C[i][j] = Cov(cᵢ, cⱼ) and σ_Cy[i] = Cov(cᵢ, y). Using
/// centered covariances makes these the multiple-regression slopes with an implicit
/// intercept. A singular/degenerate control system (collinear or zero-variance
/// controls) returns all-zero coefficients — the control adjustment then vanishes,
/// keeping the control-variate estimator unbiased. Empty `controls` → empty vec.
pub(crate) fn regression_coefficients(y: &[f64], controls: &[&[f64]]) -> Vec<f64> {
    let m = controls.len();
    if m == 0 {
        return Vec::new();
    }
    // Covariance matrix of the controls and the control–y covariance vector.
    let mut a = vec![vec![0.0f64; m]; m];
    let mut d = vec![0.0f64; m];
    for i in 0..m {
        d[i] = covariance(controls[i], y);
        for j in i..m {
            let c = covariance(controls[i], controls[j]);
            a[i][j] = c;
            a[j][i] = c;
        }
    }
    solve_linear_or_zero(a, d)
}

/// Solve `A x = b` for a small symmetric system by Gaussian elimination with partial
/// pivoting. Returns an all-zero vector if `A` is singular (near-zero pivot) rather
/// than producing NaNs/Inf — the caller treats that as "no control adjustment".
fn solve_linear_or_zero(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let n = b.len();
    for col in 0..n {
        // Partial pivot: largest-magnitude entry in this column at or below the diagonal.
        let mut piv = col;
        for r in (col + 1)..n {
            if a[r][col].abs() > a[piv][col].abs() {
                piv = r;
            }
        }
        if a[piv][col].abs() < 1e-12 {
            return vec![0.0; n]; // singular / collinear controls → no adjustment
        }
        a.swap(col, piv);
        b.swap(col, piv);
        // Eliminate below.
        for r in (col + 1)..n {
            let f = a[r][col] / a[col][col];
            if f != 0.0 {
                for c in col..n {
                    a[r][c] -= f * a[col][c];
                }
                b[r] -= f * b[col];
            }
        }
    }
    // Back-substitution.
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in (i + 1)..n {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
    }
    x
}

#[cfg(test)]
mod reducer_tests {
    use super::*;

    // Reference vector 1.0..=10.0. `percentile` uses nearest-rank on the sorted values:
    // idx = round(p/100 · (n−1)); for n=10, p=80 → round(7.2) = 7 → value 8.0.
    const V: [f64; 10] = [1., 2., 3., 4., 5., 6., 7., 8., 9., 10.];

    #[test]
    fn exceedance_is_ccdf_complement_of_cumulative_prob() {
        // P(X > 5) over 1..=10 = |{6,7,8,9,10}|/10 = 0.5; and exceedance = 1 − cumulative_prob
        // only when no sample equals the threshold exactly... here x=5 is excluded from both the
        // >5 count and included in the ≤5 count, so they sum to 1.0.
        assert_eq!(exceedance(&V, 5.0), 0.5);
        assert!((exceedance(&V, 5.0) + cumulative_prob(&V, 5.0) - 1.0).abs() < 1e-12);
        assert_eq!(exceedance(&V, 10.0), 0.0, "nothing exceeds the max");
        assert_eq!(exceedance(&V, 0.0), 1.0, "everything exceeds below-min");
    }

    #[test]
    fn bivariate_reducers_hand_checks() {
        // Perfectly linear: y = 3x + 1 → beta(x→y) = 3, corr = 1.
        let x = [1., 2., 3., 4., 5.];
        let y = [4., 7., 10., 13., 16.];
        assert!((beta(&x, &y) - 3.0).abs() < 1e-9, "beta should be 3, got {}", beta(&x, &y));
        assert!((correlation(&x, &y) - 1.0).abs() < 1e-9);
        // Negative slope: y = -2x + 10 → beta = -2, corr = -1.
        let yn = [8., 6., 4., 2., 0.];
        assert!((beta(&x, &yn) + 2.0).abs() < 1e-9);
        assert!((correlation(&x, &yn) + 1.0).abs() < 1e-9);
        // cov symmetric; cov(x,x) = var(x) = std(x)^2.
        assert!((covariance(&x, &y) - covariance(&y, &x)).abs() < 1e-9);
        assert!((covariance(&x, &x) - std(&x).powi(2)).abs() < 1e-9);
        // Degenerate control (zero variance) → beta 0 (unbiased no-op), corr 0.
        let c = [5., 5., 5., 5., 5.];
        assert_eq!(beta(&c, &y), 0.0);
        assert_eq!(correlation(&c, &y), 0.0);
    }

    #[test]
    fn regression_coefficients_hand_checks() {
        // Deterministic exact-fit: c0=[1,2,3,4], c1=[1,0,1,0], y = 2*c0 - 3*c1.
        let c0 = [1.0, 2.0, 3.0, 4.0];
        let c1 = [1.0, 0.0, 1.0, 0.0];
        let y: Vec<f64> = (0..4).map(|i| 2.0 * c0[i] - 3.0 * c1[i]).collect();
        let b = regression_coefficients(&y, &[&c0, &c1]);
        assert!((b[0] - 2.0).abs() < 1e-9, "b0 should be 2, got {}", b[0]);
        assert!((b[1] + 3.0).abs() < 1e-9, "b1 should be -3, got {}", b[1]);

        // Single control reduces to the simple slope = beta(c0, y).
        let b1 = regression_coefficients(&y, &[&c0]);
        assert!((b1[0] - beta(&c0, &y)).abs() < 1e-9);

        // Collinear controls (c1 == c0) → singular system → all-zero (safe no-op).
        let bz = regression_coefficients(&y, &[&c0, &c0]);
        assert!(bz.iter().all(|&v| v == 0.0), "collinear controls → zero coefficients");

        // No controls → empty.
        assert!(regression_coefficients(&y, &[]).is_empty());
    }

    #[test]
    fn cte_is_mean_of_upper_tail() {
        // CTE(80) = mean of samples ≥ percentile(80)=8.0 → mean{8,9,10} = 9.0.
        assert!((cte(&V, 80.0) - 9.0).abs() < 1e-12);
        // CTE(0) reduces to the overall mean (whole set is the tail).
        assert!((cte(&V, 0.0) - mean(&V)).abs() < 1e-12);
    }

    #[test]
    fn sum_min_max_over_reference() {
        assert_eq!(sum_of(&V), 55.0);
        assert_eq!(min_of(&V), 1.0);
        assert_eq!(max_of(&V), 10.0);
    }

    #[test]
    fn empty_reducers_return_zero() {
        let e: [f64; 0] = [];
        assert_eq!(exceedance(&e, 1.0), 0.0);
        assert_eq!(cte(&e, 50.0), 0.0);
        assert_eq!(sum_of(&e), 0.0);
        assert_eq!(min_of(&e), 0.0);
        assert_eq!(max_of(&e), 0.0);
    }

    #[test]
    fn weighted_cte_matches_unweighted_on_uniform_weights() {
        let w = [1.0; 10];
        assert!((weighted_cte(&V, &w, 80.0) - cte(&V, 80.0)).abs() < 1e-12);
        // Empty weights → falls back to unweighted.
        assert!((weighted_cte(&V, &[], 80.0) - cte(&V, 80.0)).abs() < 1e-12);
    }
}
