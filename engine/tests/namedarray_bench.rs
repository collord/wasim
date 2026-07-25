//! Array-path perf harness for the NamedArray work. Ignored by default; run with:
//!   cargo test --release --test namedarray_bench -- --ignored --nocapture
//! Two 200-wide `vector_map`s evaluated every step over many realizations, reduced
//! by `sum_array` — isolates the vector_map/Array allocation + move cost.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};
use std::time::Instant;

fn stress_model(width: usize) -> String {
    format!(r#"{{
      "wasim_version": "0.9.7",
      "simulation_settings": {{"duration": {{"value": 8, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 2000, "seed": 1}},
      "dimensions": [{{"id": "D", "name": "D", "size": {width}}}],
      "containers": [{{"id": "M", "name": "M", "elements": ["M/x","M/v1","M/v2","M/sink"]}}],
      "elements": [
        {{"id": "M/x","name":"x","primitive":"node","value_rule":"sample","container":"M",
         "distribution": {{"family":"uniform","parameters":{{"min":{{"value":0,"unit":"1"}},"max":{{"value":1,"unit":"1"}}}}}},"save_results":{{"final_value":false}}}},
        {{"id":"M/v1","name":"v1","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
         "expression": {{"ast": {{"op":"vector_map","over":"D","body":{{"op":"add","left":{{"op":"index_ref","axis":"row"}},"right":{{"op":"ref","element_id":"M/x"}}}}}}}},
         "outputs":[{{"name":"v1","unit":"1","dimensions":["D"]}}],"save_results":{{"final_value":false}}}},
        {{"id":"M/v2","name":"v2","primitive":"node","value_rule":"expression","container":"M",
         "expression": {{"ast": {{"op":"vector_map","over":"D","body":{{"op":"multiply","left":{{"op":"index_ref","axis":"row"}},"right":{{"op":"literal","value":2}}}}}}}},
         "outputs":[{{"name":"v2","unit":"1","dimensions":["D"]}}],"save_results":{{"final_value":false}}}},
        {{"id":"M/sink","name":"sink","primitive":"node","value_rule":"expression","container":"M","inputs":["M/v1","M/v2"],
         "expression": {{"ast": {{"op":"add","left":{{"op":"call","fn":"sum_array","args":[{{"op":"ref","element_id":"M/v1"}}]}},"right":{{"op":"call","fn":"sum_array","args":[{{"op":"ref","element_id":"M/v2"}}]}}}}}},"save_results":{{"final_value":true}}}}
      ]
    }}"#)
}

#[test]
#[ignore]
fn bench_array_path() {
    let json = stress_model(200);
    let m = parse_v2(&json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let _ = run_v2(&m, &g, &RunConfig::default()).unwrap(); // warmup
    let mut best = f64::INFINITY;
    for _ in 0..3 {
        let t = Instant::now();
        let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
        std::hint::black_box(r.elements["M/sink"].final_values[0]);
        best = best.min(t.elapsed().as_secs_f64());
    }
    eprintln!("BENCH array_path best={best:.4}s");
}
