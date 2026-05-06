use std::collections::HashMap;

use crate::error::EngineError;
use crate::model::{AstNode, BuiltinFn, ElementKind, TimeProperty, WasimModel};

pub struct EvalCtx<'a> {
    pub model: &'a WasimModel,
    /// Current-step outputs computed so far (in topo order).
    pub outputs: &'a HashMap<String, f64>,
    /// Previous-step outputs; used as fallback for self-referencing expressions.
    pub prev_outputs: &'a HashMap<String, f64>,
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
                    if remaining < dim {
                        break;
                    }
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

pub fn eval_ast(node: &AstNode, ctx: &EvalCtx) -> Result<f64, EngineError> {
    match node {
        AstNode::Literal { value, .. } => Ok(*value),

        AstNode::Ref { element_id, .. } => Ok(ctx
            .outputs
            .get(element_id)
            .or_else(|| ctx.prev_outputs.get(element_id))
            .copied()
            // Dangling refs (transpiler unit labels, sub-model outputs, loop vars)
            // fall back to 0.0 — same policy as the graph builder's inputs filter.
            .unwrap_or(0.0)),

        AstNode::TimeRef { property } => {
            let cal = ctx.calendar();
            let v = match property {
                TimeProperty::Elapsed => ctx.elapsed,
                TimeProperty::Timestep => ctx.dt,
                TimeProperty::Year => cal.year_offset as f64,
                TimeProperty::Month => cal.month as f64,
                TimeProperty::DayOfYear => cal.day_of_year as f64,
                TimeProperty::DayOfMonth => cal.day_of_month as f64,
                TimeProperty::DaysInMonth => cal.days_in_month as f64,
            };
            Ok(v)
        }

        // Binary ops
        AstNode::Add { left, right } => Ok(eval_ast(left, ctx)? + eval_ast(right, ctx)?),
        AstNode::Subtract { left, right } => Ok(eval_ast(left, ctx)? - eval_ast(right, ctx)?),
        AstNode::Multiply { left, right } => Ok(eval_ast(left, ctx)? * eval_ast(right, ctx)?),
        AstNode::Divide { left, right } => {
            let r = eval_ast(right, ctx)?;
            if r == 0.0 {
                return Err(EngineError::Eval("division by zero".into()));
            }
            Ok(eval_ast(left, ctx)? / r)
        }
        AstNode::Power { left, right } => Ok(eval_ast(left, ctx)?.powf(eval_ast(right, ctx)?)),

        // Comparisons — return 1.0 (true) or 0.0 (false)
        AstNode::Lt { left, right } => Ok(bool_val(eval_ast(left, ctx)? < eval_ast(right, ctx)?)),
        AstNode::Gt { left, right } => Ok(bool_val(eval_ast(left, ctx)? > eval_ast(right, ctx)?)),
        AstNode::Lte { left, right } => Ok(bool_val(eval_ast(left, ctx)? <= eval_ast(right, ctx)?)),
        AstNode::Gte { left, right } => Ok(bool_val(eval_ast(left, ctx)? >= eval_ast(right, ctx)?)),
        AstNode::Eq { left, right } => Ok(bool_val((eval_ast(left, ctx)? - eval_ast(right, ctx)?).abs() < f64::EPSILON)),
        AstNode::Neq { left, right } => Ok(bool_val((eval_ast(left, ctx)? - eval_ast(right, ctx)?).abs() >= f64::EPSILON)),
        AstNode::And { left, right } => Ok(bool_val(is_true(eval_ast(left, ctx)?) && is_true(eval_ast(right, ctx)?))),
        AstNode::Or { left, right } => Ok(bool_val(is_true(eval_ast(left, ctx)?) || is_true(eval_ast(right, ctx)?))),

        // Unary
        AstNode::Neg { operand } => Ok(-eval_ast(operand, ctx)?),
        AstNode::Not { operand } => Ok(bool_val(!is_true(eval_ast(operand, ctx)?))),

        // Conditional
        AstNode::If { cond, then, else_ } => {
            if is_true(eval_ast(cond, ctx)?) {
                eval_ast(then, ctx)
            } else {
                eval_ast(else_, ctx)
            }
        }

        // Built-in functions
        AstNode::Call { func, args } => eval_call(func, args, ctx),

        // Lookup table call
        AstNode::LookupCall { element_id, input, .. } => {
            let x = eval_ast(input, ctx)?;
            eval_lookup(element_id, x, ctx)
        }

        // Array construction — not yet supported in scalar engine
        AstNode::Array { .. } => Err(EngineError::Unsupported("array".into())),
    }
}

fn bool_val(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

fn is_true(v: f64) -> bool {
    v != 0.0
}

fn eval_call(func: &BuiltinFn, args: &[AstNode], ctx: &EvalCtx) -> Result<f64, EngineError> {
    let vals: Vec<f64> = args.iter().map(|a| eval_ast(a, ctx)).collect::<Result<_, _>>()?;
    let n = vals.len();

    let result = match func {
        BuiltinFn::Min => {
            require_args("min", n, 1, usize::MAX)?;
            vals.iter().cloned().fold(f64::INFINITY, f64::min)
        }
        BuiltinFn::Max => {
            require_args("max", n, 1, usize::MAX)?;
            vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        }
        BuiltinFn::Abs => { require_args("abs", n, 1, 1)?; vals[0].abs() }
        BuiltinFn::Sqrt => { require_args("sqrt", n, 1, 1)?; vals[0].sqrt() }
        BuiltinFn::Exp => { require_args("exp", n, 1, 1)?; vals[0].exp() }
        BuiltinFn::Ln => { require_args("ln", n, 1, 1)?; vals[0].ln() }
        BuiltinFn::Log => { require_args("log", n, 1, 1)?; vals[0].log10() }
        BuiltinFn::Sin => { require_args("sin", n, 1, 1)?; vals[0].sin() }
        BuiltinFn::Cos => { require_args("cos", n, 1, 1)?; vals[0].cos() }
        BuiltinFn::Tan => { require_args("tan", n, 1, 1)?; vals[0].tan() }
        BuiltinFn::Asin => { require_args("asin", n, 1, 1)?; vals[0].asin() }
        BuiltinFn::Acos => { require_args("acos", n, 1, 1)?; vals[0].acos() }
        BuiltinFn::Atan => { require_args("atan", n, 1, 1)?; vals[0].atan() }
        BuiltinFn::Atan2 => { require_args("atan2", n, 2, 2)?; vals[0].atan2(vals[1]) }
        BuiltinFn::Floor => { require_args("floor", n, 1, 1)?; vals[0].floor() }
        BuiltinFn::Ceil => { require_args("ceil", n, 1, 1)?; vals[0].ceil() }
        BuiltinFn::Round => { require_args("round", n, 1, 1)?; vals[0].round() }
        BuiltinFn::Mod => { require_args("mod", n, 2, 2)?; vals[0] % vals[1] }
        BuiltinFn::Sign => { require_args("sign", n, 1, 1)?; vals[0].signum() }
        BuiltinFn::Int => { require_args("int", n, 1, 1)?; vals[0].trunc() }
        BuiltinFn::Step => {
            require_args("step", n, 1, 1)?;
            if vals[0] >= 0.0 { 1.0 } else { 0.0 }
        }
        BuiltinFn::Tanh => { require_args("tanh", n, 1, 1)?; vals[0].tanh() }
        BuiltinFn::SumArray | BuiltinFn::SizeArray | BuiltinFn::GetElement
        | BuiltinFn::InterpArray | BuiltinFn::MeanArray | BuiltinFn::MinArray
        | BuiltinFn::DotProduct => {
            return Err(EngineError::Unsupported("array operations".into()));
        }
    };
    Ok(result)
}

fn require_args(name: &str, got: usize, min: usize, max: usize) -> Result<(), EngineError> {
    if got < min || got > max {
        return Err(EngineError::Eval(format!(
            "function '{name}' expects {min}–{max} args, got {got}"
        )));
    }
    Ok(())
}

fn eval_lookup(element_id: &str, x: f64, ctx: &EvalCtx) -> Result<f64, EngineError> {
    let elem = ctx
        .model
        .elements
        .iter()
        .find(|e| e.id == element_id)
        .ok_or_else(|| EngineError::ElementNotFound(element_id.to_string()))?;

    let (xs, ys, extrap) = match &elem.kind {
        ElementKind::Lookup { x, y, extrapolation, .. } => (x, y, extrapolation),
        _ => {
            return Err(EngineError::Eval(format!(
                "lookup_call target '{element_id}' is not a lookup element"
            )));
        }
    };

    if xs.is_empty() {
        return Err(EngineError::Eval(format!("lookup element '{element_id}' has no data")));
    }

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
            ExtrapolationMethod::Clamp => Ok(ys[0]),
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
            ExtrapolationMethod::Clamp => Ok(ys[n - 1]),
            ExtrapolationMethod::Linear => {
                if n < 2 { return Ok(ys[n - 1]); }
                let slope = (ys[n - 1] - ys[n - 2]) / (xs[n - 1] - xs[n - 2]);
                Ok(ys[n - 1] + slope * (x - xs[n - 1]))
            }
            ExtrapolationMethod::Error => Err(EngineError::LookupRange(elem_id.to_string(), x)),
        };
    }

    // Binary search for the bracketing interval
    let mut lo = 0;
    let mut hi = n - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if xs[mid] <= x { lo = mid; } else { hi = mid; }
    }

    let t = (x - xs[lo]) / (xs[hi] - xs[lo]);
    Ok(ys[lo] + t * (ys[hi] - ys[lo]))
}
