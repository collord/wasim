//! Browser-facing WASM API — the bridge onto the **v2 engine core**.
//!
//! `WasmEngine` accepts either v1 or v2-native model JSON (v1 is normalized into the v2
//! primitive model), holds a single v2 `Model` + `ModelGraphV2`, and runs via `run_v2`.
//!
//! The summary is enriched (`primitive`, `value_rule`, active `traits`, `inputs`, `value`)
//! while staying backward-compatible: the legacy `type` field is still emitted (the original
//! v1 type for imported models, else a mapping from the primitive/value_rule), so the current
//! v1 frontend keeps working until it migrates to the richer fields.
//!
//! Build: `wasm-pack build --target web --out-dir pkg -- --features wasm`

use serde::Deserialize;
use wasm_bindgen::prelude::*;

use crate::engine::RunConfig;
use crate::engine_v2::run as run_v2;
use crate::graph_v2::ModelGraphV2;
use crate::model::{DistributionKind, WasimModel};
use crate::model_v2::{self as v2, FixedValue, NodeRule, Primitive};

// ── JS-facing config ──────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct JsRunConfig {
    n_realizations: Option<u32>,
    seed: Option<u64>,
    duration_override: Option<f64>,
    timestep_override: Option<f64>,
    /// Optional A3 analysis config (custom percentiles, PDF/CDF/CCDF, capture times, final
    /// stats). Absent → the default fixed summary.
    #[serde(default)]
    results_spec: Option<crate::results_spec::ResultsSpec>,
    /// Timebase mode (B1): "fixed" (default) or "event_accurate".
    #[serde(default)]
    timebase: Option<String>,
    /// Units mode (B5): "warn" (default) or "strict".
    #[serde(default)]
    units: Option<String>,
    /// Per-realization importance weights (B7). Empty = unweighted.
    #[serde(default)]
    realization_weights: Vec<f64>,
}

// ── WasmEngine ────────────────────────────────────────────────────────────────

/// Loaded and validated simulation model (internally v2). Constructed from v1 or v2 JSON.
#[wasm_bindgen]
pub struct WasmEngine {
    model: v2::Model,
    graph: ModelGraphV2,
}

#[wasm_bindgen]
impl WasmEngine {
    /// Parse `model_json` (v2-native if its first element has a `primitive` field, else v1 →
    /// normalized) and build the dependency graph. Throws on parse/graph errors.
    #[wasm_bindgen(constructor)]
    pub fn new(model_json: &str) -> Result<WasmEngine, JsError> {
        let model = load_v2(model_json).map_err(|e| JsError::new(&e))?;
        let graph = ModelGraphV2::build(&model)
            .map_err(|e| JsError::new(&format!("model graph error: {e}")))?;
        Ok(WasmEngine { model, graph })
    }

    /// Return the current (v2) model JSON, including any in-browser parameter edits.
    pub fn model_json(&self) -> String {
        serde_json::to_string(&self.model).unwrap_or_default()
    }

    /// Lightweight model summary for the graph/dashboard views. Each element carries the
    /// legacy `type` plus the v2 `primitive`/`value_rule`/`traits`/`inputs`/`value`.
    pub fn model_summary(&self) -> String {
        crate::summary::summary_json(&self.model)
    }

    /// Run the simulation through the v2 core and return results as a JSON string.
    pub fn run_json(&self, config_json: &str) -> Result<String, JsError> {
        let js: JsRunConfig = serde_json::from_str(config_json).unwrap_or_default();
        let config = RunConfig {
            n_realizations: js.n_realizations,
            seed: js.seed,
            duration_override: js.duration_override,
            timestep_override: js.timestep_override,
            results_spec: js.results_spec,
            timebase: match js.timebase.as_deref() {
                Some("event_accurate") => crate::engine::TimebaseMode::EventAccurate,
                _ => crate::engine::TimebaseMode::Fixed,
            },
            units: match js.units.as_deref() {
                Some("strict") => crate::engine::UnitsMode::Strict,
                _ => crate::engine::UnitsMode::Warn,
            },
            realization_weights: js.realization_weights,
        };
        let mut results = run_v2(&self.model, &self.graph, &config)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // Convert results into display units (`display = value·factor + offset`) so the UI
        // shows friendly units. The engine core stays canonical; this is the display boundary.
        let disp: std::collections::HashMap<&str, (String, f64, f64)> = self
            .model
            .elements
            .iter()
            .filter_map(|e| crate::summary::display_of(e).map(|(du, f, o)| (e.id(), (du.to_string(), f, o))))
            .collect();
        for (id, r) in results.elements.iter_mut() {
            if let Some((du, f, o)) = disp.get(id.as_str()) {
                let (f, o) = (*f, *o);
                r.unit = du.clone();
                r.final_values.iter_mut().for_each(|v| *v = *v * f + o);
                if let Some(h) = &mut r.time_history {
                    for arr in [&mut h.mean, &mut h.p05, &mut h.p25, &mut h.p50, &mut h.p75, &mut h.p95] {
                        arr.iter_mut().for_each(|v| *v = *v * f + o);
                    }
                }
            }
        }

        // Convert the time axis to the timestep's display unit, if declared (e.g. an axis
        // in canonical `s` shown as `yr`). Same display boundary as element values above.
        let ts = &self.model.simulation_settings.timestep;
        if let Some(du) = ts.display_unit.as_deref() {
            if let Some((f, o)) = crate::units::display_conversion(&ts.unit, du) {
                results.time_axis.iter_mut().for_each(|t| *t = *t * f + o);
                results.time_unit = du.to_string();
            }
        }
        serde_json::to_string(&results).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Run a runtime sensitivity sweep. `spec_json` is a `SensitivitySpec` (UI-supplied,
    /// never persisted in the model). Returns serialized `SensitivityResults`.
    ///
    /// Result values are converted into the target element's display unit (same display
    /// boundary as `run_json`) so the UI charts friendly units; input values stay in the
    /// element's canonical unit (the UI supplies them canonically, as it does for edits).
    pub fn sensitivity_json(&self, spec_json: &str) -> Result<String, JsError> {
        let spec: crate::sensitivity_v2::SensitivitySpec =
            serde_json::from_str(spec_json).map_err(|e| JsError::new(&format!("bad spec: {e}")))?;
        let config = RunConfig::default();
        let mut results = crate::sensitivity_v2::sensitivity(&self.model, &spec, &config)
            .map_err(|e| JsError::new(&e.to_string()))?;

        // Convert the target's result values into its display unit (`display = value·f + o`).
        if let Some(elem) = self.model.elements.iter().find(|e| e.id() == spec.result.element_id) {
            if let Some((_, f, o)) = crate::summary::display_of(elem) {
                let conv = |v: &mut f64| *v = *v * f + o;
                conv(&mut results.base_result);
                for c in &mut results.curves {
                    c.points.iter_mut().for_each(|p| conv(&mut p.result));
                }
                for b in &mut results.tornado {
                    conv(&mut b.low);
                    conv(&mut b.high);
                    // Swing is a difference: the offset cancels, only the factor scales it.
                    b.swing *= f.abs();
                }
            }
        }
        serde_json::to_string(&results).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Update an editable fixed value (the v2 analog of a v1 `constant`).
    pub fn set_constant(&mut self, id: &str, value: f64) -> Result<(), JsError> {
        for elem in &mut self.model.elements {
            if elem.base.id != id {
                continue;
            }
            if let Primitive::Node(n) = &mut elem.primitive {
                if let NodeRule::Fixed { value: FixedValue::Scalar(q), .. } = &mut n.rule {
                    q.value = value;
                    return Ok(());
                }
            }
            return Err(JsError::new(&format!("'{id}' is not an editable fixed value")));
        }
        Err(JsError::new(&format!("element '{id}' not found")))
    }

    /// Update a distribution parameter of a `sample` node (the v2 analog of `random_variable`).
    pub fn set_rv_param(&mut self, id: &str, param_name: &str, value: f64) -> Result<(), JsError> {
        for elem in &mut self.model.elements {
            if elem.base.id != id {
                continue;
            }
            if let Primitive::Node(n) = &mut elem.primitive {
                if let NodeRule::Sample { distribution, .. } = &mut n.rule {
                    return set_dist_param(&mut distribution.kind, param_name, value)
                        .map_err(|msg| JsError::new(&msg));
                }
            }
            return Err(JsError::new(&format!("'{id}' is not a sample node")));
        }
        Err(JsError::new(&format!("element '{id}' not found")))
    }

    /// Topological evaluation order as a JSON array of element IDs.
    pub fn topo_order_json(&self) -> String {
        serde_json::to_string(&self.graph.topo_order).unwrap_or_default()
    }
}

// ── Standalone validation (authoring reconcile loop) ────────────────────────────

/// Validate a candidate model without constructing a persistent engine. Returns a JSON
/// `{ ok, errors, warnings, topo }` document — never throws — so the authoring UI can
/// surface structured diagnostics on every edit (spec §8, §13.2). `errors` are hard
/// parse/graph failures (dangling refs, illegal cycles); `warnings` are dimensional /
/// unit smells from [`crate::units::validate`]. `topo` is the evaluation order when the
/// graph builds (the causality-sequence view), else empty.
#[wasm_bindgen]
pub fn validate_json(model_json: &str) -> String {
    #[derive(serde::Serialize)]
    struct Diag {
        ok: bool,
        errors: Vec<String>,
        warnings: Vec<String>,
        topo: Vec<String>,
    }
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut topo = Vec::new();

    match load_v2(model_json) {
        Ok(model) => {
            warnings = crate::units::validate(&model);
            match ModelGraphV2::build(&model) {
                Ok(graph) => topo = graph.topo_order.clone(),
                Err(e) => errors.push(format!("graph error: {e}")),
            }
        }
        Err(e) => errors.push(e),
    }
    serde_json::to_string(&Diag { ok: errors.is_empty(), errors, warnings, topo })
        .unwrap_or_else(|_| "{\"ok\":false,\"errors\":[\"validation serialization failed\"],\"warnings\":[],\"topo\":[]}".to_string())
}

// ── Loading ───────────────────────────────────────────────────────────────────

fn load_v2(json: &str) -> Result<v2::Model, String> {
    let is_v2 = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| {
            v.get("elements")
                .and_then(|e| e.as_array())
                .and_then(|a| a.first())
                .map(|f| f.get("primitive").is_some())
        })
        .unwrap_or(false);
    if is_v2 {
        crate::v2_parse::parse(json).map_err(|e| format!("v2 model parse error: {e}"))
    } else {
        let m: WasimModel =
            serde_json::from_str(json).map_err(|e| format!("v1 model parse error: {e}"))?;
        Ok(crate::v1_import::normalize(&m))
    }
}

// ── Distribution parameter mutation ──────────────────────────────────────────

fn set_dist_param(kind: &mut DistributionKind, param: &str, value: f64) -> Result<(), String> {
    use crate::model::{Quantity, QuantityOrFormula};
    let qof = |v: f64, unit: String| QuantityOrFormula::Quantity(Quantity { value: v, unit, display_unit: None });
    match kind {
        DistributionKind::Normal { mean, stddev }
        | DistributionKind::Lognormal { mean, stddev }
        | DistributionKind::LognormalMoments { mean, stddev } => match param {
            "mean" => *mean = qof(value, mean.unit().to_string()),
            "stddev" => *stddev = qof(value, stddev.unit().to_string()),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Uniform { min, max } => match param {
            "min" => *min = qof(value, min.unit().to_string()),
            "max" => *max = qof(value, max.unit().to_string()),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Triangular { min, mode, max }
        | DistributionKind::Pert { min, mode, max } => match param {
            "min" => *min = qof(value, min.unit().to_string()),
            "mode" => *mode = qof(value, mode.unit().to_string()),
            "max" => *max = qof(value, max.unit().to_string()),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Exponential { mean } => match param {
            "mean" => *mean = qof(value, mean.unit().to_string()),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Gamma { shape, scale }
        | DistributionKind::Weibull { shape, scale }
        | DistributionKind::PearsonV { shape, scale } => match param {
            "shape" => *shape = qof(value, shape.unit().to_string()),
            "scale" => *scale = qof(value, scale.unit().to_string()),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Beta { alpha, beta, .. } => match param {
            "alpha" => *alpha = qof(value, alpha.unit().to_string()),
            "beta" => *beta = qof(value, beta.unit().to_string()),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::PearsonIii { mean, stddev, skewness } => match param {
            "mean" => *mean = qof(value, mean.unit().to_string()),
            "stddev" => *stddev = qof(value, stddev.unit().to_string()),
            "skewness" => *skewness = qof(value, skewness.unit().to_string()),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::DiscreteUniform { min, max } => match param {
            "min" => *min = value as i64,
            "max" => *max = value as i64,
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Bernoulli { prob } => match param {
            "prob" => prob.value = value,
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        _ => return Err("parameter editing not supported for this distribution".to_string()),
    }
    Ok(())
}
