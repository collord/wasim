//! Link primitive tests (v2-native): rate/fraction transfer, priority_allocation,
//! transit_buffer (plug flow), transit_decay, scheduled_flow. Mass is conserved
//! (what a source loses, targets gain — modulo decay).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run(json: &str) -> SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run")
}

fn hist(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements[id].time_history.as_ref().unwrap().mean.clone()
}

#[test]
fn basic_rate_transfer_conserves_mass() {
    // S=100 drains 10/step into T; over 5 steps S:90..50, T:10..50.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 100, "unit": "m3"}, "save_results": {"time_history": true}},
          {"id": "T", "name": "T", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"}, "save_results": {"time_history": true}},
          {"id": "L", "name": "L", "primitive": "link", "source": "S", "target": "T", "rate": {"value": 10, "unit": "m3/d"}, "save_results": {"time_history": true}}
        ]}"#,
    );
    assert_eq!(hist(&r, "S"), vec![90.0, 80.0, 70.0, 60.0, 50.0]);
    assert_eq!(hist(&r, "T"), vec![10.0, 20.0, 30.0, 40.0, 50.0]);
    assert_eq!(hist(&r, "L"), vec![10.0, 10.0, 10.0, 10.0, 10.0]);
}

#[test]
fn priority_allocation_limits_by_supply() {
    // S=15. A (prio 1) and B (prio 2) each want 10/step. step0: A=10, B=5, S→0; then dry.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 15, "unit": "m3"}, "save_results": {"time_history": true}},
          {"id": "TA", "name": "TA", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"}},
          {"id": "TB", "name": "TB", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"}},
          {"id": "A", "name": "A", "primitive": "link", "source": "S", "target": "TA", "priority": 1, "rate": {"value": 10, "unit": "m3/d"}, "save_results": {"time_history": true}},
          {"id": "B", "name": "B", "primitive": "link", "source": "S", "target": "TB", "priority": 2, "rate": {"value": 10, "unit": "m3/d"}, "save_results": {"time_history": true}}
        ]}"#,
    );
    assert_eq!(hist(&r, "A"), vec![10.0, 0.0], "priority 1 served first");
    assert_eq!(hist(&r, "B"), vec![5.0, 0.0], "priority 2 gets the remainder");
    assert_eq!(hist(&r, "S"), vec![0.0, 0.0]);
}

#[test]
fn transit_buffer_delays_delivery() {
    // transit 2 steps: T receives nothing until step 2, then 10/step (plug flow).
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 100, "unit": "m3"}, "save_results": {"time_history": true}},
          {"id": "T", "name": "T", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"}, "save_results": {"time_history": true}},
          {"id": "L", "name": "L", "primitive": "link", "source": "S", "target": "T", "rate": {"value": 10, "unit": "m3/d"}, "transit_time": {"value": 2, "unit": "d"}, "save_results": {"time_history": true}}
        ]}"#,
    );
    assert_eq!(hist(&r, "S"), vec![90.0, 80.0, 70.0, 60.0, 50.0], "source drains immediately");
    assert_eq!(hist(&r, "T"), vec![0.0, 0.0, 10.0, 20.0, 30.0], "target lags by 2 steps");
}

#[test]
fn transit_decay_loses_mass_in_transit() {
    // transit 1 step, decay_rate ln2/d → factor exp(-ln2·1)=0.5: target gains 5/step (lagged).
    let ln2 = std::f64::consts::LN_2;
    let r = run(&format!(
        r#"{{"wasim_version": "0.8.0",
        "simulation_settings": {{"duration": {{"value": 5, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}}},
        "elements": [
          {{"id": "S", "name": "S", "primitive": "stock", "initial_value": {{"value": 100, "unit": "m3"}}, "save_results": {{"time_history": true}}}},
          {{"id": "T", "name": "T", "primitive": "stock", "initial_value": {{"value": 0, "unit": "m3"}}, "save_results": {{"time_history": true}}}},
          {{"id": "L", "name": "L", "primitive": "link", "source": "S", "target": "T", "rate": {{"value": 10, "unit": "m3/d"}},
            "transit_time": {{"value": 1, "unit": "d"}}, "decay_rate": {{"value": {ln2}, "unit": "1/d"}}, "save_results": {{"time_history": true}}}}
        ]}}"#
    ));
    // Source loses 10/step; target gains the decayed 5/step, lagged one step.
    assert_eq!(hist(&r, "S"), vec![90.0, 80.0, 70.0, 60.0, 50.0]);
    let t = hist(&r, "T");
    let expected = [0.0, 5.0, 10.0, 15.0, 20.0];
    for (i, (a, b)) in t.iter().zip(expected).enumerate() {
        assert!((a - b).abs() < 1e-9, "T[{i}] = {a}, expected {b}");
    }
}

#[test]
fn scheduled_flow_transfers_only_on_schedule() {
    // schedule [2]: the single transfer of 10 happens at step 2 only.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 100, "unit": "m3"}, "save_results": {"time_history": true}},
          {"id": "T", "name": "T", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"}, "save_results": {"time_history": true}},
          {"id": "L", "name": "L", "primitive": "link", "source": "S", "target": "T", "rate": {"value": 10, "unit": "m3/d"},
            "schedule": {"mode": "on_schedule", "schedule": [{"value": 2, "unit": "d"}]}, "save_results": {"time_history": true}}
        ]}"#,
    );
    assert_eq!(hist(&r, "S"), vec![100.0, 100.0, 90.0, 90.0, 90.0]);
    assert_eq!(hist(&r, "T"), vec![0.0, 0.0, 10.0, 10.0, 10.0]);
}
