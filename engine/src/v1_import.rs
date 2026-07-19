//! v1 → v2 normalizer (the regression bridge).
//!
//! Lifts the v1 fixed-type taxonomy (`crate::model::WasimModel`) into the v2
//! primitive model (`crate::model_v2::Model`) so the 162-example corpus keeps
//! running as regression coverage once the engine core operates only on v2.
//!
//! Mapping (see `engine/V2_SCOPING.md` §4):
//!   constant            → node/fixed
//!   random_variable     → node/sample
//!   expression          → node/expression
//!   stochastic_process  → node/process
//!   lookup              → node/lookup
//!   timeseries          → node/series
//!   delay               → node/lag  (multi-step → chained one-step lags, R3)
//!   accumulator         → stock
//!   array               → node/fixed (constant) or node/expression (array AST)
//!   script              → node/expression (expressions[0]) or fixed 0.0

use crate::model::{self as v1, ElementKind, WasimModel};
use crate::model_v2 as v2;

/// Normalize a parsed v1 model into the internal v2 model.
pub fn normalize(model: &WasimModel) -> v2::Model {
    let dt = model.simulation_settings.timestep.value;

    let mut elements: Vec<v2::Element> = Vec::with_capacity(model.elements.len());
    for elem in &model.elements {
        elements.extend(normalize_element(elem, dt));
    }

    v2::Model {
        wasim_version: model.wasim_version.clone(),
        source: model.source.clone(),
        simulation_settings: model.simulation_settings.clone(),
        reporting_periods: Vec::new(),
        dimensions: Vec::new(),
        optimization: None,
        containers: model.containers.clone(),
        elements,
        time_history_displays: model.time_history_displays.clone(),
        from_v1: true,
        dynamic_optimization: false,
    }
}

/// One v1 element → one or more v2 elements (delay chains expand to several).
fn normalize_element(elem: &v1::Element, dt: f64) -> Vec<v2::Element> {
    let inputs = kind_inputs(&elem.kind);
    let base = |id: String, name: String| v2::ElementBase {
        id,
        name,
        container: elem.container.clone(),
        description: elem.description.clone(),
        outputs: elem.outputs.clone(),
        save_results: elem.save_results.clone(),
        inputs: inputs.clone(),
        source_type: Some(kind_label(&elem.kind).to_string()),
    };

    // Delay is the only kind that can expand into multiple elements.
    if let ElementKind::Delay { input, lag, initial } = &elem.kind {
        return normalize_delay(elem, input, lag, initial.as_ref(), dt);
    }

    let primitive = match &elem.kind {
        ElementKind::Constant { value, editable, bounds } => {
            v2::Primitive::Node(v2::Node {
                rule: v2::NodeRule::Fixed {
                    value: v2::FixedValue::Scalar(value.clone()),
                    editable: *editable,
                    bounds: bounds.clone(),
                },
            })
        }

        ElementKind::RandomVariable { distribution, autocorrelation, correlations } => {
            v2::Primitive::Node(v2::Node {
                rule: v2::NodeRule::Sample {
                    distribution: distribution.clone(),
                    resampling: None,
                    autocorrelation: *autocorrelation,
                    correlations: correlations.clone(),
                },
            })
        }

        ElementKind::Expression { expression, .. } => {
            v2::Primitive::Node(v2::Node {
                rule: v2::NodeRule::Expression(expression.clone()),
            })
        }

        ElementKind::Accumulator {
            initial_value, initial_expression, rate, min_value, capacity, ..
        } => v2::Primitive::Stock(v2::Stock {
            initial_value: initial_value.clone(),
            initial_expression: initial_expression.clone(),
            rate: Some(v1::QuantityOrFormula::Expression(rate.clone())),
            inflows: Vec::new(),
            outflows: Vec::new(),
            floor: min_value.map(|m| quantity(m, &initial_value.unit)),
            capacity: capacity.clone().map(v1::QuantityOrFormula::Quantity),
            overflow_target: None,
            return_rate: None,
            withdrawals: Vec::new(),
        }),

        ElementKind::Timeseries { interpolation, times_unit, times, values, .. } => {
            v2::Primitive::Node(v2::Node {
                rule: v2::NodeRule::Series {
                    timestamps: times.clone(),
                    values: values.clone(),
                    time_unit: times_unit.clone(),
                    interpolation: interpolation.clone(),
                },
            })
        }

        ElementKind::Lookup { x_unit, y_unit, x, y, columns, extrapolation, .. } => {
            v2::Primitive::Node(v2::Node {
                rule: v2::NodeRule::Lookup(v2::LookupTable {
                    x: x.clone(),
                    y: y.clone(),
                    // v1 `columns` (one column per inner vec, parallel to x) carried into
                    // `z`; the v2 lookup eval replicates v1 column-indexing when y is empty.
                    z: columns.clone(),
                    x_unit: Some(x_unit.clone()),
                    y_unit: Some(y_unit.clone()),
                    z_unit: None,
                    interpolation: v1::InterpolationMethod::Linear,
                    log_result: false,
                    extrapolation: extrapolation.clone(),
                }),
            })
        }

        ElementKind::StochasticProcess { process, lower_bound } => {
            v2::Primitive::Node(v2::Node {
                rule: v2::NodeRule::Process {
                    process: process.clone(),
                    lower_bound: lower_bound.clone(),
                },
            })
        }

        ElementKind::Script { expressions, .. } => {
            let rule = match expressions.first() {
                Some(ef) => v2::NodeRule::Expression(ef.clone()),
                None => v2::NodeRule::Fixed {
                    value: v2::FixedValue::Scalar(quantity(0.0, "1")),
                    editable: false,
                    bounds: None,
                },
            };
            v2::Primitive::Node(v2::Node { rule })
        }

        ElementKind::Array { mode, values_unit, unit, expressions, values, .. } => {
            let is_expression = match mode {
                Some(v1::ArrayMode::Expression) => true,
                Some(v1::ArrayMode::Constant) => false,
                None => !expressions.is_empty(),
            };
            if is_expression {
                // Wrap the per-element ASTs in a single `array` AST → vector output.
                let ast = v1::AstNode::Array {
                    elements: expressions.iter().map(|ef| ef.ast.clone()).collect(),
                };
                v2::Primitive::Node(v2::Node {
                    rule: v2::NodeRule::Expression(v1::ExpressionField {
                        ast,
                        display: None,
                        source: v1::ExpressionSource::Inferred,
                    }),
                })
            } else {
                let unit = unit.clone().or_else(|| values_unit.clone()).unwrap_or_else(|| "1".to_string());
                v2::Primitive::Node(v2::Node {
                    rule: v2::NodeRule::Fixed {
                        value: v2::FixedValue::Array { values: values.clone(), unit },
                        editable: false,
                        bounds: None,
                    },
                })
            }
        }

        // Delay handled above.
        ElementKind::Delay { .. } => unreachable!("delay handled before match"),
    };

    vec![v2::Element {
        base: base(elem.id.clone(), elem.name.clone()),
        primitive,
    }]
}

/// Expand a v1 `delay` (which carries a multi-timestep `lag` quantity) into a chain of
/// strictly one-step v2 `lag` nodes — semantics §2.7 (R3). `k = round(lag/dt)` chained
/// lags give an exact k-step delay. The final node keeps the original id so downstream
/// references still resolve; intermediates get synthetic ids.
fn normalize_delay(
    elem: &v1::Element,
    input: &str,
    lag: &v1::Quantity,
    initial: Option<&v1::Quantity>,
    dt: f64,
) -> Vec<v2::Element> {
    let k = if dt.is_finite() && dt > 0.0 {
        (lag.value / dt).round() as i64
    } else {
        1
    };
    let k = k.max(1) as usize;

    let make = |id: String, name: String, input: String, save: bool| {
        let mut outputs = Vec::new();
        let mut save_results = v1::SaveSpec::default();
        if save {
            outputs = elem.outputs.clone();
            save_results = elem.save_results.clone();
        }
        v2::Element {
            base: v2::ElementBase {
                id,
                name,
                container: elem.container.clone(),
                description: elem.description.clone(),
                outputs,
                save_results,
                inputs: vec![input.clone()],
                source_type: Some("delay".to_string()),
            },
            primitive: v2::Primitive::Node(v2::Node {
                rule: v2::NodeRule::Lag {
                    input: Some(input),
                    initial: initial.cloned(),
                },
            }),
        }
    };

    if k <= 1 {
        return vec![make(elem.id.clone(), elem.name.clone(), input.to_string(), true)];
    }

    let mut out = Vec::with_capacity(k);
    let mut prev = input.to_string();
    for i in 1..k {
        let id = format!("{}__lag{}", elem.id, i);
        let name = format!("{} (lag {}/{})", elem.name, i, k);
        out.push(make(id.clone(), name, prev, false));
        prev = id;
    }
    // Final node keeps the original id + saved outputs.
    out.push(make(elem.id.clone(), elem.name.clone(), prev, true));
    out
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn quantity(value: f64, unit: &str) -> v1::Quantity {
    v1::Quantity { value, unit: unit.to_string(), display_unit: None }
}

/// Inputs declared inside the v1 kind (they live on the variant, not the common base).
fn kind_inputs(kind: &ElementKind) -> Vec<String> {
    match kind {
        ElementKind::Expression { inputs, .. }
        | ElementKind::Accumulator { inputs, .. }
        | ElementKind::Script { inputs, .. }
        | ElementKind::Array { inputs, .. } => inputs.clone(),
        ElementKind::Delay { input, .. } => vec![input.clone()],
        _ => Vec::new(),
    }
}

fn kind_label(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::Constant { .. } => "constant",
        ElementKind::RandomVariable { .. } => "random_variable",
        ElementKind::Expression { .. } => "expression",
        ElementKind::Accumulator { .. } => "accumulator",
        ElementKind::Timeseries { .. } => "timeseries",
        ElementKind::Lookup { .. } => "lookup",
        ElementKind::StochasticProcess { .. } => "stochastic_process",
        ElementKind::Delay { .. } => "delay",
        ElementKind::Script { .. } => "script",
        ElementKind::Array { .. } => "array",
    }
}
