//! Shared candidate-evaluation harness used by both the optimization study
//! (`optimize_v2`) and the sensitivity sweep (`sensitivity_v2`).
//!
//! A "point" is an assignment of scalar values to a set of editable fixed-value input
//! elements. Evaluating a point = clone the model, set the inputs, build the v2 graph,
//! run the v2 engine, read the target element's per-realization final values, and reduce
//! them to a scalar by an (optional) Monte-Carlo statistic. SubModel nested Monte-Carlo
//! runs happen automatically inside `engine_v2::run`, so a probabilistic target re-runs
//! its nested ensemble per point for free.
//!
//! The two callers differ only in error policy: the optimizer coerces a failed candidate
//! to `+∞` so the search rejects it (`evaluate_point(...).unwrap_or(f64::INFINITY)`),
//! while the sweep propagates the error so a failed point surfaces rather than silently
//! poisoning a curve.

use crate::error::EngineError;
use crate::model::ObjectiveStatistic;
use crate::model_v2::{FixedValue, Model, NodeRule, Primitive};
use crate::{engine, ModelGraphV2, RunConfig};

/// Set an input variable (an editable fixed-value **scalar** node) to `value`. Errors if the
/// element is missing or is not a fixed scalar. Sample nodes and non-scalar fixed values are
/// intentionally rejected — only fixed scalars can be swept/optimized by a single number.
pub(crate) fn set_variable(model: &mut Model, id: &str, value: f64) -> Result<(), EngineError> {
    for elem in &mut model.elements {
        if elem.base.id != id {
            continue;
        }
        if let Primitive::Node(n) = &mut elem.primitive {
            if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &mut n.rule {
                q.value = value;
                return Ok(());
            }
        }
        return Err(EngineError::InvalidModel(format!(
            "variable '{id}' is not an editable fixed value"
        )));
    }
    Err(EngineError::ElementNotFound(id.to_string()))
}

/// Reduce a target element's per-realization final values to a single scalar per the
/// statistic (None = deterministic → the single/first value).
pub(crate) fn reduce(samples: &[f64], stat: Option<&ObjectiveStatistic>) -> f64 {
    use crate::model::ObjectiveStatKind::*;
    match stat {
        None => samples.first().copied().unwrap_or(0.0),
        Some(s) => match s.kind {
            Mean => engine::mean(samples),
            Percentile => engine::percentile(samples, s.p.unwrap_or(50.0)),
            Peak => samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            Valley => samples.iter().cloned().fold(f64::INFINITY, f64::min),
            Sum => samples.iter().sum(),
        },
    }
}

/// Evaluate a point: clone `base`, assign each `(var_id, value)`, build + run, then reduce
/// the target element by `stat`. Propagates any model/graph/run error and errors if the
/// target element is missing or produced no final values.
///
/// This is the natural-sense value (no maximize/minimize flip — that is the optimizer's
/// concern, applied by its caller).
pub(crate) fn evaluate_point(
    base: &Model,
    var_ids: &[String],
    point: &[f64],
    target_id: &str,
    stat: Option<&ObjectiveStatistic>,
    config: &RunConfig,
) -> Result<f64, EngineError> {
    let mut m = base.clone();
    for (id, &v) in var_ids.iter().zip(point) {
        set_variable(&mut m, id, v)?;
    }
    let graph = ModelGraphV2::build(&m)?;
    let results = crate::engine_v2::run(&m, &graph, config)?;
    let samples = match results.elements.get(target_id) {
        Some(er) if !er.final_values.is_empty() => &er.final_values,
        _ => {
            return Err(EngineError::InvalidModel(format!(
                "target element '{target_id}' produced no output"
            )))
        }
    };
    Ok(reduce(samples, stat))
}
