//! B7 realization weights: weighted stat reductions in the A3 analysis layer. Uniform/absent
//! weights reproduce the unweighted statistics; skewed weights shift the reductions.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, ResultsSpec, RunConfig};

/// A model with one sample node (uniform 0..1) over N realizations, final-value saved.
fn uniform_model(n: u32) -> String {
    format!(
        r#"{{"wasim_version": "0.9.5",
          "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}},
            "n_realizations": {n}, "seed": 4}},
          "elements": [
            {{"id": "x", "name": "X", "primitive": "node", "value_rule": "sample",
             "distribution": {{"family": "uniform", "parameters": {{"min": {{"value": 0, "unit": "1"}}, "max": {{"value": 1, "unit": "1"}}}}}},
             "save_results": {{"final_value": true, "time_history": true}}}}
          ]}}"#
    )
}

fn final_stats(n: u32, weights: Vec<f64>) -> wasim_engine::results_spec::FinalStats {
    let json = uniform_model(n);
    let m = parse_v2(&json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let spec = ResultsSpec { final_stats: true, ..Default::default() };
    let cfg = RunConfig { seed: Some(4), results_spec: Some(spec), realization_weights: weights, ..RunConfig::default() };
    let r = run_v2(&m, &g, &cfg).unwrap();
    r.elements["x"].analysis.as_ref().unwrap().final_stats.clone().unwrap()
}

/// Uniform weights reproduce the unweighted mean.
#[test]
fn uniform_weights_equal_unweighted() {
    let n = 100;
    let unweighted = final_stats(n, vec![]);
    let uniform = final_stats(n, vec![1.0; n as usize]);
    // Mean is exactly equal on uniform weights.
    assert!((unweighted.mean - uniform.mean).abs() < 1e-9, "uniform weights should match unweighted mean");
    // Std differs only by the Bessel correction (unweighted uses n−1, weighted population n),
    // so they match within the √(n/(n−1)) factor (tiny at n=100).
    let bessel = ((n as f64) / (n as f64 - 1.0)).sqrt();
    assert!((unweighted.std - uniform.std * bessel).abs() < 1e-9, "uniform-weighted std should match unweighted up to Bessel");
}

/// Weights concentrated on the high half of the samples pull the weighted mean up.
#[test]
fn weights_shift_mean() {
    let n = 200u32;
    // Sample the raw draws to know which realizations are high vs low.
    let json = uniform_model(n);
    let m = parse_v2(&json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let base = run_v2(&m, &g, &RunConfig { seed: Some(4), ..RunConfig::default() }).unwrap();
    let draws = &base.elements["x"].final_values;
    // Weight = 3 for draws above 0.5, else 1 — biases the estimate upward.
    let weights: Vec<f64> = draws.iter().map(|&d| if d > 0.5 { 3.0 } else { 1.0 }).collect();

    let unweighted = final_stats(n, vec![]);
    let weighted = final_stats(n, weights);
    assert!(weighted.mean > unweighted.mean + 0.05,
        "up-weighting the high half should raise the mean: {} vs {}", weighted.mean, unweighted.mean);
    // A uniform's unweighted mean ≈ 0.5; the biased weighted mean should exceed it clearly.
    assert!(weighted.mean > 0.55, "weighted mean {} should exceed 0.55", weighted.mean);
}

/// A weights vector of the wrong length is ignored (falls back to unweighted).
#[test]
fn wrong_length_weights_ignored() {
    let n = 50u32;
    let unweighted = final_stats(n, vec![]);
    let bad = final_stats(n, vec![1.0, 2.0, 3.0]); // length 3 ≠ 50
    assert!((unweighted.mean - bad.mean).abs() < 1e-9, "mismatched-length weights must be ignored");
}

/// Weighted percentile bands respond to weights: the median band shifts up when the high half is
/// up-weighted.
#[test]
fn weighted_percentile_band_shifts() {
    let n = 200u32;
    let json = uniform_model(n);
    let m = parse_v2(&json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let base = run_v2(&m, &g, &RunConfig { seed: Some(4), ..RunConfig::default() }).unwrap();
    let draws = &base.elements["x"].final_values;
    let weights: Vec<f64> = draws.iter().map(|&d| if d > 0.5 { 4.0 } else { 1.0 }).collect();

    let band = |w: Vec<f64>| -> f64 {
        let spec = ResultsSpec { percentiles: vec![50.0], ..Default::default() };
        let cfg = RunConfig { seed: Some(4), results_spec: Some(spec), realization_weights: w, ..RunConfig::default() };
        let r = run_v2(&m, &g, &cfg).unwrap();
        r.elements["x"].analysis.as_ref().unwrap().percentile_bands[0].values[0]
    };
    let p50_unweighted = band(vec![]);
    let p50_weighted = band(weights);
    assert!(p50_weighted > p50_unweighted, "weighted median {} should exceed unweighted {}", p50_weighted, p50_unweighted);
}
