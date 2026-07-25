//! Array-lane perf harness (ARRAY_LANE_DESIGN.md, Phase B verification). Ignored by
//! default; run with:
//!   cargo test --release --test array_lane_bench -- --ignored --nocapture
//!
//! A deep elementwise arithmetic chain over a flat Monte-Carlo model, evaluated the
//! same way twice: scalar lane (realization-outer tree-walk) vs the fused array lane
//! (bytecode, one pass per column, no per-op temporaries). Expect the Spike-1-class
//! speedup and a bit-identical result.

use std::time::Instant;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// Two samples `a,b` and one fixed `k`, then an 8-op elementwise chain in `z`.
fn chain_model(n_real: u32) -> String {
    format!(r#"{{
      "wasim_version": "0.9.7",
      "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": {n_real}, "seed": 11}},
      "containers": [{{"id": "M", "name": "M", "elements": ["M/a","M/b","M/k","M/z"]}}],
      "elements": [
        {{"id":"M/a","name":"a","primitive":"node","value_rule":"sample","container":"M","distribution":{{"family":"normal","parameters":{{"mean":{{"value":10,"unit":"1"}},"stddev":{{"value":2,"unit":"1"}}}}}},"save_results":{{"final_value":false}}}},
        {{"id":"M/b","name":"b","primitive":"node","value_rule":"sample","container":"M","distribution":{{"family":"uniform","parameters":{{"min":{{"value":1,"unit":"1"}},"max":{{"value":3,"unit":"1"}}}}}},"save_results":{{"final_value":false}}}},
        {{"id":"M/k","name":"k","primitive":"node","value_rule":"fixed","container":"M","value":{{"value":2,"unit":"1"}},"save_results":{{"final_value":false}}}},
        {{"id":"M/z","name":"z","primitive":"node","value_rule":"expression","container":"M","inputs":["M/a","M/b","M/k"],
         "expression":{{"ast":
           {{"op":"add",
             "left":{{"op":"subtract",
               "left":{{"op":"multiply","left":{{"op":"add","left":{{"op":"multiply","left":{{"op":"ref","element_id":"M/a"}},"right":{{"op":"ref","element_id":"M/k"}}}},"right":{{"op":"literal","value":1}}}},"right":{{"op":"ref","element_id":"M/b"}}}},
               "right":{{"op":"divide","left":{{"op":"ref","element_id":"M/a"}},"right":{{"op":"add","left":{{"op":"ref","element_id":"M/b"}},"right":{{"op":"literal","value":1}}}}}}}},
             "right":{{"op":"multiply","left":{{"op":"ref","element_id":"M/a"}},"right":{{"op":"ref","element_id":"M/a"}}}}}}}},
         "save_results":{{"final_value":true}}}}
      ]
    }}"#)
}

fn best_of(m: &wasim_engine::ModelV2, g: &ModelGraphV2, lane: bool, reps: usize) -> (f64, f64) {
    let cfg = RunConfig { array_lane: lane, ..Default::default() };
    let warm = run_v2(m, g, &cfg).unwrap();
    let probe = warm.elements["M/z"].final_values[0];
    let mut best = f64::INFINITY;
    for _ in 0..reps {
        let t = Instant::now();
        let r = run_v2(m, g, &cfg).unwrap();
        std::hint::black_box(r.elements["M/z"].final_values[0]);
        best = best.min(t.elapsed().as_secs_f64());
    }
    (best, probe)
}

#[test]
#[ignore]
fn bench_array_lane_vs_scalar() {
    let json = chain_model(500_000);
    let m = parse_v2(&json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();

    let (scalar_t, scalar_v) = best_of(&m, &g, false, 5);
    let (array_t, array_v) = best_of(&m, &g, true, 5);

    assert_eq!(scalar_v.to_bits(), array_v.to_bits(), "array lane result must be bit-identical");
    eprintln!(
        "BENCH chain n=500000  scalar={scalar_t:.4}s  array={array_t:.4}s  speedup={:.2}x  (z[0]={scalar_v:.6})",
        scalar_t / array_t
    );
}
