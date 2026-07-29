//! Feature C (0.9.9): queue node `leak_rate` (first-order decay over transit) and a proof that the
//! delay is sampled at admit (fixed_at_entry / conveyor equivalence).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn run(json: &str) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    run_v2(&m, &g, &RunConfig { seed: Some(1), ..RunConfig::default() }).expect("run")
}

/// A queue with a 2-step delay and leak_rate=0.5: arrivals of 10/step exit after 2 steps decayed
/// by exp(-leak·transit) = exp(-0.5·2) = exp(-1) ≈ 0.3679, so throughput ≈ 3.6788/step in steady
/// state. Mass is NOT conserved (the leak removes it in transit).
#[test]
fn queue_leak_decays_released_amount() {
    let json = r#"{"wasim_version": "0.9.9",
      "simulation_settings": {"duration": {"value": 8, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "arr", "name": "Arr", "primitive": "node", "value_rule": "fixed", "value": {"value": 10, "unit": "1"}},
        {"id": "q", "name": "Q", "primitive": "node", "value_rule": "queue", "input": "arr",
         "delay_time": {"value": 2, "unit": "d"},
         "leak_rate": {"value": 0.5, "unit": "1"},
         "outputs": [{"name": "throughput", "unit": "1"}],
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    let th = &r.elements["q"].time_history.as_ref().unwrap().mean;
    let expected = 10.0 * (-0.5f64 * 2.0).exp(); // ≈ 3.6788
    assert_eq!(th[0], 0.0);
    assert_eq!(th[1], 0.0);
    assert!(
        (th[2] - expected).abs() < 1e-6,
        "first leaked arrivals exit at t=2 as 10·exp(-1) ≈ {expected}, got {}",
        th[2]
    );
    assert!(
        (th[5] - expected).abs() < 1e-6,
        "steady leaked throughput ≈ {expected}, got {}",
        th[5]
    );
}

/// leak_rate absent → identical to the pre-0.9.9 behavior (no decay). Regression guard: a queue
/// with no leak passes exactly the admitted amount through.
#[test]
fn queue_without_leak_is_lossless() {
    let json = r#"{"wasim_version": "0.9.9",
      "simulation_settings": {"duration": {"value": 6, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "arr", "name": "Arr", "primitive": "node", "value_rule": "fixed", "value": {"value": 7, "unit": "1"}},
        {"id": "q", "name": "Q", "primitive": "node", "value_rule": "queue", "input": "arr",
         "delay_time": {"value": 2, "unit": "d"},
         "outputs": [{"name": "throughput", "unit": "1"}],
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    let th = &r.elements["q"].time_history.as_ref().unwrap().mean;
    assert!((th[2] - 7.0).abs() < 1e-9, "lossless: full 7 exits at t=2, got {}", th[2]);
}

/// The delay is sampled when the entity is admitted, not when it exits (fixed_at_entry / conveyor).
/// `delay_time` ramps up over time; a parcel admitted at t=0 with delay 1 must exit at t=1 even
/// though `delay_time` is larger by then. We check the earliest exit reflects the admit-time delay.
#[test]
fn queue_delay_sampled_at_admit() {
    // delay_time = 1 + floor(elapsed): at t=0 delay=1, so the first arrivals (admitted at t=0)
    // exit at t=1. If the delay were re-sampled at release the exit would slip later.
    let json = r#"{"wasim_version": "0.9.9",
      "simulation_settings": {"duration": {"value": 6, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "arr", "name": "Arr", "primitive": "node", "value_rule": "fixed", "value": {"value": 5, "unit": "1"}},
        {"id": "d", "name": "D", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "add", "left": {"op": "literal", "value": 1},
            "right": {"op": "call", "fn": "floor", "args": [{"op": "time_ref", "property": "elapsed"}]}}}},
        {"id": "q", "name": "Q", "primitive": "node", "value_rule": "queue", "input": "arr",
         "delay_time": {"ast": {"op": "ref", "element_id": "d"}},
         "inputs": ["d"],
         "outputs": [{"name": "throughput", "unit": "1"}],
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    let th = &r.elements["q"].time_history.as_ref().unwrap().mean;
    // Arrivals admitted at t=0 (delay=1) exit at t=1.
    assert!((th[1] - 5.0).abs() < 1e-9, "t=0 admit with delay 1 exits at t=1, got {}", th[1]);
}
