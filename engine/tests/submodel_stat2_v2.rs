//! Phase 4: submodel_stat2 — bivariate reduction (cov/corr/beta) of two outputs of the
//! same submodel, across the submodel's realizations. Submodel S draws X~N(0,1), W~N(0,1)
//! and exposes X and Y = 2X + W, so beta(X,Y)=2, cov(X,Y)=2, corr(X,Y)=2/√5. The parent
//! reduces them via submodel_stat2 — the submodel-level analog of run_stat2.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn model(statistic: &str) -> String {
    format!(r#"{{
      "wasim_version": "0.8.3",
      "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 4, "seed": 1}},
      "containers": [
        {{"id": "Model", "name": "Model", "children": ["Model/S"], "elements": ["Model/Result"]}},
        {{"id": "Model/S", "name": "S", "parent": "Model", "kind": "submodel",
         "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 8000}},
         "interface": {{"inputs": [], "outputs": ["Model/S/x", "Model/S/y"]}},
         "elements": ["Model/S/x", "Model/S/w", "Model/S/y"]}}
      ],
      "elements": [
        {{"id": "Model/S/x", "name": "x", "primitive": "node", "value_rule": "sample", "container": "Model/S",
         "distribution": {{"family": "normal", "parameters": {{"mean": {{"value": 0, "unit": "1"}}, "stddev": {{"value": 1, "unit": "1"}}}}}},
         "save_results": {{"final_value": true}}}},
        {{"id": "Model/S/w", "name": "w", "primitive": "node", "value_rule": "sample", "container": "Model/S",
         "distribution": {{"family": "normal", "parameters": {{"mean": {{"value": 0, "unit": "1"}}, "stddev": {{"value": 1, "unit": "1"}}}}}},
         "save_results": {{"final_value": true}}}},
        {{"id": "Model/S/y", "name": "y", "primitive": "node", "value_rule": "expression", "container": "Model/S",
         "inputs": ["Model/S/x", "Model/S/w"],
         "expression": {{"ast": {{"op": "add",
            "left": {{"op": "multiply", "left": {{"op": "literal", "value": 2}}, "right": {{"op": "ref", "element_id": "Model/S/x"}}}},
            "right": {{"op": "ref", "element_id": "Model/S/w"}}}}}},
         "save_results": {{"final_value": true}}}},
        {{"id": "Model/Result", "name": "Result", "primitive": "node", "value_rule": "expression",
         "container": "Model", "inputs": ["Model/S"],
         "expression": {{"ast": {{"op": "submodel_stat2", "submodel_id": "Model/S",
            "output_x": "Model/S/x", "output_y": "Model/S/y", "statistic": "{statistic}"}}}},
         "save_results": {{"final_value": true}}}}
      ]
    }}"#)
}

fn reduce(statistic: &str) -> f64 {
    let m = parse_v2(&model(statistic)).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    r.elements["Model/Result"].final_values[0]
}

#[test]
fn submodel_stat2_reduces_two_submodel_outputs() {
    let beta = reduce("beta");
    let cov = reduce("cov");
    let corr = reduce("corr");
    println!("beta={beta:.4} cov={cov:.4} corr={corr:.4}");
    assert!((beta - 2.0).abs() < 0.06, "beta≈2, got {beta}");
    assert!((cov - 2.0).abs() < 0.08, "cov≈2, got {cov}");
    assert!((corr - 2.0 / 5.0_f64.sqrt()).abs() < 0.02, "corr≈0.894, got {corr}");
}
