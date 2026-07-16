//! Runtime sensitivity-analysis tests (SENSITIVITY_ANALYSIS_SPEC.md).

use wasim_engine::{parse_v2, sensitivity, RunConfig, SensitivitySpec};

/// Linear model y = 2·x + 3. Sweeping x ∈ [0,10] in 5 steps must give the exact line
/// [(0,3),(2.5,8),(5,13),(7.5,18),(10,23)].
#[test]
fn one_at_a_time_reproduces_known_line() {
    let json = r#"{
      "wasim_version": "0.8.5",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "1"}},
        {"id": "y", "name": "Y", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "add",
           "left": {"op": "multiply", "left": {"op": "literal", "value": 2}, "right": {"op": "ref", "element_id": "x"}},
           "right": {"op": "literal", "value": 3}}},
         "save_results": {"final_value": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse");

    let spec: SensitivitySpec = serde_json::from_str(
        r#"{
          "result": {"element_id": "y"},
          "method": "one_at_a_time",
          "variables": [{"element_id": "x", "lower": 0, "upper": 10, "base": 1, "steps": 5}]
        }"#,
    )
    .expect("spec");

    let r = sensitivity(&m, &spec, &RunConfig::default()).expect("run");
    assert_eq!(r.curves.len(), 1);
    let pts = &r.curves[0].points;
    let expected = [(0.0, 3.0), (2.5, 8.0), (5.0, 13.0), (7.5, 18.0), (10.0, 23.0)];
    assert_eq!(pts.len(), expected.len());
    for (p, (ei, er)) in pts.iter().zip(expected) {
        assert!((p.input - ei).abs() < 1e-9, "input {} != {ei}", p.input);
        assert!((p.result - er).abs() < 1e-9, "result {} != {er}", p.result);
    }
    // base = 1 → y = 5.
    assert!((r.base_result - 5.0).abs() < 1e-9, "base {}", r.base_result);
    assert!(r.tornado.is_empty());
}

/// Tornado over z = 3·a + 1·b. With equal ranges, `a` (coefficient 3) must outrank `b`
/// (coefficient 1) and appear first.
#[test]
fn tornado_ranks_by_influence() {
    let json = r#"{
      "wasim_version": "0.8.5",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "a", "name": "A", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "1"}},
        {"id": "b", "name": "B", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "1"}},
        {"id": "z", "name": "Z", "primitive": "node", "value_rule": "expression", "inputs": ["a", "b"],
         "expression": {"ast": {"op": "add",
           "left": {"op": "multiply", "left": {"op": "literal", "value": 3}, "right": {"op": "ref", "element_id": "a"}},
           "right": {"op": "ref", "element_id": "b"}}},
         "save_results": {"final_value": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse");

    let spec: SensitivitySpec = serde_json::from_str(
        r#"{
          "result": {"element_id": "z"},
          "method": "tornado",
          "variables": [
            {"element_id": "a", "lower": 0, "upper": 10, "base": 1},
            {"element_id": "b", "lower": 0, "upper": 10, "base": 1}
          ]
        }"#,
    )
    .expect("spec");

    let r = sensitivity(&m, &spec, &RunConfig::default()).expect("run");
    assert!(r.curves.is_empty());
    assert_eq!(r.tornado.len(), 2);
    // Sorted by descending swing → a first.
    assert_eq!(r.tornado[0].element_id, "a");
    assert!((r.tornado[0].swing - 30.0).abs() < 1e-9, "a swing {}", r.tornado[0].swing);
    assert_eq!(r.tornado[1].element_id, "b");
    assert!((r.tornado[1].swing - 10.0).abs() < 1e-9, "b swing {}", r.tornado[1].swing);
}

/// A probabilistic target reduced by a Monte-Carlo statistic sweeps per point: the mean of
/// `x + noise` (noise ~ Normal(0, small)) tracks x across the sweep.
#[test]
fn probabilistic_result_reduces_per_point() {
    let json = r#"{
      "wasim_version": "0.8.5",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 7},
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "1"}},
        {"id": "noise", "name": "Noise", "primitive": "node", "value_rule": "sample",
         "distribution": {"family": "normal", "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 0.001, "unit": "1"}}}},
        {"id": "out", "name": "Out", "primitive": "node", "value_rule": "expression", "inputs": ["x", "noise"],
         "expression": {"ast": {"op": "add",
           "left": {"op": "ref", "element_id": "x"}, "right": {"op": "ref", "element_id": "noise"}}},
         "save_results": {"final_value": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse");

    let spec: SensitivitySpec = serde_json::from_str(
        r#"{
          "result": {"element_id": "out", "statistic": {"kind": "mean"}},
          "method": "one_at_a_time",
          "variables": [{"element_id": "x", "lower": 0, "upper": 4, "base": 0, "steps": 5}]
        }"#,
    )
    .expect("spec");

    let cfg = RunConfig { n_realizations: Some(500), ..RunConfig::default() };
    let r = sensitivity(&m, &spec, &cfg).expect("run");
    let pts = &r.curves[0].points;
    assert_eq!(pts.len(), 5);
    // mean(out) ≈ x (noise mean ≈ 0). The curve must be monotone and track the input.
    for p in pts {
        assert!((p.result - p.input).abs() < 0.05, "mean {} not ≈ input {}", p.result, p.input);
    }
    assert!(pts[4].result > pts[0].result + 3.0, "curve should rise with x");
}

/// A missing target element is a hard error, not a silent flat curve.
#[test]
fn missing_target_errors() {
    let json = r#"{
      "wasim_version": "0.8.5",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "1"}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse");
    let spec: SensitivitySpec = serde_json::from_str(
        r#"{"result": {"element_id": "nope"}, "method": "one_at_a_time",
            "variables": [{"element_id": "x", "lower": 0, "upper": 1, "base": 0, "steps": 3}]}"#,
    )
    .expect("spec");
    assert!(sensitivity(&m, &spec, &RunConfig::default()).is_err());
}
