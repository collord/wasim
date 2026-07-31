//! Value-of-information (VOI) executor. Computes the **expected value of partial perfect
//! information** (EVPPI) for one or more uncertain "probe" elements, given the model's
//! optimization spec (decision variables + objective).
//!
//! EVPPI(X) = E_X[ max_d E_{θ|X}[obj(d, θ)] ] − max_d E_θ[obj(d, θ)]
//!
//! The second term is exactly what `optimize_v2::optimize` computes today (the best decision under
//! full uncertainty). The first term is an outer expectation over scenarios of X, each requiring an
//! inner optimization with X pinned to that scenario. So VOI composes the existing optimizer with
//! the existing sampler — the only genuinely new primitive is pinning a `Sample` element to a
//! fixed scenario value (`resolve_uncertainty`), since `eval_harness::set_variable` deliberately
//! rejects sample nodes.
//!
//! Information never hurts a rational decision, so EVPPI ≥ 0 in expectation; small negative
//! estimates are Monte-Carlo noise (the caller may clamp for display).

use crate::error::EngineError;
use crate::model::{OptDirection, Quantity};
use crate::model_v2::{FixedValue, Model, NodeRule, Primitive};
use crate::optimize_v2::optimize;
use crate::{engine_v2, ModelGraphV2, RunConfig};

/// Default number of scenarios drawn per probe when the caller doesn't specify.
pub const DEFAULT_SCENARIOS: u32 = 64;

#[derive(Debug, Clone, serde::Serialize)]
pub struct VoiResults {
    /// The optimal expected objective under full uncertainty (the baseline both probes subtract).
    pub baseline_objective: f64,
    pub probes: Vec<ProbeResult>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProbeResult {
    pub element_id: String,
    /// Expected value of learning this uncertainty before deciding. ≥ 0 in expectation.
    pub evppi: f64,
    /// The number of scenarios actually averaged (may differ from the request if the draw yielded
    /// fewer values).
    pub scenarios: usize,
}

/// Pin a `Sample` (uncertain) element to a fixed scenario value by rewriting its rule to `Fixed`.
/// Errors if the element is missing or is not a sample node — the sibling of `set_variable`
/// (which pins fixed decision scalars) that VOI needs.
fn resolve_uncertainty(model: &mut Model, id: &str, value: f64) -> Result<(), EngineError> {
    for elem in &mut model.elements {
        if elem.base.id != id {
            continue;
        }
        if let Primitive::Node(n) = &mut elem.primitive {
            if matches!(n.rule, NodeRule::Sample { .. }) {
                n.rule = NodeRule::Fixed {
                    value: FixedValue::Scalar(Quantity { value, unit: String::new(), display_unit: None }),
                    editable: false,
                    bounds: None,
                };
                return Ok(());
            }
        }
        return Err(EngineError::InvalidModel(format!(
            "VOI probe '{id}' is not an uncertain (sample) element"
        )));
    }
    Err(EngineError::ElementNotFound(id.to_string()))
}

/// Draw `k` scenario values of a probe from its marginal distribution by running the model with
/// `k` realizations and reading the probe's per-realization sampled value. This reuses the
/// engine's own sampler (correct for every distribution, including formula-resolved parameters)
/// rather than re-implementing inverse-CDF draws here.
fn draw_scenarios(model: &Model, probe_id: &str, k: u32, config: &RunConfig) -> Result<Vec<f64>, EngineError> {
    let mut m = model.clone();
    let mut found = false;
    for elem in &mut m.elements {
        if elem.base.id == probe_id {
            elem.base.save_results.final_value = Some(true);
            found = true;
        }
    }
    if !found {
        return Err(EngineError::ElementNotFound(probe_id.to_string()));
    }
    // Only n_realizations + seed matter for reading the probe's per-realization draws; the rest
    // takes engine defaults (RunConfig isn't Clone, so build via struct-update from Default).
    let cfg = RunConfig { n_realizations: Some(k.max(1)), seed: config.seed, ..RunConfig::default() };
    let graph = ModelGraphV2::build(&m)?;
    let results = engine_v2::run(&m, &graph, &cfg)?;
    let er = results
        .elements
        .get(probe_id)
        .filter(|e| !e.final_values.is_empty())
        .ok_or_else(|| EngineError::InvalidModel(format!("VOI probe '{probe_id}' produced no scenarios")))?;
    Ok(er.final_values.clone())
}

/// Compute EVPPI for each probe. `model` must carry an `optimization` spec (decisions + objective).
pub fn value_of_information(
    model: &Model,
    probe_ids: &[String],
    scenarios: u32,
    config: &RunConfig,
) -> Result<VoiResults, EngineError> {
    let spec = model
        .optimization
        .as_ref()
        .ok_or_else(|| EngineError::InvalidModel("value-of-information requires an optimization spec".into()))?;
    if spec.variables.is_empty() {
        return Err(EngineError::InvalidModel("value-of-information requires decision variables".into()));
    }
    let direction = spec.objective.direction;
    let k = scenarios.max(1);

    // Baseline: the best decision under full uncertainty — max_d E_θ[obj].
    let baseline = optimize(model, config)?.objective;

    let mut probes = Vec::with_capacity(probe_ids.len());
    for probe in probe_ids {
        let xs = draw_scenarios(model, probe, k, config)?;
        // Inner term: for each scenario, optimize the decision knowing X = x, then average.
        let mut sum = 0.0;
        for &x in &xs {
            let mut m = model.clone();
            resolve_uncertainty(&mut m, probe, x)?;
            sum += optimize(&m, config)?.objective;
        }
        let expected = sum / xs.len() as f64;
        // EVPPI ≥ 0: information improves the objective in the optimization's direction.
        let evppi = match direction {
            OptDirection::Maximize => expected - baseline,
            OptDirection::Minimize => baseline - expected,
        };
        probes.push(ProbeResult { element_id: probe.clone(), evppi, scenarios: xs.len() });
    }

    Ok(VoiResults { baseline_objective: baseline, probes })
}

#[cfg(test)]
mod tests {
    use super::*;

    // A quadratic-loss decision under uniform demand: minimize (d − demand)^2, d ∈ [0, 10],
    // demand ~ Uniform(0, 10). Under uncertainty the optimum is d = E[demand] = 5 with expected
    // loss ≈ Var = 100/12 ≈ 8.33. Knowing demand, the optimum is d = demand with loss 0. So
    // EVPPI(demand) ≈ Var(demand) ≈ 8.33 — and, decisively, strictly positive.
    const MODEL: &str = r#"{
      "wasim_version": "0.1.0",
      "simulation_settings": { "duration": {"value":1,"unit":"s"}, "timestep": {"value":1,"unit":"s"}, "n_realizations": 80, "seed": 11 },
      "elements": [
        { "id":"d", "name":"Decision", "primitive":"node", "value_rule":"fixed", "value":{"value":5,"unit":"1"}, "editable":true, "bounds":{"min":0,"max":10} },
        { "id":"demand", "name":"Demand", "primitive":"node", "value_rule":"sample", "distribution":{"family":"uniform","parameters":{"min":{"value":0,"unit":"1"},"max":{"value":10,"unit":"1"}}}, "save_results":{"final_value":true} },
        { "id":"loss", "name":"Loss", "primitive":"node", "value_rule":"expression", "inputs":["d","demand"],
          "expression":{"ast":{"op":"power","left":{"op":"subtract","left":{"op":"ref","element_id":"d"},"right":{"op":"ref","element_id":"demand"}},"right":{"op":"literal","value":2}}},
          "save_results":{"final_value":true} }
      ],
      "optimization": {
        "objective": { "element_id":"loss", "direction":"minimize", "statistic":{"kind":"mean"} },
        "variables": [ { "element_id":"d", "lower":{"value":0,"unit":"1"}, "upper":{"value":10,"unit":"1"}, "initial":{"value":5,"unit":"1"} } ]
      }
    }"#;

    #[test]
    fn diag_model_samples() {
        let model = crate::v2_parse::parse(MODEL).expect("parse");
        let cfg = RunConfig { n_realizations: Some(60), seed: Some(3), ..RunConfig::default() };
        let graph = ModelGraphV2::build(&model).expect("graph");
        let r = crate::engine_v2::run(&model, &graph, &cfg).expect("run");
        let demand = &r.elements["demand"].final_values;
        let loss = &r.elements["loss"].final_values;
        let dmean = demand.iter().sum::<f64>() / demand.len() as f64;
        let dvar = demand.iter().map(|x| (x - dmean).powi(2)).sum::<f64>() / demand.len() as f64;
        let lmean = loss.iter().sum::<f64>() / loss.len() as f64;
        // The uncertainty must actually vary, and the objective expression must evaluate against
        // it (a missing `inputs` link once left loss ≡ 0 — a topo-ordering trap).
        assert!(dvar > 1.0, "demand should vary (var={dvar})");
        assert!(lmean > 0.5, "loss should evaluate against demand (mean={lmean})");
    }

    #[test]
    fn evppi_is_positive_and_near_variance() {
        let model = crate::v2_parse::parse(MODEL).expect("parse");
        let res = value_of_information(&model, &["demand".to_string()], 8, &RunConfig::default())
            .expect("voi");
        // Baseline ≈ Var(Uniform(0,10)) ≈ 8.33 (best decision under uncertainty).
        assert!(res.baseline_objective > 3.0 && res.baseline_objective < 20.0,
            "baseline = {}", res.baseline_objective);
        let p = &res.probes[0];
        // Information is worth something, and roughly the variance it removes.
        assert!(p.evppi > 1.0, "EVPPI should be clearly positive, got {}", p.evppi);
        assert!(p.evppi < 20.0, "EVPPI implausibly large: {}", p.evppi);
    }
}
