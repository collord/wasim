//! Element-wise math builtins over arrays. Before this fix, `min`/`max` and the unary math builtins
//! (`exp`, `abs`, …) collapsed every array argument to element 0 and returned a bare scalar (so
//! `min([10,20,30],[1,2,3])` gave `[1,0,0]`). They now broadcast element-wise like the operators
//! (`+ − × ÷`) and `sum_array`, via `Value::map`/`zip_with`. Scalar calls stay scalar (bit-identical).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn finals(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements.get(id).map(|e| e.final_values.clone()).unwrap_or_else(|| panic!("missing {id}"))
}
fn member(r: &SimulationResults, id: &str, k: usize) -> f64 {
    finals(r, &format!("{id}#{k}"))[0]
}

// a=[10,20,30], b=[1,2,3] over dim F. Test min/max/mod (binary, elementwise), abs (unary), and a
// scalar min (path unchanged).
const MODEL: &str = r#"{
  "wasim_version":"0.9.9",
  "simulation_settings":{"duration":{"value":1,"unit":"d"},"timestep":{"value":1,"unit":"d"},"n_realizations":1},
  "dimensions":[{"id":"F","name":"F","size":3}],
  "elements":[
    {"id":"a","name":"a","primitive":"node","value_rule":"fixed","values":[10,20,30],"unit":"1","outputs":[{"name":"a","unit":"1","dimensions":["F"]}]},
    {"id":"b","name":"b","primitive":"node","value_rule":"fixed","values":[1,2,3],"unit":"1","outputs":[{"name":"b","unit":"1","dimensions":["F"]}]},
    {"id":"nb","name":"nb","primitive":"node","value_rule":"expression","inputs":["b"],"expression":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"b"},"right":{"op":"literal","value":-1}}},"outputs":[{"name":"nb","unit":"1","dimensions":["F"]}]},
    {"id":"mn","name":"mn","primitive":"node","value_rule":"expression","inputs":["a","b"],"expression":{"ast":{"op":"call","fn":"min","args":[{"op":"ref","element_id":"a"},{"op":"ref","element_id":"b"}]}},"outputs":[{"name":"mn","unit":"1","dimensions":["F"]}],"save_results":{"final_value":true,"time_history":true}},
    {"id":"mx","name":"mx","primitive":"node","value_rule":"expression","inputs":["a","b"],"expression":{"ast":{"op":"call","fn":"max","args":[{"op":"ref","element_id":"a"},{"op":"ref","element_id":"b"}]}},"outputs":[{"name":"mx","unit":"1","dimensions":["F"]}],"save_results":{"final_value":true,"time_history":true}},
    {"id":"md","name":"md","primitive":"node","value_rule":"expression","inputs":["a","b"],"expression":{"ast":{"op":"call","fn":"mod","args":[{"op":"ref","element_id":"a"},{"op":"ref","element_id":"b"}]}},"outputs":[{"name":"md","unit":"1","dimensions":["F"]}],"save_results":{"final_value":true,"time_history":true}},
    {"id":"ab","name":"ab","primitive":"node","value_rule":"expression","inputs":["nb"],"expression":{"ast":{"op":"call","fn":"abs","args":[{"op":"ref","element_id":"nb"}]}},"outputs":[{"name":"ab","unit":"1","dimensions":["F"]}],"save_results":{"final_value":true,"time_history":true}},
    {"id":"sc","name":"sc","primitive":"node","value_rule":"expression","expression":{"ast":{"op":"call","fn":"min","args":[{"op":"literal","value":5},{"op":"literal","value":3}]}},"save_results":{"final_value":true,"time_history":true}}
  ]
}"#;

#[test]
fn math_builtins_broadcast_elementwise_over_arrays() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // min([10,20,30],[1,2,3]) == [1,2,3]  (was [1,0,0] before the fix)
    assert_eq!([member(&r, "mn", 1), member(&r, "mn", 2), member(&r, "mn", 3)], [1.0, 2.0, 3.0], "min elementwise");
    // max == [10,20,30]
    assert_eq!([member(&r, "mx", 1), member(&r, "mx", 2), member(&r, "mx", 3)], [10.0, 20.0, 30.0], "max elementwise");
    // mod([10,20,30],[1,2,3]) == [0,0,0]
    assert_eq!([member(&r, "md", 1), member(&r, "md", 2), member(&r, "md", 3)], [0.0, 0.0, 0.0], "mod elementwise");
    // abs([-1,-2,-3]) == [1,2,3]
    assert_eq!([member(&r, "ab", 1), member(&r, "ab", 2), member(&r, "ab", 3)], [1.0, 2.0, 3.0], "abs elementwise");
    // Scalar path unchanged: min(5,3) == 3.
    assert_eq!(finals(&r, "sc"), vec![3.0], "scalar min unchanged");
}
