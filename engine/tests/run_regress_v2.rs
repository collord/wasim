//! run_regress (multiple-control regression, CONTROL_VARIATE_SCOPE.md Phase 3).
//! Controls are correlated so the m×m solve is exercised (not just a diagonal system):
//!   X,U,W ~ N(0,1) iid; c0 = X, c1 = X + U, Y = 3X + 2U + W.
//! Then Y = c0 + 2·c1 + W, so regressing Y on [c0, c1] gives b0 = 1, b1 = 2.
//! Σ_C = [[1,1],[1,2]], σ_Cy = [3,5]. A single-control regression on [X] recovers
//! Cov(X,Y)/Var(X) = 3 = beta(X, Y).

use wasim_engine::{array_lane, parse_v2, run_v2, ModelGraphV2, RunConfig};

const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 16000, "seed": 5},
  "containers": [
    {"id": "M", "name": "M", "elements": ["M/x","M/u","M/w","M/c1","M/y","M/b0","M/b1","M/bsingle"]}
  ],
  "elements": [
    {"id": "M/x", "name": "x", "primitive": "node", "value_rule": "sample", "container": "M",
     "distribution": {"family": "normal", "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}},
     "save_results": {"final_value": true}},
    {"id": "M/u", "name": "u", "primitive": "node", "value_rule": "sample", "container": "M",
     "distribution": {"family": "normal", "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}},
     "save_results": {"final_value": true}},
    {"id": "M/w", "name": "w", "primitive": "node", "value_rule": "sample", "container": "M",
     "distribution": {"family": "normal", "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}},
     "save_results": {"final_value": true}},

    {"id": "M/c1", "name": "c1", "primitive": "node", "value_rule": "expression", "container": "M",
     "inputs": ["M/x", "M/u"],
     "expression": {"ast": {"op": "add", "left": {"op": "ref", "element_id": "M/x"}, "right": {"op": "ref", "element_id": "M/u"}}},
     "save_results": {"final_value": true}},

    {"id": "M/y", "name": "y", "primitive": "node", "value_rule": "expression", "container": "M",
     "inputs": ["M/x", "M/u", "M/w"],
     "expression": {"ast": {"op": "add",
        "left": {"op": "add",
          "left": {"op": "multiply", "left": {"op": "literal", "value": 3}, "right": {"op": "ref", "element_id": "M/x"}},
          "right": {"op": "multiply", "left": {"op": "literal", "value": 2}, "right": {"op": "ref", "element_id": "M/u"}}},
        "right": {"op": "ref", "element_id": "M/w"}}},
     "save_results": {"final_value": true}},

    {"id": "M/b0", "name": "b0", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_regress", "y": "M/y", "controls": ["M/x", "M/c1"], "index": 0}},
     "save_results": {"final_value": true}},
    {"id": "M/b1", "name": "b1", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_regress", "y": "M/y", "controls": ["M/x", "M/c1"], "index": 1}},
     "save_results": {"final_value": true}},

    {"id": "M/bsingle", "name": "bsingle", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_regress", "y": "M/y", "controls": ["M/x"], "index": 0}},
     "save_results": {"final_value": true}}
  ]
}"#;

#[test]
fn run_regress_recovers_multiple_control_coefficients() {
    let m = parse_v2(MODEL).expect("parse");
    // run_regress uses a linear solve, not a per-column fold → scalar lane only.
    assert!(array_lane::eligible(&m).is_err(), "run_regress should keep the model off the array lane");

    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let s = |id: &str| r.elements[id].final_values[0];

    let (b0, b1, bsingle) = (s("M/b0"), s("M/b1"), s("M/bsingle"));
    println!("b0={b0:.4} b1={b1:.4} bsingle={bsingle:.4}");

    // Joint regression on correlated controls [c0=X, c1=X+U]: b0≈1, b1≈2.
    assert!((b0 - 1.0).abs() < 0.05, "b0≈1, got {b0}");
    assert!((b1 - 2.0).abs() < 0.05, "b1≈2, got {b1}");
    // Single control recovers the simple regression slope Cov(X,Y)/Var(X)=3.
    assert!((bsingle - 3.0).abs() < 0.05, "bsingle≈3, got {bsingle}");

    // Coefficients are population statistics: constant across realizations.
    assert!(r.elements["M/b0"].final_values.iter().all(|&v| (v - b0).abs() < 1e-12));
}
