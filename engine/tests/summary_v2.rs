//! The model summary contract (consumed by the frontend): legacy `type` + v2
//! `primitive`/`value_rule`/`traits`/`editable`/`value`.

use serde_json::Value;
use wasim_engine::{normalize_v1, parse_v2, summary, WasimModel};

fn summ(model: &wasim_engine::ModelV2) -> Value {
    serde_json::from_str(&summary::summary_json(model)).unwrap()
}

fn elem<'a>(s: &'a Value, id: &str) -> &'a Value {
    s["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap()
}

fn traits(e: &Value) -> Vec<String> {
    e["traits"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect()
}

#[test]
fn summary_exposes_v2_primitives_rules_and_traits() {
    let m = parse_v2(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "k", "name": "K", "primitive": "node", "value_rule": "fixed", "value": {"value": 5, "unit": "kg"}, "editable": true},
          {"id": "rv", "name": "RV", "primitive": "node", "value_rule": "sample",
           "distribution": {"family": "normal", "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}}},
          {"id": "tank", "name": "Tank", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"},
           "rate": {"value": 1, "unit": "m3/d"}, "capacity": {"value": 10, "unit": "m3"}, "overflow_target": "k",
           "return_rate": {"value": 0.1, "unit": "1/d"}},
          {"id": "pipe", "name": "Pipe", "primitive": "link", "source": "tank", "target": "k",
           "rate": {"value": 1, "unit": "m3/d"}, "transit_time": {"value": 2, "unit": "d"}, "dispersion": {"value": 10, "unit": "1"}}
        ]}"#,
    )
    .unwrap();
    let s = summ(&m);

    let k = elem(&s, "k");
    assert_eq!(k["primitive"], "node");
    assert_eq!(k["value_rule"], "fixed");
    assert_eq!(k["type"], "constant"); // legacy mapping
    assert_eq!(k["editable"], true);
    assert_eq!(k["value"], 5.0);
    assert_eq!(k["unit"], "kg");

    let rv = elem(&s, "rv");
    assert_eq!(rv["value_rule"], "sample");
    assert_eq!(rv["type"], "random_variable");
    assert_eq!(rv["editable"], true);

    let tank = elem(&s, "tank");
    assert_eq!(tank["primitive"], "stock");
    let tt = traits(tank);
    for t in ["capacity_clamp", "overflow_routing", "compound_growth"] {
        assert!(tt.contains(&t.to_string()), "tank missing trait {t}: {tt:?}");
    }

    let pipe = elem(&s, "pipe");
    assert_eq!(pipe["primitive"], "link");
    let pt = traits(pipe);
    assert!(pt.contains(&"transit_buffer".to_string()) && pt.contains(&"transit_dispersion".to_string()), "{pt:?}");
}

#[test]
fn summary_preserves_legacy_type_for_v1_imports() {
    let v1: WasimModel = serde_json::from_str(
        r#"{"wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "c", "name": "C", "type": "constant", "value": {"value": 3, "unit": "kg"}, "editable": true},
          {"id": "acc", "name": "Acc", "type": "accumulator", "initial_value": {"value": 0, "unit": "kg"},
           "rate": {"ast": {"op": "literal", "value": 1.0}}}
        ]}"#,
    )
    .unwrap();
    let m = normalize_v1(&v1);
    let s = summ(&m);

    // Legacy type is preserved from the v1 source_type, primitive is the v2 mapping.
    let c = elem(&s, "c");
    assert_eq!(c["type"], "constant");
    assert_eq!(c["primitive"], "node");
    assert_eq!(c["editable"], true);

    let acc = elem(&s, "acc");
    assert_eq!(acc["type"], "accumulator"); // preserved, not "stock"
    assert_eq!(acc["primitive"], "stock");
}
