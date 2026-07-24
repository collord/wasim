//! Phase-3 end-to-end: the extended `submodel_stat` reducer set (exceedance/cte/sum/min/max)
//! dispatches correctly through the parse → run → eval path, not just as bare engine fns.
//!
//! Uses a submodel whose output is a `fixed` constant (7.0) over N realizations, so every
//! reduced value is predictable regardless of RNG: the whole sample vector is [7,7,…].

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// Parent `Result` reduces submodel `S`'s constant output by `statistic` (with optional `arg`).
fn model_with_reduction(statistic: &str, arg: Option<f64>) -> String {
    let stat_ast = match arg {
        Some(a) => format!(
            r#"{{"op": "submodel_stat", "submodel_id": "Model/S", "output": "Model/S/x", "statistic": "{statistic}", "arg": {{"op": "literal", "value": {a}}}}}"#),
        None => format!(
            r#"{{"op": "submodel_stat", "submodel_id": "Model/S", "output": "Model/S/x", "statistic": "{statistic}"}}"#),
    };
    format!(r#"{{
      "wasim_version": "0.8.3",
      "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 8, "seed": 1}},
      "containers": [
        {{"id": "Model", "name": "Model", "children": ["Model/S"], "elements": ["Model/Result"]}},
        {{"id": "Model/S", "name": "S", "parent": "Model", "kind": "submodel",
         "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 8}},
         "interface": {{"inputs": [], "outputs": ["Model/S/x"]}}, "elements": ["Model/S/x"]}}
      ],
      "elements": [
        {{"id": "Model/S/x", "name": "x", "primitive": "node", "value_rule": "fixed",
         "container": "Model/S", "value": {{"value": 7, "unit": "1"}}, "save_results": {{"final_value": true}}}},
        {{"id": "Model/Result", "name": "Result", "primitive": "node", "value_rule": "expression",
         "container": "Model", "inputs": ["Model/S"],
         "expression": {{"ast": {stat_ast}}}, "save_results": {{"final_value": true}}}}
      ]
    }}"#)
}

fn reduce(statistic: &str, arg: Option<f64>) -> f64 {
    let json = model_with_reduction(statistic, arg);
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    // Every realization's Result is the same reduction of the constant sample vector.
    r.elements["Model/Result"].final_values[0]
}

#[test]
fn extended_reducers_dispatch_over_constant_submodel() {
    // Sample vector is [7,7,7,7,7,7,7,7].
    assert_eq!(reduce("min", None), 7.0, "min of constant 7");
    assert_eq!(reduce("max", None), 7.0, "max of constant 7");
    assert_eq!(reduce("sum", None), 56.0, "sum = 7 × 8");
    assert_eq!(reduce("mean", None), 7.0, "mean (regression: existing reducer)");

    // Exceedance P(X > arg): all samples are 7.
    assert_eq!(reduce("exceedance", Some(5.0)), 1.0, "all 7s exceed 5");
    assert_eq!(reduce("exceedance", Some(7.0)), 0.0, "strict > 7 excludes 7");
    assert_eq!(reduce("exceedance", Some(10.0)), 0.0, "nothing exceeds 10");

    // CTE beyond any percentile of a constant vector is the constant itself.
    assert_eq!(reduce("cte", Some(90.0)), 7.0, "CTE of constant 7 is 7");
}
