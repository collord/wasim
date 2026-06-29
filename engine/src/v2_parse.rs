//! v2-native JSON → internal v2 model.
//!
//! The v2 schema is a flat object discriminated by `primitive` (and `value_rule` for
//! nodes) with trait fields activated by presence — a shape serde can't derive directly
//! into the clean [`crate::model_v2`] enums. So we deserialize into a raw DTO layer that
//! mirrors the flat schema, then *lower* it into the clean model.
//!
//! M2 scope: node (all value_rules), stock (all traits), gate, species, medium. The
//! link/event/cell primitives lower in M3/M4 alongside their engine support.

use serde::Deserialize;

use crate::error::EngineError;
use crate::model::{
    AstNode, Bounds, ContainerDef, CorrelationPair, Distribution, ExpressionField,
    InterpolationMethod, OutputSpec, ProcessSpec, Quantity, QuantityOrFormula, SamplingMethod,
    SaveSpec, SimulationSettings, SourceMetadata, TimeHistoryDisplay,
};
use crate::model_v2 as v2;

/// Parse a v2-native model document.
pub fn parse(json: &str) -> Result<v2::Model, EngineError> {
    let raw: RawModel = serde_json::from_str(json)?;
    lower_model(raw)
}

// ── Raw DTO layer (mirrors the flat schema) ───────────────────────────────────

#[derive(Deserialize)]
struct RawModel {
    wasim_version: String,
    #[serde(default)]
    source: Option<SourceMetadata>,
    simulation_settings: RawSimSettings,
    #[serde(default)]
    containers: Vec<ContainerDef>,
    elements: Vec<RawElement>,
    #[serde(default)]
    time_history_displays: Vec<TimeHistoryDisplay>,
}

#[derive(Deserialize)]
struct RawSimSettings {
    duration: Quantity,
    timestep: Quantity,
    #[serde(default = "default_one")]
    n_realizations: u32,
    #[serde(default)]
    sampling_method: SamplingMethod,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    reporting_periods: Vec<Quantity>,
}

fn default_one() -> u32 {
    1
}

#[derive(Deserialize)]
struct RawElement {
    // base
    id: String,
    name: String,
    primitive: String,
    #[serde(default)]
    container: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    outputs: Vec<OutputSpec>,
    #[serde(default)]
    save_results: SaveSpec,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    source_type: Option<String>,

    // node (all value_rules)
    #[serde(default)]
    value_rule: Option<String>,
    #[serde(default)]
    value: Option<Quantity>,
    #[serde(default)]
    values: Option<Vec<f64>>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    editable: bool,
    #[serde(default)]
    bounds: Option<Bounds>,
    #[serde(default)]
    expression: Option<ExpressionField>,
    #[serde(default)]
    distribution: Option<Distribution>,
    #[serde(default)]
    resampling: Option<RawTrigger>,
    #[serde(default)]
    autocorrelation: Option<f64>,
    #[serde(default)]
    correlations: Vec<CorrelationPair>,
    #[serde(default)]
    process: Option<ProcessSpec>,
    #[serde(default)]
    lower_bound: Option<Quantity>,
    #[serde(default)]
    table: Option<RawTable>,
    #[serde(default)]
    interpolation: Option<String>,
    #[serde(default)]
    timestamps: Option<Vec<f64>>,
    #[serde(default)]
    time_unit: Option<String>,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    initial: Option<Quantity>,
    #[serde(default)]
    response: Option<RawResponse>,
    #[serde(default)]
    states: Option<Vec<String>>,
    #[serde(default)]
    initial_state: Option<serde_json::Value>,
    #[serde(default)]
    transition_matrix: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    output_values: Option<Vec<f64>>,
    #[serde(default)]
    high_threshold: Option<Quantity>,
    #[serde(default)]
    low_threshold: Option<Quantity>,
    #[serde(default)]
    output_above: Option<Quantity>,
    #[serde(default)]
    output_below: Option<Quantity>,
    #[serde(default)]
    window: Option<usize>,
    #[serde(default)]
    statistic: Option<String>,
    #[serde(default)]
    root: Option<RawGate>,
    #[serde(default)]
    semantics: Option<String>,

    // stock
    #[serde(default)]
    initial_value: Option<Quantity>,
    #[serde(default)]
    rate: Option<QuantityOrFormula>,
    #[serde(default)]
    inflows: Vec<String>,
    #[serde(default)]
    outflows: Vec<String>,
    #[serde(default)]
    floor: Option<Quantity>,
    #[serde(default)]
    capacity: Option<QuantityOrFormula>,
    #[serde(default)]
    overflow_target: Option<String>,
    #[serde(default)]
    return_rate: Option<QuantityOrFormula>,
    #[serde(default)]
    withdrawals: Vec<RawWithdrawal>,

    // species def
    #[serde(default)]
    half_life: Option<Quantity>,
    #[serde(default)]
    decay_products: Vec<RawDecayProduct>,
    #[serde(default)]
    molecular_weight: Option<Quantity>,

    // medium def
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    density: Option<QuantityOrFormula>,
    #[serde(default)]
    porosity: Option<QuantityOrFormula>,

    // link
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    fraction: Option<QuantityOrFormula>,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    transit_time: Option<Quantity>,
    #[serde(default)]
    decay_rate: Option<QuantityOrFormula>,
    #[serde(default)]
    dispersion: Option<Quantity>,
    #[serde(default)]
    schedule: Option<RawTrigger>,

    // event
    #[serde(default)]
    trigger: Option<RawTrigger>,
    #[serde(default)]
    effects: Vec<RawEffect>,
    #[serde(default)]
    event_value: Option<RawQexpr>,
    #[serde(default)]
    count_limit: Option<i64>,
}

#[derive(Deserialize)]
struct RawEffect {
    target: String,
    #[serde(default)]
    change: Option<RawQexpr>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawQexpr {
    Quantity(Quantity),
    Ast(AstNode),
}

#[derive(Deserialize, Default)]
struct RawTrigger {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    condition: Option<QuantityOrFormula>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    period: Option<Quantity>,
    #[serde(default)]
    schedule: Vec<Quantity>,
}

#[derive(Deserialize)]
struct RawTable {
    #[serde(default)]
    x: Vec<f64>,
    #[serde(default)]
    y: Vec<f64>,
    #[serde(default)]
    z: Vec<Vec<f64>>,
    #[serde(default)]
    x_unit: Option<String>,
    #[serde(default)]
    y_unit: Option<String>,
    #[serde(default)]
    z_unit: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawResponse {
    Inline {
        #[serde(default)]
        times: Vec<f64>,
        #[serde(default)]
        values: Vec<f64>,
        #[serde(default)]
        times_unit: Option<String>,
        #[serde(default)]
        values_unit: Option<String>,
    },
    Ref(String),
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum RawGate {
    And { children: Vec<RawGate> },
    Or { children: Vec<RawGate> },
    Not { children: Vec<RawGate> },
    NVote { threshold: u32, children: Vec<RawGate> },
    Reference { reference: String },
    Condition { condition: QuantityOrFormula },
    Input { input: String },
}

#[derive(Deserialize)]
struct RawWithdrawal {
    target: String,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    request: Option<QuantityOrFormula>,
    #[serde(default)]
    limit: Option<QuantityOrFormula>,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Deserialize)]
struct RawDecayProduct {
    species: String,
    #[serde(default)]
    branching_fraction: Option<f64>,
}

// ── Lowering ──────────────────────────────────────────────────────────────────

fn lower_model(raw: RawModel) -> Result<v2::Model, EngineError> {
    let mut elements = Vec::with_capacity(raw.elements.len());
    for e in raw.elements {
        elements.push(lower_element(e)?);
    }
    Ok(v2::Model {
        wasim_version: raw.wasim_version,
        source: raw.source,
        simulation_settings: SimulationSettings {
            duration: raw.simulation_settings.duration,
            timestep: raw.simulation_settings.timestep,
            n_realizations: raw.simulation_settings.n_realizations,
            sampling_method: raw.simulation_settings.sampling_method,
            seed: raw.simulation_settings.seed,
        },
        reporting_periods: raw.simulation_settings.reporting_periods,
        containers: raw.containers,
        elements,
        time_history_displays: raw.time_history_displays,
        from_v1: false,
    })
}

fn lower_element(e: RawElement) -> Result<v2::Element, EngineError> {
    let base = v2::ElementBase {
        id: e.id.clone(),
        name: e.name.clone(),
        container: e.container.clone(),
        description: e.description.clone(),
        outputs: e.outputs.clone(),
        save_results: e.save_results.clone(),
        inputs: e.inputs.clone(),
        source_type: e.source_type.clone(),
    };

    let primitive = match e.primitive.as_str() {
        "node" => v2::Primitive::Node(lower_node(&e)?),
        "stock" => v2::Primitive::Stock(lower_stock(&e)?),
        "gate" => v2::Primitive::Gate(lower_gate_primitive(&e)?),
        "species" => v2::Primitive::Species(v2::Species {
            half_life: e.half_life.clone(),
            decay_products: e.decay_products.iter().map(|d| v2::DecayProduct {
                species: d.species.clone(),
                branching_fraction: d.branching_fraction,
            }).collect(),
            molecular_weight: e.molecular_weight.clone(),
        }),
        "medium" => v2::Primitive::Medium(v2::Medium {
            phase: lower_phase(e.phase.as_deref(), &e.id)?,
            density: e.density.clone(),
            porosity: e.porosity.clone(),
        }),
        "link" => v2::Primitive::Link(v2::Link {
            source: e.source.clone(),
            target: e.target.clone(),
            rate: e.rate.clone(),
            fraction: e.fraction.clone(),
            priority: e.priority,
            transit_time: e.transit_time.clone(),
            decay_rate: e.decay_rate.clone(),
            dispersion: e.dispersion.clone(),
            schedule: e.schedule.as_ref().map(lower_trigger),
            // species_transport (species/medium/fluxes/geometry) lands in M4.
            species: None,
            medium: None,
            fluxes: Vec::new(),
            geometry: None,
        }),
        "event" => v2::Primitive::Event(v2::Event {
            trigger: e.trigger.as_ref().map(lower_trigger),
            effects: e.effects.iter().map(lower_effect).collect(),
            event_value: e.event_value.as_ref().map(lower_qexpr),
            count_limit: e.count_limit,
            rate: e.rate.clone(),
            // failure_state_machine lowering lands with its engine support.
            failure_process: None,
        }),
        "cell" => {
            return Err(EngineError::Unsupported(format!(
                "v2 parse: primitive 'cell' (element '{}') lands in M4",
                e.id
            )));
        }
        other => {
            return Err(EngineError::InvalidModel(format!(
                "element '{}' has unknown primitive '{other}'",
                e.id
            )));
        }
    };

    Ok(v2::Element { base, primitive })
}

fn lower_node(e: &RawElement) -> Result<v2::Node, EngineError> {
    let rule_name = e.value_rule.as_deref().ok_or_else(|| {
        EngineError::InvalidModel(format!("node '{}' missing value_rule", e.id))
    })?;
    let missing = |field: &str| {
        EngineError::InvalidModel(format!("node '{}' ({rule_name}) missing '{field}'", e.id))
    };

    let rule = match rule_name {
        "fixed" => {
            let value = if let Some(q) = &e.value {
                v2::FixedValue::Scalar(q.clone())
            } else if let Some(vs) = &e.values {
                v2::FixedValue::Array {
                    values: vs.clone(),
                    unit: e.unit.clone().unwrap_or_else(|| "1".to_string()),
                }
            } else {
                return Err(missing("value or values"));
            };
            v2::NodeRule::Fixed { value, editable: e.editable, bounds: e.bounds.clone() }
        }
        "expression" => v2::NodeRule::Expression(e.expression.clone().ok_or_else(|| missing("expression"))?),
        "sample" => v2::NodeRule::Sample {
            distribution: e.distribution.clone().ok_or_else(|| missing("distribution"))?,
            resampling: e.resampling.as_ref().map(lower_trigger),
            autocorrelation: e.autocorrelation,
            correlations: e.correlations.clone(),
        },
        "process" => v2::NodeRule::Process {
            process: e.process.clone().ok_or_else(|| missing("process"))?,
            lower_bound: e.lower_bound.clone(),
        },
        "lookup" => {
            let t = e.table.as_ref().ok_or_else(|| missing("table"))?;
            v2::NodeRule::Lookup(v2::LookupTable {
                x: t.x.clone(),
                y: t.y.clone(),
                z: t.z.clone(),
                x_unit: t.x_unit.clone(),
                y_unit: t.y_unit.clone(),
                z_unit: t.z_unit.clone(),
                interpolation: lower_interp(e.interpolation.as_deref()),
                extrapolation: Default::default(),
            })
        }
        "series" => v2::NodeRule::Series {
            timestamps: e.timestamps.clone().ok_or_else(|| missing("timestamps"))?,
            values: e.values.clone().ok_or_else(|| missing("values"))?,
            time_unit: e.time_unit.clone(),
            interpolation: lower_interp(e.interpolation.as_deref()),
        },
        "lag" => v2::NodeRule::Lag {
            input: e.input.clone().ok_or_else(|| missing("input"))?,
            initial: e.initial.clone(),
        },
        "convolution" => v2::NodeRule::Convolution {
            input: e.input.clone().ok_or_else(|| missing("input"))?,
            response: match e.response.as_ref().ok_or_else(|| missing("response"))? {
                RawResponse::Inline { times, values, times_unit, values_unit } => v2::ConvResponse::Inline {
                    times: times.clone(),
                    values: values.clone(),
                    times_unit: times_unit.clone(),
                    values_unit: values_unit.clone(),
                },
                RawResponse::Ref(id) => v2::ConvResponse::Ref(id.clone()),
            },
        },
        "markov" => v2::NodeRule::Markov {
            states: e.states.clone().ok_or_else(|| missing("states"))?,
            initial_state: lower_markov_start(e.initial_state.as_ref().ok_or_else(|| missing("initial_state"))?, &e.id)?,
            transition_matrix: lower_transition_matrix(e.transition_matrix.as_ref().ok_or_else(|| missing("transition_matrix"))?, &e.id)?,
            output_values: e.output_values.clone().ok_or_else(|| missing("output_values"))?,
        },
        "hysteresis" => v2::NodeRule::Hysteresis {
            input: e.input.clone().ok_or_else(|| missing("input"))?,
            high_threshold: e.high_threshold.clone().ok_or_else(|| missing("high_threshold"))?,
            low_threshold: e.low_threshold.clone().ok_or_else(|| missing("low_threshold"))?,
            output_above: e.output_above.clone().ok_or_else(|| missing("output_above"))?,
            output_below: e.output_below.clone().ok_or_else(|| missing("output_below"))?,
        },
        "filter" => v2::NodeRule::Filter {
            input: e.input.clone().ok_or_else(|| missing("input"))?,
            window: e.window.ok_or_else(|| missing("window"))?,
            statistic: lower_filter_stat(e.statistic.as_deref(), &e.id)?,
        },
        "gate_logic" => v2::NodeRule::GateLogic {
            root: lower_gate(e.root.as_ref().ok_or_else(|| missing("root"))?),
            semantics: lower_semantics(e.semantics.as_deref()),
        },
        other => {
            return Err(EngineError::InvalidModel(format!(
                "node '{}' has unknown value_rule '{other}'",
                e.id
            )));
        }
    };
    Ok(v2::Node { rule })
}

fn lower_stock(e: &RawElement) -> Result<v2::Stock, EngineError> {
    Ok(v2::Stock {
        initial_value: e.initial_value.clone().ok_or_else(|| {
            EngineError::InvalidModel(format!("stock '{}' missing initial_value", e.id))
        })?,
        initial_expression: None,
        rate: e.rate.clone(),
        inflows: e.inflows.clone(),
        outflows: e.outflows.clone(),
        floor: e.floor.clone(),
        capacity: e.capacity.clone(),
        overflow_target: e.overflow_target.clone(),
        return_rate: e.return_rate.clone(),
        withdrawals: e.withdrawals.iter().map(|w| v2::WithdrawalSpec {
            target: w.target.clone(),
            priority: w.priority,
            request: w.request.clone(),
            limit: w.limit.clone(),
            label: w.label.clone(),
        }).collect(),
    })
}

fn lower_gate_primitive(e: &RawElement) -> Result<v2::Gate, EngineError> {
    Ok(v2::Gate {
        root: lower_gate(e.root.as_ref().ok_or_else(|| {
            EngineError::InvalidModel(format!("gate '{}' missing root", e.id))
        })?),
        semantics: lower_semantics(e.semantics.as_deref()),
    })
}

fn lower_gate(g: &RawGate) -> v2::GateNode {
    match g {
        RawGate::And { children } => v2::GateNode::And(children.iter().map(lower_gate).collect()),
        RawGate::Or { children } => v2::GateNode::Or(children.iter().map(lower_gate).collect()),
        RawGate::Not { children } => {
            // schema constrains Not to exactly one child.
            v2::GateNode::Not(Box::new(lower_gate(&children[0])))
        }
        RawGate::NVote { threshold, children } => v2::GateNode::NVote {
            threshold: *threshold,
            children: children.iter().map(lower_gate).collect(),
        },
        RawGate::Reference { reference } => v2::GateNode::Reference(reference.clone()),
        RawGate::Condition { condition } => v2::GateNode::Condition(condition.clone()),
        RawGate::Input { input } => v2::GateNode::Input(input.clone()),
    }
}

fn lower_effect(e: &RawEffect) -> v2::EffectSpec {
    v2::EffectSpec {
        target: e.target.clone(),
        change: e.change.as_ref().map(lower_qexpr),
        mode: match e.mode.as_deref() {
            Some("multiplicative") => v2::EffectMode::Multiplicative,
            Some("replace") => v2::EffectMode::Replace,
            _ => v2::EffectMode::Additive,
        },
        label: e.label.clone(),
    }
}

fn lower_qexpr(q: &RawQexpr) -> v2::QuantityExpr {
    match q {
        RawQexpr::Quantity(qty) => v2::QuantityExpr::Quantity(qty.clone()),
        RawQexpr::Ast(a) => v2::QuantityExpr::Ast(a.clone()),
    }
}

fn lower_trigger(t: &RawTrigger) -> v2::TriggerSpec {
    v2::TriggerSpec {
        mode: t.mode.as_deref().and_then(lower_trigger_mode),
        condition: t.condition.clone(),
        source: t.source.clone(),
        period: t.period.clone(),
        schedule: t.schedule.clone(),
    }
}

fn lower_trigger_mode(s: &str) -> Option<v2::TriggerMode> {
    Some(match s {
        "always" => v2::TriggerMode::Always,
        "on_condition" => v2::TriggerMode::OnCondition,
        "periodic" => v2::TriggerMode::Periodic,
        "on_schedule" => v2::TriggerMode::OnSchedule,
        "on_event" => v2::TriggerMode::OnEvent,
        _ => return None,
    })
}

/// v2 interpolation enum is {linear, step, log_linear, spline}; the engine's
/// InterpolationMethod is {linear, step, cubic}. Map the extras to the nearest method
/// for now (refined when log-linear/spline interpolation land).
fn lower_interp(s: Option<&str>) -> InterpolationMethod {
    match s {
        Some("step") => InterpolationMethod::Step,
        Some("spline") => InterpolationMethod::Cubic,
        _ => InterpolationMethod::Linear,
    }
}

fn lower_semantics(s: Option<&str>) -> v2::GateSemantics {
    match s {
        Some("failure") => v2::GateSemantics::Failure,
        _ => v2::GateSemantics::Success,
    }
}

fn lower_filter_stat(s: Option<&str>, id: &str) -> Result<v2::FilterStat, EngineError> {
    Ok(match s {
        Some("mean") => v2::FilterStat::Mean,
        Some("min") => v2::FilterStat::Min,
        Some("max") => v2::FilterStat::Max,
        Some("sum") => v2::FilterStat::Sum,
        Some("ema") => v2::FilterStat::Ema,
        other => {
            return Err(EngineError::InvalidModel(format!(
                "filter '{id}' has invalid statistic {other:?}"
            )));
        }
    })
}

fn lower_phase(s: Option<&str>, id: &str) -> Result<v2::Phase, EngineError> {
    Ok(match s {
        Some("solid") => v2::Phase::Solid,
        Some("fluid") => v2::Phase::Fluid,
        Some("gas") => v2::Phase::Gas,
        Some("reference_fluid") => v2::Phase::ReferenceFluid,
        other => {
            return Err(EngineError::InvalidModel(format!(
                "medium '{id}' has invalid phase {other:?}"
            )));
        }
    })
}

fn lower_markov_start(v: &serde_json::Value, id: &str) -> Result<v2::MarkovStart, EngineError> {
    if let Some(s) = v.as_str() {
        Ok(v2::MarkovStart::Label(s.to_string()))
    } else if let Some(i) = v.as_u64() {
        Ok(v2::MarkovStart::Index(i as usize))
    } else {
        Err(EngineError::InvalidModel(format!(
            "markov '{id}' initial_state must be a state label or index"
        )))
    }
}

fn lower_transition_matrix(
    rows: &[serde_json::Value],
    id: &str,
) -> Result<Vec<v2::TransitionRow>, EngineError> {
    rows.iter().map(|row| {
        let arr = row.as_array().ok_or_else(|| {
            EngineError::InvalidModel(format!("markov '{id}' transition row is not an array"))
        })?;
        // Fixed numeric row, or expression-valued row.
        if arr.iter().all(|v| v.is_number()) {
            Ok(v2::TransitionRow::Fixed(arr.iter().map(|v| v.as_f64().unwrap()).collect()))
        } else {
            let exprs: Result<Vec<QuantityOrFormula>, _> = arr.iter()
                .map(|v| serde_json::from_value::<QuantityOrFormula>(v.clone()))
                .collect();
            Ok(v2::TransitionRow::Expr(exprs.map_err(EngineError::Json)?))
        }
    }).collect()
}
