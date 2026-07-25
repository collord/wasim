//! Array lane (ARRAY_LANE_DESIGN.md, Phase A) — an opt-in columnar evaluator for
//! the **flat independent Monte-Carlo** subset of a v2 model. Instead of the scalar
//! realization loop, each value is a `Run`-column (a `Vec<f64>` of length
//! n_realizations) and every element's expression is evaluated **once** over the
//! column via the shared `eval_ast` (whose `zip_with` already does elementwise
//! arithmetic over the column). `run_stat` reduces a column directly — natively,
//! no two-pass MC re-run.
//!
//! Scope (Phase A): correctness first, not yet speed (materialized columns; the
//! fusing kernel is Phase B). Eligibility is deliberately narrow — no dimensions,
//! stocks, submodels, state-machine node rules, sampling correlations, or
//! array/lookup/time constructs — so draws exactly mirror the scalar lane and the
//! results are **bit-identical** to it. When a model is ineligible the caller falls
//! back to the scalar lane, so the default path is untouched.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::engine::{ElementResults, RunConfig, SimulationResults};
use crate::error::EngineError;
use crate::eval::{eval_ast, resolve_distribution, run_stat_key, EvalCtx, LookupData, Value};
use crate::graph_v2::ModelGraphV2;
use crate::model::{AstNode, SubmodelStatKind};
use crate::model_v2::{ContainerKind, Element, FixedValue, Model, NodeRule, Primitive};
use crate::sampling;

/// Is `model` runnable on the array lane? `Ok(())` if so, else a human reason.
pub fn eligible(model: &Model) -> Result<(), String> {
    if !model.dimensions.is_empty() {
        return Err("model has dimensions (array axes) — Phase A is flat Monte-Carlo only".into());
    }
    if model.containers.iter().any(|c| c.kind == ContainerKind::Submodel) {
        return Err("model has submodels".into());
    }
    for e in &model.elements {
        match &e.primitive {
            Primitive::Node(n) => match &n.rule {
                NodeRule::Fixed { value: FixedValue::Scalar(_), .. } => {}
                NodeRule::Fixed { value: FixedValue::Array { .. }, .. } => {
                    return Err(format!("{}: fixed array (needs a dimension axis)", e.id()));
                }
                NodeRule::Sample { resampling, autocorrelation, correlations, distribution } => {
                    if resampling.is_some() || autocorrelation.is_some()
                        || !correlations.is_empty() || distribution.importance.is_some()
                    {
                        return Err(format!("{}: sample uses resampling/autocorrelation/correlation/importance", e.id()));
                    }
                }
                NodeRule::Expression(ef) => expr_allowed(&ef.ast)
                    .map_err(|op| format!("{}: expression uses unsupported op `{op}`", e.id()))?,
                other => return Err(format!("{}: node rule {:?} not array-lane-eligible", e.id(), std::mem::discriminant(other))),
            },
            _ => return Err(format!("{}: non-node primitive (stock/gate/…) not eligible", e.id())),
        }
    }
    Ok(())
}

/// Only pure elementwise arithmetic / comparison / conditional + `run_stat` map to
/// a columnar pass today. Reductions over dimensions, submodels, lookups, builtins
/// (which collapse a vector to a scalar), time, and array construction are excluded.
fn expr_allowed(node: &AstNode) -> Result<(), &'static str> {
    match node {
        AstNode::Literal { .. } | AstNode::Ref { .. } => Ok(()),
        AstNode::Add { left, right } | AstNode::Subtract { left, right }
        | AstNode::Multiply { left, right } | AstNode::Divide { left, right }
        | AstNode::Power { left, right } | AstNode::Lt { left, right }
        | AstNode::Gt { left, right } | AstNode::Lte { left, right }
        | AstNode::Gte { left, right } | AstNode::Eq { left, right }
        | AstNode::Neq { left, right } | AstNode::And { left, right }
        | AstNode::Or { left, right } => { expr_allowed(left)?; expr_allowed(right) }
        AstNode::Neg { operand } | AstNode::Not { operand } => expr_allowed(operand),
        AstNode::If { cond, then, else_ } => { expr_allowed(cond)?; expr_allowed(then)?; expr_allowed(else_) }
        AstNode::RunStat { arg, .. } => match arg {
            Some(a) => expr_allowed(a),
            None => Ok(()),
        },
        AstNode::Call { .. } => Err("call"),
        AstNode::LookupCall { .. } => Err("lookup_call"),
        AstNode::SubmodelStat { .. } => Err("submodel_stat"),
        AstNode::VectorMap { .. } => Err("vector_map"),
        AstNode::Index { .. } => Err("index"),
        AstNode::IndexRef { .. } => Err("index_ref"),
        AstNode::Subscript { .. } => Err("subscript"),
        AstNode::Array { .. } => Err("array"),
        AstNode::TimeRef { .. } => Err("time_ref"),
        AstNode::ExternCall { .. } => Err("extern_call"),
    }
}

/// Owns the empty collaborators an `EvalCtx` needs, so the lane can hand out a
/// minimal ctx borrowing just the live `outputs` and `run_stats`.
struct LaneEnv {
    lookups: HashMap<String, LookupData>,
    labels: HashMap<String, Vec<String>>,
    dims: HashMap<String, usize>,
    sub: HashMap<(String, String), Vec<f64>>,
    prev: HashMap<String, Value>,
    index_stack: RefCell<Vec<usize>>,
    fired: RefCell<HashSet<String>>,
    dt: f64,
    dt_unit: String,
    calendar_start: Option<f64>,
}

impl LaneEnv {
    fn ctx<'a>(&'a self, outputs: &'a HashMap<String, Value>, run_stats: &'a HashMap<String, f64>) -> EvalCtx<'a> {
        EvalCtx {
            lookups: &self.lookups, outputs, prev_outputs: &self.prev, elapsed: 0.0, dt: self.dt,
            dt_unit: &self.dt_unit, step_index: 0, dimensions: &self.dims, dim_labels: &self.labels,
            run_stats, index_stack: &self.index_stack, submodel_outputs: &self.sub, lag: None,
            fired_events: &self.fired, calendar_start: self.calendar_start,
        }
    }
}

/// Run an eligible model on the array lane. Result is bit-identical to the scalar lane.
pub fn run_array_lane(
    model: &Model,
    graph: &ModelGraphV2,
    config: &RunConfig,
) -> Result<SimulationResults, EngineError> {
    let n = config.n_realizations.unwrap_or(model.simulation_settings.n_realizations) as usize;
    let seed = config.seed.or(model.simulation_settings.seed).unwrap_or(0);
    let env = LaneEnv {
        lookups: HashMap::new(), labels: HashMap::new(), dims: HashMap::new(),
        sub: HashMap::new(), prev: HashMap::new(),
        index_stack: RefCell::new(Vec::new()), fired: RefCell::new(HashSet::new()),
        dt: model.simulation_settings.timestep.value,
        dt_unit: model.simulation_settings.timestep.unit.clone(),
        calendar_start: model.simulation_settings.calendar_start,
    };
    let no_run_stats: HashMap<String, f64> = HashMap::new();

    // ── 1. Sampling: mirror the scalar loop's per-realization draws exactly ──
    // (independent Monte-Carlo, model element order, per-realization ChaCha8 stream).
    let mut sample_cols: HashMap<&str, Vec<f64>> = model.elements.iter()
        .filter(|e| matches!(&e.primitive, Primitive::Node(n) if matches!(n.rule, NodeRule::Sample { .. })))
        .map(|e| (e.id(), Vec::with_capacity(n)))
        .collect();

    for r in 0..n {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        rng.set_stream(r as u64);
        // dist_ctx: fixed scalars + draws so far this realization (for formula params).
        let mut dist_ctx: HashMap<String, Value> = HashMap::new();
        for e in &model.elements {
            if let Primitive::Node(n) = &e.primitive {
                if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &n.rule {
                    dist_ctx.insert(e.id().to_string(), Value::Scalar(q.value));
                }
            }
        }
        for e in &model.elements {
            if let Primitive::Node(n) = &e.primitive {
                if let NodeRule::Sample { distribution, .. } = &n.rule {
                    let resolved = {
                        let ctx = env.ctx(&dist_ctx, &no_run_stats);
                        resolve_distribution(distribution, &ctx)?
                    };
                    let v = sampling::sample(&resolved.kind, &resolved.truncation, &mut rng)?;
                    sample_cols.get_mut(e.id()).unwrap().push(v);
                    dist_ctx.insert(e.id().to_string(), Value::Scalar(v));
                }
            }
        }
    }

    let mut columns: HashMap<String, Value> = HashMap::new();
    for (id, col) in sample_cols {
        columns.insert(id.to_string(), Value::Vector(col));
    }
    // Fixed scalars: kept as Scalar so `⊕ vector` broadcasts across the Run column.
    for e in &model.elements {
        if let Primitive::Node(n) = &e.primitive {
            if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &n.rule {
                columns.insert(e.id().to_string(), Value::Scalar(q.value));
            }
        }
    }

    // ── 2. Evaluate expressions columnar, in topo order. Two passes iff run_stat. ──
    let run_stat_targets = collect_run_stats(model)?;
    let expr_order: Vec<&str> = graph.topo_order.iter()
        .map(|s| s.as_str())
        .filter(|id| is_expression(model, id))
        .collect();

    eval_expressions(model, &expr_order, &mut columns, &no_run_stats, &env)?;

    if !run_stat_targets.is_empty() {
        let mut reduced: HashMap<String, f64> = HashMap::new();
        for t in &run_stat_targets {
            let samples = columns.get(&t.element_id).map(|v| v.clone().into_vec()).unwrap_or_default();
            reduced.insert(t.key.clone(), reduce_run_stat(&samples, &t.stat, t.arg));
        }
        eval_expressions(model, &expr_order, &mut columns, &reduced, &env)?;
    }

    // ── 3. Assemble results (single step; columns → final_values) ──
    let mut elements = HashMap::new();
    for e in &model.elements {
        let col = match columns.get(e.id()) {
            Some(Value::Vector(v)) => v.clone(),
            Some(Value::Scalar(s)) => vec![*s; n],
            Some(other) => other.clone().into_vec(),
            None => continue,
        };
        elements.insert(e.id().to_string(), ElementResults {
            label: e.base.name.clone(),
            unit: primary_unit(e).to_string(),
            final_values: col,
            time_history: None,
            analysis: None,
        });
    }
    let output_ids: Vec<String> = graph.topo_order.iter()
        .filter(|id| elements.contains_key(id.as_str())).cloned().collect();

    Ok(SimulationResults {
        time_axis: vec![0.0],
        time_unit: env.dt_unit,
        elements,
        n_realizations: n as u32,
        n_steps: 1,
        output_ids,
    })
}

fn is_expression(model: &Model, id: &str) -> bool {
    model.elements.iter().any(|e| e.id() == id
        && matches!(&e.primitive, Primitive::Node(n) if matches!(n.rule, NodeRule::Expression(_))))
}

fn eval_expressions(
    model: &Model,
    order: &[&str],
    columns: &mut HashMap<String, Value>,
    run_stats: &HashMap<String, f64>,
    env: &LaneEnv,
) -> Result<(), EngineError> {
    for id in order {
        let ast = model.elements.iter().find(|e| e.id() == *id).and_then(|e| match &e.primitive {
            Primitive::Node(n) => match &n.rule { NodeRule::Expression(ef) => Some(&ef.ast), _ => None },
            _ => None,
        });
        if let Some(ast) = ast {
            let val = {
                let ctx = env.ctx(columns, run_stats);
                eval_ast(ast, &ctx)?
            };
            columns.insert(id.to_string(), val);
        }
    }
    Ok(())
}

// ── run_stat collection + reduction (mirrors engine_v2, kept local to the lane) ──

struct RunStatTarget { key: String, element_id: String, stat: SubmodelStatKind, arg: f64 }

fn collect_run_stats(model: &Model) -> Result<Vec<RunStatTarget>, EngineError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for e in &model.elements {
        if let Primitive::Node(n) = &e.primitive {
            if let NodeRule::Expression(ef) = &n.rule {
                walk_run_stats(&ef.ast, &mut out, &mut seen)?;
            }
        }
    }
    Ok(out)
}

fn walk_run_stats(node: &AstNode, out: &mut Vec<RunStatTarget>, seen: &mut HashSet<String>) -> Result<(), EngineError> {
    use AstNode::*;
    match node {
        RunStat { element_id, statistic, arg } => {
            let needs_arg = matches!(statistic,
                SubmodelStatKind::Percentile | SubmodelStatKind::CumulativeProb
                | SubmodelStatKind::Exceedance | SubmodelStatKind::Cte);
            let arg_val = match arg.as_deref() {
                Some(AstNode::Literal { value, .. }) => *value,
                None => 0.0,
                Some(_) if needs_arg => return Err(EngineError::InvalidModel(format!(
                    "run_stat on '{element_id}' with {statistic:?} requires a literal argument"))),
                Some(_) => 0.0,
            };
            let key = run_stat_key(element_id, statistic, arg_val);
            if seen.insert(key.clone()) {
                out.push(RunStatTarget { key, element_id: element_id.clone(), stat: statistic.clone(), arg: arg_val });
            }
            if let Some(a) = arg { walk_run_stats(a, out, seen)?; }
        }
        Add { left, right } | Subtract { left, right } | Multiply { left, right }
        | Divide { left, right } | Power { left, right } | Lt { left, right }
        | Gt { left, right } | Lte { left, right } | Gte { left, right }
        | Eq { left, right } | Neq { left, right } | And { left, right } | Or { left, right } => {
            walk_run_stats(left, out, seen)?; walk_run_stats(right, out, seen)?;
        }
        Neg { operand } | Not { operand } => walk_run_stats(operand, out, seen)?,
        If { cond, then, else_ } => {
            walk_run_stats(cond, out, seen)?; walk_run_stats(then, out, seen)?; walk_run_stats(else_, out, seen)?;
        }
        _ => {}
    }
    Ok(())
}

fn reduce_run_stat(samples: &[f64], stat: &SubmodelStatKind, arg: f64) -> f64 {
    use SubmodelStatKind as K;
    match stat {
        K::Mean => crate::engine::mean(samples),
        K::Percentile => crate::engine::percentile(samples, arg),
        K::Sd => crate::engine::std(samples),
        K::CumulativeProb => crate::engine::cumulative_prob(samples, arg),
        K::Exceedance => crate::engine::exceedance(samples, arg),
        K::Cte => crate::engine::cte(samples, arg),
        K::Sum => crate::engine::sum_of(samples),
        K::Min => crate::engine::min_of(samples),
        K::Max => crate::engine::max_of(samples),
    }
}

fn primary_unit(elem: &Element) -> &str {
    if let Primitive::Node(n) = &elem.primitive {
        if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &n.rule {
            return &q.unit;
        }
    }
    elem.base.outputs.first().map(|o| o.unit.as_str()).unwrap_or("1")
}
