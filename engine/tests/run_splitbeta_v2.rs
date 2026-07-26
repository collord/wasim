//! run_split_beta — the per-realization injection mechanism (CONTROL_VARIATE_SCOPE.md
//! Phase 3, split-sample). Model: X~N(0,1), W~N(0,1), Y = 2X + W ⇒ true beta(X,Y)=2.
//! run_split_beta(X, Y, folds=4) injects, per realization i, the beta estimated over
//! every realization NOT in fold i%4 — so the coefficient a realization sees excludes
//! its own data (the unbiased split-sample coefficient).

use wasim_engine::{array_lane, parse_v2, run_v2, ModelGraphV2, RunConfig};

const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 8000, "seed": 11},
  "containers": [
    {"id": "M", "name": "M", "elements": ["M/x","M/w","M/y","M/b","M/cv"]}
  ],
  "elements": [
    {"id": "M/x", "name": "x", "primitive": "node", "value_rule": "sample", "container": "M",
     "distribution": {"family": "normal", "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}},
     "save_results": {"final_value": true}},
    {"id": "M/w", "name": "w", "primitive": "node", "value_rule": "sample", "container": "M",
     "distribution": {"family": "normal", "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}},
     "save_results": {"final_value": true}},

    {"id": "M/y", "name": "y", "primitive": "node", "value_rule": "expression", "container": "M",
     "inputs": ["M/x", "M/w"],
     "expression": {"ast": {"op": "add",
        "left": {"op": "multiply", "left": {"op": "literal", "value": 2}, "right": {"op": "ref", "element_id": "M/x"}},
        "right": {"op": "ref", "element_id": "M/w"}}},
     "save_results": {"final_value": true}},

    {"id": "M/b", "name": "b", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_split_beta", "x": "M/x", "y": "M/y", "folds": 4}},
     "save_results": {"final_value": true}},

    {"id": "M/cv", "name": "cv", "primitive": "node", "value_rule": "expression", "container": "M",
     "inputs": ["M/y", "M/x"],
     "expression": {"ast": {"op": "subtract",
        "left": {"op": "ref", "element_id": "M/y"},
        "right": {"op": "multiply",
          "left": {"op": "run_split_beta", "x": "M/x", "y": "M/y", "folds": 4},
          "right": {"op": "ref", "element_id": "M/x"}}}},
     "save_results": {"final_value": true}}
  ]
}"#;

#[test]
fn run_split_beta_injects_per_realization_fold_coefficients() {
    let m = parse_v2(MODEL).expect("parse");
    assert!(array_lane::eligible(&m).is_err(), "run_split_beta is scalar-lane only");

    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let b = &r.elements["M/b"].final_values;

    // Per-realization injection: NOT a single constant. Exactly 4 distinct fold values,
    // and realization i carries fold (i % 4)'s coefficient (values repeat every 4).
    for i in 0..b.len() {
        assert!((b[i] - b[i % 4]).abs() < 1e-12, "realization {i} should carry fold {}'s beta", i % 4);
    }
    let distinct: std::collections::BTreeSet<u64> = b[..4].iter().map(|v| v.to_bits()).collect();
    assert_eq!(distinct.len(), 4, "expected 4 distinct fold coefficients, got {distinct:?}");

    // Each leave-fold-out beta ≈ the true slope 2 (each uses ~3/4 of the sample).
    for k in 0..4 {
        assert!((b[k] - 2.0).abs() < 0.1, "fold {k} beta ≈ 2, got {}", b[k]);
    }

    // The downstream control-variate estimator consumes the per-realization coefficient and
    // stays unbiased: mean(cv) = mean(Y - b·X) ≈ E[Y] = 0.
    let cv = &r.elements["M/cv"].final_values;
    let mean_cv = cv.iter().sum::<f64>() / cv.len() as f64;
    assert!(mean_cv.abs() < 0.05, "control-variate estimator mean ≈ 0, got {mean_cv}");
}
