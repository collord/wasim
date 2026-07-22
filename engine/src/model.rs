use serde::{Deserialize, Serialize};

// ── Top-level model ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WasimModel {
    pub wasim_version: String,
    pub source: Option<SourceMetadata>,
    pub simulation_settings: SimulationSettings,
    #[serde(default)]
    pub containers: Vec<ContainerDef>,
    pub elements: Vec<Element>,
    /// Display-only expressions from source-model TimeHistoryResult plots. Not part of the
    /// simulation graph, but evaluated at each timestep against finalized element outputs
    /// and surfaced in `SimulationResults.elements` so user-visible outputs aren't lost.
    #[serde(default)]
    pub time_history_displays: Vec<TimeHistoryDisplay>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeHistoryDisplay {
    pub id: String,
    pub name: String,
    pub expression: ExpressionField,
    #[serde(default)]
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceMetadata {
    pub generator: Option<String>,
    pub generator_version: Option<String>,
    pub created: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimulationSettings {
    pub duration: Quantity,
    pub timestep: Quantity,
    #[serde(default = "default_n_realizations")]
    pub n_realizations: u32,
    #[serde(default)]
    pub sampling_method: SamplingMethod,
    pub seed: Option<u64>,
    /// Calendar anchor (B6): the model clock's start time as seconds since the Unix epoch
    /// (1970-01-01). When present, `time_ref` calendar properties (year/month/day/…) use a real
    /// proleptic-Gregorian calendar **with leap years** anchored here. Absent = the fixed 365-day
    /// calendar (behavior unchanged).
    #[serde(default)]
    pub calendar_start: Option<f64>,
}

fn default_n_realizations() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SamplingMethod {
    #[default]
    MonteCarlo,
    Lhs,
}

// ── Shared building blocks ────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Quantity {
    pub value: f64,
    pub unit: String,
    pub display_unit: Option<String>,
}

/// A distribution parameter that is either a fixed Quantity, a parsed expression AST,
/// or a raw formula string referencing another element (e.g. `"Mean_Ore / 5"`).
/// Expression ASTs and formula strings are stored but currently evaluated as 0.0 at runtime.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum QuantityOrFormula {
    Quantity(Quantity),
    Expression(ExpressionField),
    Formula(String),
}

impl QuantityOrFormula {
    pub fn value(&self) -> f64 {
        match self {
            QuantityOrFormula::Quantity(q) => q.value,
            QuantityOrFormula::Expression(_) => 0.0,
            QuantityOrFormula::Formula(_) => 0.0,
        }
    }
    pub fn unit(&self) -> &str {
        match self {
            QuantityOrFormula::Quantity(q) => q.unit.as_str(),
            QuantityOrFormula::Expression(_) => "1",
            QuantityOrFormula::Formula(_) => "1",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContainerDef {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
    /// Interior element ids (v2). Membership is also carried by each element's `container`
    /// back-ref (authoritative); this list is a convenience the emit side may populate.
    #[serde(default)]
    pub elements: Vec<String>,
    /// Structural role. `container`/`group` are organizational; `submodel` is a nested run (§12).
    #[serde(default)]
    pub kind: ContainerKind,
    /// For `kind: submodel`: the nested run's settings. None inherits the parent's.
    #[serde(default)]
    pub simulation_settings: Option<SimulationSettings>,
    /// Named boundary inputs/outputs (submodel interface, §12).
    #[serde(default)]
    pub interface: Option<ContainerInterface>,
    /// For `kind: submodel`: a dynamic (per-timestep) optimization re-solved each outer
    /// timestep, so the optimized variables become per-timestep series (§13a). None = the
    /// submodel is not optimized. Distinct from the top-level study-level `optimization`.
    #[serde(default)]
    pub optimization: Option<OptimizationSpec>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerKind {
    #[default]
    Container,
    Group,
    Submodel,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ContainerInterface {
    #[serde(default)]
    pub inputs: Vec<InterfaceInput>,
    #[serde(default)]
    pub outputs: Vec<String>,
}

/// A submodel boundary input: the parent `from` element drives the interior `input` element.
/// `from` is None for an engine/dashboard-supplied input with no model driver.
#[derive(Debug, Clone, Serialize)]
pub struct InterfaceInput {
    pub input: String,
    pub from: Option<String>,
}

// Accept both the 0.8.4 object form `{input, from}` and the pre-0.8.4 bare-string form
// (which carries only the consumer id, no driver) during the corpus cutover.
impl<'de> Deserialize<'de> for InterfaceInput {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Str(String),
            Obj {
                input: String,
                #[serde(default)]
                from: Option<String>,
            },
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Str(input) => InterfaceInput { input, from: None },
            Raw::Obj { input, from } => InterfaceInput { input, from },
        })
    }
}

// ── Optimization study (§13) ────────────────────────────────────────────────────

/// A study-level optimization: search variable values that make the objective best.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OptimizationSpec {
    pub objective: Objective,
    pub variables: Vec<OptVariable>,
    #[serde(default)]
    pub constraints: Vec<OptConstraint>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Objective {
    pub element_id: String,
    pub direction: OptDirection,
    /// Present for a probabilistic objective; None = deterministic (single value).
    #[serde(default)]
    pub statistic: Option<ObjectiveStatistic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptDirection {
    Maximize,
    Minimize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObjectiveStatistic {
    pub kind: ObjectiveStatKind,
    /// Percentile in [0,100]; required when kind = percentile.
    #[serde(default)]
    pub p: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveStatKind {
    Mean,
    Percentile,
    Peak,
    Valley,
    Sum,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OptVariable {
    pub element_id: String,
    pub lower: Quantity,
    pub upper: Quantity,
    pub initial: Quantity,
    #[serde(default)]
    pub integer: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OptConstraint {
    pub condition: QuantityOrFormula,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputSpec {
    pub name: String,
    pub unit: String,
    pub display_unit: Option<String>,
    /// Ids of the dimensions (top-level `dimensions[]`) this output ranges over.
    /// Empty = scalar. See wasim-engine-semantics.md §15.
    #[serde(default)]
    pub dimensions: Vec<String>,
    /// Flow a stock's secondary output port reports (§1c). Flow-only names (0.9.7):
    /// `addition`, `withdrawal`, `overflow`, `net_change`. The fused `*_rate` names
    /// (`addition_rate`/`withdrawal_rate`/`overflow_rate`) are 0.9.6 aliases and are
    /// normalized at parse into `<flow>` + `output_kind: rate` (see `normalize_output_roles`).
    /// Role-less secondaries resolve to the element's primary value (pre-0.9.2 behavior).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// How the roled flow accumulates (§1c): `level` (the stock's own value), `rate`
    /// (per-step applied flow), or `cumulative` (running total since run start). Absent on
    /// plain outputs; defaults to `rate` when a flow `role` is present (0.9.6 back-compat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_kind: Option<String>,
}

/// A named, ordered dimension (ordinal set) — `size` members, optionally labeled.
/// `vector_map.over` iterates these; `output_spec.dimensions` reference them by id.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DimensionDef {
    pub id: String,
    pub name: String,
    pub size: usize,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SaveSpec {
    pub final_value: Option<bool>,
    pub time_history: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Bounds {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

// ── Elements ──────────────────────────────────────────────────────────────────

/// Common fields + type-specific payload via flatten.
/// serde_json correctly handles internally-tagged enums in flatten position.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Element {
    pub id: String,
    pub name: String,
    pub container: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub outputs: Vec<OutputSpec>,
    #[serde(default)]
    pub save_results: SaveSpec,
    #[serde(flatten)]
    pub kind: ElementKind,
}

impl Element {
    pub fn should_save_history(&self) -> bool {
        self.save_results.time_history.unwrap_or_else(|| {
            !matches!(self.kind, ElementKind::Constant { .. })
        })
    }
    pub fn should_save_final(&self) -> bool {
        self.save_results.final_value.unwrap_or_else(|| {
            !matches!(self.kind, ElementKind::Constant { .. })
        })
    }
    pub fn primary_unit(&self) -> &str {
        if let ElementKind::Constant { value, .. } = &self.kind {
            return value.unit.as_str();
        }
        if let ElementKind::Array { values_unit, unit, .. } = &self.kind {
            return values_unit.as_deref().or(unit.as_deref()).unwrap_or("1");
        }
        self.outputs.first().map(|o| o.unit.as_str()).unwrap_or("1")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementKind {
    Constant {
        value: Quantity,
        #[serde(default)]
        editable: bool,
        bounds: Option<Bounds>,
    },
    RandomVariable {
        distribution: Distribution,
        /// `None` = single draw per realization. `Some(ρ)` enables per-timestep AR(1)
        /// resampling in standard-normal driver space.
        #[serde(default)]
        autocorrelation: Option<f64>,
        /// Pairwise Spearman rank-correlations with other random_variable elements.
        /// Implemented via Gaussian copula (Cholesky factor applied per realization).
        #[serde(default)]
        correlations: Vec<CorrelationPair>,
    },
    Expression {
        expression: ExpressionField,
        #[serde(default)]
        inputs: Vec<String>,
    },
    Accumulator {
        initial_value: Quantity,
        initial_expression: Option<ExpressionField>,
        rate: ExpressionField,
        #[serde(default = "default_min_zero")]
        min_value: Option<f64>,
        capacity: Option<Quantity>,
        #[serde(default)]
        inputs: Vec<String>,
    },
    Timeseries {
        interpolation: InterpolationMethod,
        times_unit: Option<String>,
        values_unit: String,
        #[serde(default)]
        times: Vec<f64>,
        #[serde(default)]
        values: Vec<f64>,
        display_unit: Option<String>,
    },
    Lookup {
        x_unit: String,
        y_unit: String,
        #[serde(default)]
        x: Vec<f64>,
        #[serde(default)]
        y: Vec<f64>,
        /// Multi-column table: each inner Vec is one column, parallel to `x`.
        /// When present, `y` is ignored and column index is supplied via `lookup_call input2`.
        #[serde(default)]
        columns: Vec<Vec<f64>>,
        #[serde(default)]
        extrapolation: ExtrapolationMethod,
        display_unit: Option<String>,
    },
    StochasticProcess {
        process: ProcessSpec,
        #[serde(default)]
        lower_bound: Option<Quantity>,
    },
    Delay {
        input: String,
        lag: Quantity,
        initial: Option<Quantity>,
    },
    Script {
        language: String,
        source: String,
        #[serde(default)]
        expressions: Vec<ExpressionField>,
        #[serde(default)]
        variables: Vec<String>,
        #[serde(default)]
        procedural: bool,
        #[serde(default)]
        inputs: Vec<String>,
    },
    Array {
        /// Sub-discriminator (schema 0.2.0+). `None` for pre-0.2.0 models without it;
        /// the engine falls back to `expressions.is_empty()` in that case.
        #[serde(default)]
        mode: Option<ArrayMode>,
        /// Unit for expression-based arrays (legacy). Optional so the constant-values
        /// form (which uses `unit`) can deserialize without it.
        #[serde(default)]
        values_unit: Option<String>,
        /// Unit for constant-values arrays. Either `values_unit` or `unit` must be set.
        #[serde(default)]
        unit: Option<String>,
        /// Expression-based form: each element is computed from its expression each step.
        #[serde(default)]
        expressions: Vec<ExpressionField>,
        /// Constant form: fixed numeric values, emitted as-is.
        #[serde(default)]
        values: Vec<f64>,
        #[serde(default)]
        labels: Vec<String>,
        #[serde(default)]
        inputs: Vec<String>,
        #[serde(default)]
        display_unit: Option<String>,
        #[serde(default)]
        provenance: ArrayProvenance,
    },
}

fn default_min_zero() -> Option<f64> {
    Some(0.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InterpolationMethod {
    #[default]
    Linear,
    Step,
    Cubic,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ArrayProvenance {
    #[default]
    Extracted,
    ExtractionPending,
    Inferred,
}

/// Sub-discriminator for the overloaded `type: "array"` element (schema 0.2.0+).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ArrayMode {
    Constant,
    Expression,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExtrapolationMethod {
    #[default]
    Clamp,
    Linear,
    Error,
}

// ── Expression AST ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpressionField {
    pub ast: AstNode,
    pub display: Option<String>,
    #[serde(default)]
    pub source: ExpressionSource,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionSource {
    #[default]
    Explicit,
    Inferred,
    InferredPath,
    InferredPassthrough,
    InferredContainer,
    InferredTs,
    InferredPoolRate,
    /// Compiled-AST node graph decoded into structured AST (no formula text).
    InferredAst,
    /// Formula extraction failed in the transpiler; placeholder `Literal(0.0)` emitted
    /// with connection-derived inputs to preserve graph topology. Treat as known-incorrect.
    InferredStub,
}

/// AST node discriminated by the "op" field.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AstNode {
    Literal {
        value: f64,
        unit: Option<String>,
    },
    Ref {
        element_id: String,
        #[serde(default = "default_output_name")]
        output: String,
    },
    TimeRef {
        property: TimeProperty,
    },
    // Binary arithmetic
    Add {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Subtract {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Multiply {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Divide {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Power {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    // Comparison (return 1.0 true, 0.0 false)
    Lt {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Gt {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Lte {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Gte {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Eq {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Neq {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    And {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    Or {
        left: Box<AstNode>,
        right: Box<AstNode>,
    },
    // Unary
    Neg {
        operand: Box<AstNode>,
    },
    Not {
        operand: Box<AstNode>,
    },
    // Function call
    Call {
        #[serde(rename = "fn")]
        func: BuiltinFn,
        args: Vec<AstNode>,
    },
    // Conditional
    If {
        cond: Box<AstNode>,
        then: Box<AstNode>,
        #[serde(rename = "else")]
        else_: Box<AstNode>,
    },
    // Lookup table invocation
    LookupCall {
        element_id: String,
        input: Box<AstNode>,
        input2: Option<Box<AstNode>>,
    },
    // Monte-Carlo statistic of a submodel output, reduced across the submodel's
    // realizations (the `pdf_*` operations). See wasim-engine-semantics.md §2.13.
    SubmodelStat {
        submodel_id: String,
        output: String,
        statistic: SubmodelStatKind,
        #[serde(default)]
        arg: Option<Box<AstNode>>,
    },
    // Array construction: evaluates each element and produces a vector
    Array {
        elements: Vec<AstNode>,
    },
    // Comprehension over a dimension: evaluate `body` once per member of `over`.
    // See wasim-engine-semantics.md §15.
    VectorMap {
        over: String,
        body: Box<AstNode>,
    },
    // The implicit iteration index inside a `vector_map` body.
    IndexRef {
        #[serde(default)]
        axis: IndexAxis,
    },
    // Array/matrix element access: array[i] or matrix[i, j].
    Index {
        array: Box<AstNode>,
        indices: Vec<AstNode>,
    },
    // A source function the engine does not implement — preserved for round-tripping
    // and connectivity; evaluates to 0.0 (opaque).
    ExternCall {
        #[serde(rename = "fn")]
        func: String,
        args: Vec<AstNode>,
    },
}

/// The axis a `index_ref` refers to inside a `vector_map` body.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexAxis {
    #[default]
    Row,
    Col,
}

/// Which statistic a `submodel_stat` node reduces a submodel output to.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmodelStatKind {
    Mean,
    Percentile,
    Sd,
    CumulativeProb,
}

fn default_output_name() -> String {
    "value".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeProperty {
    Elapsed,
    Timestep,
    Year,
    Month,
    DayOfYear,
    DayOfMonth,
    DaysInMonth,
    // Calendar-of-day components (GoldSim Hour/Minute/Second). Calendar-aware: derived from the
    // absolute clock (needs a `simulation_settings.calendar_start` anchor; 0 without one).
    Hour,
    Minute,
    Second,
    /// The simulation's calendar start (`calendar_start`, seconds since the Unix epoch); 0 when
    /// no anchor is declared. GoldSim's `StartTime`.
    Start,
    /// Whole calendar months elapsed since the start (GoldSim `EMonth`) — NOT derivable from
    /// `elapsed` because month lengths vary; requires the real calendar (anchor).
    ElapsedMonths,
    /// Whole calendar years elapsed since the start (GoldSim `EYear`); requires the anchor.
    ElapsedYears,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinFn {
    Min,
    Max,
    Abs,
    Sqrt,
    Exp,
    Ln,
    Log,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Floor,
    Ceil,
    Round,
    Mod,
    Sign,
    Int,
    Step,
    Log2,
    // Hyperbolic
    Sinh,
    Cosh,
    Tanh,
    // Special functions
    /// The gamma function Γ(x). Serialized `"gamma"`. Used e.g. in Weibull scale derivation
    /// (`scale = mean / Γ(1 + 1/shape)`).
    Gamma,
    /// The error function erf(x) and its complement erfc(x).
    Erf,
    Erfc,
    // Date extraction (1 date arg = seconds since the sim epoch; §14). Extract a calendar field.
    GetYear,
    GetMonth,
    GetDay,
    GetHour,
    GetMinute,
    GetSecond,
    // Event predicate functions (§2). `occurs(event_id)` = 1.0 if the referenced event fired
    // this step (from the step's fired-event set); `changed(ref)` = 1.0 if the referenced
    // element's value differs from its previous-step value.
    Occurs,
    Changed,
    // Finance factors.
    /// Present-to-future value factor `(1 + rate)^n` — `pv_factor(rate, n)`.
    PvFactor,
    /// Annuity (present value of an ordinary annuity) factor `(1 − (1+rate)^-n) / rate`.
    AnnuityFactor,
    // Table/array introspection (need array-valued context; evaluated where resolvable).
    TableMin,
    TableMax,
    ColumnCount,
    // Array operations (evaluated against array-valued elements)
    SumArray,
    SizeArray,
    GetElement,
    InterpArray,
    MeanArray,
    MinArray,
    MaxArray,
    /// 1-based index of the minimum / maximum member of an array. Ties resolve to the
    /// LOWEST index (required for the bit-identity guarantee — dispatch stays deterministic).
    /// Serialized `"argmin_array"` / `"argmax_array"`. Used for wear-levelling dispatch
    /// ("assign the least-damaged truck"); combine with a penalty term for masked selection.
    ArgminArray,
    ArgmaxArray,
    DotProduct,
}

// ── Stochastic process ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessSpec {
    pub family: ProcessFamily,
    pub mean_type: ProcessMeanType,
    pub mean: Quantity,
    pub stddev: Quantity,
    /// Mean-reversion rate (per-time). None/zero = a non-reverting random walk (unchanged GBM).
    /// Non-zero makes the process mean-revert toward `reference_value` (§16). Scalar today; the
    /// schema allows quantity_or_formula but the engine resolves only the scalar form.
    #[serde(default)]
    pub reversion_rate: Option<QuantityOrFormula>,
    /// The long-run level reverted toward when `reversion_rate` is non-zero. None → the drift
    /// level (`mean`) is used as the target.
    #[serde(default)]
    pub reference_value: Option<QuantityOrFormula>,
    /// The process value at t=0. None → the reference/drift-implied level. Scalar today; the
    /// schema allows quantity_or_formula (an array-comprehension for correlated array processes)
    /// but the engine resolves only the scalar form.
    #[serde(default)]
    pub initial_value: Option<QuantityOrFormula>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessFamily {
    Gbm,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessMeanType {
    Geometric,
    Arithmetic,
    LogDrift,
}

// ── Correlation ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorrelationPair {
    pub partner: String,
    pub coefficient: f64,
}

// ── Distributions ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Distribution {
    #[serde(flatten)]
    pub kind: DistributionKind,
    pub truncation: Option<Truncation>,
    pub correlation_group: Option<String>,
    /// Importance-sampling override (§ importance_sampling): sample from `importance.bias`
    /// (the biased distribution g) instead of this declared distribution (the target f), and
    /// weight each realization by the likelihood ratio f(x)/g(x). Absent = ordinary sampling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<ImportanceSpec>,
}

/// Importance-sampling specification on a `sample` node's distribution. The declared distribution
/// is the target `f`; `bias` is the biased distribution `g` actually drawn from. The engine carries
/// the per-realization likelihood ratio w = pdf_f(x)/pdf_g(x) and weights every statistic by it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImportanceSpec {
    /// The biased distribution g to sample from. Boxed to break the Distribution→ImportanceSpec
    /// →Distribution type recursion.
    pub bias: Box<Distribution>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "family", content = "parameters", rename_all = "snake_case")]
pub enum DistributionKind {
    Uniform {
        min: QuantityOrFormula,
        max: QuantityOrFormula,
    },
    Normal {
        mean: QuantityOrFormula,
        stddev: QuantityOrFormula,
    },
    Lognormal {
        mean: QuantityOrFormula,
        stddev: QuantityOrFormula,
    },
    Triangular {
        min: QuantityOrFormula,
        mode: QuantityOrFormula,
        max: QuantityOrFormula,
    },
    /// Trapezoidal: a lower ramp (min→lower), a plateau (lower→upper), and an
    /// upper ramp (upper→max). Degenerates to triangular when lower == upper and
    /// to uniform when min == lower and upper == max.
    Trapezoidal {
        min: QuantityOrFormula,
        lower: QuantityOrFormula,
        upper: QuantityOrFormula,
        max: QuantityOrFormula,
    },
    LognormalMoments {
        mean: QuantityOrFormula,
        stddev: QuantityOrFormula,
    },
    Exponential {
        mean: QuantityOrFormula,
    },
    Gamma {
        shape: QuantityOrFormula,
        scale: QuantityOrFormula,
    },
    Beta {
        alpha: QuantityOrFormula,
        beta: QuantityOrFormula,
        /// 4-parameter beta: affine-scale the standard beta onto [min, max].
        #[serde(default)]
        min: Option<QuantityOrFormula>,
        #[serde(default)]
        max: Option<QuantityOrFormula>,
    },
    Weibull {
        shape: QuantityOrFormula,
        scale: QuantityOrFormula,
    },
    PearsonV {
        shape: QuantityOrFormula,
        scale: QuantityOrFormula,
    },
    PearsonIii {
        mean: QuantityOrFormula,
        stddev: QuantityOrFormula,
        skewness: QuantityOrFormula,
    },
    DiscreteUniform {
        min: i64,
        max: i64,
    },
    Bernoulli {
        prob: Quantity,
    },
    Discrete {
        outcomes: Vec<f64>,
        probabilities: Vec<f64>,
    },
    // ── v2 families ──
    Pert {
        min: QuantityOrFormula,
        mode: QuantityOrFormula,
        max: QuantityOrFormula,
    },
    Pareto {
        scale: QuantityOrFormula,
        shape: QuantityOrFormula,
        #[serde(default)]
        location: Option<QuantityOrFormula>,
    },
    ExtremeValue {
        location: QuantityOrFormula,
        scale: QuantityOrFormula,
    },
    StudentT {
        degrees_of_freedom: QuantityOrFormula,
        #[serde(default)]
        location: Option<QuantityOrFormula>,
        #[serde(default)]
        scale: Option<QuantityOrFormula>,
    },
    Cumulative {
        points: Vec<CumulativePoint>,
    },
    Sampled {
        samples: Vec<f64>,
        #[serde(default)]
        weights: Option<Vec<f64>>,
    },
    External {
        #[serde(default)]
        definition: Option<String>,
        /// Optional inline empirical fallback: if present, an `external` distribution samples
        /// this weighted empirical table instead of erroring (§6). Absent → load/sample error.
        #[serde(default)]
        fallback: Option<EmpiricalTable>,
    },

    // ── A4 roster additions (GoldSim parity, §6) ──
    /// Log-Uniform: ln(X) ~ Uniform(ln min, ln max). Params are real-space bounds (> 0).
    LogUniform {
        min: QuantityOrFormula,
        max: QuantityOrFormula,
    },
    /// Log-Triangular: ln(X) ~ Triangular(ln min, ln mode, ln max). Real-space params (> 0).
    LogTriangular {
        min: QuantityOrFormula,
        mode: QuantityOrFormula,
        max: QuantityOrFormula,
    },
    /// Log-Cumulative: a piecewise-linear CDF whose `x` breakpoints are interpolated in log
    /// space (the GoldSim log-cumulative). `x` values must be > 0.
    LogCumulative {
        points: Vec<CumulativePoint>,
    },
    /// Triangular specified by its 10th and 90th percentiles (+ mode), GoldSim's alternate
    /// parameterization. Reparameterized to (min, mode, max) at resolve time.
    Triangular1090 {
        p10: QuantityOrFormula,
        mode: QuantityOrFormula,
        p90: QuantityOrFormula,
    },
    /// Log-Triangular specified by its 10th/90th percentiles (+ mode), all real-space.
    /// Reparameterized in log space, then exponentiated.
    LogTriangular1090 {
        p10: QuantityOrFormula,
        mode: QuantityOrFormula,
        p90: QuantityOrFormula,
    },
    /// Binomial(n, p): number of successes in `n` independent Bernoulli(`p`) trials.
    Binomial {
        n: QuantityOrFormula,
        prob: QuantityOrFormula,
    },
    /// Negative Binomial(r, p): number of failures before the `r`-th success (each trial
    /// succeeds with probability `p`).
    NegativeBinomial {
        r: QuantityOrFormula,
        prob: QuantityOrFormula,
    },
    /// Poisson(λ): count with mean `lambda`. (Distinct from event `rate` Poisson generation.)
    Poisson {
        lambda: QuantityOrFormula,
    },
    /// Extreme Probability: the distribution of the min or max of `n` draws from a base
    /// distribution. Sampled via the order-statistic ICDF transform (u → u^(1/n) for max,
    /// 1−(1−u)^(1/n) for min) applied through the base's inverse CDF.
    ExtremeProbability {
        base: Box<DistributionKind>,
        n: QuantityOrFormula,
        /// "max" (default) or "min".
        #[serde(default)]
        extreme: ExtremeKind,
    },
    /// Beta specified by observed (successes, failures): Beta(successes + 1, failures + 1),
    /// optionally affine-scaled onto [min, max]. GoldSim's Beta(succ/fail) parameterization.
    BetaSuccessFailure {
        successes: QuantityOrFormula,
        failures: QuantityOrFormula,
        #[serde(default)]
        min: Option<QuantityOrFormula>,
        #[serde(default)]
        max: Option<QuantityOrFormula>,
    },
}

/// Which extreme an `extreme_probability` distribution takes.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExtremeKind {
    #[default]
    Max,
    Min,
}

/// Weighted empirical table — the inline `external` fallback (mirrors the `sampled` family).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmpiricalTable {
    pub samples: Vec<f64>,
    #[serde(default)]
    pub weights: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CumulativePoint {
    pub x: f64,
    pub cumulative_probability: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Truncation {
    pub min: Option<f64>,
    pub max: Option<f64>,
}
