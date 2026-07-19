//! B5 static dimensional analysis: `check_dimensions` detects inconsistencies; strict mode
//! rejects them; unknown units / unresolved refs are exempt (no false positives).

use wasim_engine::{parse_v2, run_v2, units, ModelGraphV2, RunConfig, UnitsMode};

fn run_strict(json: &str) -> Result<(), String> {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    let cfg = RunConfig { seed: Some(1), units: UnitsMode::Strict, ..RunConfig::default() };
    run_v2(&m, &g, &cfg).map(|_| ()).map_err(|e| e.to_string())
}

fn errors(json: &str) -> Vec<String> {
    let m = parse_v2(json).expect("parse");
    units::check_dimensions(&m)
}

/// Adding a length and a time is a dimensional error.
#[test]
fn add_incompatible_dimensions_flagged() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "len", "name": "Len", "primitive": "node", "value_rule": "fixed", "value": {"value": 3, "unit": "m"}},
        {"id": "tim", "name": "Tim", "primitive": "node", "value_rule": "fixed", "value": {"value": 2, "unit": "s"}},
        {"id": "bad", "name": "Bad", "primitive": "node", "value_rule": "expression", "inputs": ["len", "tim"],
         "outputs": [{"name": "Bad", "unit": "m"}],
         "expression": {"ast": {"op": "add", "left": {"op": "ref", "element_id": "len"}, "right": {"op": "ref", "element_id": "tim"}}}}
      ]}"#;
    let errs = errors(json);
    assert!(errs.iter().any(|e| e.contains("bad") && e.contains("incompatible")), "expected add mismatch, got {errs:?}");
    assert!(run_strict(json).is_err(), "strict mode must reject the model");
}

/// A consistent model (velocity = length / time, declared m/s) passes with no errors.
#[test]
fn consistent_model_passes() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "dist", "name": "Dist", "primitive": "node", "value_rule": "fixed", "value": {"value": 100, "unit": "m"}},
        {"id": "time", "name": "Time", "primitive": "node", "value_rule": "fixed", "value": {"value": 10, "unit": "s"}},
        {"id": "vel", "name": "Vel", "primitive": "node", "value_rule": "expression", "inputs": ["dist", "time"],
         "outputs": [{"name": "Vel", "unit": "m/s"}],
         "expression": {"ast": {"op": "divide", "left": {"op": "ref", "element_id": "dist"}, "right": {"op": "ref", "element_id": "time"}}}}
      ]}"#;
    assert!(errors(json).is_empty(), "consistent model should have no dimensional errors: {:?}", errors(json));
    assert!(run_strict(json).is_ok(), "strict mode must accept a consistent model");
}

/// The inferred expression dimension must match the declared output unit: dividing m by s gives
/// m/s, so declaring the output as `m` (length) is an error.
#[test]
fn inferred_vs_declared_output_mismatch() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "dist", "name": "Dist", "primitive": "node", "value_rule": "fixed", "value": {"value": 100, "unit": "m"}},
        {"id": "time", "name": "Time", "primitive": "node", "value_rule": "fixed", "value": {"value": 10, "unit": "s"}},
        {"id": "vel", "name": "Vel", "primitive": "node", "value_rule": "expression", "inputs": ["dist", "time"],
         "outputs": [{"name": "Vel", "unit": "m"}],
         "expression": {"ast": {"op": "divide", "left": {"op": "ref", "element_id": "dist"}, "right": {"op": "ref", "element_id": "time"}}}}
      ]}"#;
    let errs = errors(json);
    assert!(errs.iter().any(|e| e.contains("declared output dimension")), "expected declared-vs-inferred mismatch, got {errs:?}");
}

/// exp() of a dimensioned argument is an error (transcendentals require dimensionless input) —
/// this is the class of the corpus OU/GBM basis explosion.
#[test]
fn transcendental_of_dimensioned_arg_flagged() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "len", "name": "Len", "primitive": "node", "value_rule": "fixed", "value": {"value": 3, "unit": "m"}},
        {"id": "e", "name": "E", "primitive": "node", "value_rule": "expression", "inputs": ["len"],
         "outputs": [{"name": "E", "unit": "1"}],
         "expression": {"ast": {"op": "call", "fn": "exp", "args": [{"op": "ref", "element_id": "len"}]}}}
      ]}"#;
    let errs = errors(json);
    assert!(errs.iter().any(|e| e.contains("dimensionless argument")), "expected transcendental mismatch, got {errs:?}");
}

/// Unknown units make the subtree exempt — no false positive, and strict mode still loads the
/// model (so partially-emitted models with unrecognized units are not rejected).
#[test]
fn unknown_units_are_exempt() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "widgets", "name": "W", "primitive": "node", "value_rule": "fixed", "value": {"value": 5, "unit": "widgets"}},
        {"id": "sum", "name": "Sum", "primitive": "node", "value_rule": "expression", "inputs": ["widgets"],
         "outputs": [{"name": "Sum", "unit": "gadgets"}],
         "expression": {"ast": {"op": "add", "left": {"op": "ref", "element_id": "widgets"}, "right": {"op": "literal", "value": 1}}}}
      ]}"#;
    assert!(errors(json).is_empty(), "unknown units should be exempt, not errors: {:?}", errors(json));
    assert!(run_strict(json).is_ok(), "strict mode must still load a model with unrecognized units");
}

/// Comparison of incompatible dimensions is flagged (a mass < length predicate is a bug).
#[test]
fn comparison_of_incompatible_dimensions_flagged() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "mass", "name": "Mass", "primitive": "node", "value_rule": "fixed", "value": {"value": 3, "unit": "kg"}},
        {"id": "len", "name": "Len", "primitive": "node", "value_rule": "fixed", "value": {"value": 2, "unit": "m"}},
        {"id": "cmp", "name": "Cmp", "primitive": "node", "value_rule": "expression", "inputs": ["mass", "len"],
         "outputs": [{"name": "Cmp", "unit": "1"}],
         "expression": {"ast": {"op": "gt", "left": {"op": "ref", "element_id": "mass"}, "right": {"op": "ref", "element_id": "len"}}}}
      ]}"#;
    let errs = errors(json);
    assert!(errs.iter().any(|e| e.contains("comparison")), "expected comparison mismatch, got {errs:?}");
}

/// Warn mode (default) never rejects, even with a real dimensional bug.
#[test]
fn warn_mode_never_rejects() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "len", "name": "Len", "primitive": "node", "value_rule": "fixed", "value": {"value": 3, "unit": "m"}},
        {"id": "tim", "name": "Tim", "primitive": "node", "value_rule": "fixed", "value": {"value": 2, "unit": "s"}},
        {"id": "bad", "name": "Bad", "primitive": "node", "value_rule": "expression", "inputs": ["len", "tim"],
         "outputs": [{"name": "Bad", "unit": "m"}],
         "expression": {"ast": {"op": "add", "left": {"op": "ref", "element_id": "len"}, "right": {"op": "ref", "element_id": "tim"}}},
         "save_results": {"final_value": true}}
      ]}"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    // Default (warn) config runs despite the bug.
    assert!(run_v2(&m, &g, &RunConfig::default()).is_ok(), "warn mode must run despite dimensional bugs");
}
