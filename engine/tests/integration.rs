use std::fs;
use std::path::Path;

use wasim_engine::{run, ModelGraph, RunConfig, WasimModel};

fn load(json: &str) -> WasimModel {
    serde_json::from_str(json).expect("parse failed")
}

// ── Schema round-trip ─────────────────────────────────────────────────────────

#[test]
fn parse_all_schema_examples() {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("schema_examples");

    let mut count = 0;
    let mut failures = vec![];

    for entry in fs::read_dir(&examples_dir).expect("schema_examples not found") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let json = fs::read_to_string(&path).unwrap();
        match serde_json::from_str::<WasimModel>(&json) {
            Ok(_) => count += 1,
            Err(e) => failures.push(format!("{}: {e}", path.file_name().unwrap().to_string_lossy())),
        }
    }

    if !failures.is_empty() {
        panic!("Parse failures:\n{}", failures.join("\n"));
    }
    assert!(count >= 42, "expected ≥42 examples, found {count}");
}

#[test]
fn build_graph_for_all_examples() {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("schema_examples");

    let mut failures = vec![];

    for entry in fs::read_dir(&examples_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let json = fs::read_to_string(&path).unwrap();
        let model: WasimModel = serde_json::from_str(&json).unwrap();
        if let Err(e) = ModelGraph::build(&model) {
            failures.push(format!("{}: {e}", path.file_name().unwrap().to_string_lossy()));
        }
    }

    if !failures.is_empty() {
        panic!("Graph build failures:\n{}", failures.join("\n"));
    }
}

// ── Expression evaluator ──────────────────────────────────────────────────────

#[test]
fn constant_and_expression() {
    let json = r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {
            "duration": {"value": 1, "unit": "yr"},
            "timestep": {"value": 1, "unit": "yr"},
            "n_realizations": 1
        },
        "elements": [
            {
                "id": "a", "name": "A", "type": "constant",
                "value": {"value": 5.0, "unit": "1"},
                "save_results": {"final_value": true}
            },
            {
                "id": "b", "name": "B", "type": "constant",
                "value": {"value": 3.0, "unit": "1"}
            },
            {
                "id": "c", "name": "C", "type": "expression",
                "inputs": ["a", "b"],
                "expression": {
                    "ast": {
                        "op": "add",
                        "left": {"op": "ref", "element_id": "a"},
                        "right": {"op": "ref", "element_id": "b"}
                    }
                },
                "save_results": {"final_value": true}
            }
        ]
    }"#;

    let model = load(json);
    let graph = ModelGraph::build(&model).unwrap();
    let results = run(&model, &graph, &RunConfig::default()).unwrap();

    assert_eq!(results.elements["a"].final_values, vec![5.0]);
    assert_eq!(results.elements["c"].final_values, vec![8.0]);
}

// ── Monte Carlo: rainfall-runoff ──────────────────────────────────────────────

#[test]
fn rainfall_runoff_mc_mean() {
    // effective_rainfall = rainfall_rate * (1 - interception_frac)
    // rainfall_rate ~ lognormal(μ=ln(1200), σ=0.24...) [log-space params]
    // Real-space: mean=1200, stddev=300 → σ²=ln(1+(300/1200)²)=ln(1.0625)≈0.0606 → σ≈0.246
    // Expected mean(effective) ≈ 1200 * 0.85 = 1020, within 5%
    let sigma2: f64 = (1.0f64 + (300.0f64 / 1200.0f64).powi(2)).ln();
    let sigma = sigma2.sqrt();
    let mu = 1200.0f64.ln() - sigma2 / 2.0;

    let json = format!(
        r#"{{
        "wasim_version": "0.1.0",
        "simulation_settings": {{
            "duration": {{"value": 1, "unit": "yr"}},
            "timestep": {{"value": 1, "unit": "yr"}},
            "n_realizations": 2000,
            "seed": 42
        }},
        "elements": [
            {{
                "id": "rainfall_rate", "name": "Rainfall Rate",
                "type": "random_variable",
                "distribution": {{
                    "family": "lognormal",
                    "parameters": {{
                        "mean": {{"value": {mu}, "unit": "mm/yr"}},
                        "stddev": {{"value": {sigma}, "unit": "mm/yr"}}
                    }}
                }},
                "save_results": {{"final_value": true}}
            }},
            {{
                "id": "interception", "name": "Interception Fraction",
                "type": "constant",
                "value": {{"value": 0.15, "unit": "1"}}
            }},
            {{
                "id": "effective", "name": "Effective Rainfall",
                "type": "expression",
                "inputs": ["rainfall_rate", "interception"],
                "expression": {{
                    "ast": {{
                        "op": "multiply",
                        "left": {{"op": "ref", "element_id": "rainfall_rate"}},
                        "right": {{
                            "op": "subtract",
                            "left": {{"op": "literal", "value": 1.0}},
                            "right": {{"op": "ref", "element_id": "interception"}}
                        }}
                    }}
                }},
                "save_results": {{"final_value": true}}
            }}
        ]
    }}"#
    );

    let model = load(&json);
    let graph = ModelGraph::build(&model).unwrap();
    let results = run(&model, &graph, &RunConfig::default()).unwrap();

    let finals = &results.elements["effective"].final_values;
    assert_eq!(finals.len(), 2000);

    let computed_mean = finals.iter().sum::<f64>() / finals.len() as f64;
    let expected = 1020.0;
    let rel_err = (computed_mean - expected).abs() / expected;
    assert!(
        rel_err < 0.05,
        "mean effective rainfall {computed_mean:.1} is more than 5% from expected {expected:.1}"
    );
}

// ── Accumulator ───────────────────────────────────────────────────────────────

#[test]
fn accumulator_linear_growth() {
    // state starts at 0, rate = 2/yr, dt = 1 yr, 5 steps → final = 10
    let json = r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {
            "duration": {"value": 5, "unit": "yr"},
            "timestep": {"value": 1, "unit": "yr"},
            "n_realizations": 1
        },
        "elements": [
            {
                "id": "rate", "name": "Rate",
                "type": "constant",
                "value": {"value": 2.0, "unit": "1/yr"}
            },
            {
                "id": "stock", "name": "Stock",
                "type": "accumulator",
                "initial_value": {"value": 0.0, "unit": "1"},
                "rate": {
                    "ast": {"op": "ref", "element_id": "rate"}
                },
                "min_value": null,
                "inputs": ["rate"],
                "save_results": {"final_value": true}
            }
        ]
    }"#;

    let model = load(json);
    let graph = ModelGraph::build(&model).unwrap();
    let results = run(&model, &graph, &RunConfig::default()).unwrap();

    let final_val = results.elements["stock"].final_values[0];
    assert!(
        (final_val - 10.0).abs() < 1e-9,
        "expected 10.0, got {final_val}"
    );
}

// ── Lookup table ──────────────────────────────────────────────────────────────

#[test]
fn lookup_interpolation() {
    // lookup: x=[0,1,2], y=[0,10,20] → linear, so lookup_call(1.5) = 15.0
    let json = r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {
            "duration": {"value": 1, "unit": "yr"},
            "timestep": {"value": 1, "unit": "yr"},
            "n_realizations": 1
        },
        "elements": [
            {
                "id": "tbl", "name": "Table",
                "type": "lookup",
                "x_unit": "1", "y_unit": "1",
                "x": [0.0, 1.0, 2.0],
                "y": [0.0, 10.0, 20.0]
            },
            {
                "id": "inp", "name": "Input",
                "type": "constant",
                "value": {"value": 1.5, "unit": "1"}
            },
            {
                "id": "out", "name": "Output",
                "type": "expression",
                "inputs": ["inp"],
                "expression": {
                    "ast": {
                        "op": "lookup_call",
                        "element_id": "tbl",
                        "input": {"op": "ref", "element_id": "inp"}
                    }
                },
                "save_results": {"final_value": true}
            }
        ]
    }"#;

    let model = load(json);
    let graph = ModelGraph::build(&model).unwrap();
    let results = run(&model, &graph, &RunConfig::default()).unwrap();

    let v = results.elements["out"].final_values[0];
    assert!((v - 15.0).abs() < 1e-9, "expected 15.0, got {v}");
}
