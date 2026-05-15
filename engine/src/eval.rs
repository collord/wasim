use std::collections::HashMap;

use crate::error::EngineError;
use crate::model::{
    AstNode, BuiltinFn, Distribution, DistributionKind, ElementKind, Quantity, QuantityOrFormula,
    TimeProperty, WasimModel,
};

// ── Value ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum Value {
    Scalar(f64),
    Vector(Vec<f64>),
}

impl Value {
    /// Collapse to a single f64. Vectors return their first element (or 0.0).
    pub fn as_scalar(&self) -> f64 {
        match self {
            Value::Scalar(v) => *v,
            Value::Vector(vs) => vs.first().copied().unwrap_or(0.0),
        }
    }

    /// Consume into Vec<f64>. Scalars become a 1-element vec.
    pub fn into_vec(self) -> Vec<f64> {
        match self {
            Value::Scalar(v) => vec![v],
            Value::Vector(vs) => vs,
        }
    }

    /// Element-wise unary op; scalars stay scalar.
    pub fn map(self, f: impl Fn(f64) -> f64) -> Value {
        match self {
            Value::Scalar(v) => Value::Scalar(f(v)),
            Value::Vector(vs) => Value::Vector(vs.into_iter().map(f).collect()),
        }
    }

    /// Element-wise binary op with scalar broadcast.
    /// (scalar, scalar) → scalar; anything else → vector.
    pub fn zip_with(self, other: Value, f: impl Fn(f64, f64) -> f64) -> Value {
        match (self, other) {
            (Value::Scalar(a), Value::Scalar(b)) => Value::Scalar(f(a, b)),
            (Value::Vector(vs), Value::Scalar(b)) => Value::Vector(vs.into_iter().map(|a| f(a, b)).collect()),
            (Value::Scalar(a), Value::Vector(vs)) => Value::Vector(vs.into_iter().map(|b| f(a, b)).collect()),
            (Value::Vector(a), Value::Vector(b)) => {
                let n = a.len().min(b.len());
                Value::Vector((0..n).map(|i| f(a[i], b[i])).collect())
            }
        }
    }
}

// ── Evaluation context ────────────────────────────────────────────────────────

pub struct EvalCtx<'a> {
    pub model: &'a WasimModel,
    /// Current-step outputs computed so far (in topo order).
    pub outputs: &'a HashMap<String, Value>,
    /// Previous-step outputs; used as fallback for self-referencing expressions.
    pub prev_outputs: &'a HashMap<String, Value>,
    /// Elapsed time in the declared timestep unit (step_index * dt).
    pub elapsed: f64,
    /// Timestep size in the declared unit.
    pub dt: f64,
    /// 0-based step index.
    pub step_index: usize,
}

impl<'a> EvalCtx<'a> {
    fn calendar(&self) -> CalendarState {
        CalendarState::from_step(self.step_index, self.dt, &self.model.simulation_settings.timestep.unit)
    }
}

struct CalendarState {
    month: u32,
    day_of_month: u32,
    days_in_month: u32,
    day_of_year: u32,
    year_offset: u32,
}

static DAYS_PER_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

impl CalendarState {
    fn from_step(step_index: usize, dt: f64, dt_unit: &str) -> Self {
        match dt_unit {
            "mo" | "month" => {
                let total_months = step_index as u32;
                let year_offset = total_months / 12;
                let month = (total_months % 12) + 1;
                let days_in_month = DAYS_PER_MONTH[(month - 1) as usize];
                CalendarState {
                    month,
                    day_of_month: 1,
                    days_in_month,
                    day_of_year: DAYS_PER_MONTH[..(month - 1) as usize].iter().sum::<u32>() + 1,
                    year_offset,
                }
            }
            "d" | "day" => {
                let total_days = (step_index as f64 * dt) as u32;
                let day_of_year = total_days % 365;
                let year_offset = total_days / 365;
                let mut remaining = day_of_year;
                let mut month = 1u32;
                for &dim in &DAYS_PER_MONTH {
                    if remaining < dim { break; }
                    remaining -= dim;
                    month += 1;
                }
                let month = month.min(12);
                CalendarState {
                    month,
                    day_of_month: remaining + 1,
                    days_in_month: DAYS_PER_MONTH[(month - 1) as usize],
                    day_of_year: day_of_year + 1,
                    year_offset,
                }
            }
            _ => CalendarState {
                month: 1,
                day_of_month: 1,
                days_in_month: 31,
                day_of_year: 1,
                year_offset: step_index as u32,
            },
        }
    }
}

// ── Public entry points ───────────────────────────────────────────────────────

/// Evaluate an AST node to a `Value` (scalar or vector).
pub fn eval_ast(node: &AstNode, ctx: &EvalCtx) -> Result<Value, EngineError> {
    match node {
        AstNode::Literal { value, .. } => Ok(Value::Scalar(*value)),

        AstNode::Ref { element_id, .. } => {
            // Lookup elements don't self-evaluate; return their y-column as a vector
            // so that sum_array/interp_array/dot_product work on lookup refs.
            if let Some(elem) = ctx.model.elements.iter().find(|e| &e.id == element_id) {
                if let ElementKind::Lookup { y, columns, .. } = &elem.kind {
                    let data = if !columns.is_empty() { columns[0].clone() } else { y.clone() };
                    return Ok(Value::Vector(data));
                }
            }
            Ok(ctx.outputs.get(element_id.as_str())
                .or_else(|| ctx.prev_outputs.get(element_id.as_str()))
                .cloned()
                // Dangling refs fall back to 0.0 (same policy as graph builder).
                .unwrap_or(Value::Scalar(0.0)))
        }

        AstNode::TimeRef { property } => {
            let cal = ctx.calendar();
            let v = match property {
                TimeProperty::Elapsed    => ctx.elapsed,
                TimeProperty::Timestep   => ctx.dt,
                TimeProperty::Year       => cal.year_offset as f64,
                TimeProperty::Month      => cal.month as f64,
                TimeProperty::DayOfYear  => cal.day_of_year as f64,
                TimeProperty::DayOfMonth => cal.day_of_month as f64,
                TimeProperty::DaysInMonth => cal.days_in_month as f64,
            };
            Ok(Value::Scalar(v))
        }

        // Binary ops — element-wise with scalar broadcast
        AstNode::Add      { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| a + b)),
        AstNode::Subtract { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| a - b)),
        AstNode::Multiply { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| a * b)),
        AstNode::Divide   { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| a / b)),
        AstNode::Power    { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| a.powf(b))),

        // Comparisons — element-wise, return 1.0/0.0
        AstNode::Lt  { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| bool_val(a < b))),
        AstNode::Gt  { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| bool_val(a > b))),
        AstNode::Lte { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| bool_val(a <= b))),
        AstNode::Gte { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| bool_val(a >= b))),
        AstNode::Eq  { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| bool_val((a - b).abs() < f64::EPSILON))),
        AstNode::Neq { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| bool_val((a - b).abs() >= f64::EPSILON))),
        AstNode::And { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| bool_val(is_true(a) && is_true(b)))),
        AstNode::Or  { left, right } => Ok(eval_ast(left, ctx)?.zip_with(eval_ast(right, ctx)?, |a, b| bool_val(is_true(a) || is_true(b)))),

        // Unary — element-wise
        AstNode::Neg { operand } => Ok(eval_ast(operand, ctx)?.map(|v| -v)),
        AstNode::Not { operand } => Ok(eval_ast(operand, ctx)?.map(|v| bool_val(!is_true(v)))),

        // Conditional: scalar cond → lazy branch selection; vector cond → element-wise
        AstNode::If { cond, then, else_ } => {
            match eval_ast(cond, ctx)? {
                Value::Scalar(c) => {
                    if is_true(c) { eval_ast(then, ctx) } else { eval_ast(else_, ctx) }
                }
                Value::Vector(cs) => {
                    let then_vs = eval_ast(then, ctx)?.into_vec();
                    let else_vs = eval_ast(else_, ctx)?.into_vec();
                    Ok(Value::Vector(cs.iter().enumerate().map(|(i, &c)| {
                        if is_true(c) { then_vs.get(i).copied().unwrap_or(0.0) }
                        else          { else_vs.get(i).copied().unwrap_or(0.0) }
                    }).collect()))
                }
            }
        }

        // Built-in function call
        AstNode::Call { func, args } => eval_call(func, args, ctx),

        // Lookup table call — element-wise when input is a vector
        AstNode::LookupCall { element_id, input, input2 } => {
            let x_val = eval_ast(input, ctx)?;
            let col = input2.as_deref().map(|n| eval_ast_scalar(n, ctx)).transpose()?;
            match x_val {
                Value::Scalar(x) => Ok(Value::Scalar(eval_lookup(element_id, x, col, ctx)?)),
                Value::Vector(xs) => {
                    let ys: Result<Vec<f64>, _> = xs.iter()
                        .map(|&x| eval_lookup(element_id, x, col, ctx))
                        .collect();
                    Ok(Value::Vector(ys?))
                }
            }
        }

        // Array construction → Vector
        AstNode::Array { elements } => {
            let vals: Result<Vec<f64>, _> = elements.iter()
                .map(|e| eval_ast_scalar(e, ctx))
                .collect();
            Ok(Value::Vector(vals?))
        }
    }
}

/// Evaluate and collapse to f64. Vectors return their first element.
pub fn eval_ast_scalar(node: &AstNode, ctx: &EvalCtx) -> Result<f64, EngineError> {
    eval_ast(node, ctx).map(|v| v.as_scalar())
}

/// Return a copy of `dist` with any `QuantityOrFormula::Expression` parameters replaced by
/// the evaluated scalar (wrapped as `QuantityOrFormula::Quantity`). `Quantity` and `Formula`
/// variants pass through unchanged — `Formula` strings still degrade to 0.0 at `.value()`.
pub fn resolve_distribution(dist: &Distribution, ctx: &EvalCtx) -> Result<Distribution, EngineError> {
    let kind = match &dist.kind {
        DistributionKind::Normal { mean, stddev } => DistributionKind::Normal {
            mean: resolve_qof(mean, ctx)?,
            stddev: resolve_qof(stddev, ctx)?,
        },
        DistributionKind::Lognormal { mean, stddev } => DistributionKind::Lognormal {
            mean: resolve_qof(mean, ctx)?,
            stddev: resolve_qof(stddev, ctx)?,
        },
        DistributionKind::LognormalMoments { mean, stddev } => DistributionKind::LognormalMoments {
            mean: resolve_qof(mean, ctx)?,
            stddev: resolve_qof(stddev, ctx)?,
        },
        DistributionKind::Exponential { mean } => DistributionKind::Exponential {
            mean: resolve_qof(mean, ctx)?,
        },
        other => other.clone(),
    };
    Ok(Distribution {
        kind,
        truncation: dist.truncation.clone(),
        correlation_group: dist.correlation_group.clone(),
    })
}

fn resolve_qof(qof: &QuantityOrFormula, ctx: &EvalCtx) -> Result<QuantityOrFormula, EngineError> {
    match qof {
        QuantityOrFormula::Expression(ef) => {
            let val = eval_ast(&ef.ast, ctx)?.as_scalar();
            Ok(QuantityOrFormula::Quantity(Quantity {
                value: val,
                unit: "1".to_string(),
                display_unit: None,
            }))
        }
        // Quantity and Formula pass through. Formula remains a no-op (0.0 at .value()).
        _ => Ok(qof.clone()),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bool_val(b: bool) -> f64 { if b { 1.0 } else { 0.0 } }
fn is_true(v: f64) -> bool  { v != 0.0 }

fn eval_call(func: &BuiltinFn, args: &[AstNode], ctx: &EvalCtx) -> Result<Value, EngineError> {
    // Array-consuming functions: evaluate first arg as a vector, return scalar.
    match func {
        BuiltinFn::SumArray => {
            require_args("sum_array", args.len(), 1, 1)?;
            return Ok(Value::Scalar(eval_ast(&args[0], ctx)?.into_vec().iter().sum()));
        }
        BuiltinFn::MeanArray => {
            require_args("mean_array", args.len(), 1, 1)?;
            let v = eval_ast(&args[0], ctx)?.into_vec();
            return Ok(Value::Scalar(if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }));
        }
        BuiltinFn::MinArray => {
            require_args("min_array", args.len(), 1, 1)?;
            let v = eval_ast(&args[0], ctx)?.into_vec();
            return Ok(Value::Scalar(v.iter().cloned().fold(f64::INFINITY, f64::min)));
        }
        BuiltinFn::SizeArray => {
            require_args("size_array", args.len(), 1, 1)?;
            return Ok(Value::Scalar(eval_ast(&args[0], ctx)?.into_vec().len() as f64));
        }
        BuiltinFn::GetElement => {
            require_args("get_element", args.len(), 2, 2)?;
            let v = eval_ast(&args[0], ctx)?.into_vec();
            let i = eval_ast_scalar(&args[1], ctx)?;
            let idx = i.round() as i64 - 1;
            return Ok(Value::Scalar(
                if idx >= 0 && (idx as usize) < v.len() { v[idx as usize] } else { 0.0 }
            ));
        }
        BuiltinFn::DotProduct => {
            require_args("dot_product", args.len(), 2, 2)?;
            let a = eval_ast(&args[0], ctx)?.into_vec();
            let b = eval_ast(&args[1], ctx)?.into_vec();
            let n = a.len().min(b.len());
            return Ok(Value::Scalar((0..n).map(|i| a[i] * b[i]).sum()));
        }
        BuiltinFn::InterpArray => {
            // interp_array(yvec, x): linear interp into yvec at fractional 1-based index x.
            require_args("interp_array", args.len(), 2, 2)?;
            let v = eval_ast(&args[0], ctx)?.into_vec();
            let x = eval_ast_scalar(&args[1], ctx)?;
            if v.is_empty() { return Ok(Value::Scalar(0.0)); }
            let xi = (x - 1.0).clamp(0.0, (v.len() - 1) as f64);
            let lo = xi.floor() as usize;
            let hi = (lo + 1).min(v.len() - 1);
            let t = xi - lo as f64;
            return Ok(Value::Scalar(v[lo] + t * (v[hi] - v[lo])));
        }
        _ => {}
    }

    // Scalar functions: evaluate args as scalars.
    let vals: Vec<f64> = args.iter().map(|a| eval_ast_scalar(a, ctx)).collect::<Result<_, _>>()?;
    let n = vals.len();

    let result = match func {
        BuiltinFn::Min   => { require_args("min", n, 1, usize::MAX)?; vals.iter().cloned().fold(f64::INFINITY, f64::min) }
        BuiltinFn::Max   => { require_args("max", n, 1, usize::MAX)?; vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max) }
        BuiltinFn::Abs   => { require_args("abs",   n, 1, 1)?; vals[0].abs() }
        BuiltinFn::Sqrt  => { require_args("sqrt",  n, 1, 1)?; vals[0].sqrt() }
        BuiltinFn::Exp   => { require_args("exp",   n, 1, 1)?; vals[0].exp() }
        BuiltinFn::Ln    => { require_args("ln",    n, 1, 1)?; vals[0].ln() }
        BuiltinFn::Log   => { require_args("log",   n, 1, 1)?; vals[0].log10() }
        BuiltinFn::Sin   => { require_args("sin",   n, 1, 1)?; vals[0].sin() }
        BuiltinFn::Cos   => { require_args("cos",   n, 1, 1)?; vals[0].cos() }
        BuiltinFn::Tan   => { require_args("tan",   n, 1, 1)?; vals[0].tan() }
        BuiltinFn::Asin  => { require_args("asin",  n, 1, 1)?; vals[0].asin() }
        BuiltinFn::Acos  => { require_args("acos",  n, 1, 1)?; vals[0].acos() }
        BuiltinFn::Atan  => { require_args("atan",  n, 1, 1)?; vals[0].atan() }
        BuiltinFn::Atan2 => { require_args("atan2", n, 2, 2)?; vals[0].atan2(vals[1]) }
        BuiltinFn::Floor => { require_args("floor", n, 1, 1)?; vals[0].floor() }
        BuiltinFn::Ceil  => { require_args("ceil",  n, 1, 1)?; vals[0].ceil() }
        BuiltinFn::Round => { require_args("round", n, 1, 1)?; vals[0].round() }
        BuiltinFn::Mod   => { require_args("mod",   n, 2, 2)?; vals[0] % vals[1] }
        BuiltinFn::Sign  => { require_args("sign",  n, 1, 1)?; vals[0].signum() }
        BuiltinFn::Int   => { require_args("int",   n, 1, 1)?; vals[0].trunc() }
        BuiltinFn::Step  => { require_args("step",  n, 1, 1)?; if vals[0] >= 0.0 { 1.0 } else { 0.0 } }
        BuiltinFn::Tanh  => { require_args("tanh",  n, 1, 1)?; vals[0].tanh() }
        BuiltinFn::SumArray | BuiltinFn::SizeArray | BuiltinFn::GetElement
        | BuiltinFn::InterpArray | BuiltinFn::MeanArray | BuiltinFn::MinArray
        | BuiltinFn::DotProduct => unreachable!(),
    };
    Ok(Value::Scalar(result))
}

fn require_args(name: &str, got: usize, min: usize, max: usize) -> Result<(), EngineError> {
    if got < min || got > max {
        return Err(EngineError::Eval(format!(
            "function '{name}' expects {min}–{max} args, got {got}"
        )));
    }
    Ok(())
}

fn eval_lookup(element_id: &str, x: f64, col: Option<f64>, ctx: &EvalCtx) -> Result<f64, EngineError> {
    let elem = ctx.model.elements.iter().find(|e| e.id == element_id)
        .ok_or_else(|| EngineError::ElementNotFound(element_id.to_string()))?;

    let (xs, columns, y, extrap) = match &elem.kind {
        ElementKind::Lookup { x, columns, y, extrapolation, .. } => (x, columns, y, extrapolation),
        _ => return Ok(0.0),
    };

    if xs.is_empty() { return Ok(0.0); }

    let ys: &[f64] = if !columns.is_empty() {
        let idx = col.unwrap_or(1.0) as usize;
        let col_idx = idx.saturating_sub(1);
        columns.get(col_idx).map(|v| v.as_slice()).ok_or_else(|| {
            EngineError::Eval(format!(
                "lookup '{element_id}' has {} column(s), requested column {idx}",
                columns.len()
            ))
        })?
    } else {
        y.as_slice()
    };

    interp1d(xs, ys, x, extrap, element_id)
}

fn interp1d(
    xs: &[f64],
    ys: &[f64],
    x: f64,
    extrap: &crate::model::ExtrapolationMethod,
    elem_id: &str,
) -> Result<f64, EngineError> {
    use crate::model::ExtrapolationMethod;

    let n = xs.len();
    let x_lo = xs[0];
    let x_hi = xs[n - 1];

    if x <= x_lo {
        return match extrap {
            ExtrapolationMethod::Clamp  => Ok(ys[0]),
            ExtrapolationMethod::Linear => {
                if n < 2 { return Ok(ys[0]); }
                let slope = (ys[1] - ys[0]) / (xs[1] - xs[0]);
                Ok(ys[0] + slope * (x - xs[0]))
            }
            ExtrapolationMethod::Error => Err(EngineError::LookupRange(elem_id.to_string(), x)),
        };
    }
    if x >= x_hi {
        return match extrap {
            ExtrapolationMethod::Clamp  => Ok(ys[n - 1]),
            ExtrapolationMethod::Linear => {
                if n < 2 { return Ok(ys[n - 1]); }
                let slope = (ys[n - 1] - ys[n - 2]) / (xs[n - 1] - xs[n - 2]);
                Ok(ys[n - 1] + slope * (x - xs[n - 1]))
            }
            ExtrapolationMethod::Error => Err(EngineError::LookupRange(elem_id.to_string(), x)),
        };
    }

    let mut lo = 0;
    let mut hi = n - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if xs[mid] <= x { lo = mid; } else { hi = mid; }
    }
    let t = (x - xs[lo]) / (xs[hi] - xs[lo]);
    Ok(ys[lo] + t * (ys[hi] - ys[lo]))
}
