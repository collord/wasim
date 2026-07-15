//! Statistical tests for the +7 v2 distribution families (and 4-parameter beta),
//! sampled through the v2-native engine path.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const N: u32 = 8000;

/// Draw `N` final values from a single `sample` node with the given distribution JSON.
fn draws(distribution: &str) -> Vec<f64> {
    let json = format!(
        r#"{{"wasim_version": "0.8.0",
        "simulation_settings": {{"duration": {{"value": 1, "unit": "yr"}}, "timestep": {{"value": 1, "unit": "yr"}}, "n_realizations": {N}, "seed": 7}},
        "elements": [{{"id": "x", "name": "X", "primitive": "node", "value_rule": "sample",
          "distribution": {distribution}, "save_results": {{"final_value": true}}}}]}}"#
    );
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run").elements["x"].final_values.clone()
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

#[test]
fn pert_mean_and_support() {
    // Beta-PERT(0, 5, 10): mean = (min + 4·mode + max)/6 = 5; support [0, 10].
    let v = draws(r#"{"family": "pert", "parameters": {"min": {"value": 0, "unit": "1"}, "mode": {"value": 5, "unit": "1"}, "max": {"value": 10, "unit": "1"}}}"#);
    assert!((mean(&v) - 5.0).abs() < 0.2, "pert mean {}", mean(&v));
    assert!(v.iter().all(|&x| (0.0..=10.0).contains(&x)), "pert support");
}

#[test]
fn pareto_mean_and_support() {
    // Pareto(scale=1, shape=3): mean = shape·scale/(shape-1) = 1.5; support x ≥ 1.
    let v = draws(r#"{"family": "pareto", "parameters": {"scale": {"value": 1, "unit": "1"}, "shape": {"value": 3, "unit": "1"}}}"#);
    assert!((mean(&v) - 1.5).abs() < 0.2, "pareto mean {}", mean(&v));
    assert!(v.iter().all(|&x| x >= 1.0 - 1e-9), "pareto support");
}

#[test]
fn extreme_value_mean() {
    // Gumbel(loc=0, scale=1): mean = γ ≈ 0.5772.
    let v = draws(r#"{"family": "extreme_value", "parameters": {"location": {"value": 0, "unit": "1"}, "scale": {"value": 1, "unit": "1"}}}"#);
    assert!((mean(&v) - 0.5772).abs() < 0.15, "gumbel mean {}", mean(&v));
}

#[test]
fn student_t_mean() {
    // t(df=10): mean 0.
    let v = draws(r#"{"family": "student_t", "parameters": {"degrees_of_freedom": {"value": 10, "unit": "1"}}}"#);
    assert!(mean(&v).abs() < 0.15, "student_t mean {}", mean(&v));
}

#[test]
fn cumulative_uniform_via_cdf() {
    // CDF linear 0→1 over x∈[0,10] is Uniform(0,10): mean 5, support [0,10].
    let v = draws(r#"{"family": "cumulative", "parameters": {"points": [{"x": 0, "cumulative_probability": 0}, {"x": 10, "cumulative_probability": 1}]}}"#);
    assert!((mean(&v) - 5.0).abs() < 0.3, "cumulative mean {}", mean(&v));
    assert!(v.iter().all(|&x| (0.0..=10.0).contains(&x)), "cumulative support");
}

#[test]
fn sampled_weighted_empirical() {
    // Equal-weight {1,2,3}: mean 2; only those three values occur.
    let v = draws(r#"{"family": "sampled", "parameters": {"samples": [1, 2, 3]}}"#);
    assert!((mean(&v) - 2.0).abs() < 0.15, "sampled mean {}", mean(&v));
    assert!(v.iter().all(|&x| x == 1.0 || x == 2.0 || x == 3.0), "sampled support");
}

#[test]
fn external_degrades_to_zero() {
    let v = draws(r#"{"family": "external", "parameters": {"definition": "user_defined"}}"#);
    assert!(v.iter().all(|&x| x == 0.0), "external should be 0.0");
}

#[test]
fn trapezoidal_symmetric_mean_and_support() {
    // Symmetric trapezoid min=0, lower=2, upper=8, max=10 → mean = 5 (by symmetry),
    // support [0, 10].
    let v = draws(r#"{"family": "trapezoidal", "parameters": {"min": {"value": 0, "unit": "1"}, "lower": {"value": 2, "unit": "1"}, "upper": {"value": 8, "unit": "1"}, "max": {"value": 10, "unit": "1"}}}"#);
    assert!((mean(&v) - 5.0).abs() < 0.2, "trapezoid mean {}", mean(&v));
    assert!(v.iter().all(|&x| (0.0..=10.0).contains(&x)), "trapezoid support");
}

#[test]
fn trapezoidal_degenerates_to_triangular() {
    // lower == upper collapses the plateau → triangular(0, 5, 10), mean = 5.
    let v = draws(r#"{"family": "trapezoidal", "parameters": {"min": {"value": 0, "unit": "1"}, "lower": {"value": 5, "unit": "1"}, "upper": {"value": 5, "unit": "1"}, "max": {"value": 10, "unit": "1"}}}"#);
    assert!((mean(&v) - 5.0).abs() < 0.2, "trapezoid→triangular mean {}", mean(&v));
    assert!(v.iter().all(|&x| (0.0..=10.0).contains(&x)), "support");
}

#[test]
fn beta_four_parameter_scaling() {
    // Beta(2,2) is symmetric → scaled onto [10,20] has mean 15, support [10,20].
    let v = draws(r#"{"family": "beta", "parameters": {"alpha": {"value": 2, "unit": "1"}, "beta": {"value": 2, "unit": "1"}, "min": {"value": 10, "unit": "1"}, "max": {"value": 20, "unit": "1"}}}"#);
    assert!((mean(&v) - 15.0).abs() < 0.3, "beta4 mean {}", mean(&v));
    assert!(v.iter().all(|&x| (10.0..=20.0).contains(&x)), "beta4 support");
}

/// A distribution parameter may be formula-valued (0.8.5): a uniform whose `max` references
/// another element resolves that reference before sampling, so the draw responds to the model.
/// uniform(0, 2·k) with k=5 → samples in [0,10], mean ≈ 5. Changing k changes the range.
#[test]
fn formula_valued_distribution_param() {
    let mk = |k: f64| format!(r#"{{
      "wasim_version": "0.8.5",
      "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 4000, "seed": 3}},
      "elements": [
        {{"id": "k", "name": "k", "primitive": "node", "value_rule": "fixed", "value": {{"value": {k}, "unit": "1"}}}},
        {{"id": "u", "name": "U", "primitive": "node", "value_rule": "sample", "inputs": ["k"],
         "distribution": {{"family": "uniform", "parameters": {{
            "min": {{"value": 0, "unit": "1"}},
            "max": {{"ast": {{"op": "multiply", "left": {{"op": "literal", "value": 2}}, "right": {{"op": "ref", "element_id": "k"}}}}}}
         }}}},
         "save_results": {{"final_value": true}}}}
      ]
    }}"#);

    // k=5 → uniform(0,10): mean ≈ 5, all draws ≤ 10.
    let m = wasim_engine::parse_v2(&mk(5.0)).expect("parse");
    let g = wasim_engine::ModelGraphV2::build(&m).expect("graph");
    let r = wasim_engine::run_v2(&m, &g, &wasim_engine::RunConfig::default()).expect("run");
    let vals = &r.elements["u"].final_values;
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    assert!((mean - 5.0).abs() < 0.3, "k=5 mean {mean} not ≈5");
    assert!(vals.iter().all(|&v| (0.0..=10.0).contains(&v)), "draws out of [0,10]");

    // k=10 → uniform(0,20): mean ≈ 10 — the param tracked the reference.
    let m2 = wasim_engine::parse_v2(&mk(10.0)).expect("parse");
    let g2 = wasim_engine::ModelGraphV2::build(&m2).expect("graph");
    let r2 = wasim_engine::run_v2(&m2, &g2, &wasim_engine::RunConfig::default()).expect("run");
    let v2 = &r2.elements["u"].final_values;
    let mean2 = v2.iter().sum::<f64>() / v2.len() as f64;
    assert!((mean2 - 10.0).abs() < 0.6, "k=10 mean {mean2} not ≈10");
}

/// The `gamma` builtin (Γ) — needed for Weibull scale derivation (scale = mean/Γ(1+1/shape)).
/// Verify known values through the expression evaluator: Γ(5)=24, Γ(1)=1, Γ(0.5)=√π.
#[test]
fn gamma_builtin_function() {
    let expr = |arg: f64, expected: f64, tol: f64| {
        let json = format!(r#"{{
          "wasim_version": "0.8.5",
          "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}}},
          "elements": [
            {{"id": "g", "name": "G", "primitive": "node", "value_rule": "expression",
             "expression": {{"ast": {{"op": "call", "fn": "gamma", "args": [{{"op": "literal", "value": {arg}}}]}}}},
             "save_results": {{"final_value": true}}}}
          ]
        }}"#);
        let m = wasim_engine::parse_v2(&json).expect("parse");
        let g = wasim_engine::ModelGraphV2::build(&m).expect("graph");
        let r = wasim_engine::run_v2(&m, &g, &wasim_engine::RunConfig::default()).expect("run");
        let got = r.elements["g"].final_values[0];
        assert!((got - expected).abs() < tol, "gamma({arg}) = {got}, expected {expected}");
    };
    expr(5.0, 24.0, 1e-9);        // Γ(5) = 4! = 24
    expr(1.0, 1.0, 1e-12);        // Γ(1) = 1
    expr(4.0, 6.0, 1e-10);        // Γ(4) = 3! = 6
    expr(0.5, std::f64::consts::PI.sqrt(), 1e-10); // Γ(1/2) = √π (reflection path)

    // The real use: Weibull scale = mean / Γ(1 + 1/shape). For shape=2, mean=10:
    // Γ(1.5) = √π/2 ≈ 0.8862, so scale ≈ 10 / 0.8862 ≈ 11.284.
    let json = r#"{
      "wasim_version": "0.8.5",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "shape", "name": "shape", "primitive": "node", "value_rule": "fixed", "value": {"value": 2, "unit": "1"}},
        {"id": "scale", "name": "scale", "primitive": "node", "value_rule": "expression", "inputs": ["shape"],
         "expression": {"ast": {"op": "divide",
           "left": {"op": "literal", "value": 10},
           "right": {"op": "call", "fn": "gamma", "args": [{"op": "add",
             "left": {"op": "literal", "value": 1},
             "right": {"op": "divide", "left": {"op": "literal", "value": 1}, "right": {"op": "ref", "element_id": "shape"}}}]}}},
         "save_results": {"final_value": true}}
      ]
    }"#;
    let m = wasim_engine::parse_v2(json).expect("parse");
    let g = wasim_engine::ModelGraphV2::build(&m).expect("graph");
    let r = wasim_engine::run_v2(&m, &g, &wasim_engine::RunConfig::default()).expect("run");
    let scale = r.elements["scale"].final_values[0];
    assert!((scale - 11.2838).abs() < 1e-3, "weibull scale = {scale}, expected ≈11.284");
}
