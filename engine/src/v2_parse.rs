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
    dimensions: Vec<crate::model::DimensionDef>,
    #[serde(default)]
    optimization: Option<crate::model::OptimizationSpec>,
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
    /// Calendar anchor (B6): model-clock start as seconds since the Unix epoch.
    #[serde(default)]
    calendar_start: Option<f64>,
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

    // status (§2): independent set/reset triggers
    #[serde(default)]
    set: Option<RawTrigger>,
    #[serde(default)]
    reset: Option<RawTrigger>,
    // pid controller (§2)
    #[serde(default)]
    setpoint: Option<QuantityOrFormula>,
    #[serde(default)]
    kp: Option<f64>,
    #[serde(default)]
    ki: Option<f64>,
    #[serde(default)]
    kd: Option<f64>,
    #[serde(default)]
    output_min: Option<f64>,
    #[serde(default)]
    output_max: Option<f64>,
    #[serde(default)]
    deadband: Option<f64>,
    // on_off controller (§2.15): top-level fields re-gsm lifts from the `controller` role map.
    /// Controller mode: `pid` (default) | `proportional` | `on_off`.
    #[serde(default, rename = "controller_mode")]
    controller_mode: Option<String>,
    #[serde(default)]
    output_cap: Option<QuantityOrFormula>,
    /// on_off hysteresis band as a ref/formula (the PID `deadband` above is a plain number).
    #[serde(default)]
    deadband_ref: Option<QuantityOrFormula>,
    // queue (§B3)
    #[serde(default)]
    delay_time: Option<QuantityOrFormula>,
    #[serde(default)]
    discipline: Option<String>,

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
    /// Species-set members (per-nuclide decay data). See v2::SpeciesMember.
    #[serde(default)]
    members: Vec<RawMember>,

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
    count_limit: Option<f64>,
    #[serde(default)]
    failure_process: Option<RawFailure>,

    // cell
    #[serde(default)]
    volume: Option<QuantityOrFormula>,
    #[serde(default)]
    media: Vec<RawMediumRef>,
    /// `species` is an array of species refs on a cell, but a single id string on a
    /// species_transport link — kept untyped and branched during lowering.
    #[serde(default)]
    species: Option<serde_json::Value>,
    /// link species_transport medium (a single id string).
    #[serde(default)]
    medium: Option<String>,
    #[serde(default)]
    partitioning: Vec<RawPartition>,
    #[serde(default)]
    inventory: Option<QuantityOrFormula>,
    #[serde(default)]
    release_rate: Option<QuantityOrFormula>,
    #[serde(default)]
    release_schedule: Option<RawTrigger>,
    #[serde(default)]
    release_target: Option<String>,
    /// coupled_transport fluxes. Carried on both cell (`source` implicit = owning cell)
    /// and link (`source`/`target` = the link's endpoints).
    #[serde(default)]
    fluxes: Vec<RawFlux>,
}

#[derive(Deserialize)]
struct RawSpeciesRef {
    species: String,
    #[serde(default)]
    initial_inventory: Option<Quantity>,
}

#[derive(Deserialize)]
struct RawMediumRef {
    medium: String,
    #[serde(default)]
    fraction: Option<QuantityOrFormula>,
}

#[derive(Deserialize)]
struct RawPartition {
    /// Optional: `null`/absent = applies to all species in the cell's set (set-wide Kd).
    #[serde(default)]
    species: Option<String>,
    from_medium: String,
    to_medium: String,
    coefficient: QuantityOrFormula,
}

#[derive(Deserialize)]
struct RawFlux {
    mechanism: String,
    #[serde(default)]
    rate: Option<QuantityOrFormula>,
    #[serde(default)]
    coefficient: Option<QuantityOrFormula>,
    #[serde(default)]
    species: Option<String>,
    #[serde(default)]
    medium: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

#[derive(Deserialize)]
struct RawFailure {
    basis: String,
    #[serde(default)]
    time_to_failure: Option<Distribution>,
    #[serde(default)]
    repair: Option<RawRepair>,
    #[serde(default)]
    demand_capacity: Option<QuantityOrFormula>,
}

#[derive(Deserialize)]
struct RawRepair {
    #[serde(default)]
    time_to_repair: Option<Distribution>,
    #[serde(default)]
    policy: Option<String>,
}

#[derive(Deserialize)]
struct RawEffect {
    // Optional so an `interrupt` effect (which ends the realization and has no target) parses.
    #[serde(default)]
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
    // Expr must precede Inline: it requires `expression`, so serde's untagged matching only
    // selects it for the expression form; the all-default Inline would otherwise swallow it.
    Expr {
        expression: ExpressionField,
        interval: Quantity,
        length: Quantity,
        #[serde(default)]
        cumulative: bool,
    },
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

#[derive(Deserialize)]
struct RawMember {
    name: String,
    #[serde(default)]
    half_life: Option<Quantity>,
    #[serde(default)]
    decay_products: Vec<RawDecayProduct>,
    #[serde(default)]
    molecular_weight: Option<Quantity>,
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
            calendar_start: raw.simulation_settings.calendar_start,
        },
        reporting_periods: raw.simulation_settings.reporting_periods,
        dimensions: raw.dimensions,
        optimization: raw.optimization,
        containers: raw.containers,
        elements,
        time_history_displays: raw.time_history_displays,
        from_v1: false,
        dynamic_optimization: false,
    })
}

/// Normalize stock secondary-output roles into the 0.9.7 orthogonal form (§1c): split each
/// fused `*_rate` alias into `(role: <flow>, output_kind: "rate")`, and default a flow role's
/// `output_kind` to `"rate"` (0.9.6 back-compat, where every stock role was a rate). After this,
/// the engine only ever matches on flow-only names + an explicit kind — the aliases can't drift.
fn normalize_output_roles(outputs: &mut [OutputSpec]) {
    for o in outputs.iter_mut() {
        let Some(role) = o.role.as_deref() else { continue };
        let (flow, kind) = match role {
            "addition_rate" => ("addition", "rate"),
            "withdrawal_rate" => ("withdrawal", "rate"),
            "overflow_rate" => ("overflow", "rate"),
            // Already a flow-only name: keep the flow, default the kind to `rate` if unset.
            other => (other, o.output_kind.as_deref().unwrap_or("rate")),
        };
        o.role = Some(flow.to_string());
        o.output_kind = Some(kind.to_string());
    }
}

fn lower_element(e: RawElement) -> Result<v2::Element, EngineError> {
    let mut outputs = e.outputs.clone();
    normalize_output_roles(&mut outputs);
    let base = v2::ElementBase {
        id: e.id.clone(),
        name: e.name.clone(),
        container: e.container.clone(),
        description: e.description.clone(),
        outputs,
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
            decay_products: e.decay_products.iter().map(lower_decay_product).collect(),
            molecular_weight: e.molecular_weight.clone(),
            members: e.members.iter().map(|m| v2::SpeciesMember {
                name: m.name.clone(),
                half_life: m.half_life.clone(),
                decay_products: m.decay_products.iter().map(lower_decay_product).collect(),
                molecular_weight: m.molecular_weight.clone(),
            }).collect(),
        }),
        "medium" => v2::Primitive::Medium(v2::Medium {
            phase: lower_phase(e.phase.as_deref(), &e.id)?,
            density: e.density.clone(),
            porosity: e.porosity.clone(),
        }),
        "resource" => v2::Primitive::Resource(v2::Resource {
            initial: e.initial_value.clone().or_else(|| e.initial.clone()).ok_or_else(|| {
                EngineError::InvalidModel(format!("resource '{}' missing 'initial_value'", e.id))
            })?,
            capacity: e.capacity.clone(),
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
            // species_transport: species/medium are single id strings here.
            species: e.species.as_ref().and_then(|v| v.as_str()).map(String::from),
            medium: e.medium.clone(),
            fluxes: e.fluxes.iter().map(lower_flux).collect(),
            geometry: None,
        }),
        "event" => v2::Primitive::Event(v2::Event {
            trigger: e.trigger.as_ref().map(lower_trigger),
            effects: e.effects.iter().map(lower_effect).collect(),
            event_value: e.event_value.as_ref().map(lower_qexpr),
            count_limit: e.count_limit,
            rate: e.rate.clone(),
            failure_process: e.failure_process.as_ref().map(|f| lower_failure(f, &e.id)).transpose()?,
        }),
        "cell" => v2::Primitive::Cell(v2::Cell {
            volume: e.volume.clone(),
            media: e.media.iter().map(|m| v2::MediumRef {
                medium: m.medium.clone(),
                fraction: m.fraction.clone(),
            }).collect(),
            species: cell_species_refs(&e.species),
            inflows: e.inflows.clone(),
            partitioning: e.partitioning.iter().map(|p| v2::PartitionEntry {
                species: p.species.clone(),
                from_medium: p.from_medium.clone(),
                to_medium: p.to_medium.clone(),
                coefficient: p.coefficient.clone(),
            }).collect(),
            inventory: e.inventory.clone(),
            release_rate: e.release_rate.clone(),
            release_schedule: e.release_schedule.as_ref().map(lower_trigger),
            release_target: e.release_target.clone(),
            fluxes: e.fluxes.iter().map(lower_flux).collect(),
        }),
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
                // `log_linear` interpolation lowers to linear interpolation of ln(y) (§10).
                log_result: matches!(e.interpolation.as_deref(), Some("log_linear")),
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
            input: e.input.clone(),
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
                RawResponse::Expr { expression, interval, length, cumulative } => {
                    let to_s = |q: &crate::model::Quantity| {
                        crate::units::convert(q.value, &q.unit, "s").unwrap_or(q.value)
                    };
                    v2::ConvResponse::Expr {
                        ast: expression.ast.clone(),
                        interval_s: to_s(interval),
                        length_s: to_s(length),
                        cumulative: *cumulative,
                    }
                }
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
        // A filter without an `input` is schema-valid (emit's unresolvable-signal case):
        // tolerate it per the dangling-ref policy — the empty id resolves to no output,
        // so the filter runs over a 0.0 signal instead of rejecting the model at load.
        "filter" => v2::NodeRule::Filter {
            input: e.input.clone().unwrap_or_default(),
            window: e.window.ok_or_else(|| missing("window"))?,
            statistic: lower_filter_stat(e.statistic.as_deref(), &e.id)?,
        },
        "gate_logic" => v2::NodeRule::GateLogic {
            root: lower_gate(e.root.as_ref().ok_or_else(|| missing("root"))?),
            semantics: lower_semantics(e.semantics.as_deref()),
        },
        "status" => v2::NodeRule::Status {
            set: lower_trigger(e.set.as_ref().ok_or_else(|| missing("set"))?),
            reset: lower_trigger(e.reset.as_ref().ok_or_else(|| missing("reset"))?),
        },
        "milestone" => v2::NodeRule::Milestone {
            trigger: lower_trigger(e.trigger.as_ref().ok_or_else(|| missing("trigger"))?),
        },
        "pid" | "controller" => v2::NodeRule::PidController {
            input: e.input.clone().ok_or_else(|| missing("input"))?,
            setpoint: e.setpoint.clone().ok_or_else(|| missing("setpoint"))?,
            kp: e.kp.unwrap_or(0.0),
            ki: e.ki.unwrap_or(0.0),
            kd: e.kd.unwrap_or(0.0),
            output_min: e.output_min,
            output_max: e.output_max,
            deadband: e.deadband.unwrap_or(0.0),
            mode: e.controller_mode.clone(),
            output_cap: e.output_cap.clone(),
            deadband_ref: e.deadband_ref.clone(),
        },
        "queue" => v2::NodeRule::Queue {
            input: e.input.clone().ok_or_else(|| missing("input"))?,
            delay_time: e.delay_time.clone().ok_or_else(|| missing("delay_time"))?,
            capacity: e.capacity.clone(),
            discipline: match e.discipline.as_deref() {
                Some("fixed_at_entry") => v2::QueueDiscipline::FixedAtEntry,
                _ => v2::QueueDiscipline::Conveyor,
            },
        },
        // A linked-Excel element (§20): the workbook is external, so the engine cannot evaluate
        // it. Parse it as a fixed-0 placeholder (the cells/external_file binding is preserved in
        // the JSON for round-trip/inspection but not executed). Loads and runs, yields 0.0.
        "spreadsheet" => v2::NodeRule::Fixed {
            value: v2::FixedValue::Scalar(crate::model::Quantity {
                value: 0.0,
                unit: e.unit.clone().unwrap_or_else(|| "1".to_string()),
                display_unit: None,
            }),
            editable: false,
            bounds: None,
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

/// Parse a cell's `species` array (untyped because the key is overloaded with link's
/// string-valued `species`) into species refs.
fn cell_species_refs(v: &Option<serde_json::Value>) -> Vec<v2::SpeciesRef> {
    v.as_ref()
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| serde_json::from_value::<RawSpeciesRef>(item.clone()).ok())
                .map(|s| v2::SpeciesRef { species: s.species, initial_inventory: s.initial_inventory })
                .collect()
        })
        .unwrap_or_default()
}

fn lower_effect(e: &RawEffect) -> v2::EffectSpec {
    v2::EffectSpec {
        target: e.target.clone(),
        change: e.change.as_ref().map(lower_qexpr),
        mode: match e.mode.as_deref() {
            Some("multiplicative") => v2::EffectMode::Multiplicative,
            Some("replace") => v2::EffectMode::Replace,
            Some("interrupt") => v2::EffectMode::Interrupt,
            Some("spend") => v2::EffectMode::Spend,
            Some("deposit") => v2::EffectMode::Deposit,
            Some("borrow") => v2::EffectMode::Borrow,
            _ => v2::EffectMode::Additive,
        },
        label: e.label.clone(),
    }
}

fn lower_decay_product(d: &RawDecayProduct) -> v2::DecayProduct {
    v2::DecayProduct {
        species: d.species.clone(),
        branching_fraction: d.branching_fraction,
    }
}

fn lower_flux(f: &RawFlux) -> v2::FluxSpec {
    v2::FluxSpec {
        mechanism: match f.mechanism.as_str() {
            "diffusive" => v2::FluxMechanism::Diffusive,
            "direct" => v2::FluxMechanism::Direct,
            "settling" => v2::FluxMechanism::Settling,
            "precipitation" => v2::FluxMechanism::Precipitation,
            _ => v2::FluxMechanism::Advective,
        },
        rate: f.rate.clone(),
        coefficient: f.coefficient.clone(),
        species: f.species.clone(),
        medium: f.medium.clone(),
        source: f.source.clone(),
        target: f.target.clone(),
    }
}

fn lower_qexpr(q: &RawQexpr) -> v2::QuantityExpr {
    match q {
        RawQexpr::Quantity(qty) => v2::QuantityExpr::Quantity(qty.clone()),
        RawQexpr::Ast(a) => v2::QuantityExpr::Ast(a.clone()),
    }
}

fn lower_failure(f: &RawFailure, id: &str) -> Result<v2::FailureProcess, EngineError> {
    let basis = match f.basis.as_str() {
        "exposure_time" => v2::FailureBasis::ExposureTime,
        "operating_time" => v2::FailureBasis::OperatingTime,
        "demand" => v2::FailureBasis::Demand,
        "capacity_demand" => v2::FailureBasis::CapacityDemand,
        "event" => v2::FailureBasis::Event,
        "condition" => v2::FailureBasis::Condition,
        other => {
            return Err(EngineError::InvalidModel(format!(
                "event '{id}' failure_process has invalid basis '{other}'"
            )));
        }
    };
    let repair = f.repair.as_ref().map(|r| v2::RepairSpec {
        time_to_repair: r.time_to_repair.clone(),
        policy: match r.policy.as_deref() {
            Some("repair") => v2::RepairPolicy::Repair,
            Some("replace") => v2::RepairPolicy::Replace,
            Some("preventive_maintenance") => v2::RepairPolicy::PreventiveMaintenance,
            _ => v2::RepairPolicy::None,
        },
    });
    Ok(v2::FailureProcess {
        basis,
        time_to_failure: f.time_to_failure.clone(),
        repair,
        demand_capacity: f.demand_capacity.clone(),
    })
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
