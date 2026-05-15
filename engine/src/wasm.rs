//! Browser-facing WASM API.
//!
//! Build with:
//!   wasm-pack build --target web -- --features wasm
//!
//! JS usage:
//!   import init, { WasmEngine } from './pkg/wasim_engine.js';
//!   await init();
//!   const engine = new WasmEngine(modelJsonString);
//!   const results = JSON.parse(engine.run_json('{"n_realizations":1000,"seed":42}'));

use serde::Deserialize;
use wasm_bindgen::prelude::*;

use crate::engine::{run, RunConfig};
use crate::graph::ModelGraph;
use crate::model::{DistributionKind, ElementKind, WasimModel};

// ── JS-facing config ──────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct JsRunConfig {
    n_realizations: Option<u32>,
    seed: Option<u64>,
    duration_override: Option<f64>,
    timestep_override: Option<f64>,
}

// ── WasmEngine ────────────────────────────────────────────────────────────────

/// Loaded and validated simulation model. Constructed from a model.json string.
#[wasm_bindgen]
pub struct WasmEngine {
    model: WasimModel,
    graph: ModelGraph,
}

#[wasm_bindgen]
impl WasmEngine {
    /// Parse and validate `model_json`. Throws a `JsError` on parse or graph errors.
    #[wasm_bindgen(constructor)]
    pub fn new(model_json: &str) -> Result<WasmEngine, JsError> {
        let model: WasimModel = serde_json::from_str(model_json)
            .map_err(|e| JsError::new(&format!("model parse error: {e}")))?;
        let graph = ModelGraph::build(&model)
            .map_err(|e| JsError::new(&format!("model graph error: {e}")))?;
        Ok(WasmEngine { model, graph })
    }

    /// Return the current model.json (including any in-browser parameter edits).
    pub fn model_json(&self) -> String {
        serde_json::to_string(&self.model).unwrap_or_default()
    }

    /// Return a lightweight model summary for the frontend graph and dashboard views.
    ///
    /// JSON shape:
    /// ```json
    /// {
    ///   "element_count": 15,
    ///   "elements": [{ "id", "name", "type", "container", "editable" }, ...],
    ///   "containers": [...],
    ///   "simulation_settings": { ... }
    /// }
    /// ```
    pub fn model_summary(&self) -> String {
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
            kind: &'static str,
            container: Option<&'a str>,
            editable: bool,
            unit: &'a str,
            description: Option<&'a str>,
        }

        let elements: Vec<ElemSummary> = self
            .model
            .elements
            .iter()
            .map(|e| {
                let (kind_str, editable) = match &e.kind {
                    ElementKind::Constant { editable, .. } => ("constant", *editable),
                    ElementKind::RandomVariable { .. } => ("random_variable", true),
                    ElementKind::Expression { .. } => ("expression", false),
                    ElementKind::Accumulator { .. } => ("accumulator", false),
                    ElementKind::Timeseries { .. } => ("timeseries", false),
                    ElementKind::Lookup { .. } => ("lookup", false),
                    ElementKind::Delay { .. } => ("delay", false),
                    ElementKind::Script { .. } => ("script", false),
                    ElementKind::Array { .. } => ("array", false),
                    ElementKind::StochasticProcess { .. } => ("stochastic_process", false),
                };
                ElemSummary {
                    id: &e.id,
                    name: &e.name,
                    kind: kind_str,
                    container: e.container.as_deref(),
                    editable,
                    unit: e.primary_unit(),
                    description: e.description.as_deref(),
                }
            })
            .collect();

        let summary = Summary {
            element_count: elements.len(),
            elements,
            containers: &self.model.containers,
            simulation_settings: &self.model.simulation_settings,
        };

        serde_json::to_string(&summary).unwrap_or_default()
    }

    /// Run the simulation and return results as a JSON string.
    ///
    /// `config_json` keys (all optional — model defaults apply when absent):
    ///   `n_realizations: number`
    ///   `seed: number`
    pub fn run_json(&self, config_json: &str) -> Result<String, JsError> {
        let js_config: JsRunConfig =
            serde_json::from_str(config_json).unwrap_or_default();
        let config = RunConfig {
            n_realizations: js_config.n_realizations,
            seed: js_config.seed,
            duration_override: js_config.duration_override,
            timestep_override: js_config.timestep_override,
        };
        let results = run(&self.model, &self.graph, &config)
            .map_err(|e| JsError::new(&e.to_string()))?;
        serde_json::to_string(&results)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Update a `constant` element's value before running.
    /// `value` is in the element's declared unit.
    pub fn set_constant(&mut self, id: &str, value: f64) -> Result<(), JsError> {
        for elem in &mut self.model.elements {
            if elem.id != id {
                continue;
            }
            return match &mut elem.kind {
                ElementKind::Constant { value: v, .. } => {
                    v.value = value;
                    Ok(())
                }
                _ => Err(JsError::new(&format!("'{id}' is not a constant element"))),
            };
        }
        Err(JsError::new(&format!("element '{id}' not found")))
    }

    /// Update a named parameter of a `random_variable` element's distribution.
    ///
    /// `param_name` is the distribution parameter key (e.g. `"mean"`, `"stddev"`).
    /// `value` is the new magnitude in the existing declared unit.
    pub fn set_rv_param(
        &mut self,
        id: &str,
        param_name: &str,
        value: f64,
    ) -> Result<(), JsError> {
        for elem in &mut self.model.elements {
            if elem.id != id {
                continue;
            }
            return match &mut elem.kind {
                ElementKind::RandomVariable { distribution, .. } => {
                    set_dist_param(&mut distribution.kind, param_name, value)
                        .map_err(|msg| JsError::new(&msg))
                }
                _ => Err(JsError::new(&format!("'{id}' is not a random_variable"))),
            };
        }
        Err(JsError::new(&format!("element '{id}' not found")))
    }

    /// Return the topological evaluation order as a JSON array of element IDs.
    /// Useful for debugging model wiring in the frontend.
    pub fn topo_order_json(&self) -> String {
        serde_json::to_string(&self.graph.topo_order).unwrap_or_default()
    }
}

// ── Distribution parameter mutation ──────────────────────────────────────────

fn set_dist_param(kind: &mut DistributionKind, param: &str, value: f64) -> Result<(), String> {
    match kind {
        DistributionKind::Normal { mean, stddev } => match param {
            "mean" => *mean = crate::model::QuantityOrFormula::Quantity(crate::model::Quantity {
                value, unit: mean.unit().to_string(), display_unit: None,
            }),
            "stddev" => *stddev = crate::model::QuantityOrFormula::Quantity(crate::model::Quantity {
                value, unit: stddev.unit().to_string(), display_unit: None,
            }),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Lognormal { mean, stddev }
        | DistributionKind::LognormalMoments { mean, stddev } => match param {
            "mean" => *mean = crate::model::QuantityOrFormula::Quantity(crate::model::Quantity {
                value, unit: mean.unit().to_string(), display_unit: None,
            }),
            "stddev" => *stddev = crate::model::QuantityOrFormula::Quantity(crate::model::Quantity {
                value, unit: stddev.unit().to_string(), display_unit: None,
            }),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Uniform { min, max } => match param {
            "min" => min.value = value,
            "max" => max.value = value,
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Triangular { min, mode, max } => match param {
            "min" => min.value = value,
            "mode" => mode.value = value,
            "max" => max.value = value,
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Exponential { mean } => match param {
            "mean" => *mean = crate::model::QuantityOrFormula::Quantity(crate::model::Quantity {
                value, unit: mean.unit().to_string(), display_unit: None,
            }),
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Gamma { shape, scale }
        | DistributionKind::Weibull { shape, scale }
        | DistributionKind::PearsonV { shape, scale } => match param {
            "shape" => shape.value = value,
            "scale" => scale.value = value,
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::Beta { alpha, beta } => match param {
            "alpha" => alpha.value = value,
            "beta" => beta.value = value,
            _ => return Err(format!("unknown parameter '{param}'")),
        },

        DistributionKind::PearsonIii { mean, stddev, skewness } => match param {
            "mean" => mean.value = value,
            "stddev" => stddev.value = value,
            "skewness" => skewness.value = value,
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

        DistributionKind::Discrete { .. } => {
            return Err("parameter editing not supported for discrete distributions".into());
        }
    }
    Ok(())
}
