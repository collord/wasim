//! Optimization study executor (§13): Box's complex method over the model's optimization
//! variables. Each candidate = set the variables, run the model, reduce the objective element
//! by its (optional) Monte-Carlo statistic, apply the direction. Deterministic (seed-driven).
//!
//! The schema carries the problem definition; the search algorithm is an engine concern.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::error::EngineError;
use crate::model::{ObjectiveStatKind, OptDirection};
use crate::model_v2::{FixedValue, Model, NodeRule, Primitive};
use crate::{engine, ModelGraphV2, RunConfig};

/// Result of an optimization run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StudyResults {
    /// Optimal variable values, in the order of `model.optimization.variables`.
    pub variables: Vec<VariableResult>,
    /// The achieved (reduced) objective value at the optimum.
    pub objective: f64,
    /// Number of objective evaluations performed.
    pub evaluations: usize,
    /// True if the search converged (spread below tolerance) before the iteration cap.
    pub converged: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VariableResult {
    pub element_id: String,
    pub value: f64,
}

/// Set an optimization variable (an editable fixed-value node) to `value`. Errors if the
/// element is missing or not a fixed scalar.
fn set_variable(model: &mut Model, id: &str, value: f64) -> Result<(), EngineError> {
    for elem in &mut model.elements {
        if elem.base.id != id {
            continue;
        }
        if let Primitive::Node(n) = &mut elem.primitive {
            if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &mut n.rule {
                q.value = value;
                return Ok(());
            }
        }
        return Err(EngineError::InvalidModel(format!(
            "optimization variable '{id}' is not an editable fixed value"
        )));
    }
    Err(EngineError::ElementNotFound(id.to_string()))
}

/// Reduce the objective element's per-realization final values to a single scalar per the
/// objective statistic (None = deterministic → the single/first value).
fn reduce_objective(samples: &[f64], stat: Option<&crate::model::ObjectiveStatistic>) -> f64 {
    match stat {
        None => samples.first().copied().unwrap_or(0.0),
        Some(s) => match s.kind {
            ObjectiveStatKind::Mean => engine::mean(samples),
            ObjectiveStatKind::Percentile => engine::percentile(samples, s.p.unwrap_or(50.0)),
            ObjectiveStatKind::Peak => samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            ObjectiveStatKind::Valley => samples.iter().cloned().fold(f64::INFINITY, f64::min),
            ObjectiveStatKind::Sum => samples.iter().sum(),
        },
    }
}

/// Evaluate a candidate point: set variables, run, reduce the objective. Returns the
/// minimization cost (objective negated when the direction is maximize) so the solver always
/// minimizes. Infeasible/failed candidates return +∞ so they're rejected by the search.
fn evaluate(
    base: &Model,
    var_ids: &[String],
    point: &[f64],
    config: &RunConfig,
) -> f64 {
    let obj = &base.optimization.as_ref().unwrap().objective;
    let mut m = base.clone();
    for (id, &v) in var_ids.iter().zip(point) {
        if set_variable(&mut m, id, v).is_err() {
            return f64::INFINITY;
        }
    }
    let graph = match ModelGraphV2::build(&m) {
        Ok(g) => g,
        Err(_) => return f64::INFINITY,
    };
    let results = match crate::engine_v2::run(&m, &graph, config) {
        Ok(r) => r,
        Err(_) => return f64::INFINITY,
    };
    let samples = match results.elements.get(&obj.element_id) {
        Some(er) if !er.final_values.is_empty() => &er.final_values,
        _ => return f64::INFINITY,
    };
    let value = reduce_objective(samples, obj.statistic.as_ref());
    if !value.is_finite() {
        return f64::INFINITY;
    }
    // Solver minimizes; flip for maximize.
    match obj.direction {
        OptDirection::Minimize => value,
        OptDirection::Maximize => -value,
    }
}

/// Run the optimization study on `model` (which must carry an `optimization` spec).
/// Box's complex method: maintain k = 2n candidate points in the bounded box, reflect the
/// worst through the centroid of the rest, halving toward the centroid on failure.
pub fn optimize(model: &Model, config: &RunConfig) -> Result<StudyResults, EngineError> {
    let spec = model
        .optimization
        .as_ref()
        .ok_or_else(|| EngineError::InvalidModel("model has no optimization spec".into()))?;
    let n = spec.variables.len();
    if n == 0 {
        return Err(EngineError::InvalidModel("optimization has no variables".into()));
    }

    let var_ids: Vec<String> = spec.variables.iter().map(|v| v.element_id.clone()).collect();
    let lower: Vec<f64> = spec.variables.iter().map(|v| v.lower.value).collect();
    let upper: Vec<f64> = spec.variables.iter().map(|v| v.upper.value).collect();
    let integer: Vec<bool> = spec.variables.iter().map(|v| v.integer).collect();

    // Clamp + integer-round a point into the feasible box.
    let project = |p: &mut Vec<f64>| {
        for i in 0..n {
            p[i] = p[i].clamp(lower[i], upper[i]);
            if integer[i] {
                p[i] = p[i].round();
            }
        }
    };

    // The submodel/optimization objective runs its own realizations; the study is
    // deterministic given the seed.
    let seed = config.seed.or(model.simulation_settings.seed).unwrap_or(0);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // Complex of k = 2n points: the initial guess plus random points in the box.
    let k = (2 * n).max(n + 1);
    let mut points: Vec<Vec<f64>> = Vec::with_capacity(k);
    let mut initial: Vec<f64> = spec.variables.iter().map(|v| v.initial.value).collect();
    project(&mut initial);
    points.push(initial);
    for _ in 1..k {
        let mut p: Vec<f64> = (0..n)
            .map(|i| lower[i] + rng.gen::<f64>() * (upper[i] - lower[i]))
            .collect();
        project(&mut p);
        points.push(p);
    }

    let mut costs: Vec<f64> = points.iter().map(|p| evaluate(model, &var_ids, p, config)).collect();
    let mut evaluations = k;

    let max_iters = 200usize;
    let mut converged = false;
    for _ in 0..max_iters {
        // Worst (highest cost) and centroid of the rest.
        let worst = (0..k).max_by(|&a, &b| costs[a].total_cmp(&costs[b])).unwrap();
        let centroid: Vec<f64> = (0..n)
            .map(|i| {
                let s: f64 = (0..k).filter(|&j| j != worst).map(|j| points[j][i]).sum();
                s / (k - 1) as f64
            })
            .collect();

        // Reflect the worst point through the centroid (α = 1.3, Box's default).
        let alpha = 1.3;
        let mut trial: Vec<f64> = (0..n)
            .map(|i| centroid[i] + alpha * (centroid[i] - points[worst][i]))
            .collect();
        project(&mut trial);
        let mut trial_cost = evaluate(model, &var_ids, &trial, config);
        evaluations += 1;

        // If the reflection is still the worst, halve toward the centroid until it improves.
        let mut halvings = 0;
        while trial_cost >= costs[worst] && halvings < 10 {
            for i in 0..n {
                trial[i] = (trial[i] + centroid[i]) / 2.0;
            }
            project(&mut trial);
            trial_cost = evaluate(model, &var_ids, &trial, config);
            evaluations += 1;
            halvings += 1;
        }

        points[worst] = trial;
        costs[worst] = trial_cost;

        // Convergence: cost spread across the complex below tolerance.
        let best = costs.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = costs.iter().cloned().filter(|c| c.is_finite()).fold(f64::NEG_INFINITY, f64::max);
        if (hi - best).abs() <= 1e-6 * (1.0 + best.abs()) {
            converged = true;
            break;
        }
    }

    let best_idx = (0..k).min_by(|&a, &b| costs[a].total_cmp(&costs[b])).unwrap();
    let best_point = &points[best_idx];
    // Report the objective in its natural sense (undo the maximize flip).
    let objective = match spec.objective.direction {
        OptDirection::Minimize => costs[best_idx],
        OptDirection::Maximize => -costs[best_idx],
    };

    Ok(StudyResults {
        variables: var_ids
            .iter()
            .zip(best_point)
            .map(|(id, &value)| VariableResult { element_id: id.clone(), value })
            .collect(),
        objective,
        evaluations,
        converged,
    })
}
