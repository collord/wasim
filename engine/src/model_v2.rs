//! Internal v2 model — the engine's canonical representation.
//!
//! v2 reorganizes the v1 fixed-type taxonomy into six composable **primitives**
//! (`node`, `stock`, `link`, `event`, `gate`, `cell`) plus two **definition** types
//! (`species`, `medium`), with behavior driven by *traits activated by field presence*.
//! See `schema/wasim-schema-v2.json` and `schema/wasim-engine-semantics.md`.
//!
//! This is the *clean* engine-facing model. It is produced from two sources:
//!   - v1 models, via [`crate::v1_import`] (the regression bridge),
//!   - v2-native JSON (deserializer deferred — no v2 fixtures exist yet).
//!
//! A handful of fields marked `v1-import compat` are not part of the canonical v2
//! schema; they exist so the normalizer can reproduce v1 behavior faithfully.
#![allow(dead_code)] // link/event/gate/cell primitives are defined ahead of their engine support (M2–M4)

use serde::Serialize;

// Schema-shared building blocks are identical between v1 and v2; reuse them so the
// AST walker (`eval.rs`) and samplers (`sampling.rs`) work against one set of types.
pub use crate::model::{
    AstNode, Bounds, ContainerDef, CorrelationPair, Distribution, ExpressionField,
    InterpolationMethod, OutputSpec, ProcessSpec, Quantity, QuantityOrFormula, SaveSpec,
    SimulationSettings, SourceMetadata, TimeHistoryDisplay,
};

// ── Top-level ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Model {
    pub wasim_version: String,
    pub source: Option<SourceMetadata>,
    pub simulation_settings: SimulationSettings,
    /// v2 adds `reporting_periods`; carried here (v1 import leaves it empty).
    pub reporting_periods: Vec<Quantity>,
    pub containers: Vec<ContainerDef>,
    pub elements: Vec<Element>,
    pub time_history_displays: Vec<TimeHistoryDisplay>,
    /// True when produced by normalizing a v1 model. Drives the §9 cycle policy
    /// (v1-imported → warn + implicit-lag; v2-native → reject).
    pub from_v1: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Element {
    pub base: ElementBase,
    pub primitive: Primitive,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ElementBase {
    pub id: String,
    pub name: String,
    pub container: Option<String>,
    pub description: Option<String>,
    pub outputs: Vec<OutputSpec>,
    pub save_results: SaveSpec,
    pub inputs: Vec<String>,
    /// Original v1 `type` (or v2 `source_type`), for diagnostics. Engine ignores.
    pub source_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum Primitive {
    Node(Node),
    Stock(Stock),
    Link(Link),
    Event(Event),
    Gate(Gate),
    Cell(Cell),
    Species(Species),
    Medium(Medium),
}

// ── NODE ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub rule: NodeRule,
}

#[derive(Debug, Clone, Serialize)]
pub enum NodeRule {
    /// [fixed] Same value every timestep.
    Fixed {
        value: FixedValue,
        editable: bool,
        bounds: Option<Bounds>,
    },
    /// [expression] Evaluate an AST against the current-step context.
    Expression(ExpressionField),
    /// [sample] Draw from a distribution (once per realization, or per `resampling`).
    Sample {
        distribution: Distribution,
        resampling: Option<TriggerSpec>,
        autocorrelation: Option<f64>,
        correlations: Vec<CorrelationPair>,
    },
    /// [process] Time-correlated stochastic process (GBM); per-realization running state.
    Process {
        process: ProcessSpec,
        lower_bound: Option<Quantity>,
    },
    /// [lookup] Interpolation table; not self-evaluating — invoked via `lookup_call`.
    Lookup(LookupTable),
    /// [series] Interpolate a value from a fixed time axis at the current sim time.
    Series {
        timestamps: Vec<f64>,
        values: Vec<f64>,
        time_unit: Option<String>,
        interpolation: InterpolationMethod,
    },
    /// [lag] Strictly one-timestep delay of `input`. Multi-step delays are chained.
    Lag {
        input: String,
        initial: Option<Quantity>,
    },
    /// [convolution] Discrete convolution of input history with a response function.
    Convolution {
        input: String,
        response: ConvResponse,
    },
    /// [markov] Discrete-state automaton; output is `output_values[state]`.
    Markov {
        states: Vec<String>,
        initial_state: MarkovStart,
        transition_matrix: Vec<TransitionRow>,
        output_values: Vec<f64>,
    },
    /// [hysteresis] Binary state with separate high/low thresholds (Schmitt trigger).
    Hysteresis {
        input: String,
        high_threshold: Quantity,
        low_threshold: Quantity,
        output_above: Quantity,
        output_below: Quantity,
    },
    /// [filter] Rolling-window statistic over `input`.
    Filter {
        input: String,
        window: usize,
        statistic: FilterStat,
    },
    /// [gate_logic] Boolean logic tree → 1.0/0.0.
    GateLogic {
        root: GateNode,
        semantics: GateSemantics,
    },
}

/// `fixed` node payload: scalar (carries its own unit) or an array sharing one unit.
#[derive(Debug, Clone, Serialize)]
pub enum FixedValue {
    Scalar(Quantity),
    Array { values: Vec<f64>, unit: String },
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LookupTable {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// 2-D values (rows × columns). Empty for 1-D tables.
    pub z: Vec<Vec<f64>>,
    pub x_unit: Option<String>,
    pub y_unit: Option<String>,
    pub z_unit: Option<String>,
    pub interpolation: InterpolationMethod,
    /// v1-import compat: v1 lookups carried an explicit out-of-range policy.
    pub extrapolation: crate::model::ExtrapolationMethod,
}

#[derive(Debug, Clone, Serialize)]
pub enum ConvResponse {
    Inline {
        times: Vec<f64>,
        values: Vec<f64>,
        times_unit: Option<String>,
        values_unit: Option<String>,
    },
    /// Element id of a lookup/series supplying the response function.
    Ref(String),
}

#[derive(Debug, Clone, Serialize)]
pub enum MarkovStart {
    Label(String),
    Index(usize),
}

#[derive(Debug, Clone, Serialize)]
pub enum TransitionRow {
    Fixed(Vec<f64>),
    Expr(Vec<QuantityOrFormula>),
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum FilterStat {
    Mean,
    Min,
    Max,
    Sum,
    Ema,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
pub enum GateSemantics {
    #[default]
    Success,
    Failure,
}

// ── STOCK ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Stock {
    pub initial_value: Quantity,
    /// v1-import compat: v1 accumulators could seed the initial level from an AST.
    pub initial_expression: Option<ExpressionField>,
    pub rate: Option<QuantityOrFormula>,
    pub inflows: Vec<String>,
    pub outflows: Vec<String>,
    pub floor: Option<Quantity>,
    pub capacity: Option<QuantityOrFormula>,    // trait: capacity_clamp
    pub overflow_target: Option<String>,        // trait: overflow_routing
    pub return_rate: Option<QuantityOrFormula>, // trait: compound_growth
    pub withdrawals: Vec<WithdrawalSpec>,        // trait: priority_withdrawal
}

#[derive(Debug, Clone, Serialize)]
pub struct WithdrawalSpec {
    pub target: String,
    pub priority: Option<i64>,
    pub request: Option<QuantityOrFormula>,
    pub limit: Option<QuantityOrFormula>,
    pub label: Option<String>,
}

// ── LINK ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Link {
    pub source: Option<String>,
    pub target: Option<String>,
    pub rate: Option<QuantityOrFormula>,
    pub fraction: Option<QuantityOrFormula>,
    pub priority: Option<i64>,            // trait: priority_allocation
    pub transit_time: Option<Quantity>,  // trait: transit_buffer
    pub decay_rate: Option<QuantityOrFormula>, // trait: transit_decay
    pub dispersion: Option<Quantity>,    // trait: transit_dispersion (Péclet number)
    pub schedule: Option<TriggerSpec>,   // trait: scheduled_flow
    pub species: Option<String>,         // trait: species_transport
    pub medium: Option<String>,
    pub fluxes: Vec<FluxSpec>,
    pub geometry: Option<LinkGeometry>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum LinkGeometry {
    Cell,
    Aquifer,
    Pipe,
    Conduit,
}

#[derive(Debug, Clone, Serialize)]
pub struct FluxSpec {
    pub mechanism: FluxMechanism,
    pub rate: Option<QuantityOrFormula>,
    pub coefficient: Option<QuantityOrFormula>,
    pub species: Option<String>,
    pub medium: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum FluxMechanism {
    Advective,
    Diffusive,
    Direct,
    Settling,
    Precipitation,
}

// ── EVENT ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub trigger: Option<TriggerSpec>,
    pub effects: Vec<EffectSpec>,
    pub event_value: Option<QuantityExpr>,
    pub count_limit: Option<i64>,
    pub rate: Option<QuantityOrFormula>,           // trait: rate_generation
    pub failure_process: Option<FailureProcess>,    // trait: failure_state_machine
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectSpec {
    pub target: String,
    pub change: Option<QuantityExpr>,
    pub mode: EffectMode,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
pub enum EffectMode {
    #[default]
    Additive,
    Multiplicative,
    Replace,
}

/// `quantity_expr`: a fixed quantity or a bare AST (no formula-string fallback).
#[derive(Debug, Clone, Serialize)]
pub enum QuantityExpr {
    Quantity(Quantity),
    Ast(AstNode),
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureProcess {
    pub basis: FailureBasis,
    pub time_to_failure: Option<Distribution>,
    pub repair: Option<RepairSpec>,
    pub demand_capacity: Option<QuantityOrFormula>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum FailureBasis {
    ExposureTime,
    OperatingTime,
    Demand,
    CapacityDemand,
    Event,
    Condition,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepairSpec {
    pub time_to_repair: Option<Distribution>,
    pub policy: RepairPolicy,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum RepairPolicy {
    None,
    Repair,
    Replace,
    PreventiveMaintenance,
}

// ── GATE ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Gate {
    pub root: GateNode,
    pub semantics: GateSemantics,
}

#[derive(Debug, Clone, Serialize)]
pub enum GateNode {
    And(Vec<GateNode>),
    Or(Vec<GateNode>),
    Not(Box<GateNode>),
    NVote { threshold: u32, children: Vec<GateNode> },
    Reference(String),
    Condition(QuantityOrFormula),
    Input(String),
}

// ── CELL ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Cell {
    pub volume: Option<QuantityOrFormula>,
    pub media: Vec<MediumRef>,
    pub species: Vec<SpeciesRef>,
    pub inflows: Vec<String>,
    pub partitioning: Vec<PartitionEntry>, // trait: partitioning_equilibrium
    pub inventory: Option<QuantityOrFormula>,    // trait: source_release
    pub release_rate: Option<QuantityOrFormula>,
    pub release_schedule: Option<TriggerSpec>,
    pub release_target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediumRef {
    pub medium: String,
    pub fraction: Option<QuantityOrFormula>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpeciesRef {
    pub species: String,
    pub initial_inventory: Option<Quantity>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PartitionEntry {
    pub species: String,
    pub from_medium: String,
    pub to_medium: String,
    pub coefficient: QuantityOrFormula,
}

// ── Definition types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Default)]
pub struct Species {
    pub half_life: Option<Quantity>,
    pub decay_products: Vec<DecayProduct>,
    pub molecular_weight: Option<Quantity>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecayProduct {
    pub species: String,
    pub branching_fraction: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Medium {
    pub phase: Phase,
    pub density: Option<QuantityOrFormula>,
    pub porosity: Option<QuantityOrFormula>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum Phase {
    Solid,
    Fluid,
    Gas,
    ReferenceFluid,
}

// ── Shared: triggers ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Default)]
pub struct TriggerSpec {
    pub mode: Option<TriggerMode>,
    pub condition: Option<QuantityOrFormula>,
    pub source: Option<String>,
    pub period: Option<Quantity>,
    pub schedule: Vec<Quantity>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum TriggerMode {
    Always,
    OnCondition,
    Periodic,
    OnSchedule,
    OnEvent,
}

// ── Convenience accessors ─────────────────────────────────────────────────────

impl Element {
    pub fn id(&self) -> &str {
        &self.base.id
    }

    /// Trait/role helpers used by graph + engine dispatch.
    pub fn as_node(&self) -> Option<&Node> {
        match &self.primitive {
            Primitive::Node(n) => Some(n),
            _ => None,
        }
    }
    pub fn as_stock(&self) -> Option<&Stock> {
        match &self.primitive {
            Primitive::Stock(s) => Some(s),
            _ => None,
        }
    }
}
