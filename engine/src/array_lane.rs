//! Array lane (ARRAY_LANE_DESIGN.md, Phases A–B) — an opt-in evaluator for the
//! **flat independent Monte-Carlo** subset of a v2 model. Instead of the scalar
//! realization loop, each value is a `Run`-column (a `Vec<f64>` of length
//! n_realizations).
//!
//! **Phase A** (columnar, shipped) evaluated each expression via the shared
//! `eval_ast`, whose `zip_with` allocates a fresh `Vec` per operation. The spike
//! measured that naive materialization as *slower* than the scalar lane — the temp
//! allocations eat the interpretation amortization.
//!
//! **Phase B** (this file) is the fix: each expression is compiled **once** into a
//! flat bytecode program (a postfix stack machine with short-circuit `if` jumps) and
//! then **fused** — evaluated in a single tight pass over the Run column, no
//! per-operation temporaries. This is Spike-1's fused form, the source of the 5×.
//! `run_stat` reduces a column directly (native axis reduction, no MC re-run).
//!
//! Eligibility is deliberately narrow — no dimensions, stocks, submodels,
//! state-machine node rules, sampling correlations, or array/lookup/time constructs
//! — so draws exactly mirror the scalar lane and the results stay **bit-identical**
//! to it (the bytecode ops reuse the scalar lane's exact f64 arithmetic, and
//! short-circuit `if` selects the same per-realization branch value the scalar lane's
//! element-wise `if` would). When a model is ineligible the caller falls back to the
//! scalar lane, so the default path is untouched.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::engine::{ElementResults, RunConfig, SimulationResults};
use crate::error::EngineError;
use crate::eval::{eval_ast, resolve_distribution, run_stat2_key, run_stat_key, EvalCtx, LookupData, Value};
use crate::graph_v2::ModelGraphV2;
use crate::model::{AstNode, BuiltinFn, RunPairStat, SubmodelStatKind};
use crate::model_v2::{ContainerKind, Element, FixedValue, Model, NodeRule, Primitive};
use crate::sampling;

/// Is `model` runnable on the array lane? `Ok(())` if so, else a human reason.
///
/// Two shapes are accepted: the **flat Monte-Carlo** subset (no dimensions — the fused
/// scalar-column path, Phases A–D) and, when the model declares dimensions, the
/// **dimensioned** subset (the correctness-first per-realization path, `dim_eligible`).
pub fn eligible(model: &Model) -> Result<(), String> {
    if !model.dimensions.is_empty() {
        return dim_eligible(model);
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
/// the fused kernel today. Reductions over dimensions, submodels, lookups, builtins
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
        // run_stat2 (bivariate) is eligible on the flat lane (Phase 2): its two target
        // columns are materialized here, so the reduction folds in over them like a
        // univariate run_stat. It carries no sub-expression to check.
        AstNode::RunStat2 { .. } => Ok(()),
        // run_regress reduces many columns via a linear solve, not a per-column fold;
        // it runs on the scalar two-pass driver (Phase 3). Mark ineligible.
        AstNode::RunRegress { .. } => Err("run_regress"),
        // run_split_beta injects a per-realization value — only the scalar lane's
        // realization loop carries the current-realization index. Mark ineligible.
        AstNode::RunSplitBeta { .. } => Err("run_split_beta"),
        AstNode::Lsm { .. } => Err("lsm"),
        // A call is eligible iff it is a pure elementwise scalar-math builtin and every
        // argument is itself eligible. Non-elementwise builtins (array reducers, event
        // predicates, private-helper specials) keep the model on the scalar lane.
        AstNode::Call { func, args } => match builtin_shape(func) {
            Some(BuiltinShape::Un(_)) | Some(BuiltinShape::Bin(_)) | Some(BuiltinShape::Fold(_)) => {
                for a in args { expr_allowed(a)?; }
                Ok(())
            }
            None => Err("call"),
        },
        AstNode::LookupCall { .. } => Err("lookup_call"),
        AstNode::SubmodelStat { .. } => Err("submodel_stat"),
        AstNode::SubmodelStat2 { .. } => Err("submodel_stat2"),
        AstNode::VectorMap { .. } => Err("vector_map"),
        AstNode::Index { .. } => Err("index"),
        AstNode::IndexRef { .. } => Err("index_ref"),
        AstNode::Subscript { .. } => Err("subscript"),
        AstNode::Array { .. } => Err("array"),
        AstNode::TimeRef { .. } => Err("time_ref"),
        AstNode::ExternCall { .. } => Err("extern_call"),
    }
}

// ── Dimensioned eligibility (correctness-first path) ─────────────────────────────
//
// A model with declared dimensions is accepted iff every element is a scalar/sample
// node, a scalar `fixed`, or an expression built from ops the shared `eval_ast` can
// evaluate *without* engine state the lane doesn't provide (no submodels, lookups,
// time, or event predicates). Such elements are evaluated per realization by reusing
// `eval_ast` directly — bit-identical to the scalar lane by construction — so this
// path adds array-shaped coverage (vector_map/index/subscript/array/reducers) before
// the fused coordinate kernel (DIMENSIONED_ARRAY_LANE_SCOPE.md) optimizes it.
fn dim_eligible(model: &Model) -> Result<(), String> {
    // Submodels are allowed: they run on the existing scalar pre-pass (`run_submodels`)
    // and feed the parent lane one-directionally via `submodel_stat`. Their interior
    // elements are also evaluated in the parent context (as the scalar lane does), so
    // they must be dim-eligible too — the loop below checks every element uniformly.
    for e in &model.elements {
        match &e.primitive {
            Primitive::Node(n) => match &n.rule {
                NodeRule::Fixed { .. } => {} // scalar or array constant
                NodeRule::Sample { resampling, autocorrelation, correlations, distribution } => {
                    if resampling.is_some() || autocorrelation.is_some()
                        || !correlations.is_empty() || distribution.importance.is_some()
                    {
                        return Err(format!("{}: sample uses resampling/autocorrelation/correlation/importance", e.id()));
                    }
                }
                NodeRule::Expression(ef) => dim_expr_allowed(&ef.ast)
                    .map_err(|op| format!("{}: expression uses unsupported op `{op}`", e.id()))?,
                other => return Err(format!("{}: node rule {:?} not dim-lane-eligible", e.id(), std::mem::discriminant(other))),
            },
            _ => return Err(format!("{}: non-node primitive not dim-lane-eligible", e.id())),
        }
    }
    Ok(())
}

/// Ops the per-realization `eval_ast` path can handle: everything `expr_allowed` covers
/// plus the array-shaped constructs (`vector_map`/`index_ref`/`index`/`subscript`/
/// `array`) and any builtin except event predicates (which need engine state). Excludes
/// `submodel_stat`, `lookup_call`, `time_ref`, `extern_call`.
fn dim_expr_allowed(node: &AstNode) -> Result<(), &'static str> {
    use AstNode::*;
    match node {
        Literal { .. } | Ref { .. } | IndexRef { .. } => Ok(()),
        Add { left, right } | Subtract { left, right } | Multiply { left, right }
        | Divide { left, right } | Power { left, right } | Lt { left, right }
        | Gt { left, right } | Lte { left, right } | Gte { left, right }
        | Eq { left, right } | Neq { left, right } | And { left, right }
        | Or { left, right } => { dim_expr_allowed(left)?; dim_expr_allowed(right) }
        Neg { operand } | Not { operand } => dim_expr_allowed(operand),
        If { cond, then, else_ } => { dim_expr_allowed(cond)?; dim_expr_allowed(then)?; dim_expr_allowed(else_) }
        VectorMap { body, .. } => dim_expr_allowed(body),
        Subscript { array, .. } => dim_expr_allowed(array),
        Index { array, indices } => { dim_expr_allowed(array)?; for i in indices { dim_expr_allowed(i)?; } Ok(()) }
        Array { elements } => { for el in elements { dim_expr_allowed(el)?; } Ok(()) }
        RunStat { arg, .. } => match arg { Some(a) => dim_expr_allowed(a), None => Ok(()) },
        // run_stat2 on the dimensioned lane is deferred: it falls back to the scalar
        // two-pass, which handles it correctly (flat-lane support landed in Phase 2).
        RunStat2 { .. } => Err("run_stat2"),
        RunRegress { .. } => Err("run_regress"),
        RunSplitBeta { .. } => Err("run_split_beta"),
        Lsm { .. } => Err("lsm"),
        // Reduce a submodel output — served by the `run_submodels` pre-pass boundary.
        SubmodelStat { arg, .. } => match arg { Some(a) => dim_expr_allowed(a), None => Ok(()) },
        SubmodelStat2 { .. } => Err("submodel_stat2"),
        // Any builtin except the ctx-stateful event predicates; array reducers included.
        Call { func, args } => match func {
            BuiltinFn::Occurs | BuiltinFn::Changed => Err("event_predicate"),
            _ => { for a in args { dim_expr_allowed(a)?; } Ok(()) }
        },
        LookupCall { .. } => Err("lookup_call"),
        TimeRef { .. } => Err("time_ref"),
        ExternCall { .. } => Err("extern_call"),
    }
}

// ── Fused bytecode: one compiled program per expression, one pass per Run column ──

/// A postfix stack-machine instruction over one realization's scalar values. The
/// binary/unary ops reuse the scalar lane's exact f64 semantics (see `eval.rs`), so
/// the fused result is bit-identical to `eval_ast` over the same column. `If` lowers
/// to a short-circuit `JumpIfFalse`/`Jump` pair — only the taken branch executes,
/// which for pure sub-expressions yields the same value the scalar lane's element-wise
/// `if` selects.
#[derive(Debug, Clone)]
enum Op {
    Const(f64),
    /// Push column slot `idx`'s value for the current realization (scalar broadcasts).
    Col(u32),
    /// Push reduced run-stat slot `idx` (0.0 until the reduction pass fills it).
    RunStat(u32),
    Add, Sub, Mul, Div, Pow,
    Lt, Gt, Lte, Gte, Eq, Neq, And, Or,
    Neg, Not,
    /// Pop 1, push a unary math builtin's result (abs/exp/ln/sqrt/…).
    Un(UnFn),
    /// Pop 2, push a fixed-arity binary math builtin's result (atan2/mod/pv/annuity).
    Bin2(BinFn),
    /// Pop `argc` (in emit order), push their min/max fold — variadic `min`/`max`.
    Fold(FoldFn, u32),
    /// Pop cond; if false (== 0.0) jump to the instruction index in the operand.
    JumpIfFalse(u32),
    /// Unconditional jump to the instruction index in the operand.
    Jump(u32),
}

/// Unary elementwise math builtins the fused kernel supports (each a direct f64
/// method, so bit-identical to `eval_call`'s scalar path).
#[derive(Debug, Clone, Copy)]
enum UnFn { Abs, Sqrt, Exp, Ln, Log10, Log2, Sin, Cos, Tan, Asin, Acos, Atan, Sinh, Cosh, Tanh, Floor, Ceil, Round, Sign, Trunc, Step }

/// Fixed-arity binary math builtins.
#[derive(Debug, Clone, Copy)]
enum BinFn { Atan2, Mod, PvFactor, AnnuityFactor }

/// Variadic min/max fold builtins.
#[derive(Debug, Clone, Copy)]
enum FoldFn { Min, Max }

/// Lower a `BuiltinFn` to its fused-kernel shape, or `None` if it is not a pure
/// elementwise scalar-math builtin (array reducers, event predicates, and the
/// private-helper specials — gamma/erf/date extraction — stay ineligible, keeping
/// such models on the scalar lane).
enum BuiltinShape { Un(UnFn), Bin(BinFn), Fold(FoldFn) }

fn builtin_shape(f: &BuiltinFn) -> Option<BuiltinShape> {
    use BuiltinFn as F;
    Some(match f {
        F::Abs => BuiltinShape::Un(UnFn::Abs),
        F::Sqrt => BuiltinShape::Un(UnFn::Sqrt),
        F::Exp => BuiltinShape::Un(UnFn::Exp),
        F::Ln => BuiltinShape::Un(UnFn::Ln),
        F::Log => BuiltinShape::Un(UnFn::Log10),
        F::Log2 => BuiltinShape::Un(UnFn::Log2),
        F::Sin => BuiltinShape::Un(UnFn::Sin),
        F::Cos => BuiltinShape::Un(UnFn::Cos),
        F::Tan => BuiltinShape::Un(UnFn::Tan),
        F::Asin => BuiltinShape::Un(UnFn::Asin),
        F::Acos => BuiltinShape::Un(UnFn::Acos),
        F::Atan => BuiltinShape::Un(UnFn::Atan),
        F::Sinh => BuiltinShape::Un(UnFn::Sinh),
        F::Cosh => BuiltinShape::Un(UnFn::Cosh),
        F::Tanh => BuiltinShape::Un(UnFn::Tanh),
        F::Floor => BuiltinShape::Un(UnFn::Floor),
        F::Ceil => BuiltinShape::Un(UnFn::Ceil),
        F::Round => BuiltinShape::Un(UnFn::Round),
        F::Sign => BuiltinShape::Un(UnFn::Sign),
        F::Int => BuiltinShape::Un(UnFn::Trunc),
        F::Step => BuiltinShape::Un(UnFn::Step),
        F::Atan2 => BuiltinShape::Bin(BinFn::Atan2),
        F::Mod => BuiltinShape::Bin(BinFn::Mod),
        F::PvFactor => BuiltinShape::Bin(BinFn::PvFactor),
        F::AnnuityFactor => BuiltinShape::Bin(BinFn::AnnuityFactor),
        F::Min => BuiltinShape::Fold(FoldFn::Min),
        F::Max => BuiltinShape::Fold(FoldFn::Max),
        _ => return None,
    })
}

/// A materialized Run column: a scalar broadcasts across realizations, a vector is
/// indexed per realization.
enum ColData {
    Scalar(f64),
    Vec(Vec<f64>),
}

impl ColData {
    #[inline]
    fn at(&self, r: usize) -> f64 {
        match self {
            ColData::Scalar(s) => *s,
            ColData::Vec(v) => v[r],
        }
    }
}

/// Compile an eligible expression AST into a fused bytecode program. `slot_of` maps a
/// referenced element id to its column slot; `rs_of` maps a run-stat key to its slot.
fn compile(
    ast: &AstNode,
    slot_of: &HashMap<&str, u32>,
    rs_of: &HashMap<String, u32>,
) -> Result<Vec<Op>, EngineError> {
    let mut prog = Vec::new();
    emit(ast, slot_of, rs_of, &mut prog)?;
    Ok(prog)
}

fn emit(
    node: &AstNode,
    slot_of: &HashMap<&str, u32>,
    rs_of: &HashMap<String, u32>,
    prog: &mut Vec<Op>,
) -> Result<(), EngineError> {
    macro_rules! bin {
        ($l:expr, $r:expr, $op:expr) => {{
            emit($l, slot_of, rs_of, prog)?;
            emit($r, slot_of, rs_of, prog)?;
            prog.push($op);
        }};
    }
    match node {
        AstNode::Literal { value, .. } => prog.push(Op::Const(*value)),
        AstNode::Ref { element_id, .. } => {
            let slot = *slot_of.get(element_id.as_str()).ok_or_else(|| {
                EngineError::InvalidModel(format!("array lane: ref to unknown element '{element_id}'"))
            })?;
            prog.push(Op::Col(slot));
        }
        AstNode::Add { left, right } => bin!(left, right, Op::Add),
        AstNode::Subtract { left, right } => bin!(left, right, Op::Sub),
        AstNode::Multiply { left, right } => bin!(left, right, Op::Mul),
        AstNode::Divide { left, right } => bin!(left, right, Op::Div),
        AstNode::Power { left, right } => bin!(left, right, Op::Pow),
        AstNode::Lt { left, right } => bin!(left, right, Op::Lt),
        AstNode::Gt { left, right } => bin!(left, right, Op::Gt),
        AstNode::Lte { left, right } => bin!(left, right, Op::Lte),
        AstNode::Gte { left, right } => bin!(left, right, Op::Gte),
        AstNode::Eq { left, right } => bin!(left, right, Op::Eq),
        AstNode::Neq { left, right } => bin!(left, right, Op::Neq),
        AstNode::And { left, right } => bin!(left, right, Op::And),
        AstNode::Or { left, right } => bin!(left, right, Op::Or),
        AstNode::Neg { operand } => { emit(operand, slot_of, rs_of, prog)?; prog.push(Op::Neg); }
        AstNode::Not { operand } => { emit(operand, slot_of, rs_of, prog)?; prog.push(Op::Not); }
        AstNode::If { cond, then, else_ } => {
            // <cond> JumpIfFalse ELSE <then> Jump END ELSE: <else> END:
            emit(cond, slot_of, rs_of, prog)?;
            let jf = prog.len();
            prog.push(Op::JumpIfFalse(0)); // patched to ELSE
            emit(then, slot_of, rs_of, prog)?;
            let jmp = prog.len();
            prog.push(Op::Jump(0)); // patched to END
            let else_start = prog.len() as u32;
            emit(else_, slot_of, rs_of, prog)?;
            let end = prog.len() as u32;
            prog[jf] = Op::JumpIfFalse(else_start);
            prog[jmp] = Op::Jump(end);
        }
        AstNode::RunStat { element_id, statistic, arg } => {
            let arg_val = match arg.as_deref() {
                Some(AstNode::Literal { value, .. }) => *value,
                _ => 0.0,
            };
            let key = run_stat_key(element_id, statistic, arg_val);
            let slot = *rs_of.get(&key).ok_or_else(|| {
                EngineError::InvalidModel(format!("array lane: run_stat key '{key}' not collected"))
            })?;
            prog.push(Op::RunStat(slot));
        }
        // Bivariate reduction: reads its pre-reduced scalar from the same `rstats`
        // slot table as a univariate run_stat, keyed by (x, y, statistic).
        AstNode::RunStat2 { x, y, statistic } => {
            let key = run_stat2_key(x, y, statistic);
            let slot = *rs_of.get(&key).ok_or_else(|| {
                EngineError::InvalidModel(format!("array lane: run_stat2 key '{key}' not collected"))
            })?;
            prog.push(Op::RunStat(slot));
        }
        // Unreachable: run_regress is marked ineligible above → scalar two-pass.
        AstNode::RunRegress { .. } => {
            return Err(EngineError::InvalidModel(
                "array lane: run_regress is not supported (handled by the scalar two-pass)".into(),
            ));
        }
        // Unreachable: run_split_beta is marked ineligible above → scalar two-pass.
        AstNode::RunSplitBeta { .. } => {
            return Err(EngineError::InvalidModel(
                "array lane: run_split_beta is not supported (handled by the scalar two-pass)".into(),
            ));
        }
        // Unreachable: lsm is marked ineligible above → scalar two-pass.
        AstNode::Lsm { .. } => {
            return Err(EngineError::InvalidModel(
                "array lane: lsm is not supported (handled by the scalar two-pass)".into(),
            ));
        }
        AstNode::Call { func, args } => {
            let bad_arity = |name: &str| EngineError::InvalidModel(
                format!("array lane: builtin '{name}' called with {} args", args.len()));
            match builtin_shape(func) {
                Some(BuiltinShape::Un(u)) => {
                    if args.len() != 1 { return Err(bad_arity("unary")); }
                    emit(&args[0], slot_of, rs_of, prog)?;
                    prog.push(Op::Un(u));
                }
                Some(BuiltinShape::Bin(b)) => {
                    if args.len() != 2 { return Err(bad_arity("binary")); }
                    emit(&args[0], slot_of, rs_of, prog)?;
                    emit(&args[1], slot_of, rs_of, prog)?;
                    prog.push(Op::Bin2(b));
                }
                Some(BuiltinShape::Fold(fk)) => {
                    if args.is_empty() { return Err(bad_arity("min/max")); }
                    for a in args { emit(a, slot_of, rs_of, prog)?; }
                    prog.push(Op::Fold(fk, args.len() as u32));
                }
                None => return Err(EngineError::InvalidModel(format!(
                    "array lane: builtin {:?} not compilable", std::mem::discriminant(func)))),
            }
        }
        other => {
            return Err(EngineError::InvalidModel(format!(
                "array lane: expression op not compilable: {:?}",
                std::mem::discriminant(other)
            )));
        }
    }
    Ok(())
}

/// Execute a fused program for realization `r` against the current columns and reduced
/// run-stats, reusing `stack` across realizations. Returns the single result value.
#[inline]
fn exec(prog: &[Op], cols: &[ColData], rstats: &[f64], r: usize, stack: &mut Vec<f64>) -> f64 {
    stack.clear();
    let mut ip = 0usize;
    macro_rules! bin {
        ($f:expr) => {{
            let b = stack.pop().unwrap();
            let a = stack.pop().unwrap();
            stack.push($f(a, b));
        }};
    }
    while ip < prog.len() {
        match &prog[ip] {
            Op::Const(c) => stack.push(*c),
            Op::Col(s) => stack.push(cols[*s as usize].at(r)),
            Op::RunStat(s) => stack.push(rstats[*s as usize]),
            Op::Add => bin!(|a, b| a + b),
            Op::Sub => bin!(|a, b| a - b),
            Op::Mul => bin!(|a, b| a * b),
            Op::Div => bin!(|a, b| a / b),
            Op::Pow => bin!(|a: f64, b: f64| a.powf(b)),
            Op::Lt => bin!(|a, b| bool_val(a < b)),
            Op::Gt => bin!(|a, b| bool_val(a > b)),
            Op::Lte => bin!(|a, b| bool_val(a <= b)),
            Op::Gte => bin!(|a, b| bool_val(a >= b)),
            Op::Eq => bin!(|a: f64, b: f64| bool_val((a - b).abs() < f64::EPSILON)),
            Op::Neq => bin!(|a: f64, b: f64| bool_val((a - b).abs() >= f64::EPSILON)),
            Op::And => bin!(|a, b| bool_val(is_true(a) && is_true(b))),
            Op::Or => bin!(|a, b| bool_val(is_true(a) || is_true(b))),
            Op::Neg => {
                let a = stack.pop().unwrap();
                stack.push(-a);
            }
            Op::Not => {
                let a = stack.pop().unwrap();
                stack.push(bool_val(!is_true(a)));
            }
            Op::Un(u) => {
                let a = stack.pop().unwrap();
                stack.push(apply_unary(*u, a));
            }
            Op::Bin2(b) => {
                let y = stack.pop().unwrap();
                let x = stack.pop().unwrap();
                stack.push(apply_binary(*b, x, y));
            }
            Op::Fold(fk, argc) => {
                // Fold over the top `argc` values in **emit order** (arg0 deepest), with the
                // same init as `eval_call`, so min/max match bit-for-bit even with NaN args.
                let base = stack.len() - *argc as usize;
                let (init, f): (f64, fn(f64, f64) -> f64) = match fk {
                    FoldFn::Min => (f64::INFINITY, f64::min),
                    FoldFn::Max => (f64::NEG_INFINITY, f64::max),
                };
                let acc = stack[base..].iter().copied().fold(init, f);
                stack.truncate(base);
                stack.push(acc);
            }
            Op::JumpIfFalse(t) => {
                let c = stack.pop().unwrap();
                if c == 0.0 {
                    ip = *t as usize;
                    continue;
                }
            }
            Op::Jump(t) => {
                ip = *t as usize;
                continue;
            }
        }
        ip += 1;
    }
    stack.pop().unwrap_or(0.0)
}

#[inline]
fn bool_val(b: bool) -> f64 { if b { 1.0 } else { 0.0 } }
#[inline]
fn is_true(v: f64) -> bool { v != 0.0 }

/// Exactly the scalar formulas `eval_call` applies (eval.rs), so the fused result is
/// bit-identical to the scalar lane's builtin evaluation.
#[inline]
fn apply_unary(u: UnFn, a: f64) -> f64 {
    match u {
        UnFn::Abs => a.abs(),
        UnFn::Sqrt => a.sqrt(),
        UnFn::Exp => a.exp(),
        UnFn::Ln => a.ln(),
        UnFn::Log10 => a.log10(),
        UnFn::Log2 => a.log2(),
        UnFn::Sin => a.sin(),
        UnFn::Cos => a.cos(),
        UnFn::Tan => a.tan(),
        UnFn::Asin => a.asin(),
        UnFn::Acos => a.acos(),
        UnFn::Atan => a.atan(),
        UnFn::Sinh => a.sinh(),
        UnFn::Cosh => a.cosh(),
        UnFn::Tanh => a.tanh(),
        UnFn::Floor => a.floor(),
        UnFn::Ceil => a.ceil(),
        UnFn::Round => a.round(),
        UnFn::Sign => a.signum(),
        UnFn::Trunc => a.trunc(),
        UnFn::Step => if a >= 0.0 { 1.0 } else { 0.0 },
    }
}

#[inline]
fn apply_binary(b: BinFn, x: f64, y: f64) -> f64 {
    match b {
        BinFn::Atan2 => x.atan2(y),
        BinFn::Mod => x % y,
        BinFn::PvFactor => (1.0 + x).powf(y),
        BinFn::AnnuityFactor => if x == 0.0 { y } else { (1.0 - (1.0 + x).powf(-y)) / x },
    }
}

/// Owns the empty collaborators an `EvalCtx` needs, so the lane can hand out a minimal
/// ctx borrowing just the live `outputs` (used only by `resolve_distribution` for
/// formula-valued distribution parameters).
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
            run_stats, run_vecs: None, index_stack: &self.index_stack, submodel_outputs: &self.sub, lag: None,
            fired_events: &self.fired, calendar_start: self.calendar_start,
        }
    }
}

/// Run an eligible model on the array lane. Result is bit-identical to the scalar lane.
///
/// Dimensioned models take the correctness-first per-realization path (`run_dim_lane`);
/// the flat Monte-Carlo subset takes the fused scalar-column path below.
pub fn run_array_lane(
    model: &Model,
    graph: &ModelGraphV2,
    config: &RunConfig,
) -> Result<SimulationResults, EngineError> {
    if !model.dimensions.is_empty() {
        return run_dim_lane(model, graph, config);
    }
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

    // ── Column slots: one per element, dense, in model order ──
    let slot_of: HashMap<&str, u32> = model.elements.iter().enumerate()
        .map(|(i, e)| (e.id(), i as u32)).collect();
    let mut columns: Vec<ColData> = (0..model.elements.len()).map(|_| ColData::Scalar(0.0)).collect();

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

    for (id, col) in sample_cols {
        columns[slot_of[id] as usize] = ColData::Vec(col);
    }
    // Fixed scalars: kept as Scalar so `⊕ vector` broadcasts across the Run column.
    for e in &model.elements {
        if let Primitive::Node(n) = &e.primitive {
            if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &n.rule {
                columns[slot_of[e.id()] as usize] = ColData::Scalar(q.value);
            }
        }
    }

    // ── 2. Compile every expression to fused bytecode (once) ──
    let run_stat_targets = collect_run_stats(model)?;
    let pair_targets = collect_run_stat2s(model);
    let n_uni = run_stat_targets.len();
    // Both univariate and bivariate reductions share one `rstats` slot table: univariate
    // targets take slots 0..n_uni, run_stat2 targets take n_uni.. .
    let mut rs_of: HashMap<String, u32> = run_stat_targets.iter().enumerate()
        .map(|(i, t)| (t.key.clone(), i as u32)).collect();
    for (j, pt) in pair_targets.iter().enumerate() {
        rs_of.insert(pt.key.clone(), (n_uni + j) as u32);
    }
    // Which run-stat slots reduce which element's column (fill after that column finalizes).
    let mut targets_of: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, t) in run_stat_targets.iter().enumerate() {
        targets_of.entry(t.element_id.as_str()).or_default().push(i);
    }

    let mut progs: HashMap<u32, Vec<Op>> = HashMap::new();
    for e in &model.elements {
        if let Some(ast) = expression_ast(model, e.id()) {
            progs.insert(slot_of[e.id()], compile(ast, &slot_of, &rs_of)?);
        }
    }

    let mut rstats: Vec<f64> = vec![0.0; n_uni + pair_targets.len()];
    // run_stat2 targets whose scalar has been computed (both columns finalized).
    let mut pair_done: Vec<bool> = vec![false; pair_targets.len()];
    let mut stack: Vec<f64> = Vec::with_capacity(32);

    // ── 3. Fused evaluation ──
    // Phase C: a run_stat's target is materialized here (unlike the scalar lane), so
    // when a topo order exists that places every run_stat target before its consumer,
    // we fold the reduction inline in a **single** pass. That augmented order can only
    // fail to exist when a run_stat participates in a genuine cycle (ensemble
    // feedback); that case keeps the correctness-preserving two-pass fallback.
    match augmented_order(model, &slot_of) {
        Some(order) => {
            // Sample and fixed columns are materialized before the loop, so a run_stat2
            // whose targets are those can reduce immediately; every other column finalizes
            // as it is emitted below.
            let mut finalized: HashSet<&str> = HashSet::new();
            for e in &model.elements {
                if let Primitive::Node(nd) = &e.primitive {
                    if matches!(nd.rule, NodeRule::Sample { .. } | NodeRule::Fixed { .. }) {
                        finalized.insert(e.id());
                    }
                }
            }
            reduce_ready_pairs(&pair_targets, &finalized, &columns, &slot_of, n, n_uni, &mut rstats, &mut pair_done);
            for id in &order {
                let slot = slot_of[id.as_str()];
                if let Some(prog) = progs.get(&slot) {
                    let mut out = Vec::with_capacity(n);
                    for r in 0..n {
                        out.push(exec(prog, &columns, &rstats, r, &mut stack));
                    }
                    columns[slot as usize] = ColData::Vec(out);
                }
                // This element's column is now final — reduce any run_stats over it.
                if let Some(slots) = targets_of.get(id.as_str()) {
                    for &i in slots {
                        let t = &run_stat_targets[i];
                        rstats[i] = reduce_over_column(&columns[slot as usize], &t.stat, t.arg, n);
                    }
                }
                // …and any run_stat2 whose other column is already final.
                finalized.insert(id.as_str());
                reduce_ready_pairs(&pair_targets, &finalized, &columns, &slot_of, n, n_uni, &mut rstats, &mut pair_done);
            }
        }
        None => {
            // Cyclic run_stat feedback: mirror the scalar lane's two passes exactly.
            let exprs: Vec<(u32, &Vec<Op>)> = graph.topo_order.iter()
                .filter_map(|id| progs.get(&slot_of[id.as_str()]).map(|p| (slot_of[id.as_str()], p)))
                .collect();
            eval_pass(&exprs, &mut columns, &rstats, n, &mut stack);
            for (i, t) in run_stat_targets.iter().enumerate() {
                let slot = slot_of[t.element_id.as_str()] as usize;
                rstats[i] = reduce_over_column(&columns[slot], &t.stat, t.arg, n);
            }
            for (j, pt) in pair_targets.iter().enumerate() {
                let xc = &columns[slot_of[pt.x.as_str()] as usize];
                let yc = &columns[slot_of[pt.y.as_str()] as usize];
                rstats[n_uni + j] = reduce_pair_over_columns(xc, yc, &pt.stat, n);
            }
            eval_pass(&exprs, &mut columns, &rstats, n, &mut stack);
        }
    }

    // ── 4. Assemble results (single step; columns → final_values) ──
    let mut elements = HashMap::new();
    for e in &model.elements {
        let col = match &columns[slot_of[e.id()] as usize] {
            ColData::Vec(v) => v.clone(),
            ColData::Scalar(s) => vec![*s; n],
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

/// Correctness-first dimensioned lane (DIMENSIONED_ARRAY_LANE_SCOPE.md, first slice).
///
/// Draws are mirrored from the scalar loop exactly (as in the flat lane), then each
/// realization is evaluated by reusing the shared `eval_ast` over `NamedArray`/scalar
/// values — so `vector_map`/`index`/`subscript`/`array`/reducers and dimensioned
/// broadcast are handled by the scalar lane's own evaluator, making the result
/// **bit-identical** to it. A dimensioned element surfaces both its scalar collapse
/// (`<id>`) and its per-member `<id>#k` columns, matching `engine_v2`. `run_stat` uses
/// the same two-pass reduction. (The fused coordinate kernel is the follow-on
/// optimization; this slice establishes correctness and coverage first, mirroring the
/// flat lane's A→B sequencing.)
pub fn run_dim_lane(
    model: &Model,
    graph: &ModelGraphV2,
    config: &RunConfig,
) -> Result<SimulationResults, EngineError> {
    let n = config.n_realizations.unwrap_or(model.simulation_settings.n_realizations) as usize;
    let seed = config.seed.or(model.simulation_settings.seed).unwrap_or(0);

    let dims: HashMap<String, usize> = model.dimensions.iter().map(|d| (d.id.clone(), d.size)).collect();
    let labels: HashMap<String, Vec<String>> = model.dimensions.iter()
        .filter(|d| !d.labels.is_empty()).map(|d| (d.id.clone(), d.labels.clone())).collect();
    // Submodel pre-pass (§12): identical call to the scalar engine's, so `submodel_stat`
    // reduces bit-identical per-realization samples. Submodels stay on the scalar engine;
    // only their statistics enter the lane (a one-directional boundary — no loop splice).
    let sub = crate::submodel_v2::run_submodels(model, config)?;
    let env = LaneEnv {
        lookups: HashMap::new(), labels, dims, sub, prev: HashMap::new(),
        index_stack: RefCell::new(Vec::new()), fired: RefCell::new(HashSet::new()),
        dt: model.simulation_settings.timestep.value,
        dt_unit: model.simulation_settings.timestep.unit.clone(),
        calendar_start: model.simulation_settings.calendar_start,
    };
    let no_run_stats: HashMap<String, f64> = HashMap::new();

    // ── 1. Sampling: mirror the scalar loop's per-realization draws exactly ──
    let mut sample_cols: HashMap<&str, Vec<f64>> = model.elements.iter()
        .filter(|e| matches!(&e.primitive, Primitive::Node(n) if matches!(n.rule, NodeRule::Sample { .. })))
        .map(|e| (e.id(), Vec::with_capacity(n))).collect();
    for r in 0..n {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        rng.set_stream(r as u64);
        let mut dist_ctx: HashMap<String, Value> = HashMap::new();
        for e in &model.elements {
            if let Primitive::Node(nd) = &e.primitive {
                if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &nd.rule {
                    dist_ctx.insert(e.id().to_string(), Value::Scalar(q.value));
                }
            }
        }
        for e in &model.elements {
            if let Primitive::Node(nd) = &e.primitive {
                if let NodeRule::Sample { distribution, .. } = &nd.rule {
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

    // Dimensioned elements → member counts (matching engine_v2's `<id>#k` records).
    let members: HashMap<&str, usize> = model.elements.iter().filter_map(|e| {
        let d = e.base.outputs.first().map(|o| o.dimensions.as_slice()).unwrap_or(&[]);
        if d.is_empty() { return None; }
        let count: usize = d.iter().map(|id| env.dims.get(id).copied().unwrap_or(0)).product();
        (count > 0).then_some((e.id(), count))
    }).collect();

    // ── 2. Per-realization evaluation via eval_ast; two passes iff run_stat present ──
    let run_stat_targets = collect_run_stats(model)?;
    // Precompute the per-realization work once (hoisted out of the hot loop): the
    // topo-ordered expression list (avoids an O(elements) scan per id per realization),
    // the constant fixed values (avoids re-cloning e.g. a 25-wide array every
    // realization), and the `#k` member keys (avoids per-realization `format!`).
    let expr_list: Vec<(&str, &AstNode)> = graph.topo_order.iter()
        .filter_map(|id| expression_ast(model, id).map(|ast| (id.as_str(), ast))).collect();
    let fixed_vals: Vec<(&str, Value)> = model.elements.iter().filter_map(|e| {
        if let Primitive::Node(nd) = &e.primitive {
            match &nd.rule {
                NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => Some((e.id(), Value::Scalar(q.value))),
                NodeRule::Fixed { value: FixedValue::Array { values, .. }, .. } => Some((e.id(), Value::Vector(values.clone()))),
                _ => None,
            }
        } else { None }
    }).collect();
    let member_info: Vec<(&str, Vec<String>)> = members.iter()
        .map(|(&id, &count)| (id, (1..=count).map(|k| format!("{id}#{k}")).collect())).collect();
    let elem_ids: Vec<&str> = model.elements.iter().map(|e| e.id()).collect();

    let run_pass = |run_stats: &HashMap<String, f64>| -> Result<DimStores, EngineError> {
        let mut scalar: HashMap<String, Vec<f64>> =
            elem_ids.iter().map(|&id| (id.to_string(), Vec::with_capacity(n))).collect();
        let mut member: HashMap<String, Vec<f64>> = HashMap::new();
        for (_, keys) in &member_info {
            for key in keys { member.insert(key.clone(), Vec::with_capacity(n)); }
        }
        // One reused map: fixed values seeded once (constant across realizations); samples
        // and expression results are overwritten each realization in topo order, so a
        // consumer always reads the current realization's value. Bit-identical to the
        // per-realization rebuild — same `eval_ast`, same order, same values.
        let mut outputs: HashMap<String, Value> = HashMap::with_capacity(elem_ids.len());
        for (id, v) in &fixed_vals { outputs.insert(id.to_string(), v.clone()); }
        for r in 0..n {
            for (id, col) in &sample_cols {
                outputs.insert(id.to_string(), Value::Scalar(col[r]));
            }
            for (id, ast) in &expr_list {
                let v = { let ctx = env.ctx(&outputs, run_stats); eval_ast(ast, &ctx)? };
                outputs.insert(id.to_string(), v);
            }
            for &id in &elem_ids {
                let v = outputs.get(id);
                scalar.get_mut(id).unwrap().push(v.map(|v| v.as_scalar()).unwrap_or(0.0));
            }
            for (id, keys) in &member_info {
                let vec = outputs.get(*id).map(|v| v.clone().into_vec()).unwrap_or_default();
                for (k, key) in keys.iter().enumerate() {
                    member.get_mut(key).unwrap().push(vec.get(k).copied().unwrap_or(0.0));
                }
            }
        }
        Ok(DimStores { scalar, member })
    };

    let mut stores = run_pass(&no_run_stats)?;
    if !run_stat_targets.is_empty() {
        let mut reduced: HashMap<String, f64> = HashMap::new();
        for t in &run_stat_targets {
            let col = stores.scalar.get(&t.element_id).cloned().unwrap_or_default();
            reduced.insert(t.key.clone(), reduce_run_stat(&col, &t.stat, t.arg));
        }
        stores = run_pass(&reduced)?;
    }

    // ── 3. Assemble results: primary <id> (scalar collapse) + dimensioned <id>#k ──
    let mut elements = HashMap::new();
    for e in &model.elements {
        elements.insert(e.id().to_string(), ElementResults {
            label: e.base.name.clone(),
            unit: primary_unit(e).to_string(),
            final_values: stores.scalar.remove(e.id()).unwrap_or_default(),
            time_history: None,
            analysis: None,
        });
        if let Some(&count) = members.get(e.id()) {
            for k in 1..=count {
                let mid = format!("{}#{k}", e.id());
                elements.insert(mid.clone(), ElementResults {
                    label: format!("{}[{k}]", e.base.name),
                    unit: primary_unit(e).to_string(),
                    final_values: stores.member.remove(&mid).unwrap_or_default(),
                    time_history: None,
                    analysis: None,
                });
            }
        }
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

/// The two per-realization result stores the dim lane fills (primary + `#k` members).
struct DimStores {
    scalar: HashMap<String, Vec<f64>>,
    member: HashMap<String, Vec<f64>>,
}

/// One fused pass: evaluate every compiled expression's whole Run column, in topo
/// order, so later expressions see earlier columns. Used only by the two-pass fallback.
fn eval_pass(exprs: &[(u32, &Vec<Op>)], columns: &mut Vec<ColData>, rstats: &[f64], n: usize, stack: &mut Vec<f64>) {
    for (slot, prog) in exprs {
        let mut out = Vec::with_capacity(n);
        for r in 0..n {
            out.push(exec(prog, columns, rstats, r, stack));
        }
        columns[*slot as usize] = ColData::Vec(out);
    }
}

/// Reduce a finalized Run column with a run-stat, avoiding a copy for the vector case.
fn reduce_over_column(col: &ColData, stat: &SubmodelStatKind, arg: f64, n: usize) -> f64 {
    match col {
        ColData::Vec(v) => reduce_run_stat(v, stat, arg),
        ColData::Scalar(s) => reduce_run_stat(&vec![*s; n], stat, arg),
    }
}

/// A topological order over **all** elements in which every `run_stat` target
/// precedes its consumer (in addition to the ordinary ref-dependency edges), or
/// `None` when no such order exists — i.e. a `run_stat` sits on a cycle (genuine
/// across-realization feedback), which forces the two-pass fallback. Deterministic:
/// ready elements are emitted in model-declaration order.
fn augmented_order(model: &Model, slot_of: &HashMap<&str, u32>) -> Option<Vec<String>> {
    let ids: Vec<&str> = model.elements.iter().map(|e| e.id()).collect();
    let mut indeg: HashMap<&str, usize> = ids.iter().map(|&i| (i, 0)).collect();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &model.elements {
        if let Some(ast) = expression_ast(model, e.id()) {
            let mut deps: Vec<String> = Vec::new();
            collect_expr_deps(ast, &mut deps);
            deps.sort();
            deps.dedup();
            for d in deps {
                // Only edges between real elements matter (dangling refs are inert).
                if slot_of.contains_key(d.as_str()) && d != e.id() {
                    // Map the borrowed key back to the &str owned by `ids`.
                    if let Some(&src) = ids.iter().find(|&&x| x == d) {
                        adj.entry(src).or_default().push(e.id());
                        *indeg.get_mut(e.id()).unwrap() += 1;
                    }
                }
            }
        }
    }
    // Kahn's algorithm, emitting ready nodes in model order for determinism.
    let mut order: Vec<String> = Vec::with_capacity(ids.len());
    let mut ready: Vec<&str> = ids.iter().copied().filter(|id| indeg[id] == 0).collect();
    let mut emitted: HashSet<&str> = HashSet::new();
    while let Some(&next) = ready.first() {
        ready.remove(0);
        if !emitted.insert(next) { continue; }
        order.push(next.to_string());
        if let Some(succs) = adj.get(next) {
            for &s in succs {
                let d = indeg.get_mut(s).unwrap();
                *d -= 1;
                if *d == 0 {
                    // Insert keeping model order among the ready set.
                    let pos = ids.iter().position(|&x| x == s).unwrap();
                    let ins = ready.iter().position(|&x| ids.iter().position(|&y| y == x).unwrap() > pos)
                        .unwrap_or(ready.len());
                    ready.insert(ins, s);
                }
            }
        }
    }
    (order.len() == ids.len()).then_some(order)
}

/// Element ids this expression depends on for ordering: ordinary refs **and**
/// `run_stat` targets (whose columns must be finalized before the reduction).
fn collect_expr_deps(node: &AstNode, out: &mut Vec<String>) {
    use AstNode::*;
    match node {
        Ref { element_id, .. } => out.push(element_id.clone()),
        RunStat { element_id, arg, .. } => {
            out.push(element_id.clone());
            if let Some(a) = arg { collect_expr_deps(a, out); }
        }
        // Both bivariate targets must be finalized before the reduction folds in.
        RunStat2 { x, y, .. } => {
            out.push(x.clone());
            out.push(y.clone());
        }
        Add { left, right } | Subtract { left, right } | Multiply { left, right }
        | Divide { left, right } | Power { left, right } | Lt { left, right }
        | Gt { left, right } | Lte { left, right } | Gte { left, right }
        | Eq { left, right } | Neq { left, right } | And { left, right } | Or { left, right } => {
            collect_expr_deps(left, out); collect_expr_deps(right, out);
        }
        Neg { operand } | Not { operand } => collect_expr_deps(operand, out),
        If { cond, then, else_ } => {
            collect_expr_deps(cond, out); collect_expr_deps(then, out); collect_expr_deps(else_, out);
        }
        Call { args, .. } => { for a in args { collect_expr_deps(a, out); } }
        _ => {}
    }
}

fn expression_ast<'a>(model: &'a Model, id: &str) -> Option<&'a AstNode> {
    model.elements.iter().find(|e| e.id() == id).and_then(|e| match &e.primitive {
        Primitive::Node(n) => match &n.rule { NodeRule::Expression(ef) => Some(&ef.ast), _ => None },
        _ => None,
    })
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

/// A bivariate `run_stat2` target on the flat lane: its `rstats` key, the two
/// column element ids, and the statistic.
struct RunStat2Target { key: String, x: String, y: String, stat: RunPairStat }

fn collect_run_stat2s(model: &Model) -> Vec<RunStat2Target> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for e in &model.elements {
        if let Primitive::Node(n) = &e.primitive {
            if let NodeRule::Expression(ef) = &n.rule {
                walk_run_stat2s(&ef.ast, &mut out, &mut seen);
            }
        }
    }
    out
}

fn walk_run_stat2s(node: &AstNode, out: &mut Vec<RunStat2Target>, seen: &mut HashSet<String>) {
    use AstNode::*;
    match node {
        RunStat2 { x, y, statistic } => {
            let key = run_stat2_key(x, y, statistic);
            if seen.insert(key.clone()) {
                out.push(RunStat2Target { key, x: x.clone(), y: y.clone(), stat: statistic.clone() });
            }
        }
        Add { left, right } | Subtract { left, right } | Multiply { left, right }
        | Divide { left, right } | Power { left, right } | Lt { left, right }
        | Gt { left, right } | Lte { left, right } | Gte { left, right }
        | Eq { left, right } | Neq { left, right } | And { left, right } | Or { left, right } => {
            walk_run_stat2s(left, out, seen); walk_run_stat2s(right, out, seen);
        }
        Neg { operand } | Not { operand } => walk_run_stat2s(operand, out, seen),
        If { cond, then, else_ } => {
            walk_run_stat2s(cond, out, seen); walk_run_stat2s(then, out, seen); walk_run_stat2s(else_, out, seen);
        }
        Call { args, .. } => { for a in args { walk_run_stat2s(a, out, seen); } }
        _ => {}
    }
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
        Call { args, .. } => { for a in args { walk_run_stats(a, out, seen)?; } }
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

fn reduce_run_stat2(xs: &[f64], ys: &[f64], stat: &RunPairStat) -> f64 {
    use RunPairStat as P;
    match stat {
        P::Cov => crate::engine::covariance(xs, ys),
        P::Corr => crate::engine::correlation(xs, ys),
        P::Beta => crate::engine::beta(xs, ys),
    }
}

/// Reduce every run_stat2 target whose two columns are both finalized and not yet
/// done, writing into its shared `rstats` slot (`base + j`). Idempotent via `done`.
#[allow(clippy::too_many_arguments)]
fn reduce_ready_pairs(
    pairs: &[RunStat2Target],
    finalized: &HashSet<&str>,
    columns: &[ColData],
    slot_of: &HashMap<&str, u32>,
    n: usize,
    base: usize,
    rstats: &mut [f64],
    done: &mut [bool],
) {
    for (j, pt) in pairs.iter().enumerate() {
        if done[j] || !finalized.contains(pt.x.as_str()) || !finalized.contains(pt.y.as_str()) {
            continue;
        }
        let xc = &columns[slot_of[pt.x.as_str()] as usize];
        let yc = &columns[slot_of[pt.y.as_str()] as usize];
        rstats[base + j] = reduce_pair_over_columns(xc, yc, &pt.stat, n);
        done[j] = true;
    }
}

/// Reduce two finalized Run columns to a bivariate scalar, broadcasting a `Scalar`
/// column to length `n` (matching `reduce_over_column`'s convention).
fn reduce_pair_over_columns(xc: &ColData, yc: &ColData, stat: &RunPairStat, n: usize) -> f64 {
    let as_vec = |c: &ColData| match c {
        ColData::Vec(v) => v.clone(),
        ColData::Scalar(s) => vec![*s; n],
    };
    reduce_run_stat2(&as_vec(xc), &as_vec(yc), stat)
}

fn primary_unit(elem: &Element) -> &str {
    if let Primitive::Node(n) = &elem.primitive {
        if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &n.rule {
            return &q.unit;
        }
    }
    elem.base.outputs.first().map(|o| o.unit.as_str()).unwrap_or("1")
}

#[cfg(test)]
mod tests {
    use super::augmented_order;
    use crate::model_v2::Model;
    use std::collections::HashMap;

    fn slots(m: &Model) -> HashMap<&str, u32> {
        m.elements.iter().enumerate().map(|(i, e)| (e.id(), i as u32)).collect()
    }

    // run_stat over an expression target, consumer declared first → still orderable.
    #[test]
    fn augmented_order_orders_expression_target_before_consumer() {
        let json = r#"{
          "wasim_version": "0.9.7",
          "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 4, "seed": 1},
          "containers": [{"id":"M","name":"M","elements":["M/x","M/d","M/z"]}],
          "elements": [
            {"id":"M/x","name":"x","primitive":"node","value_rule":"sample","container":"M","distribution":{"family":"uniform","parameters":{"min":{"value":0,"unit":"1"},"max":{"value":1,"unit":"1"}}}},
            {"id":"M/d","name":"d","primitive":"node","value_rule":"expression","container":"M","expression":{"ast":{"op":"run_stat","element_id":"M/z","statistic":"mean"}}},
            {"id":"M/z","name":"z","primitive":"node","value_rule":"expression","container":"M","expression":{"ast":{"op":"add","left":{"op":"ref","element_id":"M/x"},"right":{"op":"literal","value":1}}}}
          ]
        }"#;
        let m = crate::v2_parse::parse(json).unwrap();
        let order = augmented_order(&m, &slots(&m)).expect("acyclic → Some");
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("M/z") < pos("M/d"), "run_stat target M/z must precede consumer M/d: {order:?}");
        assert!(pos("M/x") < pos("M/z"), "ref dep M/x must precede M/z");
    }

    // Cyclic run_stat feedback (M/c ← mean(M/e), M/e ← M/c) → no valid order.
    #[test]
    fn augmented_order_rejects_run_stat_cycle() {
        let json = r#"{
          "wasim_version": "0.9.7",
          "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 4, "seed": 1},
          "containers": [{"id":"M","name":"M","elements":["M/c","M/e"]}],
          "elements": [
            {"id":"M/c","name":"c","primitive":"node","value_rule":"expression","container":"M","expression":{"ast":{"op":"run_stat","element_id":"M/e","statistic":"mean"}}},
            {"id":"M/e","name":"e","primitive":"node","value_rule":"expression","container":"M","expression":{"ast":{"op":"add","left":{"op":"ref","element_id":"M/c"},"right":{"op":"literal","value":1}}}}
          ]
        }"#;
        let m = crate::v2_parse::parse(json).unwrap();
        assert!(augmented_order(&m, &slots(&m)).is_none(), "run_stat cycle → None (two-pass fallback)");
    }
}
