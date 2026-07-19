//! B3 discrete-event depth: queue/delay node rule + Resource definition with spend/deposit/borrow.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn run(json: &str) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    run_v2(&m, &g, &RunConfig { seed: Some(1), ..RunConfig::default() }).expect("run")
}

/// A queue with a 3-step delay: a unit pulse of arrivals at every step exits 3 steps later, so
/// the throughput series is the arrivals series shifted by 3 (0,0,0, then the arrivals).
#[test]
fn queue_delays_throughput() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 8, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "arr", "name": "Arr", "primitive": "node", "value_rule": "fixed", "value": {"value": 2, "unit": "1"}},
        {"id": "q", "name": "Q", "primitive": "node", "value_rule": "queue", "input": "arr",
         "delay_time": {"value": 3, "unit": "d"},
         "outputs": [{"name": "throughput", "unit": "1"}, {"name": "n", "unit": "1", "role": "num_in_queue"}],
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    let th = &r.elements["q"].time_history.as_ref().unwrap().mean;
    // Arrivals of 2/step, 3-step delay: nothing exits for the first 3 steps, then 2/step.
    assert_eq!(th[0], 0.0, "no throughput at t=0");
    assert_eq!(th[1], 0.0);
    assert_eq!(th[2], 0.0);
    assert!((th[3] - 2.0).abs() < 1e-9, "first arrivals exit at t=3, got {}", th[3]);
    assert!((th[5] - 2.0).abs() < 1e-9, "steady 2/step throughput, got {}", th[5]);
}

/// The queue level grows when arrivals accumulate faster than they exit: with a long delay the
/// num_in_queue port climbs.
#[test]
fn queue_level_grows_with_arrivals() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "arr", "name": "Arr", "primitive": "node", "value_rule": "fixed", "value": {"value": 3, "unit": "1"}},
        {"id": "q", "name": "Q", "primitive": "node", "value_rule": "queue", "input": "arr",
         "delay_time": {"value": 10, "unit": "d"},
         "outputs": [{"name": "throughput", "unit": "1"}, {"name": "n", "unit": "1", "role": "num_in_queue"}],
         "save_results": {"time_history": true}},
        {"id": "watch", "name": "Watch", "primitive": "node", "value_rule": "expression", "inputs": ["q"],
         "expression": {"ast": {"op": "ref", "element_id": "q", "output": "q#2"}},
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    let level = &r.elements["watch"].time_history.as_ref().unwrap().mean;
    // Delay (10d) > run (5d) so nothing exits; the queue accumulates 3/step. The num_in_queue
    // port is read one step late (same-step consumers see the previous step's value, matching
    // stock-port semantics), so `watch` traces 0, 3, 6, 9, 12.
    assert!((level[0] - 0.0).abs() < 1e-9, "t=0 level {}", level[0]);
    assert!((level[1] - 3.0).abs() < 1e-9, "t=1 level {}", level[1]);
    assert!((level[4] - 12.0).abs() < 1e-9, "t=4 level {}", level[4]);
}

/// Capacity blocks arrivals that would exceed the cap.
#[test]
fn queue_capacity_blocks_excess() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "arr", "name": "Arr", "primitive": "node", "value_rule": "fixed", "value": {"value": 10, "unit": "1"}},
        {"id": "q", "name": "Q", "primitive": "node", "value_rule": "queue", "input": "arr",
         "delay_time": {"value": 10, "unit": "d"}, "capacity": {"value": 25, "unit": "1"},
         "outputs": [{"name": "throughput", "unit": "1"}, {"name": "n", "unit": "1", "role": "num_in_queue"}],
         "save_results": {"time_history": true}},
        {"id": "watch", "name": "Watch", "primitive": "node", "value_rule": "expression", "inputs": ["q"],
         "expression": {"ast": {"op": "ref", "element_id": "q", "output": "q#2"}},
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    let level = &r.elements["watch"].time_history.as_ref().unwrap().mean;
    // 10/step arrivals, cap 25, nothing exits: queue is 10, 20, 25(capped), 25, 25. The port is
    // read one step late, so `watch` traces 0, 10, 20, 25, 25.
    assert!((level[0] - 0.0).abs() < 1e-9);
    assert!((level[1] - 10.0).abs() < 1e-9);
    assert!((level[2] - 20.0).abs() < 1e-9);
    assert!((level[3] - 25.0).abs() < 1e-9, "should cap at 25, got {}", level[3]);
    assert!((level[4] - 25.0).abs() < 1e-9, "stays at cap, got {}", level[4]);
}

/// A Resource is spent by an event each step, limited to available supply (exhaustion).
#[test]
fn resource_spend_exhausts() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 6, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "fuel", "name": "Fuel", "primitive": "resource", "initial_value": {"value": 10, "unit": "1"},
         "save_results": {"time_history": true, "final_value": true}},
        {"id": "burn", "name": "Burn", "primitive": "event",
         "trigger": {"mode": "always"},
         "effects": [{"target": "fuel", "mode": "spend", "change": {"value": 3, "unit": "1"}}]}
      ]}"#;
    let r = run(json);
    let bal = &r.elements["fuel"].time_history.as_ref().unwrap().mean;
    // 10 spent 3/step → 7, 4, 1, 0 (can't go below 0), 0, 0.
    assert!((bal[0] - 7.0).abs() < 1e-9, "t=0 {}", bal[0]);
    assert!((bal[1] - 4.0).abs() < 1e-9);
    assert!((bal[2] - 1.0).abs() < 1e-9);
    assert!((bal[3] - 0.0).abs() < 1e-9, "exhausted, clamped at 0, got {}", bal[3]);
    assert!((bal[5] - 0.0).abs() < 1e-9);
}

/// Deposit adds back to a resource, clamped to capacity.
#[test]
fn resource_deposit_clamps_to_capacity() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tank", "name": "Tank", "primitive": "resource", "initial_value": {"value": 0, "unit": "1"},
         "capacity": {"value": 8, "unit": "1"},
         "save_results": {"time_history": true}},
        {"id": "fill", "name": "Fill", "primitive": "event", "trigger": {"mode": "always"},
         "effects": [{"target": "tank", "mode": "deposit", "change": {"value": 3, "unit": "1"}}]}
      ]}"#;
    let r = run(json);
    let bal = &r.elements["tank"].time_history.as_ref().unwrap().mean;
    // +3/step from 0, cap 8: 3, 6, 8 (clamped), 8, 8.
    assert!((bal[0] - 3.0).abs() < 1e-9);
    assert!((bal[1] - 6.0).abs() < 1e-9);
    assert!((bal[2] - 8.0).abs() < 1e-9, "should clamp at cap 8, got {}", bal[2]);
    assert!((bal[4] - 8.0).abs() < 1e-9);
}

/// Two events spending the same resource in short supply: the total spent cannot exceed the
/// balance (exhaustion allocation, first-come in event order).
#[test]
fn resource_shared_spend_cannot_overdraw() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "pool", "name": "Pool", "primitive": "resource", "initial_value": {"value": 5, "unit": "1"},
         "save_results": {"time_history": true, "final_value": true}},
        {"id": "a", "name": "A", "primitive": "event", "trigger": {"mode": "always"},
         "effects": [{"target": "pool", "mode": "spend", "change": {"value": 4, "unit": "1"}}]},
        {"id": "b", "name": "B", "primitive": "event", "trigger": {"mode": "always"},
         "effects": [{"target": "pool", "mode": "spend", "change": {"value": 4, "unit": "1"}}]}
      ]}"#;
    let r = run(json);
    let bal = &r.elements["pool"].time_history.as_ref().unwrap().mean;
    // Step 0: A spends 4 (5→1), B spends min(4,1)=1 (1→0). Never negative.
    assert!(bal.iter().all(|&b| b >= -1e-9), "balance must never go negative: {bal:?}");
    assert!((bal[0] - 0.0).abs() < 1e-9, "both events drain to 0 at t=0, got {}", bal[0]);
}
