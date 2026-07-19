//! Reserved global identifiers (§1b), TBL_* lookup modes (§1b), and stock output
//! ports / output-qualified refs (§1c) — the 0.9.2 detention-pond-parity round.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn run_json(json: &str) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run")
}

fn final_of(r: &wasim_engine::SimulationResults, id: &str) -> f64 {
    r.elements.get(id).expect("element saved").final_values[0]
}

/// gee / TimestepLength / SimDuration resolve as reserved globals (SI seconds), not 0.0.
#[test]
fn reserved_globals_resolve() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 4, "unit": "d"}, "timestep": {"value": 2, "unit": "d"}},
      "elements": [
        {"id": "g", "name": "G", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "ref", "element_id": "gee"}}},
        {"id": "ts", "name": "Ts", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "ref", "element_id": "TimestepLength"}}},
        {"id": "dur", "name": "Dur", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "ref", "element_id": "SimDuration"}}}
      ]
    }"#;
    let r = run_json(json);
    assert!((final_of(&r, "g") - 9.80665).abs() < 1e-9);
    assert!((final_of(&r, "ts") - 172800.0).abs() < 1e-6, "2 d in seconds");
    assert!((final_of(&r, "dur") - 345600.0).abs() < 1e-6, "4 d in seconds");
}

/// Realization is the 1-based realization index.
#[test]
fn realization_global_is_one_based_index() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 3},
      "elements": [
        {"id": "r", "name": "R", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "ref", "element_id": "Realization"}}}
      ]
    }"#;
    let r = run_json(json);
    assert_eq!(r.elements["r"].final_values, vec![1.0, 2.0, 3.0]);
}

/// A model element with a reserved name shadows the global.
#[test]
fn model_element_shadows_reserved_global() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "gee", "name": "gee", "primitive": "node", "value_rule": "fixed", "value": {"value": 1.5, "unit": "1"}},
        {"id": "g", "name": "G", "primitive": "node", "value_rule": "expression", "inputs": ["gee"],
         "expression": {"ast": {"op": "ref", "element_id": "gee"}}}
      ]
    }"#;
    let r = run_json(json);
    assert!((final_of(&r, "g") - 1.5).abs() < 1e-12);
}

/// TBL_* second-argument modes: integral, inverse, and inverse-of-integral.
/// Table: x = [0,1,2], y = [0,10,20] (so ∫y dx = 5x² on [0,1] … full integral 20).
#[test]
fn tbl_lookup_modes() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "tbl", "name": "Tbl", "primitive": "node", "value_rule": "lookup",
         "table": {"x": [0.0, 1.0, 2.0], "y": [0.0, 10.0, 20.0], "x_unit": "1", "y_unit": "1"},
         "interpolation": "linear"},
        {"id": "integ", "name": "I", "primitive": "node", "value_rule": "expression", "inputs": ["tbl"],
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 1.0},
           "input2": {"op": "ref", "element_id": "TBL_Integral"}}}},
        {"id": "integ_full", "name": "If", "primitive": "node", "value_rule": "expression", "inputs": ["tbl"],
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 99.0},
           "input2": {"op": "ref", "element_id": "TBL_Integral"}}}},
        {"id": "inv", "name": "Inv", "primitive": "node", "value_rule": "expression", "inputs": ["tbl"],
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 15.0},
           "input2": {"op": "ref", "element_id": "TBL_Inverse"}}}},
        {"id": "invint_mid", "name": "Vm", "primitive": "node", "value_rule": "expression", "inputs": ["tbl"],
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 1.25},
           "input2": {"op": "ref", "element_id": "TBL_Inv_Integral"}}}},
        {"id": "invint_knot", "name": "Vk", "primitive": "node", "value_rule": "expression", "inputs": ["tbl"],
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 5.0},
           "input2": {"op": "ref", "element_id": "TBL_Inv_Integral"}}}},
        {"id": "invint_over", "name": "Vo", "primitive": "node", "value_rule": "expression", "inputs": ["tbl"],
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 1000.0},
           "input2": {"op": "ref", "element_id": "TBL_Inv_Integral"}}}}
      ]
    }"#;
    let r = run_json(json);
    assert!((final_of(&r, "integ") - 5.0).abs() < 1e-9, "∫ to x=1 is 5");
    assert!((final_of(&r, "integ_full") - 20.0).abs() < 1e-9, "above range clamps to full integral");
    assert!((final_of(&r, "inv") - 1.5).abs() < 1e-9, "y=15 inverts to x=1.5");
    assert!((final_of(&r, "invint_mid") - 0.5).abs() < 1e-9, "5x²=1.25 → x=0.5");
    assert!((final_of(&r, "invint_knot") - 1.0).abs() < 1e-9, "v=5 → knot x=1");
    assert!((final_of(&r, "invint_over") - 2.0).abs() < 1e-9, "v beyond table clamps to x_hi");
}

/// Stock secondary outputs with roles publish applied rates; an output-qualified ref
/// reads them (previous-step causality), and a role-less port falls back to the primary.
#[test]
fn stock_ports_and_qualified_refs() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 4, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "in_rate", "name": "InRate", "primitive": "node", "value_rule": "fixed", "value": {"value": 5.0, "unit": "1"}},
        {"id": "out_rate", "name": "OutRate", "primitive": "node", "value_rule": "fixed", "value": {"value": 2.0, "unit": "1"}},
        {"id": "S", "name": "S", "primitive": "stock",
         "outputs": [
           {"name": "S", "unit": "1"},
           {"name": "S#2", "unit": "1", "role": "addition_rate"},
           {"name": "S#3", "unit": "1", "role": "withdrawal_rate"},
           {"name": "S#4", "unit": "1", "role": "net_change"},
           {"name": "S#5", "unit": "1"}
         ],
         "initial_value": {"value": 10.0, "unit": "1"},
         "inflows": ["in_rate"], "outflows": ["out_rate"]},
        {"id": "read_add", "name": "RA", "primitive": "node", "value_rule": "expression", "inputs": ["S"],
         "expression": {"ast": {"op": "ref", "element_id": "S", "output": "S#2"}}},
        {"id": "read_wd", "name": "RW", "primitive": "node", "value_rule": "expression", "inputs": ["S"],
         "expression": {"ast": {"op": "ref", "element_id": "S", "output": "S#3"}}},
        {"id": "read_net", "name": "RN", "primitive": "node", "value_rule": "expression", "inputs": ["S"],
         "expression": {"ast": {"op": "ref", "element_id": "S", "output": "S#4"}}},
        {"id": "read_roleless", "name": "RR", "primitive": "node", "value_rule": "expression", "inputs": ["S"],
         "expression": {"ast": {"op": "ref", "element_id": "S", "output": "S#5"}}}
      ]
    }"#;
    let r = run_json(json);
    // Constant rates: ports are steady from step 1; readers see the previous step's value.
    assert!((final_of(&r, "read_add") - 5.0).abs() < 1e-9, "addition_rate = Σ inflows");
    assert!((final_of(&r, "read_wd") - 2.0).abs() < 1e-9, "withdrawal_rate = Σ outflows");
    assert!((final_of(&r, "read_net") - 3.0).abs() < 1e-9, "net_change = +3/step");
    // Role-less secondary: no published port → falls back to the primary (the level).
    // Level at the reader's view (start of final step) = 10 + 3·3 = 19.
    assert!((final_of(&r, "read_roleless") - 19.0).abs() < 1e-9, "role-less port → primary level");
}
