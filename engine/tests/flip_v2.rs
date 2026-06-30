//! The canonical entry points route all input through the v2 engine core:
//! `simulate` (v1 model → normalize → v2) and `simulate_json` (format-detecting).
//! v2-native models are cycle-rejected; v1-imported models warn-and-skip.

use wasim_engine::{simulate, simulate_json, ModelGraphV2, RunConfig, WasimModel};

#[test]
fn simulate_runs_v1_model_through_v2_core() {
    let json = r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 1, "unit": "yr"}, "timestep": {"value": 1, "unit": "yr"}},
        "elements": [
            {"id": "a", "name": "A", "type": "constant", "value": {"value": 5.0, "unit": "1"}, "save_results": {"final_value": true}},
            {"id": "b", "name": "B", "type": "constant", "value": {"value": 3.0, "unit": "1"}},
            {"id": "c", "name": "C", "type": "expression", "inputs": ["a", "b"],
             "expression": {"ast": {"op": "add", "left": {"op": "ref", "element_id": "a"}, "right": {"op": "ref", "element_id": "b"}}},
             "save_results": {"final_value": true}}
        ]
    }"#;
    let model: WasimModel = serde_json::from_str(json).unwrap();
    let r = simulate(&model, &RunConfig::default()).unwrap();
    assert_eq!(r.elements["c"].final_values, vec![8.0]);
}

#[test]
fn simulate_json_detects_v2_native() {
    let json = r#"{
        "wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
            {"id": "k", "name": "K", "primitive": "node", "value_rule": "fixed", "value": {"value": 5, "unit": "1"}},
            {"id": "d", "name": "D", "primitive": "node", "value_rule": "expression", "inputs": ["k"],
             "expression": {"ast": {"op": "multiply", "left": {"op": "ref", "element_id": "k"}, "right": {"op": "literal", "value": 2}}},
             "save_results": {"final_value": true}}
        ]
    }"#;
    let r = simulate_json(json, &RunConfig::default()).unwrap();
    assert_eq!(r.elements["d"].final_values, vec![10.0]);
}

#[test]
fn simulate_json_detects_v1() {
    let json = r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 1, "unit": "yr"}, "timestep": {"value": 1, "unit": "yr"}},
        "elements": [{"id": "a", "name": "A", "type": "constant", "value": {"value": 7.0, "unit": "1"}, "save_results": {"final_value": true}}]
    }"#;
    let r = simulate_json(json, &RunConfig::default()).unwrap();
    assert_eq!(r.elements["a"].final_values, vec![7.0]);
}

#[test]
fn v2_native_cycle_is_rejected() {
    // a → b → a (no lag): a v2-native model must be rejected at graph build.
    let json = r#"{
        "wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
            {"id": "a", "name": "A", "primitive": "node", "value_rule": "expression", "inputs": ["b"],
             "expression": {"ast": {"op": "ref", "element_id": "b"}}},
            {"id": "b", "name": "B", "primitive": "node", "value_rule": "expression", "inputs": ["a"],
             "expression": {"ast": {"op": "ref", "element_id": "a"}}}
        ]
    }"#;
    let m = wasim_engine::parse_v2(json).unwrap();
    assert!(!m.from_v1);
    assert!(ModelGraphV2::build(&m).is_err(), "v2-native cycle should be rejected");
    assert!(simulate_json(json, &RunConfig::default()).is_err());
}
