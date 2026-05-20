use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::engine::RunConfig;
use crate::model::{DistributionKind, ElementKind, Quantity, QuantityOrFormula, WasimModel};

/// Serializable parameter overrides produced by the frontend "Save parameters" button.
/// Apply to a loaded model with [`apply`] before building the graph and running.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ModelParams {
    /// Constant element overrides: element ID → new value (in the element's declared unit).
    #[serde(default)]
    pub constants: HashMap<String, f64>,
    /// Random variable parameter overrides: element ID → { param_name → value }.
    #[serde(default)]
    pub rv_params: HashMap<String, HashMap<String, f64>>,
    /// Run configuration overrides (merged into RunConfig at call time).
    #[serde(default)]
    pub run_config: RunConfigOverride,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RunConfigOverride {
    pub n_realizations: Option<u32>,
    pub seed: Option<u64>,
    pub duration_override: Option<f64>,
    pub timestep_override: Option<f64>,
}

impl ModelParams {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Apply constant and RV parameter overrides to `model` in place.
    /// Unknown element IDs and parameter names are silently ignored.
    pub fn apply(&self, model: &mut WasimModel) {
        for elem in &mut model.elements {
            if let ElementKind::Constant { value, .. } = &mut elem.kind {
                if let Some(&v) = self.constants.get(&elem.id) {
                    value.value = v;
                }
            } else if let ElementKind::RandomVariable { distribution, .. } = &mut elem.kind {
                if let Some(overrides) = self.rv_params.get(&elem.id) {
                    apply_dist_params(&mut distribution.kind, overrides);
                }
            }
        }
    }

    /// Merge the run_config overrides into an existing RunConfig.
    pub fn merge_run_config(&self, base: RunConfig) -> RunConfig {
        RunConfig {
            n_realizations: self.run_config.n_realizations.or(base.n_realizations),
            seed: self.run_config.seed.or(base.seed),
            duration_override: self.run_config.duration_override.or(base.duration_override),
            timestep_override: self.run_config.timestep_override.or(base.timestep_override),
        }
    }
}

fn apply_dist_params(kind: &mut DistributionKind, overrides: &HashMap<String, f64>) {
    match kind {
        DistributionKind::Normal { mean, stddev }
        | DistributionKind::Lognormal { mean, stddev }
        | DistributionKind::LognormalMoments { mean, stddev } => {
            if let Some(&v) = overrides.get("mean") { set_qof(mean, v); }
            if let Some(&v) = overrides.get("stddev") { set_qof(stddev, v); }
        }
        DistributionKind::Exponential { mean } => {
            if let Some(&v) = overrides.get("mean") { set_qof(mean, v); }
        }
        DistributionKind::Uniform { min, max } => {
            if let Some(&v) = overrides.get("min") { min.value = v; }
            if let Some(&v) = overrides.get("max") { max.value = v; }
        }
        DistributionKind::Triangular { min, mode, max } => {
            if let Some(&v) = overrides.get("min") { min.value = v; }
            if let Some(&v) = overrides.get("mode") { mode.value = v; }
            if let Some(&v) = overrides.get("max") { max.value = v; }
        }
        DistributionKind::Gamma { shape, scale }
        | DistributionKind::Weibull { shape, scale }
        | DistributionKind::PearsonV { shape, scale } => {
            if let Some(&v) = overrides.get("shape") { shape.value = v; }
            if let Some(&v) = overrides.get("scale") { scale.value = v; }
        }
        DistributionKind::Beta { alpha, beta } => {
            if let Some(&v) = overrides.get("alpha") { alpha.value = v; }
            if let Some(&v) = overrides.get("beta") { beta.value = v; }
        }
        DistributionKind::PearsonIii { mean, stddev, skewness } => {
            if let Some(&v) = overrides.get("mean") { mean.value = v; }
            if let Some(&v) = overrides.get("stddev") { stddev.value = v; }
            if let Some(&v) = overrides.get("skewness") { skewness.value = v; }
        }
        DistributionKind::DiscreteUniform { min, max } => {
            if let Some(&v) = overrides.get("min") { *min = v as i64; }
            if let Some(&v) = overrides.get("max") { *max = v as i64; }
        }
        DistributionKind::Bernoulli { prob } => {
            if let Some(&v) = overrides.get("prob") { prob.value = v; }
        }
        DistributionKind::Discrete { .. } => {}
    }
}

fn set_qof(qof: &mut QuantityOrFormula, value: f64) {
    match qof {
        QuantityOrFormula::Quantity(q) => q.value = value,
        _ => {
            *qof = QuantityOrFormula::Quantity(Quantity {
                value,
                unit: "1".to_string(),
                display_unit: None,
            })
        }
    }
}
