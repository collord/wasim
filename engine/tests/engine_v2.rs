//! v2 engine validation: inline equivalence — deterministic and seeded-stochastic models
//! produce identical results through the v1 engine and the v2 engine (normalize → run_v2).
//!
//! A whole-corpus v1-vs-v2 equivalence test used to live here; it was retired when the
//! corpus went v2-native (the v1 `WasimModel` shape can no longer represent it). The v2
//! corpus is covered by the parse/run tests in `integration.rs`, `v2_parse.rs`, and the
//! frontend corpus smoke suites.

use wasim_engine::{
    normalize_v1, run, run_v2, ModelGraph, ModelGraphV2, RunConfig, SimulationResults, WasimModel,
};

fn load(json: &str) -> WasimModel {
    serde_json::from_str(json).expect("parse failed")
}

fn cfg() -> RunConfig {
    RunConfig { n_realizations: Some(4), seed: Some(12345), duration_override: None, timestep_override: None, results_spec: None, timebase: Default::default(), units: Default::default() }
}

/// Run a model through both engines with identical config.
fn run_both(model: &WasimModel) -> (SimulationResults, SimulationResults) {
    let g1 = ModelGraph::build(model).expect("v1 graph");
    let r1 = run(model, &g1, &cfg()).expect("v1 run");
    let m2 = normalize_v1(model);
    let g2 = ModelGraphV2::build(&m2).expect("v2 graph");
    let r2 = run_v2(&m2, &g2, &cfg()).expect("v2 run");
    (r1, r2)
}

fn close(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    if a == b {
        return true; // exact, including matching ±∞
    }
    (a - b).abs() <= 1e-9 + 1e-9 * a.abs().max(b.abs())
}

fn vecs_close(a: &[f64], b: &[f64]) -> Option<String> {
    if a.len() != b.len() {
        return Some(format!("len {} vs {}", a.len(), b.len()));
    }
    for (i, (&x, &y)) in a.iter().zip(b).enumerate() {
        if !close(x, y) {
            return Some(format!("[{i}] {x} vs {y}"));
        }
    }
    None
}

// ── Inline equivalence ────────────────────────────────────────────────────────

#[test]
fn v2_constant_expression_matches_v1() {
    let m = load(
        r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 1, "unit": "yr"}, "timestep": {"value": 1, "unit": "yr"}},
        "elements": [
            {"id": "a", "name": "A", "type": "constant", "value": {"value": 5.0, "unit": "1"}, "save_results": {"final_value": true}},
            {"id": "b", "name": "B", "type": "constant", "value": {"value": 3.0, "unit": "1"}},
            {"id": "c", "name": "C", "type": "expression", "inputs": ["a", "b"],
             "expression": {"ast": {"op": "add", "left": {"op": "ref", "element_id": "a"}, "right": {"op": "ref", "element_id": "b"}}},
             "save_results": {"final_value": true}}
        ]
    }"#,
    );
    let (r1, r2) = run_both(&m);
    assert_eq!(r2.elements["c"].final_values, vec![8.0; 4]);
    assert!(vecs_close(&r1.elements["c"].final_values, &r2.elements["c"].final_values).is_none());
}

#[test]
fn v2_accumulator_matches_v1() {
    // Stock integrating a constant rate of 2/step from 10, over 5 steps.
    let m = load(
        r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [{
            "id": "s", "name": "S", "type": "accumulator",
            "initial_value": {"value": 10.0, "unit": "m3"},
            "rate": {"ast": {"op": "literal", "value": 2.0}},
            "save_results": {"final_value": true, "time_history": true}
        }]
    }"#,
    );
    let (r1, r2) = run_both(&m);
    let m1 = vecs_close(&r1.elements["s"].final_values, &r2.elements["s"].final_values);
    assert!(m1.is_none(), "final mismatch: {m1:?}");
    let h1 = r1.elements["s"].time_history.as_ref().unwrap();
    let h2 = r2.elements["s"].time_history.as_ref().unwrap();
    assert!(vecs_close(&h1.mean, &h2.mean).is_none(), "history mean mismatch");
}

#[test]
fn v2_seeded_random_matches_v1() {
    // Same seed + same element order ⇒ identical draws through both engines.
    let m = load(
        r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 1, "unit": "yr"}, "timestep": {"value": 1, "unit": "yr"}, "n_realizations": 64, "seed": 7},
        "elements": [
            {"id": "x", "name": "X", "type": "random_variable",
             "distribution": {"family": "normal", "parameters": {"mean": {"value": 10, "unit": "1"}, "stddev": {"value": 2, "unit": "1"}}},
             "save_results": {"final_value": true}},
            {"id": "y", "name": "Y", "type": "expression", "inputs": ["x"],
             "expression": {"ast": {"op": "multiply", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 3.0}}},
             "save_results": {"final_value": true}}
        ]
    }"#,
    );
    // Use the model's own seed/realizations here (not the shared cfg) for a fuller sample.
    let g1 = ModelGraph::build(&m).unwrap();
    let r1 = run(&m, &g1, &RunConfig::default()).unwrap();
    let m2 = normalize_v1(&m);
    let g2 = ModelGraphV2::build(&m2).unwrap();
    let r2 = run_v2(&m2, &g2, &RunConfig::default()).unwrap();
    assert!(vecs_close(&r1.elements["x"].final_values, &r2.elements["x"].final_values).is_none(), "x draws diverged");
    assert!(vecs_close(&r1.elements["y"].final_values, &r2.elements["y"].final_values).is_none(), "y diverged");
}

#[test]
fn v2_chained_lag_is_exact_delay() {
    // src steps 0,1,2,3,4; a 2-step delay (lag=2,dt=1) → chained lags emit src[t-2].
    let m = load(
        r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
            {"id": "t", "name": "T", "type": "expression",
             "expression": {"ast": {"op": "time_ref", "property": "elapsed"}}, "save_results": {"time_history": true}},
            {"id": "d", "name": "D", "type": "delay", "input": "t", "lag": {"value": 2.0, "unit": "d"},
             "initial": {"value": -1.0, "unit": "d"}, "save_results": {"time_history": true}}
        ]
    }"#,
    );
    let m2 = normalize_v1(&m);
    let g2 = ModelGraphV2::build(&m2).unwrap();
    let r2 = run_v2(&m2, &g2, &RunConfig::default()).unwrap();
    // elapsed = [0,1,2,3,4]; 2-step delay with initial -1 → [-1,-1,0,1,2].
    let d = &r2.elements["d"].time_history.as_ref().unwrap().mean;
    assert!(vecs_close(d, &[-1.0, -1.0, 0.0, 1.0, 2.0]).is_none(), "got {d:?}");
}
