use std::cell::RefCell;
use std::collections::HashMap;

use crate::error::EngineError;
use crate::model::{
    AstNode, BuiltinFn, Distribution, DistributionKind, ExtrapolationMethod, Quantity,
    QuantityOrFormula, TimeProperty,
};

/// Lookup-table data extracted from a model, keyed by element id. Decouples the AST
/// walker from any particular model representation (v1 `WasimModel` or v2 `Model`).
#[derive(Debug, Clone, Default)]
pub struct LookupData {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// Multi-column table: each inner vec is one column, parallel to `x`. When present,
    /// `y` is ignored and the column index comes from `lookup_call`'s `input2`.
    pub columns: Vec<Vec<f64>>,
    pub extrapolation: ExtrapolationMethod,
    /// Interpolation between knots: linear (default), step (piecewise-constant), or monotone
    /// cubic (Fritsch-Carlson — no overshoot). Applies to the 1-D interpolation path.
    pub interpolation: crate::model::InterpolationMethod,
    /// Log-result interpolation (§10): interpolate ln(y) linearly and return exp. Requires
    /// y > 0 at the bracketing knots; falls back to linear where a knot is ≤ 0.
    pub log_result: bool,
    /// N-D table axes beyond x (§10): `extra_axes[0]` = 2nd-axis breakpoints, `extra_axes[1]`
    /// = 3rd-axis breakpoints. Empty for a 1-D table. When present, `nd_values` holds the
    /// flattened value grid (row-major over x, then each extra axis) and multilinear
    /// interpolation is used via a `lookup_call` with the extra-axis coordinates in `input2`.
    pub extra_axes: Vec<Vec<f64>>,
    pub nd_values: Vec<f64>,
}

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
    /// Lookup tables by element id (for `lookup_call` and lookup `ref`s).
    pub lookups: &'a HashMap<String, LookupData>,
    /// Current-step outputs computed so far (in topo order).
    pub outputs: &'a HashMap<String, Value>,
    /// Previous-step outputs; used as fallback for self-referencing expressions.
    pub prev_outputs: &'a HashMap<String, Value>,
    /// Elapsed time in the declared timestep unit (step_index * dt).
    pub elapsed: f64,
    /// Timestep size in the declared unit.
    pub dt: f64,
    /// Declared timestep unit (for calendar time properties).
    pub dt_unit: &'a str,
    /// 0-based step index.
    pub step_index: usize,
    /// Dimension id → member count, for `vector_map` comprehensions (§15).
    pub dimensions: &'a HashMap<String, usize>,
    /// Iteration-index stack for nested `vector_map`s. The innermost `vector_map`
    /// pushes its current 0-based index; `index_ref` reads the top (`row`) or the
    /// one below (`col`). Interior mutability so it survives the shared `&EvalCtx`.
    pub index_stack: &'a RefCell<Vec<usize>>,
    /// Per-realization sample vectors for submodel outputs, keyed by (submodel_id, output).
    /// Populated by a pre-pass (§12); `submodel_stat` reduces these on demand.
    pub submodel_outputs: &'a HashMap<(String, String), Vec<f64>>,
    /// The local lag value (seconds) bound while sampling a convolution response expression
    /// (§17); read by `extern_call fn:"lag"`. None outside convolution-response sampling.
    pub lag: Option<f64>,
    /// Ids of events that fired this step (§2), read by the `occurs(event_id)` builtin.
    pub fired_events: &'a std::cell::RefCell<std::collections::HashSet<String>>,
    /// Calendar anchor (B6): model-clock start as seconds since the Unix epoch. When `Some`,
    /// `time_ref` calendar properties use a real proleptic-Gregorian calendar (leap years);
    /// `None` = the fixed 365-day calendar.
    pub calendar_start: Option<f64>,
}

impl<'a> EvalCtx<'a> {
    fn calendar(&self) -> CalendarState {
        match self.calendar_start {
            // B6: real proleptic-Gregorian calendar (leap years) anchored at `start` seconds.
            Some(start) => {
                let elapsed_secs = self.elapsed * dt_unit_seconds(self.dt_unit);
                CalendarState::from_epoch_secs(start + elapsed_secs)
            }
            // Fixed 365-day calendar (behavior unchanged).
            None => CalendarState::from_elapsed(self.elapsed, self.dt_unit),
        }
    }

    /// Absolute clock time (seconds since the Unix epoch) at the current step — only meaningful
    /// with a `calendar_start` anchor. `None` without one (calendar-of-day is undefined).
    fn abs_epoch_secs(&self) -> Option<f64> {
        self.calendar_start
            .map(|start| start + self.elapsed * dt_unit_seconds(self.dt_unit))
    }

    /// Calendar (years, months) elapsed since the anchor date, counting **field boundaries
    /// crossed** (GoldSim EYear/EMonth): years = (year_now − year_start); months = the total
    /// month-field difference (year·12 + month). Not `elapsed/30` — month/year lengths vary.
    /// `None` without an anchor.
    fn elapsed_calendar(&self) -> Option<(i64, i64)> {
        let start = self.calendar_start?;
        let now = start + self.elapsed * dt_unit_seconds(self.dt_unit);
        let (y0, m0, _) = civil_from_secs(start);
        let (y1, m1, _) = civil_from_secs(now);
        let months = (y1 - y0) * 12 + (m1 as i64 - m0 as i64);
        let years = y1 - y0;
        Some((years, months))
    }
}

/// Seconds per one unit of the declared timestep unit (for converting elapsed → absolute secs).
fn dt_unit_seconds(unit: &str) -> f64 {
    match unit.trim() {
        "yr" | "year" | "years" => 365.25 * 86400.0,
        "mo" | "month" | "months" => 365.25 * 86400.0 / 12.0,
        "wk" | "week" | "weeks" => 7.0 * 86400.0,
        "d" | "day" | "days" => 86400.0,
        "h" | "hr" | "hour" | "hours" => 3600.0,
        "min" | "minute" | "minutes" => 60.0,
        "s" | "sec" | "second" | "seconds" => 1.0,
        _ => 1.0,
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
    /// Derive the (fixed 365-day) calendar from **elapsed time in `dt_unit`** rather than a step
    /// count. On a uniform grid `elapsed == step_index * dt`, so this is behavior-identical to the
    /// former step-based derivation; deriving from elapsed makes it correct at sub-interval times
    /// too (B1). `elapsed` is in the declared timestep unit: months for `mo`, days for `d`.
    fn from_elapsed(elapsed: f64, dt_unit: &str) -> Self {
        match dt_unit {
            "mo" | "month" => {
                let total_months = elapsed.floor().max(0.0) as u32;
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
                let total_days = elapsed.max(0.0) as u32;
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
            // Fallback: year_offset from elapsed treated as a year count (matches the old
            // step-index behavior when dt is in years / an unrecognized unit with 1 step = 1 unit).
            _ => CalendarState {
                month: 1,
                day_of_month: 1,
                days_in_month: 31,
                day_of_year: 1,
                year_offset: elapsed.max(0.0) as u32,
            },
        }
    }

    /// Real proleptic-Gregorian calendar (B6) from seconds since the Unix epoch — leap-year
    /// aware. `year_offset` here carries the *actual* calendar year (e.g. 2024), not an offset.
    fn from_epoch_secs(secs: f64) -> Self {
        let (year, month, day) = civil_from_secs(secs);
        // Day of year: sum of prior months' lengths (leap-aware) + day.
        let doy: u32 = (1..month).map(|m| days_in_month(year, m)).sum::<u32>() + day;
        CalendarState {
            month,
            day_of_month: day,
            days_in_month: days_in_month(year, month),
            day_of_year: doy,
            // Store the actual calendar year; TimeProperty::Year reads this.
            year_offset: year.max(0) as u32,
        }
    }
}

/// True if `year` is a leap year in the proleptic Gregorian calendar.
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in `month` (1–12) of `year`, leap-aware (February = 29 in a leap year).
fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 => if is_leap_year(year) { 29 } else { 28 },
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// ── Public entry points ───────────────────────────────────────────────────────

/// Evaluate an AST node to a `Value` (scalar or vector).
pub fn eval_ast(node: &AstNode, ctx: &EvalCtx) -> Result<Value, EngineError> {
    match node {
        AstNode::Literal { value, .. } => Ok(Value::Scalar(*value)),

        AstNode::Ref { element_id, output } => {
            // Lookup elements don't self-evaluate; return their y-column as a vector
            // so that sum_array/interp_array/dot_product work on lookup refs.
            if let Some(lk) = ctx.lookups.get(element_id.as_str()) {
                let data = if !lk.columns.is_empty() { lk.columns[0].clone() } else { lk.y.clone() };
                return Ok(Value::Vector(data));
            }
            // Output-qualified ref (§1c): a secondary port "<Name>#k" is published under
            // "<element_id>#k" (stocks with role-declared outputs). Unpublished ports fall
            // through to the element's primary value (pre-0.9.2 behavior).
            if let Some((_, suffix)) = output.rsplit_once('#') {
                let key = format!("{element_id}#{suffix}");
                if let Some(v) = ctx
                    .outputs
                    .get(key.as_str())
                    .or_else(|| ctx.prev_outputs.get(key.as_str()))
                {
                    return Ok(v.clone());
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
                // Calendar-of-day components (calendar-aware; 0 without an anchor).
                TimeProperty::Hour   => ctx.abs_epoch_secs().map(|s| secs_of_day(s).0 as f64).unwrap_or(0.0),
                TimeProperty::Minute => ctx.abs_epoch_secs().map(|s| secs_of_day(s).1 as f64).unwrap_or(0.0),
                TimeProperty::Second => ctx.abs_epoch_secs().map(|s| secs_of_day(s).2 as f64).unwrap_or(0.0),
                TimeProperty::Start  => ctx.calendar_start.unwrap_or(0.0),
                // Whole calendar months/years elapsed since the anchor date.
                TimeProperty::ElapsedMonths => ctx.elapsed_calendar().map(|(_, m)| m as f64).unwrap_or(0.0),
                TimeProperty::ElapsedYears  => ctx.elapsed_calendar().map(|(y, _)| y as f64).unwrap_or(0.0),
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

        // Lookup table call — element-wise when input is a vector. The second argument is
        // either a column selector (multi-column tables) or a reserved TBL_* mode name
        // (semantics §1b): TBL_Integral / TBL_Inverse / TBL_Inv_Integral.
        AstNode::LookupCall { element_id, input, input2 } => {
            let x_val = eval_ast(input, ctx)?;
            let mode = match input2.as_deref() {
                Some(AstNode::Ref { element_id: name, .. }) if name == "TBL_Integral" => {
                    LookupMode::Integral
                }
                Some(AstNode::Ref { element_id: name, .. }) if name == "TBL_Inverse" => {
                    LookupMode::Inverse
                }
                Some(AstNode::Ref { element_id: name, .. }) if name == "TBL_Inv_Integral" => {
                    LookupMode::InvIntegral
                }
                Some(AstNode::Ref { element_id: name, .. }) if name == "TBL_Derivative" => {
                    LookupMode::Derivative
                }
                Some(n) => LookupMode::Column(Some(eval_ast_scalar(n, ctx)?)),
                None => LookupMode::Column(None),
            };
            // N-D tables (§10): when the target table declares extra axes and `input2` is a
            // numeric coordinate (a plain column selector, not a TBL_* mode), interpolate
            // bilinearly with `input2` as the 2nd-axis coordinate. Tables without extra axes
            // keep the legacy column/1-D behavior unchanged.
            let nd_coord = match &mode {
                LookupMode::Column(Some(c)) => ctx
                    .lookups
                    .get(element_id)
                    .filter(|lk| !lk.extra_axes.is_empty())
                    .map(|_| *c),
                _ => None,
            };
            match x_val {
                Value::Scalar(x) => {
                    let v = match nd_coord {
                        Some(c) => eval_lookup_nd(element_id, x, &[c], ctx)?,
                        None => eval_lookup(element_id, x, &mode, ctx)?,
                    };
                    Ok(Value::Scalar(v))
                }
                Value::Vector(xs) => {
                    let ys: Result<Vec<f64>, _> = xs.iter()
                        .map(|&x| match nd_coord {
                            Some(c) => eval_lookup_nd(element_id, x, &[c], ctx),
                            None => eval_lookup(element_id, x, &mode, ctx),
                        })
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

        // Submodel statistic (pdf_*): reduce the submodel output's per-realization samples
        // (pre-computed by the §12 pre-pass) by the named statistic. Unresolved submodel/
        // output → 0.0 (dangling-ref policy). See wasim-engine-semantics.md §2.13.
        AstNode::SubmodelStat { submodel_id, output, statistic, arg } => {
            let key = (submodel_id.clone(), output.clone());
            let samples = match ctx.submodel_outputs.get(&key) {
                Some(s) => s,
                None => return Ok(Value::Scalar(0.0)),
            };
            let arg_val = arg
                .as_deref()
                .map(|n| eval_ast_scalar(n, ctx))
                .transpose()?
                .unwrap_or(0.0);
            let reduced = match statistic {
                crate::model::SubmodelStatKind::Mean => crate::engine::mean(samples),
                crate::model::SubmodelStatKind::Percentile => {
                    crate::engine::percentile(samples, arg_val)
                }
                crate::model::SubmodelStatKind::Sd => crate::engine::std(samples),
                crate::model::SubmodelStatKind::CumulativeProb => {
                    crate::engine::cumulative_prob(samples, arg_val)
                }
            };
            Ok(Value::Scalar(reduced))
        }

        // Array-comprehension nodes (§15). Indices are 1-based (matching `get_element` and
        // GoldSim arrays): `vector_map` pushes the current 1-based member index onto the
        // shared stack, `index_ref` reads it, `index` subtracts 1 to select.
        AstNode::VectorMap { over, body } => {
            let size = *ctx.dimensions.get(over.as_str()).unwrap_or(&0);
            if size == 0 {
                // Unknown/empty dimension: degrade to an empty vector (dangling-ref policy).
                return Ok(Value::Vector(Vec::new()));
            }
            let mut out = Vec::with_capacity(size);
            for i in 1..=size {
                ctx.index_stack.borrow_mut().push(i);
                let r = eval_ast(body, ctx);
                ctx.index_stack.borrow_mut().pop();
                out.push(r?.as_scalar());
            }
            Ok(Value::Vector(out))
        }
        AstNode::IndexRef { axis } => {
            let stack = ctx.index_stack.borrow();
            // `row` = innermost (top); `col` = the enclosing vector_map (one below).
            let v = match axis {
                crate::model::IndexAxis::Row => stack.last().copied(),
                crate::model::IndexAxis::Col => {
                    let n = stack.len();
                    if n >= 2 { stack.get(n - 2).copied() } else { None }
                }
            };
            Ok(Value::Scalar(v.unwrap_or(0) as f64))
        }
        AstNode::Index { array, indices } => {
            let v = eval_ast(array, ctx)?.into_vec();
            // First index selects the (1-based) member; a second index (matrix col) is only
            // meaningful for nested arrays, which the flat Value::Vector doesn't model — take
            // the first index for the vector case.
            let i = indices
                .first()
                .map(|n| eval_ast_scalar(n, ctx))
                .transpose()?
                .unwrap_or(0.0);
            let idx = (i.round() as i64 - 1).max(0) as usize;
            Ok(Value::Scalar(v.get(idx).copied().unwrap_or(0.0)))
        }
        // Opaque source function — preserved for round-tripping, evaluates to 0.0 (§15).
        AstNode::ExternCall { func, .. } if func == "lag" => {
            // The convolution lag variable (§17): bound while sampling a response expression,
            // else 0.0 (an unbound lag outside convolution has no meaning).
            Ok(Value::Scalar(ctx.lag.unwrap_or(0.0)))
        }
        AstNode::ExternCall { .. } => Ok(Value::Scalar(0.0)),
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
        // Continuous families whose params may be formula-valued (§2.3). Resolve each to a
        // scalar before sampling. Discrete/table families (discrete, sampled, cumulative,
        // bernoulli, discrete_uniform, external) have no formula params and pass through.
        DistributionKind::Uniform { min, max } => DistributionKind::Uniform {
            min: resolve_qof(min, ctx)?,
            max: resolve_qof(max, ctx)?,
        },
        DistributionKind::Triangular { min, mode, max } => DistributionKind::Triangular {
            min: resolve_qof(min, ctx)?,
            mode: resolve_qof(mode, ctx)?,
            max: resolve_qof(max, ctx)?,
        },
        DistributionKind::Trapezoidal { min, lower, upper, max } => DistributionKind::Trapezoidal {
            min: resolve_qof(min, ctx)?,
            lower: resolve_qof(lower, ctx)?,
            upper: resolve_qof(upper, ctx)?,
            max: resolve_qof(max, ctx)?,
        },
        DistributionKind::Gamma { shape, scale } => DistributionKind::Gamma {
            shape: resolve_qof(shape, ctx)?,
            scale: resolve_qof(scale, ctx)?,
        },
        DistributionKind::Beta { alpha, beta, min, max } => DistributionKind::Beta {
            alpha: resolve_qof(alpha, ctx)?,
            beta: resolve_qof(beta, ctx)?,
            min: min.as_ref().map(|q| resolve_qof(q, ctx)).transpose()?,
            max: max.as_ref().map(|q| resolve_qof(q, ctx)).transpose()?,
        },
        DistributionKind::Weibull { shape, scale } => DistributionKind::Weibull {
            shape: resolve_qof(shape, ctx)?,
            scale: resolve_qof(scale, ctx)?,
        },
        DistributionKind::PearsonV { shape, scale } => DistributionKind::PearsonV {
            shape: resolve_qof(shape, ctx)?,
            scale: resolve_qof(scale, ctx)?,
        },
        DistributionKind::PearsonIii { mean, stddev, skewness } => DistributionKind::PearsonIii {
            mean: resolve_qof(mean, ctx)?,
            stddev: resolve_qof(stddev, ctx)?,
            skewness: resolve_qof(skewness, ctx)?,
        },
        DistributionKind::Pert { min, mode, max } => DistributionKind::Pert {
            min: resolve_qof(min, ctx)?,
            mode: resolve_qof(mode, ctx)?,
            max: resolve_qof(max, ctx)?,
        },
        DistributionKind::Pareto { scale, shape, location } => DistributionKind::Pareto {
            scale: resolve_qof(scale, ctx)?,
            shape: resolve_qof(shape, ctx)?,
            location: location.as_ref().map(|q| resolve_qof(q, ctx)).transpose()?,
        },
        DistributionKind::ExtremeValue { location, scale } => DistributionKind::ExtremeValue {
            location: resolve_qof(location, ctx)?,
            scale: resolve_qof(scale, ctx)?,
        },
        DistributionKind::StudentT { degrees_of_freedom, location, scale } => DistributionKind::StudentT {
            degrees_of_freedom: resolve_qof(degrees_of_freedom, ctx)?,
            location: location.as_ref().map(|q| resolve_qof(q, ctx)).transpose()?,
            scale: scale.as_ref().map(|q| resolve_qof(q, ctx)).transpose()?,
        },
        // ── A4 roster additions with formula-valued params ──
        DistributionKind::LogUniform { min, max } => DistributionKind::LogUniform {
            min: resolve_qof(min, ctx)?,
            max: resolve_qof(max, ctx)?,
        },
        DistributionKind::LogTriangular { min, mode, max } => DistributionKind::LogTriangular {
            min: resolve_qof(min, ctx)?,
            mode: resolve_qof(mode, ctx)?,
            max: resolve_qof(max, ctx)?,
        },
        DistributionKind::Triangular1090 { p10, mode, p90 } => DistributionKind::Triangular1090 {
            p10: resolve_qof(p10, ctx)?,
            mode: resolve_qof(mode, ctx)?,
            p90: resolve_qof(p90, ctx)?,
        },
        DistributionKind::LogTriangular1090 { p10, mode, p90 } => DistributionKind::LogTriangular1090 {
            p10: resolve_qof(p10, ctx)?,
            mode: resolve_qof(mode, ctx)?,
            p90: resolve_qof(p90, ctx)?,
        },
        DistributionKind::Binomial { n, prob } => DistributionKind::Binomial {
            n: resolve_qof(n, ctx)?,
            prob: resolve_qof(prob, ctx)?,
        },
        DistributionKind::NegativeBinomial { r, prob } => DistributionKind::NegativeBinomial {
            r: resolve_qof(r, ctx)?,
            prob: resolve_qof(prob, ctx)?,
        },
        DistributionKind::Poisson { lambda } => DistributionKind::Poisson {
            lambda: resolve_qof(lambda, ctx)?,
        },
        DistributionKind::ExtremeProbability { base, n, extreme } => DistributionKind::ExtremeProbability {
            // The nested base distribution's params may themselves be formula-valued.
            base: Box::new(
                resolve_distribution(
                    &Distribution { kind: (**base).clone(), truncation: None, correlation_group: None, importance: None },
                    ctx,
                )?
                .kind,
            ),
            n: resolve_qof(n, ctx)?,
            extreme: *extreme,
        },
        DistributionKind::BetaSuccessFailure { successes, failures, min, max } => {
            DistributionKind::BetaSuccessFailure {
                successes: resolve_qof(successes, ctx)?,
                failures: resolve_qof(failures, ctx)?,
                min: min.as_ref().map(|q| resolve_qof(q, ctx)).transpose()?,
                max: max.as_ref().map(|q| resolve_qof(q, ctx)).transpose()?,
            }
        }
        other => other.clone(),
    };
    Ok(Distribution {
        kind,
        truncation: dist.truncation.clone(),
        correlation_group: dist.correlation_group.clone(),
        // The importance spec (biased distribution g) is carried through unresolved here; the
        // engine resolves `bias` separately when it draws the importance node.
        importance: dist.importance.clone(),
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

/// The gamma function Γ(x), via the Lanczos approximation (g=7, n=9). Accurate to ~1e-14
/// for real x. Uses the reflection formula for x < 0.5. Poles at non-positive integers
/// return infinity (Γ is undefined there); the caller degrades like any non-finite value.
fn gamma_fn(x: f64) -> f64 {
    // Lanczos coefficients (g = 7).
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection: Γ(x) = π / (sin(πx) · Γ(1−x)).
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma_fn(1.0 - x))
    } else {
        let x = x - 1.0;
        let mut a = C[0];
        let t = x + G + 0.5;
        for (i, &c) in C.iter().enumerate().skip(1) {
            a += c / (x + i as f64);
        }
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
    }
}

/// Error function erf(x), Abramowitz & Stegun 7.1.26 rational approximation (|error| ≤ 1.5e-7).
fn erf(x: f64) -> f64 {
    let sign = x.signum();
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    sign * y
}

/// Civil (year, month, day) from seconds since the sim epoch, treated as Unix time (1970-01-01).
/// Uses Howard Hinnant's days→civil algorithm. GoldSim date values are seconds since its epoch;
/// the engine treats the arg as seconds-since-1970 (the rebased convention emit uses, §14).
fn civil_from_secs(secs: f64) -> (i64, u32, u32) {
    let days = (secs / 86400.0).floor() as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (hour, minute, second) within the day from seconds since the epoch.
fn secs_of_day(secs: f64) -> (u32, u32, u32) {
    let sod = secs.rem_euclid(86400.0) as u32;
    (sod / 3600, (sod % 3600) / 60, sod % 60)
}

fn bool_val(b: bool) -> f64 { if b { 1.0 } else { 0.0 } }
fn is_true(v: f64) -> bool  { v != 0.0 }

fn eval_call(func: &BuiltinFn, args: &[AstNode], ctx: &EvalCtx) -> Result<Value, EngineError> {
    // Event predicate functions (§2). Their argument names an element/event id, so it is read
    // as a `Ref` rather than evaluated to a scalar.
    match func {
        BuiltinFn::Occurs => {
            require_args("occurs", args.len(), 1, 1)?;
            let id = match &args[0] {
                AstNode::Ref { element_id, .. } => element_id.as_str(),
                _ => return Ok(Value::Scalar(0.0)),
            };
            return Ok(Value::Scalar(if ctx.fired_events.borrow().contains(id) { 1.0 } else { 0.0 }));
        }
        BuiltinFn::Changed => {
            require_args("changed", args.len(), 1, 1)?;
            let id = match &args[0] {
                AstNode::Ref { element_id, .. } => element_id.as_str(),
                // A non-ref argument: compare the evaluated value against nothing → unchanged.
                _ => return Ok(Value::Scalar(0.0)),
            };
            let cur = ctx.outputs.get(id).or_else(|| ctx.prev_outputs.get(id)).map(|v| v.as_scalar());
            let prev = ctx.prev_outputs.get(id).map(|v| v.as_scalar());
            let changed = match (cur, prev) {
                (Some(c), Some(p)) => c != p,
                // No previous value (step 0) → treat as unchanged.
                _ => false,
            };
            return Ok(Value::Scalar(if changed { 1.0 } else { 0.0 }));
        }
        _ => {}
    }
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
        BuiltinFn::MaxArray => {
            require_args("max_array", args.len(), 1, 1)?;
            let v = eval_ast(&args[0], ctx)?.into_vec();
            return Ok(Value::Scalar(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)));
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
        // Table/array introspection: reduce over the (array) first arg (§1a).
        BuiltinFn::TableMin => {
            require_args("table_min", args.len(), 1, 1)?;
            return Ok(Value::Scalar(eval_ast(&args[0], ctx)?.into_vec().iter().cloned().fold(f64::INFINITY, f64::min)));
        }
        BuiltinFn::TableMax => {
            require_args("table_max", args.len(), 1, 1)?;
            return Ok(Value::Scalar(eval_ast(&args[0], ctx)?.into_vec().iter().cloned().fold(f64::NEG_INFINITY, f64::max)));
        }
        BuiltinFn::ColumnCount => {
            require_args("column_count", args.len(), 1, 1)?;
            return Ok(Value::Scalar(eval_ast(&args[0], ctx)?.into_vec().len() as f64));
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
        BuiltinFn::Log2  => { require_args("log2",  n, 1, 1)?; vals[0].log2() }
        BuiltinFn::Sinh  => { require_args("sinh",  n, 1, 1)?; vals[0].sinh() }
        BuiltinFn::Cosh  => { require_args("cosh",  n, 1, 1)?; vals[0].cosh() }
        BuiltinFn::Tanh  => { require_args("tanh",  n, 1, 1)?; vals[0].tanh() }
        BuiltinFn::Gamma => { require_args("gamma", n, 1, 1)?; gamma_fn(vals[0]) }
        BuiltinFn::Erf   => { require_args("erf",  n, 1, 1)?; erf(vals[0]) }
        BuiltinFn::Erfc  => { require_args("erfc", n, 1, 1)?; 1.0 - erf(vals[0]) }
        // Date extraction: the arg is seconds since the sim epoch; decompose to a civil date.
        BuiltinFn::GetYear   => { require_args("get_year",   n, 1, 1)?; civil_from_secs(vals[0]).0 as f64 }
        BuiltinFn::GetMonth  => { require_args("get_month",  n, 1, 1)?; civil_from_secs(vals[0]).1 as f64 }
        BuiltinFn::GetDay    => { require_args("get_day",    n, 1, 1)?; civil_from_secs(vals[0]).2 as f64 }
        BuiltinFn::GetHour   => { require_args("get_hour",   n, 1, 1)?; secs_of_day(vals[0]).0 as f64 }
        BuiltinFn::GetMinute => { require_args("get_minute", n, 1, 1)?; secs_of_day(vals[0]).1 as f64 }
        BuiltinFn::GetSecond => { require_args("get_second", n, 1, 1)?; secs_of_day(vals[0]).2 as f64 }
        // Finance factors.
        BuiltinFn::PvFactor      => { require_args("pv_factor", n, 2, 2)?; (1.0 + vals[0]).powf(vals[1]) }
        BuiltinFn::AnnuityFactor => {
            require_args("annuity_factor", n, 2, 2)?;
            let (r, np) = (vals[0], vals[1]);
            if r == 0.0 { np } else { (1.0 - (1.0 + r).powf(-np)) / r }
        }
        BuiltinFn::TableMin | BuiltinFn::TableMax | BuiltinFn::ColumnCount
        | BuiltinFn::SumArray | BuiltinFn::SizeArray | BuiltinFn::GetElement
        | BuiltinFn::InterpArray | BuiltinFn::MeanArray | BuiltinFn::MinArray
        | BuiltinFn::MaxArray | BuiltinFn::DotProduct
        // Event predicates are handled by the early return above; never reach the scalar path.
        | BuiltinFn::Occurs | BuiltinFn::Changed => unreachable!(),
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

/// How a `lookup_call` interprets its table (selected by the second argument, §1b).
enum LookupMode {
    /// Plain interpolation; the value selects a column on multi-column tables.
    Column(Option<f64>),
    /// ∫y dx from the first knot to x (cumulative trapezoid), x clamped into the x-range.
    Integral,
    /// Inverse table: given a y-value, return the x that maps to it (y must be monotonic).
    Inverse,
    /// Inverse of the integral: given v = ∫y dx, return the x where the integral reaches v
    /// (the stage-storage pattern: table is stage→area, v is a volume, result is the stage).
    InvIntegral,
    /// Derivative dy/dx of the interpolated table at x (slope of the bracketing segment;
    /// step interpolation → 0). §10.
    Derivative,
}

fn eval_lookup(
    element_id: &str,
    x: f64,
    mode: &LookupMode,
    ctx: &EvalCtx,
) -> Result<f64, EngineError> {
    let lk = match ctx.lookups.get(element_id) {
        Some(l) => l,
        // Non-lookup or missing target: degrade to 0.0 (matches v1's non-lookup branch).
        None => return Ok(0.0),
    };

    if lk.x.is_empty() { return Ok(0.0); }

    let col = match mode {
        LookupMode::Column(c) => *c,
        // TBL_* modes operate on the first (only meaningful) column.
        _ => None,
    };
    let ys: &[f64] = if !lk.columns.is_empty() {
        let idx = col.unwrap_or(1.0) as usize;
        let col_idx = idx.saturating_sub(1);
        lk.columns.get(col_idx).map(|v| v.as_slice()).ok_or_else(|| {
            EngineError::Eval(format!(
                "lookup '{element_id}' has {} column(s), requested column {idx}",
                lk.columns.len()
            ))
        })?
    } else {
        lk.y.as_slice()
    };

    match mode {
        LookupMode::Column(_) => {
            interp1d(&lk.x, ys, x, &lk.extrapolation, lk.interpolation, lk.log_result, element_id)
        }
        LookupMode::Integral => Ok(table_integral_at(&lk.x, ys, x)),
        LookupMode::Inverse => {
            // Invert the table: interpolate x as a function of y. Requires monotonic y;
            // a descending table is reversed into ascending order first. (Inverse always
            // interpolates linearly in y — log-result/cubic apply to the forward direction.)
            let linear = crate::model::InterpolationMethod::Linear;
            if ys.len() >= 2 && ys[0] > ys[ys.len() - 1] {
                let ys_r: Vec<f64> = ys.iter().rev().copied().collect();
                let xs_r: Vec<f64> = lk.x.iter().rev().copied().collect();
                interp1d(&ys_r, &xs_r, x, &lk.extrapolation, linear, false, element_id)
            } else {
                interp1d(ys, &lk.x, x, &lk.extrapolation, linear, false, element_id)
            }
        }
        LookupMode::InvIntegral => Ok(table_inv_integral(&lk.x, ys, x)),
        LookupMode::Derivative => Ok(table_derivative_at(&lk.x, ys, x, lk.interpolation)),
    }
}

/// Multidimensional (2-D bilinear / 3-D trilinear) table lookup. `coords` holds the extra-axis
/// coordinates (2nd axis, then 3rd) supplied via `lookup_call input2`; `x` is the first-axis
/// coordinate. Values are stored row-major over (x, axis2, axis3). Falls back to the 1-D path
/// if the table has no extra axes.
fn eval_lookup_nd(element_id: &str, x: f64, coords: &[f64], ctx: &EvalCtx) -> Result<f64, EngineError> {
    let lk = match ctx.lookups.get(element_id) {
        Some(l) => l,
        None => return Ok(0.0),
    };
    if lk.extra_axes.is_empty() || lk.nd_values.is_empty() {
        // Not an N-D table: treat the first coord as a plain column selector / ignore.
        let mode = LookupMode::Column(coords.first().copied());
        return eval_lookup(element_id, x, &mode, ctx);
    }
    let axes: Vec<&[f64]> = std::iter::once(lk.x.as_slice())
        .chain(lk.extra_axes.iter().map(|a| a.as_slice()))
        .collect();
    let mut pt = Vec::with_capacity(axes.len());
    pt.push(x);
    for (i, _) in lk.extra_axes.iter().enumerate() {
        pt.push(coords.get(i).copied().unwrap_or(0.0));
    }
    Ok(multilinear(&axes, &lk.nd_values, &pt))
}

/// Multilinear interpolation over `axes` (each an ascending breakpoint vector) of a value grid
/// `values` flattened row-major (axis 0 outermost). `pt[i]` is clamped into axis i's range.
/// Interpolates over all 2^d corners of the bracketing hypercube.
fn multilinear(axes: &[&[f64]], values: &[f64], pt: &[f64]) -> f64 {
    let d = axes.len();
    // Per-axis: lower index `i0`, and fractional weight `t` toward `i0+1`.
    let mut i0 = vec![0usize; d];
    let mut frac = vec![0.0f64; d];
    let mut strides = vec![1usize; d];
    for a in (0..d - 1).rev() {
        strides[a] = strides[a + 1] * axes[a + 1].len();
    }
    for a in 0..d {
        let ax = axes[a];
        let n = ax.len();
        if n == 1 {
            i0[a] = 0;
            frac[a] = 0.0;
            continue;
        }
        let v = pt[a].clamp(ax[0], ax[n - 1]);
        // Binary search for the bracketing interval.
        let mut lo = 0usize;
        let mut hi = n - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if ax[mid] <= v { lo = mid; } else { hi = mid; }
        }
        i0[a] = lo;
        let denom = ax[hi] - ax[lo];
        frac[a] = if denom.abs() < 1e-300 { 0.0 } else { (v - ax[lo]) / denom };
    }
    // Sum over the 2^d corners.
    let mut acc = 0.0;
    for corner in 0..(1usize << d) {
        let mut weight = 1.0;
        let mut idx = 0usize;
        for a in 0..d {
            let bit = (corner >> a) & 1;
            let n = axes[a].len();
            let hi_here = (bit == 1) && (n > 1);
            let ai = if hi_here { i0[a] + 1 } else { i0[a] };
            weight *= if hi_here { frac[a] } else if n > 1 { 1.0 - frac[a] } else { 1.0 };
            idx += ai * strides[a];
        }
        if weight != 0.0 {
            acc += weight * values.get(idx).copied().unwrap_or(0.0);
        }
    }
    acc
}

/// dy/dx of the table at x: slope of the bracketing segment (linear/cubic → segment slope,
/// step → 0). At or beyond the ends, uses the nearest interior segment slope.
fn table_derivative_at(xs: &[f64], ys: &[f64], x: f64, interp: crate::model::InterpolationMethod) -> f64 {
    use crate::model::InterpolationMethod;
    let n = xs.len().min(ys.len());
    if n < 2 {
        return 0.0;
    }
    if matches!(interp, InterpolationMethod::Step) {
        return 0.0;
    }
    // Bracketing segment index.
    let mut i = 0usize;
    while i + 1 < n && xs[i + 1] < x {
        i += 1;
    }
    if i + 1 >= n {
        i = n - 2;
    }
    let dx = xs[i + 1] - xs[i];
    if dx.abs() < 1e-300 { 0.0 } else { (ys[i + 1] - ys[i]) / dx }
}

/// Cumulative trapezoid integral of the table evaluated at `x` (clamped into the x-range,
/// so below-range → 0 and above-range → the full integral).
fn table_integral_at(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len().min(ys.len());
    if n < 2 {
        return 0.0;
    }
    let x = x.clamp(xs[0], xs[n - 1]);
    let mut acc = 0.0;
    for i in 0..n - 1 {
        if x <= xs[i] {
            break;
        }
        let seg_hi = x.min(xs[i + 1]);
        let dx_full = xs[i + 1] - xs[i];
        let t = seg_hi - xs[i];
        let slope = if dx_full != 0.0 { (ys[i + 1] - ys[i]) / dx_full } else { 0.0 };
        // ∫ over the (partial) segment: linear y ⇒ y_i·t + slope·t²/2.
        acc += ys[i] * t + 0.5 * slope * t * t;
    }
    acc
}

/// Inverse of the cumulative trapezoid integral: the x where ∫y dx reaches `v`.
/// `v` is clamped into [0, full integral]; within a segment the integral is quadratic in x,
/// solved in closed form (falling back to the linear solution for a flat slope).
fn table_inv_integral(xs: &[f64], ys: &[f64], v: f64) -> f64 {
    let n = xs.len().min(ys.len());
    if n < 2 {
        return xs.first().copied().unwrap_or(0.0);
    }
    if v <= 0.0 {
        return xs[0];
    }
    let mut acc = 0.0;
    for i in 0..n - 1 {
        let dx = xs[i + 1] - xs[i];
        let seg = 0.5 * (ys[i] + ys[i + 1]) * dx;
        if acc + seg >= v || i == n - 2 {
            let want = (v - acc).min(seg.max(0.0));
            let slope = if dx != 0.0 { (ys[i + 1] - ys[i]) / dx } else { 0.0 };
            // Solve y_i·t + slope·t²/2 = want for t ∈ [0, dx].
            let t = if slope.abs() < 1e-12 {
                if ys[i].abs() < 1e-12 { 0.0 } else { want / ys[i] }
            } else {
                let disc = ys[i] * ys[i] + 2.0 * slope * want;
                if disc >= 0.0 {
                    (-ys[i] + disc.sqrt()) / slope
                } else if ys[i].abs() > 1e-12 {
                    want / ys[i]
                } else {
                    0.0
                }
            };
            return xs[i] + t.clamp(0.0, dx.max(0.0));
        }
        acc += seg;
    }
    xs[n - 1]
}

fn interp1d(
    xs: &[f64],
    ys: &[f64],
    x: f64,
    extrap: &crate::model::ExtrapolationMethod,
    interp: crate::model::InterpolationMethod,
    log_result: bool,
    elem_id: &str,
) -> Result<f64, EngineError> {
    use crate::model::{ExtrapolationMethod, InterpolationMethod};

    let n = xs.len();
    let x_lo = xs[0];
    let x_hi = xs[n - 1];

    // Log-result interpolation (§10): interpolate ln(y) linearly, return exp. Only valid where
    // the bracketing knots are > 0; otherwise fall through to ordinary interpolation.
    if log_result && ys.iter().all(|&y| y > 0.0) {
        let lys: Vec<f64> = ys.iter().map(|y| y.ln()).collect();
        let ln_val = interp1d(xs, &lys, x, extrap, InterpolationMethod::Linear, false, elem_id)?;
        return Ok(ln_val.exp());
    }

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

    match interp {
        InterpolationMethod::Step => Ok(ys[lo]),
        InterpolationMethod::Linear => {
            let t = (x - xs[lo]) / (xs[hi] - xs[lo]);
            Ok(ys[lo] + t * (ys[hi] - ys[lo]))
        }
        InterpolationMethod::Cubic => Ok(monotone_cubic(xs, ys, x, lo)),
    }
}

/// Fritsch-Carlson monotone cubic Hermite interpolation on the segment [lo, lo+1]. Produces a
/// C¹ curve that never overshoots the data (no ringing), unlike a natural cubic spline — this
/// replaces the old silent cubic→linear downgrade. Tangents are limited per Fritsch & Carlson
/// (1980) so monotonicity of the data is preserved on each segment.
fn monotone_cubic(xs: &[f64], ys: &[f64], x: f64, lo: usize) -> f64 {
    let n = xs.len();
    // Secant slopes Δ_k = (y_{k+1} − y_k)/(x_{k+1} − x_k).
    let delta = |k: usize| -> f64 {
        let dx = xs[k + 1] - xs[k];
        if dx.abs() < 1e-300 { 0.0 } else { (ys[k + 1] - ys[k]) / dx }
    };
    // Endpoint-aware tangent m_k at knot k (one-sided at the ends; monotone-limited interior).
    let tangent = |k: usize| -> f64 {
        if n < 2 {
            return 0.0;
        }
        if k == 0 {
            return delta(0);
        }
        if k == n - 1 {
            return delta(n - 2);
        }
        let (d0, d1) = (delta(k - 1), delta(k));
        if d0 * d1 <= 0.0 {
            0.0 // local extremum → flat tangent, prevents overshoot
        } else {
            // Weighted harmonic mean (Fritsch-Carlson), weights from the interval widths.
            let (h0, h1) = (xs[k] - xs[k - 1], xs[k + 1] - xs[k]);
            let w0 = 2.0 * h1 + h0;
            let w1 = h1 + 2.0 * h0;
            (w0 + w1) / (w0 / d0 + w1 / d1)
        }
    };

    let (x0, x1) = (xs[lo], xs[lo + 1]);
    let (y0, y1) = (ys[lo], ys[lo + 1]);
    let h = x1 - x0;
    if h.abs() < 1e-300 {
        return y0;
    }
    let (m0, m1) = (tangent(lo), tangent(lo + 1));
    let t = (x - x0) / h;
    let t2 = t * t;
    let t3 = t2 * t;
    // Hermite basis.
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    h00 * y0 + h10 * h * m0 + h01 * y1 + h11 * h * m1
}
