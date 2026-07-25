//! Array lane (Phase A) tests. The lane must produce **bit-identical** results to
//! the scalar lane on eligible models (it mirrors the scalar per-realization draws
//! and reuses the same reducers), and eligibility must reject anything outside the
//! flat-Monte-Carlo subset (so the caller falls back to the scalar lane).

use wasim_engine::{array_lane, parse_v2, run_v2, ModelGraphV2, RunConfig};

fn run(json: &str, lane: bool) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { n_realizations: Some(600), array_lane: lane, ..Default::default() };
    run_v2(&m, &g, &cfg).expect("run")
}

fn assert_bit_identical(a: &wasim_engine::SimulationResults, b: &wasim_engine::SimulationResults) {
    for (id, ea) in &a.elements {
        let eb = b.elements.get(id).unwrap_or_else(|| panic!("array lane missing element {id}"));
        assert_eq!(ea.final_values, eb.final_values, "element {id}: array lane != scalar lane");
    }
}

// Flat Monte-Carlo with a mid-graph run_stat feeding downstream — the §2 shape.
const RUNSTAT: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 600, "seed": 7},
  "containers": [{"id": "M", "name": "M", "elements": ["M/x","M/y","M/prob","M/mean_x","M/downstream"]}],
  "elements": [
    {"id": "M/x","name":"x","primitive":"node","value_rule":"sample","container":"M",
     "distribution": {"family":"uniform","parameters":{"min":{"value":0,"unit":"1"},"max":{"value":10,"unit":"1"}}},"save_results":{"final_value":true}},
    {"id": "M/y","name":"y","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
     "expression": {"ast": {"op":"add","left":{"op":"multiply","left":{"op":"ref","element_id":"M/x"},"right":{"op":"literal","value":2}},"right":{"op":"literal","value":1}}},"save_results":{"final_value":true}},
    {"id": "M/prob","name":"prob","primitive":"node","value_rule":"expression","container":"M",
     "expression": {"ast": {"op":"run_stat","element_id":"M/x","statistic":"exceedance","arg":{"op":"literal","value":6}}},"save_results":{"final_value":true}},
    {"id": "M/mean_x","name":"mean_x","primitive":"node","value_rule":"expression","container":"M",
     "expression": {"ast": {"op":"run_stat","element_id":"M/x","statistic":"mean"}},"save_results":{"final_value":true}},
    {"id": "M/downstream","name":"downstream","primitive":"node","value_rule":"expression","container":"M","inputs":["M/prob"],
     "expression": {"ast": {"op":"multiply","left":{"op":"ref","element_id":"M/prob"},"right":{"op":"literal","value":1000}}},"save_results":{"final_value":true}}
  ]
}"#;

#[test]
fn array_lane_matches_scalar_lane_bit_for_bit() {
    let scalar = run(RUNSTAT, false);
    let array = run(RUNSTAT, true);
    assert_bit_identical(&scalar, &array);

    // sanity on the actual values: P(x>6) ≈ 0.4, mean ≈ 5, downstream = prob*1000
    let p = array.elements["M/prob"].final_values[0];
    assert!((p - 0.4).abs() < 0.04, "P(x>6) ≈ 0.4, got {p}");
    assert!((array.elements["M/mean_x"].final_values[0] - 5.0).abs() < 0.3);
    assert!((array.elements["M/downstream"].final_values[0] - p * 1000.0).abs() < 1e-9);
}

// A pure-arithmetic-over-samples model (no run_stat) — single columnar pass.
const ARITH: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 600, "seed": 3},
  "containers": [{"id": "M", "name": "M", "elements": ["M/a","M/b","M/k","M/z"]}],
  "elements": [
    {"id":"M/a","name":"a","primitive":"node","value_rule":"sample","container":"M","distribution":{"family":"normal","parameters":{"mean":{"value":10,"unit":"1"},"stddev":{"value":2,"unit":"1"}}},"save_results":{"final_value":true}},
    {"id":"M/b","name":"b","primitive":"node","value_rule":"sample","container":"M","distribution":{"family":"uniform","parameters":{"min":{"value":1,"unit":"1"},"max":{"value":3,"unit":"1"}}},"save_results":{"final_value":true}},
    {"id":"M/k","name":"k","primitive":"node","value_rule":"fixed","container":"M","value":{"value":100,"unit":"1"},"save_results":{"final_value":true}},
    {"id":"M/z","name":"z","primitive":"node","value_rule":"expression","container":"M","inputs":["M/a","M/b","M/k"],
     "expression":{"ast":{"op":"if","cond":{"op":"gt","left":{"op":"ref","element_id":"M/a"},"right":{"op":"literal","value":9}},
        "then":{"op":"add","left":{"op":"divide","left":{"op":"ref","element_id":"M/k"},"right":{"op":"ref","element_id":"M/b"}},"right":{"op":"ref","element_id":"M/a"}},
        "else":{"op":"literal","value":0}}},"save_results":{"final_value":true}}
  ]
}"#;

#[test]
fn array_lane_arithmetic_matches_scalar() {
    assert_bit_identical(&run(ARITH, false), &run(ARITH, true));
}

#[test]
fn eligibility_accepts_and_rejects() {
    let m = parse_v2(RUNSTAT).unwrap();
    assert!(array_lane::eligible(&m).is_ok(), "flat MC + run_stat should be eligible");

    // A model with a dimension is NOT eligible → the flag falls back to scalar.
    let dim_model = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 10, "seed": 1},
      "dimensions": [{"id":"D","name":"D","size":2}],
      "containers": [{"id":"M","name":"M","elements":["M/v"]}],
      "elements": [{"id":"M/v","name":"v","primitive":"node","value_rule":"expression","container":"M",
        "expression":{"ast":{"op":"vector_map","over":"D","body":{"op":"index_ref","axis":"row"}}},
        "outputs":[{"name":"v","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}}]
    }"#;
    let dm = parse_v2(dim_model).unwrap();
    assert!(array_lane::eligible(&dm).is_err(), "dimensioned model must be rejected");
    // and it still runs (fallback to scalar) with the flag on:
    let g = ModelGraphV2::build(&dm).unwrap();
    let cfg = RunConfig { array_lane: true, ..Default::default() };
    assert!(run_v2(&dm, &g, &cfg).is_ok(), "ineligible model with flag on must fall back and run");
}
