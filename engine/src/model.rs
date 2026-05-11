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

/// A distribution parameter that is either a fixed Quantity or a formula string
/// referencing another element (e.g. `"Mean_Ore / 5"`).
/// Formula strings are stored but currently evaluated as 0.0 at runtime.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum QuantityOrFormula {
    Quantity(Quantity),
    Formula(String),
}

impl QuantityOrFormula {
    pub fn value(&self) -> f64 {
        match self {
            QuantityOrFormula::Quantity(q) => q.value,
            QuantityOrFormula::Formula(_) => 0.0,
        }
    }
    pub fn unit(&self) -> &str {
        match self {
            QuantityOrFormula::Quantity(q) => q.unit.as_str(),
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputSpec {
    pub name: String,
    pub unit: String,
    pub display_unit: Option<String>,
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
        if let ElementKind::Array { values_unit, .. } = &self.kind {
            return values_unit.as_str();
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
        inputs: Vec<String>,
    },
    Array {
        values_unit: String,
        expressions: Vec<ExpressionField>,
        #[serde(default)]
        labels: Vec<String>,
        #[serde(default)]
        inputs: Vec<String>,
    },
}

fn default_min_zero() -> Option<f64> {
    Some(0.0)
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InterpolationMethod {
    #[default]
    Linear,
    Step,
    Cubic,
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
    // Array construction: evaluates each element and produces a vector
    Array {
        elements: Vec<AstNode>,
    },
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
    // Trig extension
    Tanh,
    // Array operations (evaluated against array-valued elements)
    SumArray,
    SizeArray,
    GetElement,
    InterpArray,
    MeanArray,
    MinArray,
    DotProduct,
}

// ── Stochastic process ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessSpec {
    pub family: ProcessFamily,
    pub mean_type: ProcessMeanType,
    pub mean: Quantity,
    pub stddev: Quantity,
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "family", content = "parameters", rename_all = "snake_case")]
pub enum DistributionKind {
    Uniform {
        min: Quantity,
        max: Quantity,
    },
    Normal {
        mean: Quantity,
        stddev: Quantity,
    },
    Lognormal {
        mean: QuantityOrFormula,
        stddev: QuantityOrFormula,
    },
    Triangular {
        min: Quantity,
        mode: Quantity,
        max: Quantity,
    },
    LognormalMoments {
        mean: QuantityOrFormula,
        stddev: QuantityOrFormula,
    },
    Exponential {
        mean: Quantity,
    },
    Gamma {
        shape: Quantity,
        scale: Quantity,
    },
    Beta {
        alpha: Quantity,
        beta: Quantity,
    },
    Weibull {
        shape: Quantity,
        scale: Quantity,
    },
    PearsonV {
        shape: Quantity,
        scale: Quantity,
    },
    PearsonIii {
        mean: Quantity,
        stddev: Quantity,
        skewness: Quantity,
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Truncation {
    pub min: Option<f64>,
    pub max: Option<f64>,
}
