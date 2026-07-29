//! Isolates whether a 2-D array accumulates correctly through a lag recurrence:
//! acc[A,B] = lag(acc)[A,B] + rate[A,B], with rate = va[A] ⊗ vb[B] constant.
//! After the run, acc[A,B] should equal n_steps · rate[A,B] per cell.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
  "dimensions": [ {"id": "A", "name": "A", "size": 2}, {"id": "B", "name": "B", "size": 2} ],
  "elements": [
    {"id": "va", "name": "va", "primitive": "node", "value_rule": "expression",
     "expression": {"ast": {"op": "vector_map", "over": "A", "body": {"op": "index_ref", "axis": "row"}}},
     "outputs": [{"name": "va", "unit": "1", "dimensions": ["A"]}]},
    {"id": "vb", "name": "vb", "primitive": "node", "value_rule": "expression",
     "expression": {"ast": {"op": "vector_map", "over": "B", "body": {"op": "index_ref", "axis": "row"}}},
     "outputs": [{"name": "vb", "unit": "1", "dimensions": ["B"]}]},
    {"id": "rate", "name": "rate", "primitive": "node", "value_rule": "expression", "inputs": ["va", "vb"],
     "expression": {"ast": {"op": "multiply", "left": {"op": "ref", "element_id": "va"}, "right": {"op": "ref", "element_id": "vb"}}},
     "outputs": [{"name": "rate", "unit": "1", "dimensions": ["A", "B"]}]},
    {"id": "acc_prev", "name": "acc_prev", "primitive": "node", "value_rule": "lag",
     "input": "acc", "initial": {"value": 0, "unit": "1"},
     "outputs": [{"name": "acc_prev", "unit": "1", "dimensions": ["A", "B"]}]},
    {"id": "acc", "name": "acc", "primitive": "node", "value_rule": "expression", "inputs": ["acc_prev", "rate"],
     "expression": {"ast": {"op": "add", "left": {"op": "ref", "element_id": "acc_prev"}, "right": {"op": "ref", "element_id": "rate"}}},
     "outputs": [{"name": "acc", "unit": "1", "dimensions": ["A", "B"]}], "save_results": {"final_value": true, "time_history": true}}
  ]
}"#;

#[test]
fn two_d_accumulates_through_lag() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // rate[A,B] = va⊗vb = [[1,2],[2,4]] (A outer, B inner). 5 steps → acc = 5·rate.
    // members #1..#4 row-major: [1,2,2,4] · 5 = [5,10,10,20].
    let fin = |k: usize| r.elements[&format!("acc#{k}")].final_values[0];
    let got: Vec<f64> = (1..=4).map(fin).collect();
    println!("acc final per cell = {got:?}  (expected [5,10,10,20])");
    assert_eq!(got, vec![5.0, 10.0, 10.0, 20.0], "2-D array must accumulate per-cell through the lag recurrence");

    // And it should grow linearly each step (not stall): history[k] = k·rate for acc#4.
    let h = &r.elements["acc#4"].time_history.as_ref().unwrap().mean;
    println!("acc#4 history = {h:?}");
    assert_eq!(h, &vec![4.0, 8.0, 12.0, 16.0, 20.0], "acc#4 must grow 4 per step");
}
