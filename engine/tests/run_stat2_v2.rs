//! run_stat2 (bivariate cross-realization reducer) end-to-end, on BOTH the scalar
//! two-pass lane and the fused array lane (Phase 2). Model: X~N(0,1), W~N(0,1)
//! independent, Y = 2X + W. Then Cov(X,Y)=2, Var(X)=1 ⇒ beta=2, Var(Y)=5 ⇒
//! corr = 2/sqrt(5) ≈ 0.8944. A downstream node consumes beta, proving the
//! bivariate reduction feeds the graph like run_stat.

use wasim_engine::{array_lane, parse_v2, run_v2, ModelGraphV2, RunConfig};

const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 8000, "seed": 21},
  "containers": [
    {"id": "M", "name": "M", "elements": ["M/x", "M/w", "M/y", "M/beta", "M/cov", "M/corr", "M/downstream"]}
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

    {"id": "M/beta", "name": "beta", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_stat2", "x": "M/x", "y": "M/y", "statistic": "beta"}},
     "save_results": {"final_value": true}},

    {"id": "M/cov", "name": "cov", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_stat2", "x": "M/x", "y": "M/y", "statistic": "cov"}},
     "save_results": {"final_value": true}},

    {"id": "M/corr", "name": "corr", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_stat2", "x": "M/x", "y": "M/y", "statistic": "corr"}},
     "save_results": {"final_value": true}},

    {"id": "M/downstream", "name": "downstream", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "multiply",
        "left": {"op": "run_stat2", "x": "M/x", "y": "M/y", "statistic": "beta"},
        "right": {"op": "literal", "value": 10}}},
     "save_results": {"final_value": true}}
  ]
}"#;

fn run_lane(lane: bool, id: &str) -> f64 {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { array_lane: lane, ..Default::default() };
    let r = run_v2(&m, &g, &cfg).expect("run");
    r.elements[id].final_values[0]
}

#[test]
fn run_stat2_correct_and_lane_agnostic() {
    let m = parse_v2(MODEL).expect("parse");
    assert!(array_lane::eligible(&m).is_ok(), "flat MC + run_stat2 should be array-lane eligible");

    for &lane in &[false, true] {
        let tag = if lane { "array" } else { "scalar" };
        let beta = run_lane(lane, "M/beta");
        let cov = run_lane(lane, "M/cov");
        let corr = run_lane(lane, "M/corr");
        let downstream = run_lane(lane, "M/downstream");
        println!("[{tag}] beta={beta:.6} cov={cov:.6} corr={corr:.6}");

        assert!((beta - 2.0).abs() < 0.05, "[{tag}] beta≈2, got {beta}");
        assert!((cov - 2.0).abs() < 0.08, "[{tag}] cov≈2, got {cov}");
        assert!((corr - 2.0 / 5.0_f64.sqrt()).abs() < 0.02, "[{tag}] corr≈0.894, got {corr}");
        // The reduction feeds a downstream node.
        assert!((downstream - beta * 10.0).abs() < 1e-9, "[{tag}] downstream must consume beta");
    }

    // The two lanes must agree (array lane is bit-identical to the scalar lane by design).
    for id in ["M/beta", "M/cov", "M/corr"] {
        let (s, a) = (run_lane(false, id), run_lane(true, id));
        assert!((s - a).abs() < 1e-9, "{id}: scalar {s} vs array {a} disagree");
    }
}
