//! failure_state_machine tests (v2-native). Deterministic time-to-failure/repair via
//! single-sample `sampled` distributions. Event output = failed state (1) / working (0).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn hist2(json: &str, a: &str, b: &str) -> (Vec<f64>, Vec<f64>) {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { n_realizations: Some(1), seed: Some(1), duration_override: None, timestep_override: None, results_spec: None, timebase: Default::default(), units: Default::default(), realization_weights: vec![] };
    let r = run_v2(&m, &g, &cfg).expect("run");
    let h = |id: &str| r.elements[id].time_history.as_ref().unwrap().mean.clone();
    (h(a), h(b))
}

#[test]
fn exposure_time_no_repair_fails_permanently() {
    // TTF = 3 → fails at step 2 (ttf: 3→2→1→0); applies +100 to S, then stays failed.
    let (s, e) = hist2(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "E", "name": "E", "primitive": "event",
           "failure_process": {"basis": "exposure_time",
             "time_to_failure": {"family": "sampled", "parameters": {"samples": [3]}},
             "repair": {"policy": "none"}},
           "effects": [{"target": "S", "mode": "additive", "change": {"value": 100, "unit": "1"}}],
           "save_results": {"time_history": true}}
        ]}"#,
        "S", "E",
    );
    assert_eq!(e, vec![0.0, 0.0, 1.0, 1.0, 1.0], "fails at step 2, stays failed");
    assert_eq!(s, vec![0.0, 0.0, 100.0, 100.0, 100.0]);
}

#[test]
fn exposure_time_with_repair_cycles_and_reverses_effect() {
    // TTF=2, TTR=2: fail@1 (+50), repair@3 (−50, fresh TTF=2), working after.
    let (s, e) = hist2(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "E", "name": "E", "primitive": "event",
           "failure_process": {"basis": "exposure_time",
             "time_to_failure": {"family": "sampled", "parameters": {"samples": [2]}},
             "repair": {"policy": "repair", "time_to_repair": {"family": "sampled", "parameters": {"samples": [2]}}}},
           "effects": [{"target": "S", "mode": "additive", "change": {"value": 50, "unit": "1"}}],
           "save_results": {"time_history": true}}
        ]}"#,
        "S", "E",
    );
    assert_eq!(e, vec![0.0, 1.0, 1.0, 0.0, 0.0], "fail@1, working again@3");
    assert_eq!(s, vec![0.0, 50.0, 50.0, 0.0, 0.0], "effect applied on failure, reversed on repair");
}

#[test]
fn condition_basis_fails_when_trigger_true() {
    // Fails when elapsed ≥ 2 becomes true (step 2), applies +10, stays failed (no repair).
    let (s, e) = hist2(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "E", "name": "E", "primitive": "event",
           "trigger": {"mode": "on_condition", "condition": {"ast": {"op": "gte",
             "left": {"op": "time_ref", "property": "elapsed"}, "right": {"op": "literal", "value": 2}}}},
           "failure_process": {"basis": "condition", "repair": {"policy": "none"}},
           "effects": [{"target": "S", "mode": "additive", "change": {"value": 10, "unit": "1"}}],
           "save_results": {"time_history": true}}
        ]}"#,
        "S", "E",
    );
    assert_eq!(e, vec![0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(s, vec![0.0, 0.0, 10.0, 10.0, 10.0]);
}
