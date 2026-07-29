//! Feature B (0.9.9): non-constant stock initial values (general solve). A stock's initial level
//! may be an expression over other elements — including other stocks' evaluated initials —
//! resolved by an init-only topological order with cycle fallback.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn run(json: &str) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    run_v2(&m, &g, &RunConfig { seed: Some(1), ..RunConfig::default() }).expect("run")
}

fn initial(r: &wasim_engine::SimulationResults, id: &str) -> f64 {
    // With no flows the stock holds its initial value throughout; read the first step.
    r.elements[id].time_history.as_ref().unwrap().mean[0]
}

/// Stock B initial = A * 2, where A is a stock with a scalar initial of 5. B must seed to 10 —
/// proving the evaluated stock initial (A) feeds back so B sees 5, not the scalar seed.
#[test]
fn stock_initial_references_another_stock_initial() {
    let json = r#"{"wasim_version": "0.9.9",
      "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "A", "name": "A", "primitive": "stock", "initial_value": {"value": 5, "unit": "1"},
         "save_results": {"time_history": true}},
        {"id": "B", "name": "B", "primitive": "stock",
         "initial_value": {"ast": {"op": "multiply",
            "left": {"op": "ref", "element_id": "A"}, "right": {"op": "literal", "value": 2}}},
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    assert!((initial(&r, "A") - 5.0).abs() < 1e-9, "A initial = 5");
    assert!((initial(&r, "B") - 10.0).abs() < 1e-9, "B initial = A*2 = 10, got {}", initial(&r, "B"));
}

/// Interleave: aux X = B_stock's initial; stock C initial = X + 1. Both an aux-references-stock and
/// a stock-references-aux edge must be ordered correctly. B=4 → X=4 → C=5.
#[test]
fn stock_initial_interleaves_with_aux() {
    let json = r#"{"wasim_version": "0.9.9",
      "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "B", "name": "B", "primitive": "stock", "initial_value": {"value": 4, "unit": "1"},
         "save_results": {"time_history": true}},
        {"id": "X", "name": "X", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "ref", "element_id": "B"}}},
        {"id": "C", "name": "C", "primitive": "stock",
         "initial_value": {"ast": {"op": "add",
            "left": {"op": "ref", "element_id": "X"}, "right": {"op": "literal", "value": 1}}},
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    assert!((initial(&r, "C") - 5.0).abs() < 1e-9, "C initial = X+1 = B+1 = 5, got {}", initial(&r, "C"));
}

/// A genuine cycle: A initial = B+1, B initial = A+1. Both fall back to their scalar (0) and the
/// run completes (no infinite loop). The key assertion is that it TERMINATES and produces finite
/// values.
#[test]
fn cyclic_stock_initials_fall_back_without_hanging() {
    let json = r#"{"wasim_version": "0.9.9",
      "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "A", "name": "A", "primitive": "stock",
         "initial_value": {"ast": {"op": "add",
            "left": {"op": "ref", "element_id": "B"}, "right": {"op": "literal", "value": 1}}},
         "save_results": {"time_history": true}},
        {"id": "B", "name": "B", "primitive": "stock",
         "initial_value": {"ast": {"op": "add",
            "left": {"op": "ref", "element_id": "A"}, "right": {"op": "literal", "value": 1}}},
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    // Cyclic → scalar fallback (the Expression variant seeds initial_value to 0).
    assert!(initial(&r, "A").is_finite() && initial(&r, "B").is_finite(), "finite, terminated");
    assert!((initial(&r, "A")).abs() < 1e-9 && (initial(&r, "B")).abs() < 1e-9,
        "cyclic initials fall back to scalar 0");
}

/// Regression: a plain scalar stock initial is unchanged.
#[test]
fn plain_scalar_stock_initial_unchanged() {
    let json = r#"{"wasim_version": "0.9.9",
      "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "S", "name": "S", "primitive": "stock", "initial_value": {"value": 42, "unit": "1"},
         "save_results": {"time_history": true}}
      ]}"#;
    let r = run(json);
    assert!((initial(&r, "S") - 42.0).abs() < 1e-9, "scalar initial 42 unchanged");
}
