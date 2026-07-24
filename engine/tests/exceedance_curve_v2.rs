//! Phase-4 (sweep composition): the **exceedance curve** — the Analytica-critical deliverable
//! `Probability(loss > threshold)` evaluated across a threshold grid, feeding a downstream node.
//!
//! The design decision this test validates: the curve needs **no new AST node**. It composes
//! from pieces that already exist after Phase 3 —
//!   - `submodel_stat` with the `exceedance` reducer (Phase 3), whose `arg` is the threshold;
//!   - `vector_map` over a threshold dimension (the array executor), which pushes the 1-based
//!     member index so the body can pick `threshold[k]` and emit one exceedance per threshold;
//!   - `#k` member expansion, so the resulting `Value::Vector` reaches the results surface.
//!
//! So the "vector-valued boundary reduction" is authoring, not engine work. This test is the
//! proof it composes and emits the correct curve.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// Submodel `S` emits a constant `loss = 5.0` over N realizations (so the sample vector is
/// deterministic without depending on RNG). The parent builds an exceedance curve over the
/// threshold grid [2, 5, 8]:
///   exceedance(loss > 2) = 1.0   (all 5s exceed 2)
///   exceedance(loss > 5) = 0.0   (strict > excludes the 5s)
///   exceedance(loss > 8) = 0.0
/// so the curve is [1, 0, 0], readable member-by-member as curve#1..#3.
const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 8, "seed": 1},
  "dimensions": [{"id": "Thr", "name": "Thr", "size": 3}],
  "containers": [
    {"id": "Model", "name": "Model", "children": ["Model/S"], "elements": ["Model/thresholds", "Model/curve"]},
    {"id": "Model/S", "name": "S", "parent": "Model", "kind": "submodel",
     "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 8},
     "interface": {"inputs": [], "outputs": ["Model/S/loss"]}, "elements": ["Model/S/loss"]}
  ],
  "elements": [
    {"id": "Model/S/loss", "name": "loss", "primitive": "node", "value_rule": "fixed",
     "container": "Model/S", "value": {"value": 5, "unit": "1"}, "save_results": {"final_value": true}},

    {"id": "Model/thresholds", "name": "thresholds", "primitive": "node", "value_rule": "fixed",
     "container": "Model", "values": [2, 5, 8], "unit": "1",
     "outputs": [{"name": "thresholds", "unit": "1", "dimensions": ["Thr"]}]},

    {"id": "Model/curve", "name": "curve", "primitive": "node", "value_rule": "expression",
     "container": "Model", "inputs": ["Model/S", "Model/thresholds"],
     "outputs": [{"name": "curve", "unit": "1", "dimensions": ["Thr"]}],
     "expression": {"ast": {"op": "vector_map", "over": "Thr",
       "body": {"op": "submodel_stat", "submodel_id": "Model/S", "output": "Model/S/loss",
                "statistic": "exceedance",
                "arg": {"op": "index", "array": {"op": "ref", "element_id": "Model/thresholds"},
                        "indices": [{"op": "index_ref", "axis": "row"}]}}}},
     "save_results": {"final_value": true}}
  ]
}"#;

#[test]
fn exceedance_curve_composes_from_vector_map_over_submodel_stat() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // The curve is a Value::Vector reaching results as per-member series curve#1..#3.
    let c1 = &r.elements["Model/curve#1"].final_values;
    let c2 = &r.elements["Model/curve#2"].final_values;
    let c3 = &r.elements["Model/curve#3"].final_values;

    // Every realization computes the same curve (loss is constant), so [0] is representative.
    assert_eq!(c1[0], 1.0, "P(loss > 2) = 1.0 — all losses exceed 2");
    assert_eq!(c2[0], 0.0, "P(loss > 5) = 0.0 — strict > excludes the 5s");
    assert_eq!(c3[0], 0.0, "P(loss > 8) = 0.0 — nothing exceeds 8");
}

/// The real-use-case shape: a submodel with an *actual* distribution (uniform 0..10 over many
/// realizations) yields a genuine **monotone non-increasing** exceedance curve across an
/// ascending threshold grid — P(X > t) can only fall as t rises. This is what every
/// risk-analysis exceedance/loss curve looks like; the constant test above only proves the
/// wiring, this proves the curve is meaningful.
const MODEL_DISTRIBUTION: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 2000, "seed": 7},
  "dimensions": [{"id": "Thr", "name": "Thr", "size": 5}],
  "containers": [
    {"id": "Model", "name": "Model", "children": ["Model/S"], "elements": ["Model/thresholds", "Model/curve"]},
    {"id": "Model/S", "name": "S", "parent": "Model", "kind": "submodel",
     "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 2000},
     "interface": {"inputs": [], "outputs": ["Model/S/loss"]}, "elements": ["Model/S/loss"]}
  ],
  "elements": [
    {"id": "Model/S/loss", "name": "loss", "primitive": "node", "value_rule": "sample",
     "container": "Model/S",
     "distribution": {"family": "uniform", "parameters": {"min": {"value": 0, "unit": "1"}, "max": {"value": 10, "unit": "1"}}},
     "save_results": {"final_value": true}},
    {"id": "Model/thresholds", "name": "thresholds", "primitive": "node", "value_rule": "fixed",
     "container": "Model", "values": [0, 2.5, 5, 7.5, 10], "unit": "1",
     "outputs": [{"name": "thresholds", "unit": "1", "dimensions": ["Thr"]}]},
    {"id": "Model/curve", "name": "curve", "primitive": "node", "value_rule": "expression",
     "container": "Model", "inputs": ["Model/S", "Model/thresholds"],
     "outputs": [{"name": "curve", "unit": "1", "dimensions": ["Thr"]}],
     "expression": {"ast": {"op": "vector_map", "over": "Thr",
       "body": {"op": "submodel_stat", "submodel_id": "Model/S", "output": "Model/S/loss",
                "statistic": "exceedance",
                "arg": {"op": "index", "array": {"op": "ref", "element_id": "Model/thresholds"},
                        "indices": [{"op": "index_ref", "axis": "row"}]}}}},
     "save_results": {"final_value": true}}
  ]
}"#;

#[test]
fn exceedance_curve_is_monotone_for_a_real_distribution() {
    let m = parse_v2(MODEL_DISTRIBUTION).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    let curve: Vec<f64> = (1..=5)
        .map(|k| r.elements[&format!("Model/curve#{k}")].final_values[0])
        .collect();

    // Uniform(0,10): P(X > t) ≈ (10 − t)/10 → curve ≈ [1.0, 0.75, 0.5, 0.25, 0.0].
    assert!((curve[0] - 1.0).abs() < 1e-9, "P(X>0)=1 exactly, got {}", curve[0]);
    assert_eq!(curve[4], 0.0, "P(X>10)=0 exactly");
    // Monotone non-increasing across ascending thresholds (the defining property).
    for k in 1..5 {
        assert!(curve[k] <= curve[k - 1] + 1e-12,
            "exceedance curve must be non-increasing: curve[{k}]={} > curve[{}]={}", curve[k], k - 1, curve[k - 1]);
    }
    // Sampling accuracy: interior points within a few % of the analytic (10−t)/10 at n=2000.
    for (k, expected) in [(1, 0.75), (2, 0.50), (3, 0.25)] {
        assert!((curve[k] - expected).abs() < 0.05,
            "P(X>{}) ≈ {expected}, got {} (n=2000)", 2.5 * k as f64, curve[k]);
    }
}
