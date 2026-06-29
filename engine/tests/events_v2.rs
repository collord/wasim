//! Event primitive tests (v2-native): trigger + effects (additive/multiplicative/replace
//! on stocks and nodes) and Poisson rate_generation.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run(json: &str, n: Option<u32>) -> SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { n_realizations: n, seed: Some(7), duration_override: None, timestep_override: None };
    run_v2(&m, &g, &cfg).expect("run")
}

fn hist(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements[id].time_history.as_ref().unwrap().mean.clone()
}

#[test]
fn scheduled_additive_effect_on_stock() {
    // Event fires at step 2 only and adds 50 to S.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"}, "save_results": {"time_history": true}},
          {"id": "E", "name": "E", "primitive": "event",
           "trigger": {"mode": "on_schedule", "schedule": [{"value": 2, "unit": "d"}]},
           "effects": [{"target": "S", "mode": "additive", "change": {"value": 50, "unit": "m3"}}],
           "save_results": {"time_history": true}}
        ]}"#,
        Some(1),
    );
    assert_eq!(hist(&r, "S"), vec![0.0, 0.0, 50.0, 50.0, 50.0]);
    assert_eq!(hist(&r, "E"), vec![0.0, 0.0, 1.0, 0.0, 0.0], "event occurrence count");
}

#[test]
fn multiplicative_effect_on_stock() {
    // Event at step 1 doubles S (10 → 20).
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 10, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "E", "name": "E", "primitive": "event",
           "trigger": {"mode": "on_schedule", "schedule": [{"value": 1, "unit": "d"}]},
           "effects": [{"target": "S", "mode": "multiplicative", "change": {"value": 2, "unit": "1"}}]}
        ]}"#,
        Some(1),
    );
    assert_eq!(hist(&r, "S"), vec![10.0, 20.0, 20.0]);
}

#[test]
fn replace_effect_on_node_is_per_step() {
    // Event at step 1 replaces node N's output with 99; otherwise N is its fixed 0.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "N", "name": "N", "primitive": "node", "value_rule": "fixed", "value": {"value": 0, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "E", "name": "E", "primitive": "event",
           "trigger": {"mode": "on_schedule", "schedule": [{"value": 1, "unit": "d"}]},
           "effects": [{"target": "N", "mode": "replace", "change": {"value": 99, "unit": "1"}}]}
        ]}"#,
        Some(1),
    );
    assert_eq!(hist(&r, "N"), vec![0.0, 99.0, 0.0], "effect applies only on the firing step");
}

#[test]
fn poisson_rate_generation_counts() {
    // rate 5/d over 10 d → ~50 occurrences; a counter stock adds 1 per event.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 10, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 300},
        "elements": [
          {"id": "C", "name": "C", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"}, "save_results": {"final_value": true}},
          {"id": "E", "name": "E", "primitive": "event", "rate": {"value": 5, "unit": "1/d"},
           "effects": [{"target": "C", "mode": "additive", "change": {"value": 1, "unit": "1"}}]}
        ]}"#,
        Some(300),
    );
    let finals = &r.elements["C"].final_values;
    let mean = finals.iter().sum::<f64>() / finals.len() as f64;
    assert!((mean - 50.0).abs() < 4.0, "mean event count {mean}, expected ≈ 50");
}
