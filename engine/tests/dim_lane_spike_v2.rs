//! Spike for DIMENSIONED_ARRAY_LANE_SCOPE.md — validates the one design claim the
//! whole scope rests on: that evaluating a dimensioned element **Run-major** (one big
//! `[Run, D]` layout, per-cell elementwise, then a fixed-order fold over the `D` axis)
//! reproduces the scalar lane **bit-for-bit**.
//!
//! It does this without a new evaluator: run the model on the scalar lane, take that
//! run's own per-realization draws of `a`, recompute the `vector_map`+elementwise+
//! `sum_array` chain columnar (Run as the outer axis, `reduce_data`'s plain `+` fold
//! over `D`), and assert the reduction matches the scalar lane's `to_bits()`.
//!
//! Run: `cargo test --test dim_lane_spike_v2 -- --nocapture`

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

// D size 4. a ~ N(0,1); b[d] = (d+1) + a  (vector_map, 1-based index_ref);
// c[d] = b[d] * 2; s = sum_array(c).
const DIM_MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 2000, "seed": 13},
  "dimensions": [{"id":"D","name":"D","size":4}],
  "containers": [{"id":"M","name":"M","elements":["M/a","M/b","M/c","M/s"]}],
  "elements": [
    {"id":"M/a","name":"a","primitive":"node","value_rule":"sample","container":"M","distribution":{"family":"normal","parameters":{"mean":{"value":0,"unit":"1"},"stddev":{"value":1,"unit":"1"}}},"save_results":{"final_value":true}},
    {"id":"M/b","name":"b","primitive":"node","value_rule":"expression","container":"M","inputs":["M/a"],
     "expression":{"ast":{"op":"vector_map","over":"D","body":{"op":"add","left":{"op":"index_ref","axis":"row"},"right":{"op":"ref","element_id":"M/a"}}}},
     "outputs":[{"name":"b","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/c","name":"c","primitive":"node","value_rule":"expression","container":"M","inputs":["M/b"],
     "expression":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"M/b"},"right":{"op":"literal","value":2}}},
     "outputs":[{"name":"c","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/s","name":"s","primitive":"node","value_rule":"expression","container":"M","inputs":["M/c"],
     "expression":{"ast":{"op":"call","fn":"sum_array","args":[{"op":"ref","element_id":"M/c"}]}},"save_results":{"final_value":true}}
  ]
}"#;

#[test]
fn run_major_columnar_reduction_is_bit_identical() {
    let m = parse_v2(DIM_MODEL).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let res = run_v2(&m, &g, &RunConfig { n_realizations: Some(2000), seed: Some(13), ..Default::default() }).unwrap();

    let a = &res.elements["M/a"].final_values; // this run's own per-realization draws
    let s_scalar = &res.elements["M/s"].final_values;
    assert_eq!(a.len(), s_scalar.len());

    const K: usize = 4;
    let mut mismatches = 0usize;
    for r in 0..a.len() {
        // Run-major cell layout for realization r: cell[d] = ((d+1) + a[r]) * 2.
        // reduce_data(sum) is a plain fold over the D axis in member order, init 0.0.
        let s_col = (0..K).fold(0.0f64, |acc, d| acc + (((d + 1) as f64 + a[r]) * 2.0));
        if s_col.to_bits() != s_scalar[r].to_bits() {
            mismatches += 1;
        }
    }
    assert_eq!(mismatches, 0, "{mismatches} of {} realizations diverged", a.len());

    // Per-member reproduction too, if the engine surfaced the #k columns.
    for d in 1..=K {
        if let Some(er) = res.elements.get(&format!("M/c#{d}")) {
            for r in 0..a.len() {
                let cell = (d as f64 + a[r]) * 2.0;
                assert_eq!(cell.to_bits(), er.final_values[r].to_bits(),
                    "c#{d} realization {r}: columnar cell != scalar lane");
            }
        }
    }
    eprintln!("SPIKE dim run-major columnar: {} realizations × {K} cells, bit-identical", a.len());
}
