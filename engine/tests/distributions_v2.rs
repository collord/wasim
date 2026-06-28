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
fn beta_four_parameter_scaling() {
    // Beta(2,2) is symmetric → scaled onto [10,20] has mean 15, support [10,20].
    let v = draws(r#"{"family": "beta", "parameters": {"alpha": {"value": 2, "unit": "1"}, "beta": {"value": 2, "unit": "1"}, "min": {"value": 10, "unit": "1"}, "max": {"value": 20, "unit": "1"}}}"#);
    assert!((mean(&v) - 15.0).abs() < 0.3, "beta4 mean {}", mean(&v));
    assert!(v.iter().all(|&x| (10.0..=20.0).contains(&x)), "beta4 support");
}
