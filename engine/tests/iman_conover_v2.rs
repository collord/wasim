//! Iman-Conover correlation: the achieved Spearman rank correlation matches the target,
//! and marginal moments are preserved (v2-native engine path).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn spearman(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len();
    let ranks = |v: &[f64]| -> Vec<f64> {
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by(|&a, &b| v[a].total_cmp(&v[b]));
        let mut r = vec![0.0; n];
        for (rank, &i) in idx.iter().enumerate() {
            r[i] = (rank + 1) as f64;
        }
        r
    };
    let (rx, ry) = (ranks(xs), ranks(ys));
    let m = (n as f64 + 1.0) / 2.0;
    let num: f64 = (0..n).map(|i| (rx[i] - m) * (ry[i] - m)).sum();
    let den: f64 = ((0..n).map(|i| (rx[i] - m).powi(2)).sum::<f64>()
        * (0..n).map(|i| (ry[i] - m).powi(2)).sum::<f64>())
    .sqrt();
    if den < 1e-12 { 0.0 } else { num / den }
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}
fn stddev(v: &[f64]) -> f64 {
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

#[test]
fn iman_conover_recovers_target_rho_and_marginals() {
    let json = r#"{"wasim_version": "0.8.0",
      "simulation_settings": {"duration": {"value": 1, "unit": "yr"}, "timestep": {"value": 1, "unit": "yr"}, "n_realizations": 3000, "seed": 99},
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "sample",
         "distribution": {"family": "normal", "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}},
         "correlations": [{"partner": "y", "coefficient": 0.7}],
         "save_results": {"final_value": true}},
        {"id": "y", "name": "Y", "primitive": "node", "value_rule": "sample",
         "distribution": {"family": "normal", "parameters": {"mean": {"value": 5, "unit": "1"}, "stddev": {"value": 2, "unit": "1"}}},
         "save_results": {"final_value": true}}
      ]}"#;

    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    let x = &r.elements["x"].final_values;
    let y = &r.elements["y"].final_values;

    let rho = spearman(x, y);
    assert!((rho - 0.7).abs() < 0.05, "achieved Spearman ρ̂ = {rho:.4}, target 0.7");

    // Marginals preserved (reordering doesn't change the multiset of draws).
    assert!((mean(x) - 0.0).abs() < 0.1, "x mean {}", mean(x));
    assert!((mean(y) - 5.0).abs() < 0.15, "y mean {}", mean(y));
    assert!((stddev(x) - 1.0).abs() < 0.1, "x sd {}", stddev(x));
    assert!((stddev(y) - 2.0).abs() < 0.15, "y sd {}", stddev(y));
}
