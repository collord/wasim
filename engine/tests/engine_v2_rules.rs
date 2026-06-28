//! End-to-end tests for the net-new M2 node rules + gate + compound_growth, driven from
//! v2-native fixtures. n_realizations defaults to 1, so history mean = the single path.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// Run a v2-native model and return an element's per-step history mean.
fn hist(json: &str, id: &str) -> Vec<f64> {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    r.elements[id].time_history.as_ref().unwrap_or_else(|| panic!("{id} has no history")).mean.clone()
}

fn close(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len(), "len {} vs {}: {a:?} vs {b:?}", a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!((x - y).abs() < 1e-9, "[{i}] {x} vs {y} in {a:?}");
    }
}

const SETTINGS: &str =
    r#""simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}}"#;

/// An `elapsed`-time driver node (values 0,1,2,3,4 over the 5-step run).
const ELAPSED: &str = r#"{"id": "t", "name": "T", "primitive": "node", "value_rule": "expression",
  "expression": {"ast": {"op": "time_ref", "property": "elapsed"}}, "save_results": {"time_history": true}}"#;

#[test]
fn hysteresis_band() {
    // drive = [0,4,4,0,0] (step interp); high=3, low=1 → output [0,1,1,0,0].
    let json = format!(
        r#"{{ "wasim_version": "0.8.0", {SETTINGS}, "elements": [
        {{"id": "drive", "name": "Drive", "primitive": "node", "value_rule": "series",
         "timestamps": [0,1,2,3,4], "values": [0,4,4,0,0], "interpolation": "step"}},
        {{"id": "h", "name": "H", "primitive": "node", "value_rule": "hysteresis", "input": "drive",
         "high_threshold": {{"value": 3, "unit": "1"}}, "low_threshold": {{"value": 1, "unit": "1"}},
         "output_above": {{"value": 1, "unit": "1"}}, "output_below": {{"value": 0, "unit": "1"}},
         "save_results": {{"time_history": true}}}}
    ]}}"#
    );
    close(&hist(&json, "h"), &[0.0, 1.0, 1.0, 0.0, 0.0]);
}

#[test]
fn filter_rolling_mean() {
    // mean over window 3 of [0,1,2,3,4] → [0, .5, 1, 2, 3].
    let json = format!(
        r#"{{ "wasim_version": "0.8.0", {SETTINGS}, "elements": [
        {ELAPSED},
        {{"id": "f", "name": "F", "primitive": "node", "value_rule": "filter", "input": "t",
         "window": 3, "statistic": "mean", "save_results": {{"time_history": true}}}}
    ]}}"#
    );
    close(&hist(&json, "f"), &[0.0, 0.5, 1.0, 2.0, 3.0]);
}

#[test]
fn filter_ema() {
    // α = 2/(3+1) = 0.5; ema over [0,1,2,3,4] → [0, .5, 1.25, 2.125, 3.0625].
    let json = format!(
        r#"{{ "wasim_version": "0.8.0", {SETTINGS}, "elements": [
        {ELAPSED},
        {{"id": "e", "name": "E", "primitive": "node", "value_rule": "filter", "input": "t",
         "window": 3, "statistic": "ema", "save_results": {{"time_history": true}}}}
    ]}}"#
    );
    close(&hist(&json, "e"), &[0.0, 0.5, 1.25, 2.125, 3.0625]);
}

#[test]
fn convolution_two_tap() {
    // response [1,1] → output(t) = input(t) + input(t-1); input = [0,1,2,3,4].
    let json = format!(
        r#"{{ "wasim_version": "0.8.0", {SETTINGS}, "elements": [
        {ELAPSED},
        {{"id": "c", "name": "C", "primitive": "node", "value_rule": "convolution", "input": "t",
         "response": {{"times": [0,1], "values": [1,1]}}, "save_results": {{"time_history": true}}}}
    ]}}"#
    );
    close(&hist(&json, "c"), &[0.0, 1.0, 3.0, 5.0, 7.0]);
}

#[test]
fn markov_forced_transition() {
    // From state 0, row [0,1] forces state 1; output current-then-advance → [0,1,1,1,1].
    let json = format!(
        r#"{{ "wasim_version": "0.8.0", {SETTINGS}, "elements": [
        {{"id": "m", "name": "M", "primitive": "node", "value_rule": "markov",
         "states": ["a", "b"], "initial_state": "a",
         "transition_matrix": [[0,1],[0,1]], "output_values": [0,1],
         "save_results": {{"time_history": true}}}}
    ]}}"#
    );
    close(&hist(&json, "m"), &[0.0, 1.0, 1.0, 1.0, 1.0]);
}

#[test]
fn gate_logic_n_vote() {
    // n_vote(2 of [elapsed≥1, elapsed≥2, elapsed≥3]) over [0..4] → [0,0,1,1,1].
    let cond = |k: i32| format!(
        r#"{{"op": "condition", "condition": {{"ast": {{"op": "gte",
            "left": {{"op": "time_ref", "property": "elapsed"}}, "right": {{"op": "literal", "value": {k}}}}}}}}}"#
    );
    let json = format!(
        r#"{{ "wasim_version": "0.8.0", {SETTINGS}, "elements": [
        {{"id": "g", "name": "G", "primitive": "node", "value_rule": "gate_logic",
         "root": {{"op": "n_vote", "threshold": 2, "children": [{}, {}, {}]}},
         "save_results": {{"time_history": true}}}}
    ]}}"#,
        cond(1), cond(2), cond(3)
    );
    close(&hist(&json, "g"), &[0.0, 0.0, 1.0, 1.0, 1.0]);
}

#[test]
fn gate_primitive_and_not() {
    // and(elapsed≥1, not(elapsed≥3)) over [0..4] → [0,1,1,0,0].
    let json = format!(
        r#"{{ "wasim_version": "0.8.0", {SETTINGS}, "elements": [
        {{"id": "g", "name": "G", "primitive": "gate",
         "root": {{"op": "and", "children": [
            {{"op": "condition", "condition": {{"ast": {{"op": "gte",
               "left": {{"op": "time_ref", "property": "elapsed"}}, "right": {{"op": "literal", "value": 1}}}}}}}},
            {{"op": "not", "children": [{{"op": "condition", "condition": {{"ast": {{"op": "gte",
               "left": {{"op": "time_ref", "property": "elapsed"}}, "right": {{"op": "literal", "value": 3}}}}}}}}]}}
         ]}},
         "save_results": {{"time_history": true}}}}
    ]}}"#
    );
    close(&hist(&json, "g"), &[0.0, 1.0, 1.0, 0.0, 0.0]);
}

#[test]
fn compound_growth_stock() {
    // 100 growing 10%/step for 3 steps → 110, 121, 133.1.
    let json = r#"{ "wasim_version": "0.8.0",
      "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "fund", "name": "Fund", "primitive": "stock",
         "initial_value": {"value": 100, "unit": "1"},
         "return_rate": {"value": 0.1, "unit": "1/d"},
         "save_results": {"final_value": true, "time_history": true}}
    ]}"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    close(&r.elements["fund"].final_values, &[133.1]);
    close(&r.elements["fund"].time_history.as_ref().unwrap().mean, &[110.0, 121.0, 133.1]);
}
