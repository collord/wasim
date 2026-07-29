//! Feature D (0.9.9): arrays with ≥3 dimensions. A triple-nested `vector_map` builds a genuine
//! 3-D NamedArray (axes [A, B, C]); `index_ref` with `depth` reads the index of each enclosing
//! comprehension; `#k` surfacing multiplies all three dims; `index` gathers a cell by 3 indices.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

// grid[A,B,C] = 100*i_A + 10*i_B + i_C, over A=2, B=3, C=2 (1-based indices).
// Row-major over [A,B,C]: A slowest, C fastest → 12 members.
const MODEL: &str = r#"{
  "wasim_version": "0.9.9",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
  "dimensions": [
    {"id": "A", "name": "A", "size": 2},
    {"id": "B", "name": "B", "size": 3},
    {"id": "C", "name": "C", "size": 2}
  ],
  "elements": [
    {"id": "grid", "name": "grid", "primitive": "node", "value_rule": "expression",
     "expression": {"ast":
       {"op": "vector_map", "over": "A", "body":
         {"op": "vector_map", "over": "B", "body":
           {"op": "vector_map", "over": "C", "body":
             {"op": "add",
              "left": {"op": "add",
                "left": {"op": "multiply", "left": {"op": "literal", "value": 100},
                         "right": {"op": "index_ref", "depth": 2}},
                "right": {"op": "multiply", "left": {"op": "literal", "value": 10},
                         "right": {"op": "index_ref", "depth": 1}}},
              "right": {"op": "index_ref", "depth": 0}}}}}},
     "outputs": [{"name": "grid", "unit": "1", "dimensions": ["A", "B", "C"]}],
     "save_results": {"final_value": true}}
  ]
}"#;

#[test]
fn three_d_vector_map_builds_named_array() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // #k surfacing multiplies all three dims: 2*3*2 = 12 members.
    let member = |k: usize| -> f64 {
        r.elements
            .get(&format!("grid#{k}"))
            .and_then(|e| e.final_values.first().copied())
            .unwrap_or_else(|| panic!("missing grid#{k}"))
    };

    // Row-major [A,B,C]: member index (k, 1-based) → (a,b,c). Value = 100a+10b+c.
    // k=1 → (1,1,1) = 111; k=2 → (1,1,2) = 112; k=3 → (1,2,1) = 121; …
    assert!((member(1) - 111.0).abs() < 1e-9, "grid#1 = 111, got {}", member(1));
    assert!((member(2) - 112.0).abs() < 1e-9, "grid#2 = 112, got {}", member(2));
    assert!((member(3) - 121.0).abs() < 1e-9, "grid#3 = 121, got {}", member(3));
    assert!((member(7) - 211.0).abs() < 1e-9, "grid#7 = 211 (a flips to 2), got {}", member(7));
    assert!((member(12) - 232.0).abs() < 1e-9, "grid#12 = 232 (last cell), got {}", member(12));
}

#[test]
fn three_d_index_gathers_cell_by_three_indices() {
    // A node that reads grid[2,3,1] via a 3-index Index → 100*2 + 10*3 + 1 = 231.
    let model = MODEL.replace(
        r#"     "outputs": [{"name": "grid", "unit": "1", "dimensions": ["A", "B", "C"]}],
     "save_results": {"final_value": true}}"#,
        r#"     "outputs": [{"name": "grid", "unit": "1", "dimensions": ["A", "B", "C"]}],
     "save_results": {"final_value": true}},
    {"id": "cell", "name": "cell", "primitive": "node", "value_rule": "expression",
     "inputs": ["grid"],
     "expression": {"ast": {"op": "index",
        "array": {"op": "ref", "element_id": "grid"},
        "indices": [{"op": "literal", "value": 2}, {"op": "literal", "value": 3}, {"op": "literal", "value": 1}]}},
     "save_results": {"final_value": true}}"#,
    );
    let m = parse_v2(&model).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let cell = r.elements["cell"].final_values[0];
    assert!((cell - 231.0).abs() < 1e-9, "grid[2,3,1] = 231, got {cell}");
}
