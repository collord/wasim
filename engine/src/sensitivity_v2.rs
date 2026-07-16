//! Runtime sensitivity analysis (§ SENSITIVITY_ANALYSIS_SPEC.md).
//!
//! Vary one or more editable fixed-value inputs across a range and observe how a chosen
//! target element responds. This is a **runtime action** — the spec is supplied live by
//! the UI, never persisted in the model — and it reuses the optimization candidate-eval
//! harness (`eval_harness`) verbatim: each sweep point is evaluated exactly like an
//! optimization candidate, minus the maximize/minimize flip.
//!
//! Methods:
//! - **One-at-a-time**: vary each variable across `steps` points holding others at `base`;
//!   yields one response curve `(input → result)` per variable.
//! - **Tornado**: evaluate the target at each variable's `lower`/`upper` (others at base);
//!   the bar is the swing `|result(hi) − result(lo)|`, sorted descending by influence.
//!
//! Unlike the optimizer, a failed sweep point **propagates its error** rather than being
//! coerced to `+∞` — a silent ∞ would poison a curve or misrank a tornado.

use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::eval_harness::evaluate_point;
use crate::model::ObjectiveStatistic;
use crate::model_v2::Model;
use crate::RunConfig;

// ── Spec (runtime-supplied) ─────────────────────────────────────────────────────

/// The target output to observe: an element, optionally reduced by a Monte-Carlo statistic
/// (mean/percentile/peak/valley/sum). Mirrors `Objective`'s shape sans `direction`.
#[derive(Debug, Clone, Deserialize)]
pub struct ResultRef {
    pub element_id: String,
    #[serde(default)]
    pub statistic: Option<ObjectiveStatistic>,
}

/// One swept input: an editable fixed-scalar element, varied across `[lower, upper]` in
/// `steps` points, with the others pinned at `base`.
#[derive(Debug, Clone, Deserialize)]
pub struct SweepVar {
    pub element_id: String,
    pub lower: f64,
    pub upper: f64,
    pub base: f64,
    /// Number of sweep points for one-at-a-time (≥ 2). Ignored by tornado (always 2 pts).
    #[serde(default = "default_steps")]
    pub steps: usize,
}

fn default_steps() -> usize {
    5
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityMethod {
    OneAtATime,
    Tornado,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SensitivitySpec {
    pub result: ResultRef,
    pub variables: Vec<SweepVar>,
    pub method: SensitivityMethod,
}

// ── Results ─────────────────────────────────────────────────────────────────────

/// One sweep point of a one-at-a-time curve.
#[derive(Debug, Clone, Serialize)]
pub struct CurvePoint {
    pub input: f64,
    pub result: f64,
}

/// A one-at-a-time response curve for a single variable.
#[derive(Debug, Clone, Serialize)]
pub struct VarCurve {
    pub element_id: String,
    pub points: Vec<CurvePoint>,
}

/// A tornado bar: the target's value at the variable's low/high, and the absolute swing.
#[derive(Debug, Clone, Serialize)]
pub struct TornadoBar {
    pub element_id: String,
    pub low: f64,
    pub high: f64,
    pub swing: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SensitivityResults {
    /// The target evaluated with every variable at its `base` (the reference/center point).
    pub base_result: f64,
    /// One response curve per variable (one-at-a-time). Empty for tornado.
    pub curves: Vec<VarCurve>,
    /// One bar per variable, sorted by descending swing (tornado). Empty for one-at-a-time.
    pub tornado: Vec<TornadoBar>,
}

// ── Entry point ─────────────────────────────────────────────────────────────────

/// Run a sensitivity sweep. Deterministic given the seed (submodel nested Monte-Carlo
/// re-seeds per call, exactly as in an optimization candidate).
pub fn sensitivity(
    model: &Model,
    spec: &SensitivitySpec,
    config: &RunConfig,
) -> Result<SensitivityResults, EngineError> {
    if spec.variables.is_empty() {
        return Err(EngineError::InvalidModel(
            "sensitivity analysis has no variables".into(),
        ));
    }

    let var_ids: Vec<String> = spec.variables.iter().map(|v| v.element_id.clone()).collect();
    let base_point: Vec<f64> = spec.variables.iter().map(|v| v.base).collect();
    let stat = spec.result.statistic.as_ref();
    let target = &spec.result.element_id;

    // Evaluate the target with all variables at base — the shared reference point.
    let base_result = evaluate_point(model, &var_ids, &base_point, target, stat, config)?;

    // Evaluate the target with variable `i` set to `value`, all others at their base.
    let eval_one = |i: usize, value: f64| -> Result<f64, EngineError> {
        let mut point = base_point.clone();
        point[i] = value;
        evaluate_point(model, &var_ids, &point, target, stat, config)
    };

    match spec.method {
        SensitivityMethod::OneAtATime => {
            let mut curves = Vec::with_capacity(spec.variables.len());
            for (i, v) in spec.variables.iter().enumerate() {
                let steps = v.steps.max(2);
                let mut points = Vec::with_capacity(steps);
                for s in 0..steps {
                    // Linear from lower→upper inclusive; a single value if the range is degenerate.
                    let t = s as f64 / (steps - 1) as f64;
                    let input = v.lower + t * (v.upper - v.lower);
                    let result = eval_one(i, input)?;
                    points.push(CurvePoint { input, result });
                }
                curves.push(VarCurve { element_id: v.element_id.clone(), points });
            }
            Ok(SensitivityResults { base_result, curves, tornado: Vec::new() })
        }

        SensitivityMethod::Tornado => {
            let mut tornado = Vec::with_capacity(spec.variables.len());
            for (i, v) in spec.variables.iter().enumerate() {
                let low = eval_one(i, v.lower)?;
                let high = eval_one(i, v.upper)?;
                tornado.push(TornadoBar {
                    element_id: v.element_id.clone(),
                    low,
                    high,
                    swing: (high - low).abs(),
                });
            }
            // Rank by influence: largest swing first. total_cmp keeps NaN swings from panicking.
            tornado.sort_by(|a, b| b.swing.total_cmp(&a.swing));
            Ok(SensitivityResults { base_result, curves: Vec::new(), tornado })
        }
    }
}
