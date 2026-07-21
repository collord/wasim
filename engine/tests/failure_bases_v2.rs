//! S1 — failure-mode / trigger no-ops closed (gap analysis Rev 2 §0.2 item 4).
//! `TriggerMode::OnEvent` (an event firing when another event fires) and `FailureBasis::Event`
//! (an FSM failing on its triggering event) both used to be `_ => false` dead arms. These tests
//! pin the wired behavior. `CapacityDemand` basis remains a documented no-op (needs schema fields).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run(json: &str) -> SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { n_realizations: Some(1), seed: Some(1), ..RunConfig::default() };
    run_v2(&m, &g, &cfg).expect("run")
}

fn hist(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements[id].time_history.as_ref().unwrap().mean.clone()
}

/// `on_event` trigger: event B fires the step its source event A fires. A is scheduled at step 2;
/// B carries `{mode: on_event, source: "A"}` and adds +5 to S. Because A is trigger-driven it is
/// in the pre-pass fired-set, so B (declared *after* A) sees the fire the same step.
#[test]
fn on_event_trigger_fires_when_source_fires() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "A", "name": "A", "primitive": "event",
           "trigger": {"mode": "on_schedule", "schedule": [{"value": 2, "unit": "d"}]},
           "effects": [], "save_results": {"time_history": true}},
          {"id": "B", "name": "B", "primitive": "event",
           "trigger": {"mode": "on_event", "source": "A"},
           "effects": [{"target": "S", "mode": "additive", "change": {"value": 5, "unit": "1"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    // A fires only at step 2; B fires the same step; S gains +5 there and holds it.
    assert_eq!(hist(&r, "A"), vec![0.0, 0.0, 1.0, 0.0, 0.0], "A fires at step 2");
    assert_eq!(hist(&r, "B"), vec![0.0, 0.0, 1.0, 0.0, 0.0], "B fires the step A fires");
    assert_eq!(hist(&r, "S"), vec![0.0, 0.0, 5.0, 5.0, 5.0], "B's effect applied at step 2");
}

/// `on_event` never fires when the named source never fires (dangling / non-firing source).
#[test]
fn on_event_no_fire_when_source_silent() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "B", "name": "B", "primitive": "event",
           "trigger": {"mode": "on_event", "source": "nonexistent"},
           "effects": [{"target": "S", "mode": "additive", "change": {"value": 5, "unit": "1"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    assert_eq!(hist(&r, "B"), vec![0.0, 0.0, 0.0], "no source fire → B never fires");
    assert_eq!(hist(&r, "S"), vec![0.0, 0.0, 0.0], "no effect");
}

/// `event`-basis FSM: fails the step its triggering event fires (here an `on_event` trigger from
/// a scheduled source A). On failure it applies its +10 effect and stays failed (no repair).
#[test]
fn event_basis_fsm_fails_on_triggering_event() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "A", "name": "A", "primitive": "event",
           "trigger": {"mode": "on_schedule", "schedule": [{"value": 3, "unit": "d"}]},
           "effects": [], "save_results": {"time_history": true}},
          {"id": "F", "name": "F", "primitive": "event",
           "trigger": {"mode": "on_event", "source": "A"},
           "failure_process": {"basis": "event", "repair": {"policy": "none"}},
           "effects": [{"target": "S", "mode": "additive", "change": {"value": 10, "unit": "1"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    // F (the FSM) is working (0) until A fires at step 3, then failed (1) permanently.
    assert_eq!(hist(&r, "F"), vec![0.0, 0.0, 0.0, 1.0, 1.0], "FSM fails when A fires");
    assert_eq!(hist(&r, "S"), vec![0.0, 0.0, 0.0, 10.0, 10.0], "failure effect applied at step 3");
}

/// `event`-basis FSM with a source that never fires stays working the whole run (was already the
/// `_ => false` behavior; pin it so the new arm didn't regress the never-fire case).
#[test]
fn event_basis_fsm_survives_without_trigger() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 4, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"}, "save_results": {"time_history": true}},
          {"id": "F", "name": "F", "primitive": "event",
           "trigger": {"mode": "on_event", "source": "never"},
           "failure_process": {"basis": "event", "repair": {"policy": "none"}},
           "effects": [{"target": "S", "mode": "additive", "change": {"value": 10, "unit": "1"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    assert_eq!(hist(&r, "F"), vec![0.0, 0.0, 0.0, 0.0], "no trigger → FSM stays working");
    assert_eq!(hist(&r, "S"), vec![0.0, 0.0, 0.0, 0.0], "no failure effect");
}
