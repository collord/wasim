//! Optimization study executor (§13): Box's complex method over the model's optimization
//! variables. Each candidate = set the variables, run the model, reduce the objective element
//! by its (optional) Monte-Carlo statistic, apply the direction. Deterministic (seed-driven).
//!
//! The schema carries the problem definition; the search algorithm is an engine concern.

use std::collections::HashMap;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::error::EngineError;
use crate::eval_harness::{evaluate_point, evaluate_point_with_extras};
use crate::model::{AstNode, OptConstraint, OptDirection, QuantityOrFormula};
use crate::model_v2::Model;
use crate::RunConfig;

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

/// Collect every `Ref` element id appearing in a constraint condition AST, so the run can
/// save + return those elements' values (they must be evaluated in the same run).
fn collect_refs(ast: &AstNode, out: &mut Vec<String>) {
    use AstNode::*;
    match ast {
        Ref { element_id, .. } => out.push(element_id.clone()),
        Add { left, right }
        | Subtract { left, right }
        | Multiply { left, right }
        | Divide { left, right }
        | Power { left, right }
        | Lt { left, right }
        | Gt { left, right }
        | Lte { left, right }
        | Gte { left, right }
        | Eq { left, right }
        | Neq { left, right }
        | And { left, right }
        | Or { left, right } => {
            collect_refs(left, out);
            collect_refs(right, out);
        }
        Neg { operand } | Not { operand } => collect_refs(operand, out),
        If { cond, then, else_ } => {
            collect_refs(cond, out);
            collect_refs(then, out);
            collect_refs(else_, out);
        }
        Call { args, .. } => args.iter().for_each(|a| collect_refs(a, out)),
        _ => {}
    }
}

/// Every element id a constraint set references, deduplicated. Only `Expression`
/// conditions carry an AST; `Quantity`/`Formula` conditions contribute none.
fn constraint_refs(constraints: &[OptConstraint]) -> Vec<String> {
    let mut ids = Vec::new();
    for c in constraints {
        if let QuantityOrFormula::Expression(e) = &c.condition {
            collect_refs(&e.ast, &mut ids);
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// Evaluate a constraint condition AST to a number, resolving each `Ref` from the run's
/// reduced element values (`extras`). This is a deliberately small deterministic subset —
/// literals, refs, arithmetic, comparisons, boolean ops, `abs/min/max`, and `if` — which
/// covers every realistic feasibility predicate. Anything outside it (or a `Ref` to an
/// element that produced no output) yields `NaN`, which the caller treats as *unverifiable*
/// rather than *violated*, so a constraint the engine can't evaluate never spuriously
/// rejects a candidate.
fn eval_condition(ast: &AstNode, extras: &HashMap<String, Option<f64>>) -> f64 {
    use crate::model::BuiltinFn;
    use AstNode::*;
    let bin = |l: &AstNode, r: &AstNode| (eval_condition(l, extras), eval_condition(r, extras));
    match ast {
        Literal { value, .. } => *value,
        Ref { element_id, .. } => extras.get(element_id).copied().flatten().unwrap_or(f64::NAN),
        Add { left, right } => { let (l, r) = bin(left, right); l + r }
        Subtract { left, right } => { let (l, r) = bin(left, right); l - r }
        Multiply { left, right } => { let (l, r) = bin(left, right); l * r }
        Divide { left, right } => { let (l, r) = bin(left, right); l / r }
        Power { left, right } => { let (l, r) = bin(left, right); l.powf(r) }
        Lt { left, right } => { let (l, r) = bin(left, right); (l < r) as u8 as f64 }
        Gt { left, right } => { let (l, r) = bin(left, right); (l > r) as u8 as f64 }
        Lte { left, right } => { let (l, r) = bin(left, right); (l <= r) as u8 as f64 }
        Gte { left, right } => { let (l, r) = bin(left, right); (l >= r) as u8 as f64 }
        Eq { left, right } => { let (l, r) = bin(left, right); (l == r) as u8 as f64 }
        Neq { left, right } => { let (l, r) = bin(left, right); (l != r) as u8 as f64 }
        And { left, right } => { let (l, r) = bin(left, right); ((l != 0.0) && (r != 0.0)) as u8 as f64 }
        Or { left, right } => { let (l, r) = bin(left, right); ((l != 0.0) || (r != 0.0)) as u8 as f64 }
        Neg { operand } => -eval_condition(operand, extras),
        Not { operand } => (eval_condition(operand, extras) == 0.0) as u8 as f64,
        If { cond, then, else_ } => {
            if eval_condition(cond, extras) != 0.0 {
                eval_condition(then, extras)
            } else {
                eval_condition(else_, extras)
            }
        }
        Call { func, args } => {
            let v: Vec<f64> = args.iter().map(|a| eval_condition(a, extras)).collect();
            match (func, v.as_slice()) {
                (BuiltinFn::Abs, [x]) => x.abs(),
                (BuiltinFn::Min, xs) if !xs.is_empty() => xs.iter().cloned().fold(f64::INFINITY, f64::min),
                (BuiltinFn::Max, xs) if !xs.is_empty() => xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                _ => f64::NAN,
            }
        }
        _ => f64::NAN,
    }
}

/// Is a single constraint satisfied given the run's reduced element values? A condition is
/// *satisfied* when it evaluates truthy (≥ 0.5, so a comparison's 1.0 passes and 0.0 fails).
/// An unverifiable condition (NaN — unparsed formula, bare quantity, or a ref with no output)
/// is treated as satisfied so the engine never rejects a candidate on a constraint it cannot
/// actually evaluate.
fn constraint_satisfied(c: &OptConstraint, extras: &HashMap<String, Option<f64>>) -> bool {
    let v = match &c.condition {
        QuantityOrFormula::Expression(e) => eval_condition(&e.ast, extras),
        // A bare quantity is a constant flag; a raw formula string is unparsed → unverifiable.
        QuantityOrFormula::Quantity(q) => q.value,
        QuantityOrFormula::Formula(_) => f64::NAN,
    };
    !v.is_finite() || v >= 0.5
}

/// Evaluate a candidate point: set variables, run, reduce the objective, and reject the point
/// (+∞) if any optimization constraint is violated. Returns the minimization cost (objective
/// negated when the direction is maximize) so the solver always minimizes. Infeasible/failed
/// candidates return +∞ so they're rejected by the search (Box's complex treats implicit
/// constraints exactly this way).
///
/// The point-evaluation body is shared with the sensitivity sweep via `eval_harness`; only
/// the ∞-on-failure coercion, constraint check, and the maximize flip are optimizer-specific.
fn evaluate(base: &Model, var_ids: &[String], point: &[f64], config: &RunConfig) -> f64 {
    let spec = base.optimization.as_ref().unwrap();
    let obj = &spec.objective;

    let value = if spec.constraints.is_empty() {
        // No constraints: the single-run objective path, unchanged.
        match evaluate_point(base, var_ids, point, &obj.element_id, obj.statistic.as_ref(), config) {
            Ok(v) if v.is_finite() => v,
            _ => return f64::INFINITY,
        }
    } else {
        // One run serves objective + constraints: fetch each constraint-referenced element's
        // value from the same run, then reject the point if any constraint is violated.
        let extra_ids = constraint_refs(&spec.constraints);
        let (v, extras) = match evaluate_point_with_extras(
            base,
            var_ids,
            point,
            &obj.element_id,
            obj.statistic.as_ref(),
            &extra_ids,
            config,
        ) {
            Ok((v, e)) if v.is_finite() => (v, e),
            _ => return f64::INFINITY,
        };
        if spec.constraints.iter().any(|c| !constraint_satisfied(c, &extras)) {
            return f64::INFINITY;
        }
        v
    };

    // Solver minimizes; flip for maximize.
    match obj.direction {
        OptDirection::Minimize => value,
        OptDirection::Maximize => -value,
    }
}

/// Box's boundary treatment: any coordinate of `p` that violates its bound is reset to a small
/// fraction (β = 1e-3 of the range) *inside* the violated bound, rather than hard-clamped onto
/// it. Keeping reflections strictly interior stops them piling onto a boundary and collapsing
/// the complex to a single stuck point (the per-step dynamic-optimization failure, §13a).
fn retract_into_bounds(p: &mut [f64], lower: &[f64], upper: &[f64]) {
    for i in 0..p.len() {
        let (lo, hi) = (lower[i], upper[i]);
        let span = (hi - lo).abs().max(1e-12);
        if p[i] < lo {
            p[i] = (lo + 1e-3 * span).min(hi);
        } else if p[i] > hi {
            p[i] = (hi - 1e-3 * span).max(lo);
        }
    }
}

/// Bounds for one search variable (the parts of `OptVariable` the solver needs).
pub(crate) struct SearchBounds {
    pub lower: Vec<f64>,
    pub upper: Vec<f64>,
    pub initial: Vec<f64>,
    pub integer: Vec<bool>,
}

/// Outcome of a Box's-complex search: the best point, its (minimization) cost, the number of
/// evaluations, and whether the complex converged before the iteration cap.
pub(crate) struct SolveResult {
    pub point: Vec<f64>,
    pub cost: f64,
    pub evaluations: usize,
    pub converged: bool,
}

/// Box's complex method over a bounded box, minimizing `cost`. Maintains k = 2n candidate
/// points, reflects the worst through the centroid of the rest, halving toward the centroid on
/// failure. `cost` is injectable so the same search drives both the static study (whole-model
/// run per candidate) and dynamic per-timestep optimization (§13a). The caller applies any
/// maximize→minimize flip inside `cost`; the solver always minimizes.
pub(crate) fn solve(
    bounds: &SearchBounds,
    seed: u64,
    mut cost: impl FnMut(&[f64]) -> f64,
) -> SolveResult {
    let n = bounds.lower.len();
    let (lower, upper, integer) = (&bounds.lower, &bounds.upper, &bounds.integer);

    // Clamp + integer-round a point into the feasible box.
    let project = |p: &mut Vec<f64>| {
        for i in 0..n {
            p[i] = p[i].clamp(lower[i], upper[i]);
            if integer[i] {
                p[i] = p[i].round();
            }
        }
    };

    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    // Complex of k points: the initial guess plus points spread across the box. Box recommends
    // k = 2n, but floor at n + 2 so even a 1-D search keeps ≥ 3 points — with only 2 points the
    // "centroid of the rest" is a single point and reflection collapses onto a bound.
    //
    // Each non-initial point mixes a per-dimension stratified position (guaranteeing spread across
    // each bound — points on both sides of any interior optimum) with a jitter. Purely-random
    // fill could cluster all points on one side of a near-boundary optimum, whose costs are then
    // similar enough to trip the convergence test before the basin is found — the per-step
    // dynamic-optimization false-convergence at a bound (§13a).
    let k = (2 * n).max(n + 2);
    let mut points: Vec<Vec<f64>> = Vec::with_capacity(k);
    let mut initial = bounds.initial.clone();
    project(&mut initial);
    points.push(initial);
    for j in 1..k {
        // Stratify j across (0,1): the k-1 extra points evenly partition each dimension's range,
        // then jitter within the stratum so repeated seeds still differ.
        let frac = j as f64 / k as f64;
        let mut p: Vec<f64> = (0..n)
            .map(|i| {
                let jitter = (rng.gen::<f64>() - 0.5) / k as f64;
                lower[i] + (frac + jitter).clamp(0.0, 1.0) * (upper[i] - lower[i])
            })
            .collect();
        project(&mut p);
        points.push(p);
    }

    let mut costs: Vec<f64> = points.iter().map(|p| cost(p)).collect();
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

        // Reflect the worst point through the centroid (α = 1.3, Box's default). Box's boundary
        // rule: a reflection that violates a bound is RETRACTED toward the centroid to just
        // inside the feasible region — NOT hard-clamped onto the bound. Hard-clamping piles
        // successive reflections onto the same boundary coordinate, collapsing the complex to a
        // single boundary point and stalling (the per-step dynamic-optimization failure, §13a).
        let alpha = 1.3;
        let reflect = |worst_pt: &[f64], factor: f64| -> Vec<f64> {
            let mut t: Vec<f64> = (0..n)
                .map(|i| centroid[i] + factor * (centroid[i] - worst_pt[i]))
                .collect();
            retract_into_bounds(&mut t, lower, upper);
            for i in 0..n {
                if integer[i] {
                    t[i] = t[i].round().clamp(lower[i], upper[i]);
                }
            }
            t
        };
        let mut trial = reflect(&points[worst], alpha);
        let mut trial_cost = cost(&trial);
        evaluations += 1;

        // If the reflection is still the worst, halve toward the centroid until it improves.
        let mut halvings = 0;
        while trial_cost >= costs[worst] && halvings < 10 {
            for i in 0..n {
                trial[i] = (trial[i] + centroid[i]) / 2.0;
            }
            for i in 0..n {
                if integer[i] {
                    trial[i] = trial[i].round().clamp(lower[i], upper[i]);
                }
            }
            trial_cost = cost(&trial);
            evaluations += 1;
            halvings += 1;
        }

        // Accept the trial as the new worst point. Even if it did not beat the old worst, the
        // retract-toward-centroid move keeps it strictly interior, so the complex cannot pile up
        // on a bound; the geometric-convergence gate then stops the search once it has shrunk.
        points[worst] = trial;
        costs[worst] = trial_cost;

        // Convergence requires BOTH a small cost spread AND a small geometric spread. Cost
        // spread alone is unreliable: a complex straddling a symmetric valley (e.g. points on
        // both sides of `(x − c)²`) has near-equal costs while still bracketing the minimum —
        // declaring convergence there returns a boundary point, not the optimum (the per-step
        // dynamic-optimization false-convergence, §13a).
        let best = costs.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = costs.iter().cloned().filter(|c| c.is_finite()).fold(f64::NEG_INFINITY, f64::max);
        let cost_tight = (hi - best).abs() <= 1e-6 * (1.0 + best.abs());
        // Geometric spread: max |p_j - p_best| across dims, relative to the box size.
        let geom_spread = (0..n)
            .map(|i| {
                let span = (upper[i] - lower[i]).abs().max(1e-12);
                let lo = points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min);
                let hi = points.iter().map(|p| p[i]).fold(f64::NEG_INFINITY, f64::max);
                (hi - lo) / span
            })
            .fold(0.0_f64, f64::max);
        if cost_tight && geom_spread <= 1e-4 {
            converged = true;
            break;
        }
    }

    let best_idx = (0..k).min_by(|&a, &b| costs[a].total_cmp(&costs[b])).unwrap();
    let mut best_pt = points[best_idx].clone();
    let mut best_cost = costs[best_idx];

    // Coordinate-wise golden-section polish over the FULL bound of each dimension. Box's complex
    // reliably finds the basin but, with a small complex, can settle short of an interior minimum
    // near a bound; a per-dimension golden-section sweep from `best_pt` refines each coordinate
    // over [lower, upper] and can only improve. Cheap (a handful of evals per dim) and, for the
    // smooth low-dimensional per-step dynamic-optimization objectives (§13a), the decisive step
    // that pins the true optimum. Integer dims are skipped (their box is already discrete).
    for i in 0..n {
        if integer[i] {
            continue;
        }
        let (mut a, mut b) = (lower[i], upper[i]);
        const INV_PHI: f64 = 0.618_033_988_749_895;
        let mut eval_at = |x: f64, base: &[f64]| {
            let mut p = base.to_vec();
            p[i] = x;
            cost(&p)
        };
        let mut c = b - INV_PHI * (b - a);
        let mut d = a + INV_PHI * (b - a);
        let mut fc = eval_at(c, &best_pt);
        let mut fd = eval_at(d, &best_pt);
        evaluations += 2;
        for _ in 0..40 {
            if (b - a).abs() <= 1e-7 * ((a.abs() + b.abs()) + 1.0) {
                break;
            }
            if fc < fd {
                b = d;
                d = c;
                fd = fc;
                c = b - INV_PHI * (b - a);
                fc = eval_at(c, &best_pt);
            } else {
                a = c;
                c = d;
                fc = fd;
                d = a + INV_PHI * (b - a);
                fd = eval_at(d, &best_pt);
            }
            evaluations += 1;
        }
        let x = 0.5 * (a + b);
        let fx = eval_at(x, &best_pt);
        evaluations += 1;
        if fx < best_cost {
            best_cost = fx;
            best_pt[i] = x;
        }
    }

    SolveResult {
        point: best_pt,
        cost: best_cost,
        evaluations,
        converged,
    }
}

#[cfg(test)]
mod solve_tests {
    use super::*;
    #[test]
    fn solve_1d_interior_minimum() {
        // minimize (x - target)^2 over [0,20], target near the lower bound (the dynamic-opt case).
        for &target in &[3.79f64, 2.24, 3.87, 0.5, 19.5, 10.0] {
            for seed in 0..20u64 {
                let b = SearchBounds { lower: vec![0.0], upper: vec![20.0], initial: vec![10.0], integer: vec![false] };
                let r = solve(&b, seed, |p| (p[0] - target).powi(2));
                assert!((r.point[0] - target).abs() < 0.05,
                    "target={target} seed={seed}: got x={} cost={} (should be ≈{target})", r.point[0], r.cost);
            }
        }
    }
}

/// Build `SearchBounds` from an optimization spec's variables.
pub(crate) fn bounds_of(spec: &crate::model::OptimizationSpec) -> SearchBounds {
    SearchBounds {
        lower: spec.variables.iter().map(|v| v.lower.value).collect(),
        upper: spec.variables.iter().map(|v| v.upper.value).collect(),
        initial: spec.variables.iter().map(|v| v.initial.value).collect(),
        integer: spec.variables.iter().map(|v| v.integer).collect(),
    }
}

/// Run the optimization study on `model` (which must carry an `optimization` spec).
/// Box's complex method (via `solve`), evaluating each candidate as a whole static model run.
pub fn optimize(model: &Model, config: &RunConfig) -> Result<StudyResults, EngineError> {
    let spec = model
        .optimization
        .as_ref()
        .ok_or_else(|| EngineError::InvalidModel("model has no optimization spec".into()))?;
    if spec.variables.is_empty() {
        return Err(EngineError::InvalidModel("optimization has no variables".into()));
    }

    let var_ids: Vec<String> = spec.variables.iter().map(|v| v.element_id.clone()).collect();
    let bounds = bounds_of(spec);
    // The submodel/optimization objective runs its own realizations; the study is
    // deterministic given the seed.
    let seed = config.seed.or(model.simulation_settings.seed).unwrap_or(0);

    let result = solve(&bounds, seed, |p| evaluate(model, &var_ids, p, config));

    // Report the objective in its natural sense (undo the maximize flip).
    let objective = match spec.objective.direction {
        OptDirection::Minimize => result.cost,
        OptDirection::Maximize => -result.cost,
    };

    Ok(StudyResults {
        variables: var_ids
            .iter()
            .zip(&result.point)
            .map(|(id, &value)| VariableResult { element_id: id.clone(), value })
            .collect(),
        objective,
        evaluations: result.evaluations,
        converged: result.converged,
    })
}
