//! Phase-3 (NamedArray label subscript) end-to-end: `x[Dim = 'label']` selects a
//! member by its declared label — the Analytica idiom (`x[Region='West']`) that
//! was the top translation blocker. Exercises both a fixed vector (positional
//! label resolution) and a `vector_map`-tagged array (real axis subscript).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
  "dimensions": [
    {"id": "Region", "name": "Region", "size": 3, "labels": ["East", "West", "North"]}
  ],
  "containers": [
    {"id": "M", "name": "M", "elements": ["M/rates", "M/pick_fixed", "M/seq", "M/pick_seq"]}
  ],
  "elements": [
    {"id": "M/rates", "name": "rates", "primitive": "node", "value_rule": "fixed", "container": "M",
     "values": [10, 20, 30], "unit": "1",
     "outputs": [{"name": "rates", "unit": "1", "dimensions": ["Region"]}], "save_results": {"final_value": true}},

    {"id": "M/pick_fixed", "name": "pick_fixed", "primitive": "node", "value_rule": "expression", "container": "M",
     "inputs": ["M/rates"],
     "expression": {"ast": {"op": "subscript", "array": {"op": "ref", "element_id": "M/rates"},
        "dim": "Region", "label": "West"}},
     "save_results": {"final_value": true}},

    {"id": "M/seq", "name": "seq", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "vector_map", "over": "Region", "body": {"op": "index_ref", "axis": "row"}}},
     "outputs": [{"name": "seq", "unit": "1", "dimensions": ["Region"]}], "save_results": {"final_value": true}},

    {"id": "M/pick_seq", "name": "pick_seq", "primitive": "node", "value_rule": "expression", "container": "M",
     "inputs": ["M/seq"],
     "expression": {"ast": {"op": "subscript", "array": {"op": "ref", "element_id": "M/seq"},
        "dim": "Region", "label": "North"}},
     "save_results": {"final_value": true}}
  ]
}"#;

#[test]
fn label_subscript_selects_member() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let s = |id: &str| r.elements[id].final_values[0];

    // rates over [East,West,North] = [10,20,30]; 'West' → 20 (positional path).
    assert_eq!(s("M/pick_fixed"), 20.0);
    // seq = [1,2,3] tagged Region (vector_map); 'North' → 3 (Array subscript path).
    assert_eq!(s("M/pick_seq"), 3.0);
}
