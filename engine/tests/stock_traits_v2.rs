//! Stock trait tests: overflow_routing and priority_withdrawal (v2-native).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn run(json: &str) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run")
}

fn hist(r: &wasim_engine::SimulationResults, id: &str) -> Vec<f64> {
    r.elements[id].time_history.as_ref().unwrap().mean.clone()
}

#[test]
fn overflow_routing_fills_target() {
    // A: +10/step, capacity 5 → overflows 5 then 10 then 10 into B.
    // A history [5,5,5]; B accumulates [5,15,25].
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "A", "name": "A", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"},
           "rate": {"value": 10, "unit": "m3/d"}, "capacity": {"value": 5, "unit": "m3"}, "overflow_target": "B",
           "save_results": {"time_history": true}},
          {"id": "B", "name": "B", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"},
           "save_results": {"time_history": true}}
        ]}"#,
    );
    assert_eq!(hist(&r, "A"), vec![5.0, 5.0, 5.0]);
    assert_eq!(hist(&r, "B"), vec![5.0, 15.0, 25.0]);
}

#[test]
fn priority_withdrawal_allocates_by_priority() {
    // S=100, no inflow. hi (prio 1) requests 30/d, lo (prio 2) requests 50/d.
    // step0: hi=30, lo=50, S→20. step1: hi=20, lo=0, S→0. step2: hi=0, lo=0.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 100, "unit": "m3"},
           "withdrawals": [
             {"target": "hi", "priority": 1, "request": {"value": 30, "unit": "m3/d"}},
             {"target": "lo", "priority": 2, "request": {"value": 50, "unit": "m3/d"}}
           ],
           "save_results": {"time_history": true}},
          {"id": "hi", "name": "Hi", "primitive": "node", "value_rule": "fixed", "value": {"value": 0, "unit": "m3"},
           "save_results": {"time_history": true}},
          {"id": "lo", "name": "Lo", "primitive": "node", "value_rule": "fixed", "value": {"value": 0, "unit": "m3"},
           "save_results": {"time_history": true}}
        ]}"#,
    );
    assert_eq!(hist(&r, "S"), vec![20.0, 0.0, 0.0]);
    assert_eq!(hist(&r, "hi"), vec![30.0, 20.0, 0.0], "high priority served first");
    assert_eq!(hist(&r, "lo"), vec![50.0, 0.0, 0.0], "low priority gets the remainder");
}
