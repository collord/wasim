//! v2 model summary for the frontend.
//!
//! Emits, per element: the legacy v1-ish `type` (the original v1 type for imported models,
//! else mapped from the primitive/value_rule — keeps the current frontend working) plus the
//! v2 fields `primitive`, `value_rule`, active `traits`, `editable`, current `value`, and
//! `inputs`. This is the contract the frontend's graph/dashboard/editing views build on, so it
//! lives here (host-testable) rather than inside the wasm-gated bridge.

use crate::model::{AstNode, BuiltinFn, TimeProperty};
use crate::model_v2::{Element, FixedValue, Model, NodeRule, Primitive};

/// Serialize a model summary to JSON (the shape `WasmEngine.model_summary()` returns).
pub fn summary_json(model: &Model) -> String {
    #[derive(serde::Serialize)]
    struct Summary<'a> {
        element_count: usize,
        elements: Vec<ElemSummary<'a>>,
        containers: &'a [crate::model::ContainerDef],
        simulation_settings: &'a crate::model::SimulationSettings,
    }

    #[derive(serde::Serialize)]
    struct ElemSummary<'a> {
        id: &'a str,
        name: &'a str,
        #[serde(rename = "type")]
        kind: String,
        primitive: &'static str,
        value_rule: Option<&'static str>,
        traits: Vec<&'static str>,
        container: Option<&'a str>,
        editable: bool,
        /// Canonical unit the engine computes in.
        unit: &'a str,
        /// Preferred display unit + affine mapping (`display = value·factor + offset`).
        /// Present only when a valid conversion exists; else the frontend shows `unit`.
        display_unit: Option<&'a str>,
        display_factor: f64,
        display_offset: f64,
        value: Option<f64>,
        bounds: Option<&'a crate::model::Bounds>,
        /// Full distribution (family + parameters + truncation) for `sample` nodes.
        dist: Option<serde_json::Value>,
        /// Readable formula for `expression` nodes (the display string, else a rendered AST).
        formula: Option<String>,
        /// Interpolation data for `lookup`/`series` nodes.
        table: Option<TableSummary<'a>>,
        inputs: &'a [String],
        description: Option<&'a str>,
    }

    #[derive(serde::Serialize)]
    struct TableSummary<'a> {
        x: &'a [f64],
        y: &'a [f64],
        columns: &'a [Vec<f64>],
        x_unit: Option<&'a str>,
        y_unit: Option<&'a str>,
    }

    let elements: Vec<ElemSummary> = model
        .elements
        .iter()
        .map(|e| ElemSummary {
            id: &e.base.id,
            name: &e.base.name,
            kind: legacy_type(e),
            primitive: primitive_name(&e.primitive),
            value_rule: node_value_rule(&e.primitive),
            traits: active_traits(e),
            container: e.base.container.as_deref(),
            editable: is_editable(e),
            unit: unit_of(e),
            display_unit: display_of(e).map(|(du, _, _)| du),
            display_factor: display_of(e).map(|(_, f, _)| f).unwrap_or(1.0),
            display_offset: display_of(e).map(|(_, _, o)| o).unwrap_or(0.0),
            value: current_value(e),
            bounds: bounds_of(e),
            dist: dist_of(e),
            formula: formula_of(e),
            table: match &e.primitive {
                Primitive::Node(n) => match &n.rule {
                    NodeRule::Lookup(t) => Some(TableSummary {
                        x: &t.x, y: &t.y, columns: &t.z,
                        x_unit: t.x_unit.as_deref(), y_unit: t.y_unit.as_deref(),
                    }),
                    NodeRule::Series { timestamps, values, time_unit, .. } => Some(TableSummary {
                        x: timestamps, y: values, columns: &[],
                        x_unit: time_unit.as_deref(), y_unit: None,
                    }),
                    _ => None,
                },
                _ => None,
            },
            inputs: &e.base.inputs,
            description: e.base.description.as_deref(),
        })
        .collect();

    let summary = Summary {
        element_count: elements.len(),
        elements,
        containers: &model.containers,
        simulation_settings: &model.simulation_settings,
    };
    serde_json::to_string(&summary).unwrap_or_default()
}

pub fn primitive_name(p: &Primitive) -> &'static str {
    match p {
        Primitive::Node(_) => "node",
        Primitive::Stock(_) => "stock",
        Primitive::Link(_) => "link",
        Primitive::Event(_) => "event",
        Primitive::Gate(_) => "gate",
        Primitive::Cell(_) => "cell",
        Primitive::Species(_) => "species",
        Primitive::Medium(_) => "medium",
    }
}

fn value_rule_str(rule: &NodeRule) -> &'static str {
    match rule {
        NodeRule::Fixed { .. } => "fixed",
        NodeRule::Expression(_) => "expression",
        NodeRule::Sample { .. } => "sample",
        NodeRule::Process { .. } => "process",
        NodeRule::Lookup(_) => "lookup",
        NodeRule::Series { .. } => "series",
        NodeRule::Lag { .. } => "lag",
        NodeRule::Convolution { .. } => "convolution",
        NodeRule::Markov { .. } => "markov",
        NodeRule::Hysteresis { .. } => "hysteresis",
        NodeRule::Filter { .. } => "filter",
        NodeRule::GateLogic { .. } => "gate_logic",
    }
}

pub fn node_value_rule(p: &Primitive) -> Option<&'static str> {
    match p {
        Primitive::Node(n) => Some(value_rule_str(&n.rule)),
        _ => None,
    }
}

/// Legacy v1-ish `type`: the original v1 type for imported models, else mapped from the
/// primitive / node value_rule.
pub fn legacy_type(elem: &Element) -> String {
    if let Some(st) = &elem.base.source_type {
        return st.clone();
    }
    match &elem.primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::Fixed { .. } => "constant",
            NodeRule::Sample { .. } => "random_variable",
            NodeRule::Process { .. } => "stochastic_process",
            NodeRule::Series { .. } => "timeseries",
            NodeRule::Lag { .. } => "delay",
            other => value_rule_str(other),
        }
        .to_string(),
        p => primitive_name(p).to_string(),
    }
}

pub fn active_traits(elem: &Element) -> Vec<&'static str> {
    let mut t = Vec::new();
    match &elem.primitive {
        Primitive::Stock(s) => {
            if s.capacity.is_some() {
                t.push("capacity_clamp");
            }
            if s.overflow_target.is_some() {
                t.push("overflow_routing");
            }
            if s.return_rate.is_some() {
                t.push("compound_growth");
            }
            if !s.withdrawals.is_empty() {
                t.push("priority_withdrawal");
            }
        }
        Primitive::Link(l) => {
            if l.priority.is_some() {
                t.push("priority_allocation");
            }
            if l.transit_time.is_some() {
                t.push("transit_buffer");
            }
            if l.decay_rate.is_some() {
                t.push("transit_decay");
            }
            if l.dispersion.is_some() {
                t.push("transit_dispersion");
            }
            if l.schedule.is_some() {
                t.push("scheduled_flow");
            }
            if l.species.is_some() || l.medium.is_some() || !l.fluxes.is_empty() {
                t.push("species_transport");
            }
        }
        Primitive::Event(e) => {
            if e.rate.is_some() {
                t.push("rate_generation");
            }
            if e.failure_process.is_some() {
                t.push("failure_state_machine");
            }
        }
        Primitive::Cell(c) => {
            if !c.partitioning.is_empty() {
                t.push("partitioning_equilibrium");
            }
            if c.inventory.is_some() && c.release_rate.is_some() {
                t.push("source_release");
            }
        }
        _ => {}
    }
    t
}

pub fn is_editable(elem: &Element) -> bool {
    if let Primitive::Node(n) = &elem.primitive {
        match &n.rule {
            NodeRule::Fixed { editable, .. } => return *editable,
            NodeRule::Sample { .. } => return true,
            _ => {}
        }
    }
    false
}

pub fn unit_of(elem: &Element) -> &str {
    if let Primitive::Node(n) = &elem.primitive {
        match &n.rule {
            NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => return &q.unit,
            NodeRule::Fixed { value: FixedValue::Array { unit, .. }, .. } => return unit,
            _ => {}
        }
    }
    elem.base.outputs.first().map(|o| o.unit.as_str()).unwrap_or("1")
}

pub fn current_value(elem: &Element) -> Option<f64> {
    if let Primitive::Node(n) = &elem.primitive {
        if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &n.rule {
            return Some(q.value);
        }
    }
    None
}

/// The element's preferred display unit (from a fixed value's `display_unit`, else the
/// primary output's `display_unit`).
fn display_unit_of(elem: &Element) -> Option<&str> {
    if let Primitive::Node(n) = &elem.primitive {
        if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &n.rule {
            if let Some(du) = q.display_unit.as_deref() {
                return Some(du);
            }
        }
    }
    elem.base.outputs.first().and_then(|o| o.display_unit.as_deref())
}

/// (display_unit, factor, offset) when a valid canonical→display conversion exists.
/// `display = value·factor + offset`.
pub fn display_of(elem: &Element) -> Option<(&str, f64, f64)> {
    let du = display_unit_of(elem)?;
    let (f, o) = crate::units::display_conversion(unit_of(elem), du)?;
    Some((du, f, o))
}

/// Readable formula for an expression node: the transpiler-provided `display` string when
/// present, else a rendered AST (fallback for inferred/v2-native expressions).
fn formula_of(elem: &Element) -> Option<String> {
    if let Primitive::Node(n) = &elem.primitive {
        if let NodeRule::Expression(ef) = &n.rule {
            let disp = ef.display.as_deref().map(str::trim).filter(|s| !s.is_empty());
            return Some(disp.map(String::from).unwrap_or_else(|| render_ast(&ef.ast)));
        }
    }
    None
}

/// Compact infix rendering of an AST (fully parenthesized — a readable fallback, not a
/// minimal-parens pretty-printer).
fn render_ast(n: &AstNode) -> String {
    let bin = |l: &AstNode, op: &str, r: &AstNode| format!("({} {op} {})", render_ast(l), render_ast(r));
    match n {
        AstNode::Literal { value, unit } => match unit.as_deref() {
            Some(u) if u != "1" => format!("{value} {u}"),
            _ => format!("{value}"),
        },
        AstNode::Ref { element_id, .. } => element_id.clone(),
        AstNode::TimeRef { property } => time_prop_name(property).to_string(),
        AstNode::Add { left, right } => bin(left, "+", right),
        AstNode::Subtract { left, right } => bin(left, "-", right),
        AstNode::Multiply { left, right } => bin(left, "*", right),
        AstNode::Divide { left, right } => bin(left, "/", right),
        AstNode::Power { left, right } => bin(left, "^", right),
        AstNode::Lt { left, right } => bin(left, "<", right),
        AstNode::Gt { left, right } => bin(left, ">", right),
        AstNode::Lte { left, right } => bin(left, "<=", right),
        AstNode::Gte { left, right } => bin(left, ">=", right),
        AstNode::Eq { left, right } => bin(left, "==", right),
        AstNode::Neq { left, right } => bin(left, "!=", right),
        AstNode::And { left, right } => bin(left, "&&", right),
        AstNode::Or { left, right } => bin(left, "||", right),
        AstNode::Neg { operand } => format!("-{}", render_ast(operand)),
        AstNode::Not { operand } => format!("!{}", render_ast(operand)),
        AstNode::Call { func, args } => {
            let a: Vec<String> = args.iter().map(render_ast).collect();
            format!("{}({})", fn_name(func), a.join(", "))
        }
        AstNode::If { cond, then, else_ } => {
            format!("if({}, {}, {})", render_ast(cond), render_ast(then), render_ast(else_))
        }
        AstNode::LookupCall { element_id, input, input2 } => match input2 {
            Some(i2) => format!("{element_id}[{}, {}]", render_ast(input), render_ast(i2)),
            None => format!("{element_id}[{}]", render_ast(input)),
        },
        AstNode::Array { elements } => {
            let e: Vec<String> = elements.iter().map(render_ast).collect();
            format!("[{}]", e.join(", "))
        }
    }
}

fn time_prop_name(p: &TimeProperty) -> &'static str {
    match p {
        TimeProperty::Elapsed => "elapsed",
        TimeProperty::Timestep => "dt",
        TimeProperty::Year => "year",
        TimeProperty::Month => "month",
        TimeProperty::DayOfYear => "day_of_year",
        TimeProperty::DayOfMonth => "day_of_month",
        TimeProperty::DaysInMonth => "days_in_month",
    }
}

fn fn_name(f: &BuiltinFn) -> &'static str {
    match f {
        BuiltinFn::Min => "min",
        BuiltinFn::Max => "max",
        BuiltinFn::Abs => "abs",
        BuiltinFn::Sqrt => "sqrt",
        BuiltinFn::Exp => "exp",
        BuiltinFn::Ln => "ln",
        BuiltinFn::Log => "log",
        BuiltinFn::Sin => "sin",
        BuiltinFn::Cos => "cos",
        BuiltinFn::Tan => "tan",
        BuiltinFn::Asin => "asin",
        BuiltinFn::Acos => "acos",
        BuiltinFn::Atan => "atan",
        BuiltinFn::Atan2 => "atan2",
        BuiltinFn::Floor => "floor",
        BuiltinFn::Ceil => "ceil",
        BuiltinFn::Round => "round",
        BuiltinFn::Mod => "mod",
        BuiltinFn::Sign => "sign",
        BuiltinFn::Int => "int",
        BuiltinFn::Step => "step",
        BuiltinFn::Log2 => "log2",
        BuiltinFn::Sinh => "sinh",
        BuiltinFn::Cosh => "cosh",
        BuiltinFn::Tanh => "tanh",
        BuiltinFn::SumArray => "sum_array",
        BuiltinFn::SizeArray => "size_array",
        BuiltinFn::GetElement => "get_element",
        BuiltinFn::InterpArray => "interp_array",
        BuiltinFn::MeanArray => "mean_array",
        BuiltinFn::MinArray => "min_array",
        BuiltinFn::MaxArray => "max_array",
        BuiltinFn::DotProduct => "dot_product",
    }
}

fn bounds_of(elem: &Element) -> Option<&crate::model::Bounds> {
    if let Primitive::Node(n) = &elem.primitive {
        if let NodeRule::Fixed { bounds, .. } = &n.rule {
            return bounds.as_ref();
        }
    }
    None
}

fn dist_of(elem: &Element) -> Option<serde_json::Value> {
    if let Primitive::Node(n) = &elem.primitive {
        if let NodeRule::Sample { distribution, .. } = &n.rule {
            return serde_json::to_value(distribution).ok();
        }
    }
    None
}
