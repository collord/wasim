//! A6 lookup-table leftovers (§10): TBL_Derivative, log-result interpolation, monotone-cubic
//! (Fritsch-Carlson) no-overshoot, and 2-D/3-D multilinear tables.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// Run a single-realization model and return the final value of `id`.
fn eval(json: &str, id: &str) -> f64 {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    r.elements[id].final_values[0]
}

/// TBL_Derivative returns the slope of the bracketing segment. Table y = 2x over x∈[0,10]:
/// derivative is 2 everywhere in the interior.
#[test]
fn tbl_derivative_slope() {
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup",
         "table": {"x": [0, 5, 10], "y": [0, 10, 20], "x_unit": "1", "y_unit": "1"}},
        {"id": "deriv", "name": "D", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 3},
           "input2": {"op": "ref", "element_id": "TBL_Derivative"}}},
         "save_results": {"final_value": true}}
      ]}"#;
    assert!((eval(json, "deriv") - 2.0).abs() < 1e-9, "derivative of 2x should be 2");
}

/// TBL_Derivative on a table with a slope change: y goes 0→10 (slope 2) then 10→40 (slope 6).
#[test]
fn tbl_derivative_picks_the_right_segment() {
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup",
         "table": {"x": [0, 5, 10], "y": [0, 10, 40], "x_unit": "1", "y_unit": "1"}},
        {"id": "d_lo", "name": "Dlo", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 2}, "input2": {"op": "ref", "element_id": "TBL_Derivative"}}},
         "save_results": {"final_value": true}},
        {"id": "d_hi", "name": "Dhi", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 8}, "input2": {"op": "ref", "element_id": "TBL_Derivative"}}},
         "save_results": {"final_value": true}}
      ]}"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    assert!((r.elements["d_lo"].final_values[0] - 2.0).abs() < 1e-9, "first segment slope 2");
    assert!((r.elements["d_hi"].final_values[0] - 6.0).abs() < 1e-9, "second segment slope 6");
}

/// Log-result interpolation (interpolation: log_linear): interpolate ln(y). Table y = [1, 100]
/// at x = [0, 2]; at x=1 the log-interp midpoint is exp((ln1+ln100)/2) = 10, not the linear 50.5.
#[test]
fn log_result_interpolation() {
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup", "interpolation": "log_linear",
         "table": {"x": [0, 2], "y": [1, 100], "x_unit": "1", "y_unit": "1"}},
        {"id": "mid", "name": "M", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl", "input": {"op": "literal", "value": 1}}},
         "save_results": {"final_value": true}}
      ]}"#;
    assert!((eval(json, "mid") - 10.0).abs() < 1e-6, "log-interp midpoint of [1,100] should be 10");
}

/// Monotone cubic (spline) interpolation must NOT overshoot a monotone step in the data — the
/// defining Fritsch-Carlson property. A sharp riser [0,0,0,1,1,1] stays within [0,1] everywhere.
#[test]
fn monotone_cubic_no_overshoot() {
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup", "interpolation": "spline",
         "table": {"x": [0, 1, 2, 3, 4, 5], "y": [0, 0, 0, 1, 1, 1], "x_unit": "1", "y_unit": "1"}}
      ]}"#;
    // Probe many points; a natural cubic spline would ring below 0 / above 1 near the step.
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let _ = run_v2(&m, &g, &RunConfig::default()).unwrap();
    for i in 0..=500 {
        let x = i as f64 / 100.0; // 0..5
        let probe = format!(r#"{{"wasim_version": "0.9.2",
          "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "seed": 1}},
          "elements": [
            {{"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup", "interpolation": "spline",
             "table": {{"x": [0, 1, 2, 3, 4, 5], "y": [0, 0, 0, 1, 1, 1], "x_unit": "1", "y_unit": "1"}}}},
            {{"id": "p", "name": "P", "primitive": "node", "value_rule": "expression",
             "expression": {{"ast": {{"op": "lookup_call", "element_id": "tbl", "input": {{"op": "literal", "value": {x}}}}}}},
             "save_results": {{"final_value": true}}}}
          ]}}"#);
        let v = eval(&probe, "p");
        assert!(v >= -1e-9 && v <= 1.0 + 1e-9, "monotone cubic overshot at x={x}: y={v}");
    }
}

/// Monotone cubic is exact at the knots (Hermite interpolation passes through the data).
#[test]
fn monotone_cubic_hits_knots() {
    for (xk, yk) in [(1.0, 3.0), (2.0, 9.0), (4.0, 2.0)] {
        let probe = format!(r#"{{"wasim_version": "0.9.2",
          "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "seed": 1}},
          "elements": [
            {{"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup", "interpolation": "spline",
             "table": {{"x": [0, 1, 2, 4, 6], "y": [1, 3, 9, 2, 8], "x_unit": "1", "y_unit": "1"}}}},
            {{"id": "p", "name": "P", "primitive": "node", "value_rule": "expression",
             "expression": {{"ast": {{"op": "lookup_call", "element_id": "tbl", "input": {{"op": "literal", "value": {xk}}}}}}},
             "save_results": {{"final_value": true}}}}
          ]}}"#);
        assert!((eval(&probe, "p") - yk).abs() < 1e-9, "cubic should pass through knot ({xk},{yk})");
    }
}

/// 2-D bilinear table (emit `z = [axis2_bp, flat]`). Grid x=[0,10], a2=[0,10], values at the
/// four corners [[0, 10], [20, 30]] flattened row-major over (x, a2). Center (x=5, a2=5) = 15.
#[test]
fn bilinear_2d_table() {
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup",
         "table": {"x": [0, 10], "z": [[0, 10], [0, 10, 20, 30]], "x_unit": "1", "y_unit": "1"}},
        {"id": "center", "name": "C", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 5}, "input2": {"op": "literal", "value": 5}}},
         "save_results": {"final_value": true}},
        {"id": "corner", "name": "K", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 10}, "input2": {"op": "literal", "value": 0}}},
         "save_results": {"final_value": true}}
      ]}"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    // Corners: f(0,0)=0, f(0,10)=10, f(10,0)=20, f(10,10)=30. Center bilinear = 15.
    assert!((r.elements["center"].final_values[0] - 15.0).abs() < 1e-9, "bilinear center should be 15");
    assert!((r.elements["corner"].final_values[0] - 20.0).abs() < 1e-9, "f(10,0) corner should be 20");
}

/// 3-D corpus table (basictable.json `Three_Dimentional`): probe corners against the known flat
/// grid. x=[2.0,2.1,2.8,3.0], a2=[10,20], a3=[0,5,10]; flat row-major over (x,a2,a3).
/// Corner f(x=2.0, a2=10, a3=0) = flat[0] = 2.0; f(2.0,10,10)=flat[2]=5.0; f(2.0,20,0)=flat[3]=7.0.
#[test]
fn trilinear_3d_corners() {
    // The engine's multilinear takes a 2nd-axis coord via input2; a 3rd coord needs the emit
    // side to plumb an extra call arg (not yet produced). Verify the 2-D face at a3=0 (the
    // engine reads the 3rd axis's first slice when no 3rd coord is supplied — a3 clamps to its
    // low end). So this probes the a3=0 face: f(x, a2) over flat[.. :3 == 0].
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup",
         "table": {"x": [2.0, 2.1, 2.8, 3.0], "z": [[10, 20], [0, 5, 10],
           [2,3,5, 7,5.5,6, 13,15,5, 7,8,10, 9,12,17, 20,8,10, 11,13,12, 15,20,23]],
           "x_unit": "1", "y_unit": "1"}},
        {"id": "c1", "name": "C1", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 2.0}, "input2": {"op": "literal", "value": 10}}},
         "save_results": {"final_value": true}},
        {"id": "c2", "name": "C2", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 2.0}, "input2": {"op": "literal", "value": 20}}},
         "save_results": {"final_value": true}}
      ]}"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    // At (x=2.0, a2=10), a3 clamps to its low end (0) → flat[0] = 2.0.
    assert!((r.elements["c1"].final_values[0] - 2.0).abs() < 1e-9, "f(2.0,10,a3=0) should be 2.0, got {}", r.elements["c1"].final_values[0]);
    // At (x=2.0, a2=20), a3=0 → flat[3] = 7.0.
    assert!((r.elements["c2"].final_values[0] - 7.0).abs() < 1e-9, "f(2.0,20,a3=0) should be 7.0, got {}", r.elements["c2"].final_values[0]);
}
