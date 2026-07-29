//! Axis-selective reduction: the array reducers take an optional 1-based axis-position
//! argument and collapse ONE axis of an n-d array, keeping the rest (previously they only
//! collapsed everything to a scalar). Builds a 2-D grid[A,B] = va[A] ⊗ vb[B] via align-by-name
//! and reduces each axis. Axis order is canonical (sorted by id): A = axis 1, B = axis 2.
//!
//!   va = [1,2] (over A),  vb = [1,2,3] (over B)
//!   grid[A,B] = [[1,2,3],[2,4,6]]  (A outer, B inner, row-major)

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
  "dimensions": [ {"id": "A", "name": "A", "size": 2}, {"id": "B", "name": "B", "size": 3} ],
  "elements": [
    {"id": "va", "name": "va", "primitive": "node", "value_rule": "expression",
     "expression": {"ast": {"op": "vector_map", "over": "A", "body": {"op": "index_ref", "axis": "row"}}},
     "outputs": [{"name": "va", "unit": "1", "dimensions": ["A"]}]},
    {"id": "vb", "name": "vb", "primitive": "node", "value_rule": "expression",
     "expression": {"ast": {"op": "vector_map", "over": "B", "body": {"op": "index_ref", "axis": "row"}}},
     "outputs": [{"name": "vb", "unit": "1", "dimensions": ["B"]}]},
    {"id": "grid", "name": "grid", "primitive": "node", "value_rule": "expression", "inputs": ["va", "vb"],
     "expression": {"ast": {"op": "multiply", "left": {"op": "ref", "element_id": "va"}, "right": {"op": "ref", "element_id": "vb"}}},
     "outputs": [{"name": "grid", "unit": "1", "dimensions": ["A", "B"]}], "save_results": {"final_value": true}},

    {"id": "sum_B", "name": "sum_B", "primitive": "node", "value_rule": "expression", "inputs": ["grid"],
     "expression": {"ast": {"op": "call", "fn": "sum_array", "args": [{"op": "ref", "element_id": "grid"}, {"op": "literal", "value": 2}]}},
     "outputs": [{"name": "sum_B", "unit": "1", "dimensions": ["A"]}], "save_results": {"final_value": true}},
    {"id": "sum_A", "name": "sum_A", "primitive": "node", "value_rule": "expression", "inputs": ["grid"],
     "expression": {"ast": {"op": "call", "fn": "sum_array", "args": [{"op": "ref", "element_id": "grid"}, {"op": "literal", "value": 1}]}},
     "outputs": [{"name": "sum_A", "unit": "1", "dimensions": ["B"]}], "save_results": {"final_value": true}},
    {"id": "max_B", "name": "max_B", "primitive": "node", "value_rule": "expression", "inputs": ["grid"],
     "expression": {"ast": {"op": "call", "fn": "max_array", "args": [{"op": "ref", "element_id": "grid"}, {"op": "literal", "value": 2}]}},
     "outputs": [{"name": "max_B", "unit": "1", "dimensions": ["A"]}], "save_results": {"final_value": true}},
    {"id": "min_B", "name": "min_B", "primitive": "node", "value_rule": "expression", "inputs": ["grid"],
     "expression": {"ast": {"op": "call", "fn": "min_array", "args": [{"op": "ref", "element_id": "grid"}, {"op": "literal", "value": 2}]}},
     "outputs": [{"name": "min_B", "unit": "1", "dimensions": ["A"]}], "save_results": {"final_value": true}},
    {"id": "mean_B", "name": "mean_B", "primitive": "node", "value_rule": "expression", "inputs": ["grid"],
     "expression": {"ast": {"op": "call", "fn": "mean_array", "args": [{"op": "ref", "element_id": "grid"}, {"op": "literal", "value": 2}]}},
     "outputs": [{"name": "mean_B", "unit": "1", "dimensions": ["A"]}], "save_results": {"final_value": true}},
    {"id": "amax_B", "name": "amax_B", "primitive": "node", "value_rule": "expression", "inputs": ["grid"],
     "expression": {"ast": {"op": "call", "fn": "argmax_array", "args": [{"op": "ref", "element_id": "grid"}, {"op": "literal", "value": 2}]}},
     "outputs": [{"name": "amax_B", "unit": "1", "dimensions": ["A"]}], "save_results": {"final_value": true}},
    {"id": "amin_B", "name": "amin_B", "primitive": "node", "value_rule": "expression", "inputs": ["grid"],
     "expression": {"ast": {"op": "call", "fn": "argmin_array", "args": [{"op": "ref", "element_id": "grid"}, {"op": "literal", "value": 2}]}},
     "outputs": [{"name": "amin_B", "unit": "1", "dimensions": ["A"]}], "save_results": {"final_value": true}},

    {"id": "sum_all", "name": "sum_all", "primitive": "node", "value_rule": "expression", "inputs": ["grid"],
     "expression": {"ast": {"op": "call", "fn": "sum_array", "args": [{"op": "ref", "element_id": "grid"}]}},
     "save_results": {"final_value": true}}
  ]
}"#;

fn member(r: &SimulationResults, id: &str) -> f64 {
    r.elements.get(id).and_then(|e| e.final_values.first().copied()).unwrap_or_else(|| panic!("missing {id}"))
}

#[test]
fn axis_selective_reduction() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // grid[A,B] = [[1,2,3],[2,4,6]], row-major.
    assert_eq!((1..=6).map(|k| member(&r, &format!("grid#{k}"))).collect::<Vec<_>>(),
        vec![1.0, 2.0, 3.0, 2.0, 4.0, 6.0], "2-D grid");

    // Reduce axis 2 (B), keep A → per-A row results.
    assert_eq!([member(&r, "sum_B#1"), member(&r, "sum_B#2")], [6.0, 12.0], "sum over B");
    assert_eq!([member(&r, "max_B#1"), member(&r, "max_B#2")], [3.0, 6.0], "max over B");
    assert_eq!([member(&r, "min_B#1"), member(&r, "min_B#2")], [1.0, 2.0], "min over B");
    assert_eq!([member(&r, "mean_B#1"), member(&r, "mean_B#2")], [2.0, 4.0], "mean over B");
    assert_eq!([member(&r, "amax_B#1"), member(&r, "amax_B#2")], [3.0, 3.0], "argmax over B (1-based, per row)");
    assert_eq!([member(&r, "amin_B#1"), member(&r, "amin_B#2")], [1.0, 1.0], "argmin over B");

    // Reduce axis 1 (A), keep B → per-B column results.
    assert_eq!([member(&r, "sum_A#1"), member(&r, "sum_A#2"), member(&r, "sum_A#3")], [3.0, 6.0, 9.0], "sum over A");

    // No axis arg → full collapse to a scalar (bit-identical to the historical behavior).
    assert_eq!(member(&r, "sum_all"), 18.0, "full sum unchanged without an axis argument");
}
