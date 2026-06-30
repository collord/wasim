//! v2 model summary for the frontend.
//!
//! Emits, per element: the legacy v1-ish `type` (the original v1 type for imported models,
//! else mapped from the primitive/value_rule — keeps the current frontend working) plus the
//! v2 fields `primitive`, `value_rule`, active `traits`, `editable`, current `value`, and
//! `inputs`. This is the contract the frontend's graph/dashboard/editing views build on, so it
//! lives here (host-testable) rather than inside the wasm-gated bridge.

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
        unit: &'a str,
        value: Option<f64>,
        inputs: &'a [String],
        description: Option<&'a str>,
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
            value: current_value(e),
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
