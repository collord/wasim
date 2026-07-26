//! Phase-2 (NamedArray align-by-name) end-to-end proof: two `vector_map` results
//! over *different* dimensions, multiplied, produce a genuine 2-D output —
//! something the flat `Value::Vector` engine could not express. The result
//! surfaces as `<id>#1..#N` in row-major, canonical (sorted-by-id) axis order.
//!
//! This is the capability the Analytica "Intelligent Arrays" gap needs
//! (WASIM_NAMEDARRAY_DESIGN.md §4, Phase 2); before this, `a[A] * b[B]` truncated
//! to a positional 1-D zip.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

// va[A] = [1,2] (index_ref over A), vb[B] = [1,2,3] (index_ref over B),
// prod[A,B] = va*vb. Canonical axis order A<B ⇒ A outer, B inner, row-major:
//   [1*1, 1*2, 1*3, 2*1, 2*2, 2*3] = [1,2,3,2,4,6]
const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
  "dimensions": [
    {"id": "A", "name": "A", "size": 2},
    {"id": "B", "name": "B", "size": 3}
  ],
  "containers": [
    {"id": "M", "name": "M", "elements": ["M/va", "M/vb", "M/prod"]}
  ],
  "elements": [
    {"id": "M/va", "name": "va", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "vector_map", "over": "A", "body": {"op": "index_ref", "axis": "row"}}},
     "outputs": [{"name": "va", "unit": "1", "dimensions": ["A"]}], "save_results": {"final_value": true}},

    {"id": "M/vb", "name": "vb", "primitive": "node", "value_rule": "expression", "container": "M",
     "expression": {"ast": {"op": "vector_map", "over": "B", "body": {"op": "index_ref", "axis": "row"}}},
     "outputs": [{"name": "vb", "unit": "1", "dimensions": ["B"]}], "save_results": {"final_value": true}},

    {"id": "M/prod", "name": "prod", "primitive": "node", "value_rule": "expression", "container": "M",
     "inputs": ["M/va", "M/vb"],
     "expression": {"ast": {"op": "multiply",
        "left":  {"op": "ref", "element_id": "M/va"},
        "right": {"op": "ref", "element_id": "M/vb"}}},
     "outputs": [{"name": "prod", "unit": "1", "dimensions": ["A", "B"]}], "save_results": {"final_value": true}}
  ]
}"#;

#[test]
fn two_axis_broadcast_produces_2d_output() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    let member = |k: usize| -> f64 {
        r.elements
            .get(&format!("M/prod#{k}"))
            .and_then(|e| e.final_values.first().copied())
            .unwrap_or_else(|| panic!("missing M/prod#{k}"))
    };

    let got: Vec<f64> = (1..=6).map(member).collect();
    assert_eq!(got, vec![1.0, 2.0, 3.0, 2.0, 4.0, 6.0],
        "prod[A,B] must be the row-major outer product va⊗vb");
}
