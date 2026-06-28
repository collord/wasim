//! v2-native parser tests: hand-authored v2 fixtures lower into the clean model and,
//! where the engine already supports the rules, run end-to-end.

use wasim_engine::model_v2::{FilterStat, FixedValue, GateNode, NodeRule, Primitive};
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

// ── Structural lowering ───────────────────────────────────────────────────────

#[test]
fn parses_node_rules_stock_and_gate() {
    let json = r#"{
      "wasim_version": "0.8.0",
      "simulation_settings": {"duration": {"value": 10, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "k", "name": "K", "primitive": "node", "value_rule": "fixed", "value": {"value": 3.0, "unit": "kg"}},
        {"id": "arr", "name": "Arr", "primitive": "node", "value_rule": "fixed", "values": [1, 2, 3], "unit": "m"},
        {"id": "sig", "name": "Sig", "primitive": "node", "value_rule": "filter",
         "input": "k", "window": 4, "statistic": "mean"},
        {"id": "hy", "name": "Hy", "primitive": "node", "value_rule": "hysteresis",
         "input": "k", "high_threshold": {"value": 8, "unit": "1"}, "low_threshold": {"value": 2, "unit": "1"},
         "output_above": {"value": 1, "unit": "1"}, "output_below": {"value": 0, "unit": "1"}},
        {"id": "mk", "name": "Mk", "primitive": "node", "value_rule": "markov",
         "states": ["ok", "bad"], "initial_state": "ok",
         "transition_matrix": [[0.9, 0.1], [0.0, 1.0]], "output_values": [0.0, 1.0]},
        {"id": "ft", "name": "FT", "primitive": "node", "value_rule": "gate_logic", "semantics": "failure",
         "root": {"op": "or", "children": [
            {"op": "reference", "reference": "mk"},
            {"op": "n_vote", "threshold": 1, "children": [{"op": "input", "input": "k"}]}
         ]}},
        {"id": "tank", "name": "Tank", "primitive": "stock",
         "initial_value": {"value": 0, "unit": "m3"}, "rate": {"value": 1, "unit": "m3/d"},
         "capacity": {"value": 5, "unit": "m3"}, "floor": {"value": 0, "unit": "m3"}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse v2");
    assert!(!m.from_v1);
    assert_eq!(m.elements.len(), 7);

    let by = |id: &str| m.elements.iter().find(|e| e.id() == id).unwrap();

    match &by("arr").primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::Fixed { value: FixedValue::Array { values, unit }, .. } => {
                assert_eq!(values, &[1.0, 2.0, 3.0]);
                assert_eq!(unit, "m");
            }
            other => panic!("arr: {other:?}"),
        },
        _ => panic!("arr not node"),
    }

    match &by("sig").primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::Filter { window, statistic, .. } => {
                assert_eq!(*window, 4);
                assert!(matches!(statistic, FilterStat::Mean));
            }
            other => panic!("sig: {other:?}"),
        },
        _ => panic!(),
    }

    match &by("mk").primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::Markov { states, transition_matrix, output_values, .. } => {
                assert_eq!(states.len(), 2);
                assert_eq!(transition_matrix.len(), 2);
                assert_eq!(output_values, &[0.0, 1.0]);
            }
            other => panic!("mk: {other:?}"),
        },
        _ => panic!(),
    }

    match &by("ft").primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::GateLogic { root, .. } => match root {
                GateNode::Or(children) => assert_eq!(children.len(), 2),
                other => panic!("ft root: {other:?}"),
            },
            other => panic!("ft: {other:?}"),
        },
        _ => panic!(),
    }

    let stock = by("tank").as_stock().unwrap();
    assert!(stock.capacity.is_some());
    assert!(stock.floor.is_some());
}

#[test]
fn rejects_deferred_primitives() {
    let json = r#"{
      "wasim_version": "0.8.0",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [{"id": "p", "name": "P", "primitive": "cell", "volume": {"value": 1, "unit": "m3"}}]
    }"#;
    assert!(parse_v2(json).is_err(), "cell should be rejected until M4");
}

// ── End-to-end: v2-native parse → run ─────────────────────────────────────────

#[test]
fn v2_native_runs_with_capacity_clamp() {
    // four = two * 2 = 4; tank integrates +4/step from 0 with capacity 10 over 5 steps:
    // 4, 8, 12→10(clamped), 10, 10 → final 10.
    let json = r#"{
      "wasim_version": "0.8.0",
      "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "two", "name": "Two", "primitive": "node", "value_rule": "fixed", "value": {"value": 2, "unit": "1"}},
        {"id": "four", "name": "Four", "primitive": "node", "value_rule": "expression", "inputs": ["two"],
         "expression": {"ast": {"op": "multiply", "left": {"op": "ref", "element_id": "two"}, "right": {"op": "literal", "value": 2}}},
         "save_results": {"final_value": true}},
        {"id": "tank", "name": "Tank", "primitive": "stock", "inputs": ["four"],
         "initial_value": {"value": 0, "unit": "m3"},
         "rate": {"ast": {"op": "ref", "element_id": "four"}},
         "capacity": {"value": 10, "unit": "m3"},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    assert_eq!(r.elements["four"].final_values, vec![4.0]);
    assert_eq!(r.elements["tank"].final_values, vec![10.0], "capacity clamp to 10");
}
