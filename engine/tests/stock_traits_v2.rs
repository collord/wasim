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

// A stock wired to an inflow (and *no* `rate` field) accumulates that inflow — the flow
// path. This is the authoring-UI default after dropping the seeded zero rate; without it a
// stock wired to an inflow stayed flat because a present rate shadows inflows.
const INFLOW_MODEL: &str = r#"{"wasim_version": "0.8.0",
    "simulation_settings": {"duration": {"value": 3, "unit": "s"}, "timestep": {"value": 1, "unit": "s"}, "n_realizations": 1, "seed": 1},
    "elements": [
      {"id": "in", "name": "In", "primitive": "node", "value_rule": "fixed", "value": {"value": 5, "unit": "1"}},
      {"id": "tank", "name": "Tank", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"},
       "inflows": ["in"], "outflows": [], PLACEHOLDER "save_results": {"time_history": true}}
    ]}"#;

#[test]
fn inflow_without_rate_accumulates() {
    let r = run(&INFLOW_MODEL.replace("PLACEHOLDER ", ""));
    assert_eq!(hist(&r, "tank"), vec![5.0, 10.0, 15.0], "inflow of 5/step accumulates");
}

#[test]
fn present_rate_shadows_inflows() {
    // Same wiring but with an explicit zero rate present: the engine takes the rate path and
    // ignores the inflow, so the stock stays flat. Documents the either-or semantics (and the
    // old palette-scaffold bug that seeded exactly this zero rate).
    let r = run(&INFLOW_MODEL.replace("PLACEHOLDER", r#""rate": {"ast": {"op": "literal", "value": 0}, "display": "0"},"#));
    assert_eq!(hist(&r, "tank"), vec![0.0, 0.0, 0.0], "zero rate shadows the inflow");
}

#[test]
fn return_rate_compounds_and_composes_with_inflow() {
    // A bank account: interest (return_rate 0.1/step) on the current balance PLUS a transfer
    // in (inflow 100/step). Each step: next = bal·(1 + 0.1) + 100. What the Inspector's
    // 'Growth rate' + inflows path emits. return_rate is unused in the example corpus, so this
    // is the coverage for the compound_growth + flow composition.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 3, "unit": "s"}, "timestep": {"value": 1, "unit": "s"}, "n_realizations": 1, "seed": 1},
        "elements": [
          {"id": "deposit", "name": "Deposit", "primitive": "node", "value_rule": "fixed", "value": {"value": 100, "unit": "1"}},
          {"id": "acct", "name": "Account", "primitive": "stock", "initial_value": {"value": 1000, "unit": "1"},
           "return_rate": {"value": 0.1, "unit": "1"}, "inflows": ["deposit"], "outflows": [],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    // 1000·1.1+100=1200 ; 1200·1.1+100=1420 ; 1420·1.1+100=1662
    let h = hist(&r, "acct");
    let expect = [1200.0, 1420.0, 1662.0];
    for (i, (&got, &want)) in h.iter().zip(expect.iter()).enumerate() {
        assert!((got - want).abs() < 1e-6, "step {i}: got {got}, want {want}");
    }
}
