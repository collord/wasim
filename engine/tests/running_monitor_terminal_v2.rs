// gap #3 / Option B (running monitors): a `filter` with `include_terminal: true` folds the input's
// terminal value S(T) into its running statistic after the run, so a barrier/lookback monitor
// natively covers the terminal date instead of stopping one step short. Proven as an EXACT
// per-realization identity against the interior-only filter combined with a terminal_expression
// read of S(T) — no statistics needed.
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, RunConfig, WasimModel};

const JSON: &str = r#"{
  "wasim_version":"0.1.0",
  "simulation_settings":{"duration":{"value":1.0,"unit":"yr"},"timestep":{"value":0.05,"unit":"yr"},"n_realizations":3000,"seed":7},
  "elements":[
    {"id":"P","name":"P","type":"stochastic_process","process":{"family":"gbm","mean_type":"arithmetic","mean":{"value":0.05,"unit":"1/yr"},"stddev":{"value":0.30,"unit":"1/yr"}}},
    {"id":"S","name":"S","type":"accumulator","initial_value":{"value":100.0,"unit":"USD"},"inputs":["S","P"],
     "rate":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"S"},"right":{"op":"ref","element_id":"P"}},"display":"S*P"},
     "min_value":null,"capacity":null,"save_results":{"final_value":true}},

    {"id":"rmin_i","name":"rmin_i","type":"filter","input":"S","statistic":"min","save_results":{"final_value":true}},
    {"id":"rmin_t","name":"rmin_t","type":"filter","input":"S","statistic":"min","include_terminal":true,"save_results":{"final_value":true}},
    {"id":"rmax_i","name":"rmax_i","type":"filter","input":"S","statistic":"max","save_results":{"final_value":true}},
    {"id":"rmax_t","name":"rmax_t","type":"filter","input":"S","statistic":"max","include_terminal":true,"save_results":{"final_value":true}},

    {"id":"S_T","name":"S_T","type":"terminal_expression","inputs":["S"],
     "expression":{"ast":{"op":"ref","element_id":"S"},"display":"S(T)"},"save_results":{"final_value":true}}
  ]
}"#;

#[test]
fn include_terminal_filter_folds_the_terminal_point() {
    let m = normalize_v1(&serde_json::from_str::<WasimModel>(JSON).unwrap());
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    let f = |id: &str| r.elements[id].final_values.clone();
    let (s, rmin_i, rmin_t, rmax_i, rmax_t, st) =
        (f("S"), f("rmin_i"), f("rmin_t"), f("rmax_i"), f("rmax_t"), f("S_T"));
    let n = s.len();
    assert!(n >= 3000);

    let mut lowered = 0;
    for i in 0..n {
        // The terminal_expression reads S(T) exactly (identity with the stock's own final).
        assert!((st[i] - s[i]).abs() < 1e-9, "S_T must equal S final at {i}");
        // include_terminal folds exactly S(T): the terminal-inclusive extreme equals the interior
        // extreme combined with S(T) — an exact per-realization identity.
        assert!((rmin_t[i] - rmin_i[i].min(st[i])).abs() < 1e-9,
            "rmin_t must equal min(interior, S(T)) at {i}: {} vs {}", rmin_t[i], rmin_i[i].min(st[i]));
        assert!((rmax_t[i] - rmax_i[i].max(st[i])).abs() < 1e-9,
            "rmax_t must equal max(interior, S(T)) at {i}: {} vs {}", rmax_t[i], rmax_i[i].max(st[i]));
        // Folding the terminal can only lower a running min / raise a running max.
        assert!(rmin_t[i] <= rmin_i[i] + 1e-9 && rmax_t[i] >= rmax_i[i] - 1e-9, "monotone fold at {i}");
        if rmin_t[i] < rmin_i[i] - 1e-9 { lowered += 1; }
    }
    // On a fraction of paths the terminal is a new minimum, so the fold actually changes the monitor
    // (otherwise the test would pass trivially with include_terminal being a no-op).
    assert!(lowered > 0, "the terminal fold should strictly lower the running min on some paths");
    println!("include_terminal: rmin_t==min(interior,S(T)), rmax_t==max(interior,S(T)); terminal lowered the min on {lowered}/{n} paths");
}
