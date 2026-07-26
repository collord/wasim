// gap #3 / Option B — the global closing tick (`close_at_terminal`). The per-filter
// `include_terminal` folds a filter's OWN input at the terminal, but if that input is an
// *expression* of a stock (e.g. filter(min, 2*S)) it folds the one-step-stale expression value,
// because ordinary expressions aren't re-evaluated at T. `close_at_terminal` fixes that: after the
// final integration it runs one read-only pass at t=T where expressions re-evaluate against S(T)
// and every filter folds one more observation — so a filter over an expression monitors f(S(T)).
//
// Proven by running the SAME model (same seed → same paths) with the closing tick off and on:
//   - OFF: an expression e=3*S finals at 3*S(T-dt); a filter over g=2*S misses the terminal.
//   - ON : e finals at exactly 3*S(T) (identity with the stock final); the filter over g equals the
//          terminal-inclusive stock filter scaled by 2 — i.e. it saw g(S(T)).
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, RunConfig, WasimModel};

const JSON: &str = r#"{
  "wasim_version":"0.1.0",
  "simulation_settings":{"duration":{"value":1.0,"unit":"yr"},"timestep":{"value":0.05,"unit":"yr"},"n_realizations":3000,"seed":11},
  "elements":[
    {"id":"P","name":"P","type":"stochastic_process","process":{"family":"gbm","mean_type":"arithmetic","mean":{"value":0.06,"unit":"1/yr"},"stddev":{"value":0.30,"unit":"1/yr"}}},
    {"id":"S","name":"S","type":"accumulator","initial_value":{"value":100.0,"unit":"USD"},"inputs":["S","P"],
     "rate":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"S"},"right":{"op":"ref","element_id":"P"}},"display":"S*P"},
     "min_value":null,"capacity":null,"save_results":{"final_value":true}},

    {"id":"g","name":"g","type":"expression","inputs":["S"],
     "expression":{"ast":{"op":"multiply","left":{"op":"literal","value":2.0},"right":{"op":"ref","element_id":"S"}},"display":"2*S"}},
    {"id":"fmin_g","name":"fmin_g","type":"filter","input":"g","statistic":"min","save_results":{"final_value":true}},
    {"id":"fmin_s","name":"fmin_s","type":"filter","input":"S","statistic":"min","save_results":{"final_value":true}},

    {"id":"e_S","name":"e_S","type":"expression","inputs":["S"],
     "expression":{"ast":{"op":"multiply","left":{"op":"literal","value":3.0},"right":{"op":"ref","element_id":"S"}},"display":"3*S"},"save_results":{"final_value":true}},
    {"id":"S_T","name":"S_T","type":"terminal_expression","inputs":["S"],
     "expression":{"ast":{"op":"ref","element_id":"S"},"display":"S(T)"},"save_results":{"final_value":true}}
  ]
}"#;

fn run(close: bool) -> std::collections::HashMap<String, Vec<f64>> {
    let m = normalize_v1(&serde_json::from_str::<WasimModel>(JSON).unwrap());
    let g = ModelGraphV2::build(&m).unwrap();
    let cfg = RunConfig { close_at_terminal: Some(close), ..Default::default() };
    let r = run_v2(&m, &g, &cfg).unwrap();
    ["S", "fmin_g", "fmin_s", "e_S", "S_T"].iter()
        .map(|id| (id.to_string(), r.elements[*id].final_values.clone())).collect()
}

#[test]
fn close_at_terminal_reevaluates_expressions_and_folds_all_filters() {
    let off = run(false);
    let on = run(true);
    let n = off["S"].len();
    assert!(n >= 3000);

    // Same seed → identical paths: the stock final S(T) matches, and S_T reads it either way.
    for i in 0..n {
        assert!((off["S"][i] - on["S"][i]).abs() < 1e-9, "same path at {i}");
        assert!((off["S_T"][i] - off["S"][i]).abs() < 1e-9 && (on["S_T"][i] - on["S"][i]).abs() < 1e-9,
            "S_T == S(T) at {i}");
    }

    let mut moved = 0;
    for i in 0..n {
        let (s_t, fmin_s_off) = (off["S"][i], off["fmin_s"][i]);

        // (1) close ON re-evaluates ordinary expressions at S(T): e_S finals at exactly 3*S(T).
        assert!((on["e_S"][i] - 3.0 * s_t).abs() < 1e-9,
            "close ON: e_S must be 3*S(T) at {i}: {} vs {}", on["e_S"][i], 3.0 * s_t);

        // (2) close ON folds EVERY filter's terminal: the plain stock filter gains S(T).
        assert!((on["fmin_s"][i] - fmin_s_off.min(s_t)).abs() < 1e-9,
            "close ON: fmin_s must include the terminal at {i}");

        // (3) THE DEFERRED CAPABILITY — a filter over an EXPRESSION g=2*S. Off, it misses the
        // terminal (== 2*interior-min). On, the closing tick re-evaluates g at S(T) before folding,
        // so it equals 2*(terminal-inclusive min) — i.e. it monitored g(S(T)).
        assert!((off["fmin_g"][i] - 2.0 * fmin_s_off).abs() < 1e-9,
            "close OFF: fmin_g must be 2*interior-min at {i}: {} vs {}", off["fmin_g"][i], 2.0 * fmin_s_off);
        assert!((on["fmin_g"][i] - 2.0 * fmin_s_off.min(s_t)).abs() < 1e-9,
            "close ON: fmin_g must be 2*min(interior,S(T)) at {i}: {} vs {}", on["fmin_g"][i], 2.0 * fmin_s_off.min(s_t));

        if (on["fmin_g"][i] - off["fmin_g"][i]).abs() > 1e-9 { moved += 1; }
    }
    // On the fraction of paths where the terminal is a new minimum, the expression-filter's value
    // actually changes — so the closing tick is doing real work, not a no-op.
    assert!(moved > 0, "close_at_terminal should change the expression-filter on some paths");
    println!("close_at_terminal: expressions re-eval at S(T); every filter folds the terminal; \
              expression-filter changed on {moved}/{n} paths");
}
