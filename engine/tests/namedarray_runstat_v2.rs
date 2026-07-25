//! Phase-4 (NamedArray `Run`-as-axis) end-to-end: an across-realization reduction
//! of a plain model element that **feeds a downstream node**, with no submodel
//! wrapper — the exact §2 gap from ANALYTICA_ENGINE_GAP_ANALYSIS.md (Analytica's
//! `Probability(X > t)` feeding another variable). The engine runs two passes:
//! pass 1 collects `x`'s per-realization values, reduces them, and pass 2 injects
//! the scalar so `downstream` can consume it.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 4000, "seed": 7},
  "containers": [
    {"id": "M", "name": "M", "elements": ["M/x", "M/prob", "M/mean_x", "M/downstream"]}
  ],
  "elements": [
    {"id": "M/x", "name": "x", "primitive": "node", "value_rule": "sample", "container": "M",
     "distribution": {"family": "uniform", "parameters": {"min": {"value": 0, "unit": "1"}, "max": {"value": 10, "unit": "1"}}},
     "save_results": {"final_value": true}},

    {"id": "M/prob", "name": "prob", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_stat", "element_id": "M/x", "statistic": "exceedance",
        "arg": {"op": "literal", "value": 6}}},
     "save_results": {"final_value": true}},

    {"id": "M/mean_x", "name": "mean_x", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "run_stat", "element_id": "M/x", "statistic": "mean"}},
     "save_results": {"final_value": true}},

    {"id": "M/downstream", "name": "downstream", "primitive": "node", "value_rule": "expression", "container": "M",
     "inputs": ["M/prob"],
     "expression": {"ast": {"op": "multiply",
        "left": {"op": "ref", "element_id": "M/prob"},
        "right": {"op": "literal", "value": 1000}}},
     "save_results": {"final_value": true}}
  ]
}"#;

#[test]
fn run_stat_reduces_across_realizations_and_feeds_downstream() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let s = |id: &str| r.elements[id].final_values[0];

    // P(X > 6) for Uniform(0,10) = 0.4; a mid-graph across-realization reduction.
    let prob = s("M/prob");
    assert!((prob - 0.4).abs() < 0.03, "P(x>6) ≈ 0.4, got {prob}");
    // mean(X) = 5.
    assert!((s("M/mean_x") - 5.0).abs() < 0.15, "mean(x) ≈ 5, got {}", s("M/mean_x"));
    // The reduction FEEDS a downstream node (the §2 gap): downstream = prob * 1000.
    assert!((s("M/downstream") - prob * 1000.0).abs() < 1e-9,
        "downstream must consume the run_stat value");
    // The reduced scalar is constant across realizations (a population statistic).
    let probs = &r.elements["M/prob"].final_values;
    assert!(probs.iter().all(|&p| (p - prob).abs() < 1e-12), "run_stat is constant per realization");
}
