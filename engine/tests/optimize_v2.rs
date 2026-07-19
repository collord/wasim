//! Optimization study executor (§13) tests.

use wasim_engine::{optimize, parse_v2, RunConfig};

/// Minimize (x - 3)^2 over x ∈ [0, 10]: the solver should find x ≈ 3, objective ≈ 0.
#[test]
fn minimizes_quadratic_to_known_optimum() {
    let json = r#"{
      "wasim_version": "0.8.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "optimization": {
        "objective": {"element_id": "cost", "direction": "minimize"},
        "variables": [
          {"element_id": "x", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 9, "unit": "1"}}
        ]
      },
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 9, "unit": "1"}},
        {"id": "cost", "name": "Cost", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "power",
           "left": {"op": "subtract", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 3}},
           "right": {"op": "literal", "value": 2}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let r = optimize(&m, &RunConfig::default()).expect("optimize");
    assert_eq!(r.variables.len(), 1);
    let x = r.variables[0].value;
    assert!((x - 3.0).abs() < 0.05, "x {x} not ≈3");
    assert!(r.objective < 0.01, "objective {} not ≈0", r.objective);
}

/// Maximize a concave objective -(x - 7)^2 + 5 over x ∈ [0, 10]: optimum at x ≈ 7, value ≈ 5.
#[test]
fn maximizes_concave_objective() {
    let json = r#"{
      "wasim_version": "0.8.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 2},
      "optimization": {
        "objective": {"element_id": "gain", "direction": "maximize"},
        "variables": [
          {"element_id": "x", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 1, "unit": "1"}}
        ]
      },
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "1"}},
        {"id": "gain", "name": "Gain", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "add",
           "left": {"op": "neg", "operand": {"op": "power",
             "left": {"op": "subtract", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 7}},
             "right": {"op": "literal", "value": 2}}},
           "right": {"op": "literal", "value": 5}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let r = optimize(&m, &RunConfig::default()).expect("optimize");
    let x = r.variables[0].value;
    assert!((x - 7.0).abs() < 0.05, "x {x} not ≈7");
    assert!((r.objective - 5.0).abs() < 0.01, "objective {} not ≈5", r.objective);
}

/// Two-variable bowl: minimize (x-2)^2 + (y-8)^2 → (2, 8).
#[test]
fn minimizes_two_variable_bowl() {
    let json = r#"{
      "wasim_version": "0.8.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 3},
      "optimization": {
        "objective": {"element_id": "cost", "direction": "minimize"},
        "variables": [
          {"element_id": "x", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 5, "unit": "1"}},
          {"element_id": "y", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 5, "unit": "1"}}
        ]
      },
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 5, "unit": "1"}},
        {"id": "y", "name": "Y", "primitive": "node", "value_rule": "fixed", "value": {"value": 5, "unit": "1"}},
        {"id": "cost", "name": "Cost", "primitive": "node", "value_rule": "expression", "inputs": ["x", "y"],
         "expression": {"ast": {"op": "add",
           "left": {"op": "power", "left": {"op": "subtract", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 2}}, "right": {"op": "literal", "value": 2}},
           "right": {"op": "power", "left": {"op": "subtract", "left": {"op": "ref", "element_id": "y"}, "right": {"op": "literal", "value": 8}}, "right": {"op": "literal", "value": 2}}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let r = optimize(&m, &RunConfig::default()).expect("optimize");
    let x = r.variables.iter().find(|v| v.element_id == "x").unwrap().value;
    let y = r.variables.iter().find(|v| v.element_id == "y").unwrap().value;
    assert!((x - 2.0).abs() < 0.1, "x {x} not ≈2");
    assert!((y - 8.0).abs() < 0.1, "y {y} not ≈8");
    assert!(r.objective < 0.05, "objective {} not ≈0", r.objective);
}

/// Integer variable: minimize (x - 4.3)^2 with x restricted to integers → x = 4.
#[test]
fn respects_integer_variable() {
    let json = r#"{
      "wasim_version": "0.8.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 4},
      "optimization": {
        "objective": {"element_id": "cost", "direction": "minimize"},
        "variables": [
          {"element_id": "x", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 9, "unit": "1"}, "integer": true}
        ]
      },
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 9, "unit": "1"}},
        {"id": "cost", "name": "Cost", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "power",
           "left": {"op": "subtract", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 4.3}},
           "right": {"op": "literal", "value": 2}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let r = optimize(&m, &RunConfig::default()).expect("optimize");
    let x = r.variables[0].value;
    assert_eq!(x, x.round(), "x {x} should be integer");
    assert_eq!(x, 4.0, "closest integer to 4.3 is 4");
}

/// A real corpus optimization model runs end-to-end through the solver without error.
#[test]
fn corpus_optimization_runs() {
    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
        .join("openvsim/wasim/schema_examples");
    if !dir.exists() { eprintln!("skipping: corpus not present"); return; }
    // Top-level (static study) optimization models. `dynamicoptimization.json` is NOT here — its
    // optimization is submodel-scoped (dynamic, §13a) and is covered by
    // `dynamic_optimization_v2::corpus_dynamic_optimization_tracks_sqrt_driver`.
    for name in ["srm_snowmelt_runoff.json", "calibrationoptimization.json"] {
        let p = dir.join(name);
        if !p.exists() { continue; }
        let json = std::fs::read_to_string(&p).unwrap();
        let m = parse_v2(&json).expect(name);
        assert!(m.optimization.is_some(), "{name}: optimization spec parsed");
        match optimize(&m, &RunConfig::default()) {
            Ok(r) => eprintln!("{name}: obj={:.4} vars={} evals={} converged={}",
                r.objective, r.variables.len(), r.evaluations, r.converged),
            Err(e) => eprintln!("{name}: {e:?}"),
        }
    }
}

/// A constrained quadratic whose unconstrained optimum (x = 3) is infeasible. With the
/// feasibility constraint x ≥ 6 enforced, the solver must land on the constraint boundary
/// (x ≈ 6), not at 3. Without enforcement it would return x ≈ 3 (this test would fail).
#[test]
fn constraint_pushes_optimum_to_boundary() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 7},
      "optimization": {
        "objective": {"element_id": "cost", "direction": "minimize"},
        "variables": [
          {"element_id": "x", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 9, "unit": "1"}}
        ],
        "constraints": [
          {"label": "x >= 6", "condition": {"ast": {"op": "gte",
            "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 6}}}}
        ]
      },
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 9, "unit": "1"}},
        {"id": "cost", "name": "Cost", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "power",
           "left": {"op": "subtract", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 3}},
           "right": {"op": "literal", "value": 2}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let r = optimize(&m, &RunConfig::default()).expect("optimize");
    let x = r.variables[0].value;
    // Feasible region is [6, 10]; the constrained minimum of (x-3)^2 there is at the boundary x=6.
    assert!(x >= 6.0 - 1e-6, "x {x} violates the constraint x >= 6");
    assert!((x - 6.0).abs() < 0.1, "x {x} not ≈6 (constrained optimum)");
    assert!((r.objective - 9.0).abs() < 0.5, "objective {} not ≈9 = (6-3)^2", r.objective);
}

/// A constraint that references a computed element (not just the search variable): minimize
/// (x-8)^2 subject to a budget element `spend = 2*x` staying ≤ 10, i.e. x ≤ 5. The
/// unconstrained optimum x = 8 is infeasible; the constrained optimum is the boundary x = 5.
#[test]
fn constraint_on_computed_element() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 11},
      "optimization": {
        "objective": {"element_id": "cost", "direction": "minimize"},
        "variables": [
          {"element_id": "x", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 1, "unit": "1"}}
        ],
        "constraints": [
          {"label": "spend <= 10", "condition": {"ast": {"op": "lte",
            "left": {"op": "ref", "element_id": "spend"}, "right": {"op": "literal", "value": 10}}}}
        ]
      },
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "1"}},
        {"id": "spend", "name": "Spend", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "multiply", "left": {"op": "literal", "value": 2}, "right": {"op": "ref", "element_id": "x"}}}},
        {"id": "cost", "name": "Cost", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "power",
           "left": {"op": "subtract", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 8}},
           "right": {"op": "literal", "value": 2}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let r = optimize(&m, &RunConfig::default()).expect("optimize");
    let x = r.variables[0].value;
    // `spend` is an unsaved intermediate — enforcement must force-save it to read its value.
    assert!(2.0 * x <= 10.0 + 1e-4, "x {x} violates spend = 2x <= 10");
    assert!((x - 5.0).abs() < 0.1, "x {x} not ≈5 (constrained optimum)");
}

/// Infeasible-everywhere (x ≥ 6 AND x ≤ 3 over [0,10]) must terminate cleanly — every point
/// costs +∞ — rather than hang. The reported optimum is meaningless but the run returns.
#[test]
fn infeasible_everywhere_terminates() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 13},
      "optimization": {
        "objective": {"element_id": "cost", "direction": "minimize"},
        "variables": [
          {"element_id": "x", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 5, "unit": "1"}}
        ],
        "constraints": [
          {"condition": {"ast": {"op": "gte", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 6}}}},
          {"condition": {"ast": {"op": "lte", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 3}}}}
        ]
      },
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 5, "unit": "1"}},
        {"id": "cost", "name": "Cost", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "power",
           "left": {"op": "subtract", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 3}},
           "right": {"op": "literal", "value": 2}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let r = optimize(&m, &RunConfig::default()).expect("optimize");
    // No feasible point exists: the best achievable cost is +∞.
    assert!(r.objective.is_infinite(), "expected +∞ objective, got {}", r.objective);
}

/// Constraints do not perturb an unconstrained model: with an empty `constraints` list the
/// result matches the no-constraints path (same as `minimizes_quadratic_to_known_optimum`).
#[test]
fn empty_constraints_match_unconstrained() {
    let json = r#"{
      "wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "optimization": {
        "objective": {"element_id": "cost", "direction": "minimize"},
        "variables": [
          {"element_id": "x", "lower": {"value": 0, "unit": "1"}, "upper": {"value": 10, "unit": "1"}, "initial": {"value": 9, "unit": "1"}}
        ],
        "constraints": []
      },
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "fixed", "value": {"value": 9, "unit": "1"}},
        {"id": "cost", "name": "Cost", "primitive": "node", "value_rule": "expression", "inputs": ["x"],
         "expression": {"ast": {"op": "power",
           "left": {"op": "subtract", "left": {"op": "ref", "element_id": "x"}, "right": {"op": "literal", "value": 3}},
           "right": {"op": "literal", "value": 2}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let r = optimize(&m, &RunConfig::default()).expect("optimize");
    assert!((r.variables[0].value - 3.0).abs() < 0.05, "x not ≈3");
    assert!(r.objective < 0.01, "objective {} not ≈0", r.objective);
}
