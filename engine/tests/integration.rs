use std::fs;
use std::path::{Path, PathBuf};

use wasim_engine::{
    normalize_v1, parse_v2, run, simulate_json, EngineError, ModelGraph, ModelGraphV2, ModelParams,
    ModelV2, RunConfig, WasimModel,
};

/// The corpus is a mix of v1 and v2-native models. Detect the format (first element carries a
/// `primitive` field ⇒ v2-native) and load into the v2 primitive model either way.
fn is_v2_native(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| {
            v.get("elements")
                .and_then(|e| e.as_array())
                .and_then(|a| a.first())
                .map(|f| f.get("primitive").is_some())
        })
        .unwrap_or(false)
}

fn load_v2(json: &str) -> Result<ModelV2, String> {
    if is_v2_native(json) {
        parse_v2(json).map_err(|e| e.to_string())
    } else {
        serde_json::from_str::<WasimModel>(json)
            .map(|m| normalize_v1(&m))
            .map_err(|e| e.to_string())
    }
}

// ── Rank-correlation (Gaussian copula) ───────────────────────────────────────

/// Spearman rank correlation of two equal-length samples.
fn spearman(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len();
    assert_eq!(n, ys.len());
    let ranks = |vals: &[f64]| -> Vec<f64> {
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by(|&a, &b| vals[a].partial_cmp(&vals[b]).unwrap());
        let mut r = vec![0.0f64; n];
        for (rank, &i) in idx.iter().enumerate() { r[i] = (rank + 1) as f64; }
        r
    };
    let rx = ranks(xs);
    let ry = ranks(ys);
    let mx: f64 = rx.iter().sum::<f64>() / n as f64;
    let my: f64 = ry.iter().sum::<f64>() / n as f64;
    let num: f64 = (0..n).map(|i| (rx[i] - mx) * (ry[i] - my)).sum();
    let den: f64 = ((0..n).map(|i| (rx[i] - mx).powi(2)).sum::<f64>()
                  * (0..n).map(|i| (ry[i] - my).powi(2)).sum::<f64>()).sqrt();
    if den < 1e-12 { 0.0 } else { num / den }
}

#[test]
fn rank_correlation_recovers_spearman_rho() {
    // Two normal variables with a declared Spearman ρ = 0.7.
    // Over 5000 realizations the sample Spearman ρ should land within 0.05.
    // Y specifies no correlations field; X lists Y — asymmetric is fine.
    let json = r#"{
      "wasim_version": "0.1.0",
      "simulation_settings": {
        "duration": {"value": 1, "unit": "yr"},
        "timestep": {"value": 1, "unit": "yr"},
        "n_realizations": 5000,
        "seed": 99
      },
      "elements": [
        {
          "id": "X", "name": "X", "type": "random_variable",
          "distribution": {
            "family": "normal",
            "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}
          },
          "correlations": [{"partner": "Y", "coefficient": 0.7}],
          "save_results": {"final_value": true}
        },
        {
          "id": "Y", "name": "Y", "type": "random_variable",
          "distribution": {
            "family": "normal",
            "parameters": {"mean": {"value": 5, "unit": "1"}, "stddev": {"value": 2, "unit": "1"}}
          },
          "save_results": {"final_value": true}
        }
      ]
    }"#;

    let model = load(json);
    let graph = ModelGraph::build(&model).unwrap();
    let r = run(&model, &graph, &RunConfig::default()).unwrap();
    let xs = &r.elements["X"].final_values;
    let ys = &r.elements["Y"].final_values;
    let rho = spearman(xs, ys);
    eprintln!("rank_correlation: Spearman ρ̂ = {rho:.4} (target 0.7)");
    assert!((rho - 0.7).abs() < 0.05, "expected ρ̂ ≈ 0.7, got {rho:.4}");
}

#[test]
fn rank_correlation_marginals_preserved() {
    // Correlated lognormal variable: marginal mean and stddev should still match
    // the specified distribution regardless of the copula structure.
    // lognormal_moments: mean=100, stddev=30 → E[X] = 100.
    let json = r#"{
      "wasim_version": "0.1.0",
      "simulation_settings": {
        "duration": {"value": 1, "unit": "yr"},
        "timestep": {"value": 1, "unit": "yr"},
        "n_realizations": 4000,
        "seed": 7
      },
      "elements": [
        {
          "id": "A", "name": "A", "type": "random_variable",
          "distribution": {
            "family": "lognormal_moments",
            "parameters": {"mean": {"value": 100, "unit": "1"}, "stddev": {"value": 30, "unit": "1"}}
          },
          "correlations": [{"partner": "B", "coefficient": 0.8}],
          "save_results": {"final_value": true}
        },
        {
          "id": "B", "name": "B", "type": "random_variable",
          "distribution": {
            "family": "normal",
            "parameters": {"mean": {"value": 50, "unit": "1"}, "stddev": {"value": 10, "unit": "1"}}
          },
          "save_results": {"final_value": true}
        }
      ]
    }"#;

    let model = load(json);
    let graph = ModelGraph::build(&model).unwrap();
    let r = run(&model, &graph, &RunConfig::default()).unwrap();

    let a_vals = &r.elements["A"].final_values;
    let b_vals = &r.elements["B"].final_values;
    let n = a_vals.len() as f64;

    let a_mean = a_vals.iter().sum::<f64>() / n;
    let b_mean = b_vals.iter().sum::<f64>() / n;
    eprintln!("marginals: A mean={a_mean:.2} (target 100), B mean={b_mean:.2} (target 50)");

    assert!((a_mean - 100.0).abs() / 100.0 < 0.05, "A marginal mean off: {a_mean:.2}");
    assert!((b_mean -  50.0).abs() /  50.0 < 0.05, "B marginal mean off: {b_mean:.2}");

    // Spearman ρ should also be near 0.8
    let rho = spearman(a_vals, b_vals);
    eprintln!("marginals: Spearman ρ̂ = {rho:.4} (target 0.8)");
    assert!((rho - 0.8).abs() < 0.05, "expected ρ̂ ≈ 0.8, got {rho:.4}");
}

#[test]
fn rank_correlation_non_psd_matrix_rejected() {
    // Three variables with mutually inconsistent correlations (det < 0 → not PSD).
    // ρ(A,B)=0.9, ρ(B,C)=0.9, ρ(A,C)=-0.9 → det ≈ -2.9.
    // run() should return InvalidModel before any sampling occurs.
    let json = r#"{
      "wasim_version": "0.1.0",
      "simulation_settings": {
        "duration": {"value": 1, "unit": "yr"},
        "timestep": {"value": 1, "unit": "yr"},
        "n_realizations": 10
      },
      "elements": [
        {
          "id": "A", "name": "A", "type": "random_variable",
          "distribution": {
            "family": "normal",
            "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}
          },
          "correlations": [
            {"partner": "B", "coefficient": 0.9},
            {"partner": "C", "coefficient": -0.9}
          ],
          "save_results": {"final_value": true}
        },
        {
          "id": "B", "name": "B", "type": "random_variable",
          "distribution": {
            "family": "normal",
            "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}
          },
          "correlations": [{"partner": "C", "coefficient": 0.9}],
          "save_results": {"final_value": true}
        },
        {
          "id": "C", "name": "C", "type": "random_variable",
          "distribution": {
            "family": "normal",
            "parameters": {"mean": {"value": 0, "unit": "1"}, "stddev": {"value": 1, "unit": "1"}}
          },
          "save_results": {"final_value": true}
        }
      ]
    }"#;

    let model = load(json);
    let graph = ModelGraph::build(&model).unwrap();
    let err = run(&model, &graph, &RunConfig::default())
        .err()
        .expect("expected InvalidModel for non-PSD correlation matrix");
    let msg = err.to_string();
    eprintln!("non-PSD error: {msg}");
    assert!(msg.contains("positive semi-definite"), "unexpected error: {msg}");
}

fn load(json: &str) -> WasimModel {
    serde_json::from_str(json).expect("parse failed")
}

/// Transpiled schema examples (OpenVSim output). Override with `WASIM_SCHEMA_EXAMPLES`.
fn openvsim_examples_dir() -> PathBuf {
    std::env::var("WASIM_SCHEMA_EXAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("HOME not set");
            PathBuf::from(home).join("openvsim/wasim/schema_examples")
        })
}

/// Manually authored example fixtures kept in-repo for engine tests.
fn manual_examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("schema_examples_manual")
}

// ── Schema round-trip ─────────────────────────────────────────────────────────

#[test]
fn parse_all_schema_examples() {
    let examples_dir = openvsim_examples_dir();
    if !examples_dir.exists() {
        eprintln!("skipping parse_all_schema_examples: {} not present", examples_dir.display());
        return;
    }

    let mut count = 0;
    let mut failures = vec![];

    for entry in fs::read_dir(&examples_dir).expect("schema_examples not found") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let json = fs::read_to_string(&path).unwrap();
        // Load via the format that matches each file (v1 or v2-native).
        match load_v2(&json) {
            Ok(_) => count += 1,
            Err(e) => failures.push(format!("{}: {e}", path.file_name().unwrap().to_string_lossy())),
        }
    }

    if !failures.is_empty() {
        panic!("Parse failures:\n{}", failures.join("\n"));
    }
    assert!(count >= 100, "expected ≥100 examples, found {count}");
}

#[test]
fn build_graph_for_all_examples() {
    let examples_dir = openvsim_examples_dir();
    if !examples_dir.exists() {
        eprintln!("skipping build_graph_for_all_examples: {} not present", examples_dir.display());
        return;
    }

    let mut failures = vec![];

    for entry in fs::read_dir(&examples_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let json = fs::read_to_string(&path).unwrap();
        match load_v2(&json) {
            Err(e) => failures.push(format!("{name}: load: {e}")),
            Ok(model) => match ModelGraphV2::build(&model) {
                Ok(_) => {}
                // v2-native cycles are rejected by design (semantics §9); that's not a failure.
                Err(EngineError::CycleDetected(_)) => {}
                Err(e) => failures.push(format!("{name}: graph: {e}")),
            },
        }
    }

    if !failures.is_empty() {
        panic!("Graph build failures:\n{}", failures.join("\n"));
    }
}

// ── Parameterised run ─────────────────────────────────────────────────────────

/// Run a model with an optional params file applied.
///
/// Usage:
///   WASIM_MODEL=/path/to/model.json WASIM_PARAMS=/path/to/model.params.json \
///     cargo test run_with_params -- --nocapture
///
/// If WASIM_MODEL is not set the test skips.  WASIM_PARAMS is optional.
#[test]
fn run_with_params() {
    let model_path = match std::env::var("WASIM_MODEL") {
        Ok(p) => PathBuf::from(p),
        Err(_) => {
            eprintln!("skipping run_with_params: WASIM_MODEL not set");
            return;
        }
    };

    let model_json = fs::read_to_string(&model_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", model_path.display()));
    let mut model = load(&model_json);

    let mut run_config = RunConfig::default();

    if let Ok(params_path) = std::env::var("WASIM_PARAMS") {
        let params_json = fs::read_to_string(&params_path)
            .unwrap_or_else(|e| panic!("cannot read {params_path}: {e}"));
        let params = ModelParams::from_json(&params_json)
            .unwrap_or_else(|e| panic!("cannot parse params: {e}"));
        params.apply(&mut model);
        run_config = params.merge_run_config(run_config);
        eprintln!("applied params from {params_path}");
    }

    let graph = ModelGraph::build(&model).expect("graph build failed");
    if !graph.skipped_cycle_ids.is_empty() {
        eprintln!("skipped cyclic elements: {}", graph.skipped_cycle_ids.join(", "));
    }

    let results = run(&model, &graph, &run_config).expect("simulation failed");

    eprintln!(
        "model: {}  |  {} elements  |  {} steps  |  {} realizations",
        model_path.display(),
        results.elements.len(),
        results.n_steps,
        results.n_realizations,
    );
    for id in &results.output_ids {
        if let Some(el) = results.elements.get(id) {
            let final_mean = el.final_values.iter().sum::<f64>() / el.final_values.len().max(1) as f64;
            eprintln!("  {id} ({}) final mean = {final_mean:.6}", el.unit);
        }
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

#[test]
fn two_tank_hydraulic_oscillation() {
    // This model uses bistable pipe-flow hysteresis to produce a relaxation oscillator.
    // Tank 1 rises on part-full (air-entrained) flow, primes once submergence above
    // the pipe crown exceeds S_crit×D, then drains on full pressurized flow until the
    // crown is exposed again. Expected period ~8 min (steady-state with Tank 2 full).
    let json = fs::read_to_string(
        manual_examples_dir().join("two_tank_hydraulic.json")
    ).unwrap();
    let model = load(&json);
    let graph = ModelGraph::build(&model).unwrap();
    let results = run(&model, &graph, &RunConfig::default()).unwrap();

    let h1_hist  = results.elements["h1"].time_history.as_ref().unwrap();
    let h2_hist  = results.elements["h2"].time_history.as_ref().unwrap();
    let pp_hist  = results.elements["pipe_prime"].time_history.as_ref().unwrap();

    let max_h1: f64 = h1_hist.mean.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let ever_primed   = pp_hist.mean.iter().any(|&v| v > 0.9);
    let ever_unprimed = pp_hist.mean.iter().skip(50).any(|&v| v < 0.1);
    let h2_final = *h2_hist.mean.last().unwrap();

    println!("max h1={max_h1:.3} ft");
    println!("ever_primed={ever_primed}  ever_unprimed_after_init={ever_unprimed}");
    println!("h2 final={h2_final:.3} ft");

    // h1 oscillates below capacity — never hits the 2-ft rim
    assert!(max_h1 < 1.9, "h1 should not reach capacity, got {max_h1:.3}");
    // pipe actually primes at least once
    assert!(ever_primed, "pipe_prime never reached 1 — oscillation did not trigger");
    // pipe breaks prime and re-enters unprimed regime (relaxation, not one-shot)
    assert!(ever_unprimed, "pipe stayed primed forever — no oscillation back");
    // Tank 2 fills substantially over 1 hour
    assert!(h2_final > 1.5, "Tank 2 should be nearly full after 1 hr, got {h2_final:.3}");
}

#[test]
fn predatorprey_runs_without_error() {
    let path = openvsim_examples_dir().join("predatorprey1.json");
    if !path.exists() { eprintln!("skipping: {} not present", path.display()); return; }
    let json = fs::read_to_string(&path).unwrap();
    let results = simulate_json(&json, &RunConfig::default()).unwrap();
    // Just check it doesn't crash; values may be degenerate
    eprintln!("predatorprey element count: {}", results.elements.len());
    for (id, el) in &results.elements {
        if let Some(hist) = &el.time_history {
            let last = hist.mean.last().unwrap_or(&f64::NAN);
            eprintln!("  {id}: final={last:.6}");
        }
    }
}

#[test]
fn array_models_run_without_error() {
    let cases = [
        "cemaneige_snow_model.json",
        "demonstration_llw_sa_model_v1_15.json",
        "hydropower_optimization.json",
        "minewaterbalance.json",
        "minmaxvector.json",
        "oil_sands_production.json",
        "plume.json",
        "populationgrowthagingchain.json",
        "precipgen.json",
        "randomsequencegenerator.json",
        "simplemixing.json",
        "srm_snowmelt_runoff.json",
        "watershed_yield_nrcs.json",
        "wind_model_parameters.json",
    ];
    // Models that are expected to run today with the lightweight array support.
    // The remaining models need either a Value enum refactor (vector-valued
    // expressions or array-state accumulators) or unrelated features (script).
    let must_pass: &[&str] = &[
        "hydropower_optimization.json",
        "minmaxvector.json",
        "precipgen.json",
        "randomsequencegenerator.json",
        "watershed_yield_nrcs.json",
    ];
    let mut passed = 0;
    let mut failed = Vec::new();
    let dir = openvsim_examples_dir();
    if !dir.exists() {
        eprintln!("skipping: {} not present", dir.display());
        return;
    }
    for name in cases {
        let path = dir.join(name);
        let json = fs::read_to_string(&path).expect(name);
        match simulate_json(&json, &RunConfig::default()) {
            Ok(_) => { eprintln!("{name}: ok"); passed += 1; }
            Err(e) => {
                eprintln!("{name}: FAIL — {e:?}");
                failed.push(name);
                assert!(!must_pass.contains(&name), "regression: {name} used to pass");
            }
        }
    }
    eprintln!("\n{} passed, {} failed", passed, failed.len());
}

#[test]
fn random_variable_with_autocorrelation_resamples_per_step() {
    // Same RV, two configurations: with and without autocorrelation field.
    // The one with autocorrelation should vary across timesteps; the other should not.
    let make_model = |with_autocorr: bool| {
        let autocorr_field = if with_autocorr { r#","autocorrelation":0"# } else { "" };
        format!(r#"{{
          "wasim_version": "0.1.0",
          "simulation_settings": {{
            "duration": {{"value": 10, "unit": "d"}},
            "timestep": {{"value": 1, "unit": "d"}},
            "n_realizations": 1,
            "seed": 42
          }},
          "elements": [
            {{
              "id": "U",
              "name": "U",
              "type": "random_variable",
              "distribution": {{
                "family": "uniform",
                "parameters": {{
                  "mean": {{"value": 0, "unit": "1"}},
                  "stddev": {{"value": 0, "unit": "1"}},
                  "min": {{"value": 0, "unit": "1"}},
                  "max": {{"value": 1, "unit": "1"}}
                }}
              }}{autocorr_field},
              "save_results": {{"final_value": false, "time_history": true}}
            }}
          ]
        }}"#)
    };

    let json_one_shot = make_model(false);
    let model = load(&json_one_shot);
    let graph = ModelGraph::build(&model).unwrap();
    let r = run(&model, &graph, &RunConfig::default()).unwrap();
    let h = r.elements["U"].time_history.as_ref().unwrap();
    let first = h.mean[0];
    let all_same = h.mean.iter().all(|&v| v == first);
    assert!(all_same, "without autocorrelation, RV should be constant across timesteps");

    let json_per_step = make_model(true);
    let model = load(&json_per_step);
    let graph = ModelGraph::build(&model).unwrap();
    let r = run(&model, &graph, &RunConfig::default()).unwrap();
    let h = r.elements["U"].time_history.as_ref().unwrap();
    let first = h.mean[0];
    let any_diff = h.mean.iter().any(|&v| v != first);
    assert!(any_diff, "with autocorrelation, RV should vary across timesteps");
}

#[test]
fn random_variable_autocorrelation_recovers_rho() {
    // Generate a long series with ρ = 0.7 and verify the sample lag-1
    // autocorrelation lands near 0.7. Uses uniform distribution to exercise
    // the inverse-CDF path through the standard normal CDF.
    let json = r#"{
      "wasim_version": "0.1.0",
      "simulation_settings": {
        "duration": {"value": 5000, "unit": "d"},
        "timestep": {"value": 1, "unit": "d"},
        "n_realizations": 1,
        "seed": 7
      },
      "elements": [
        {
          "id": "X",
          "name": "X",
          "type": "random_variable",
          "distribution": {
            "family": "normal",
            "parameters": {
              "mean":   {"value": 0,   "unit": "1"},
              "stddev": {"value": 1.0, "unit": "1"}
            }
          },
          "autocorrelation": 0.7,
          "save_results": {"final_value": false, "time_history": true}
        }
      ]
    }"#;
    let model = load(json);
    let graph = ModelGraph::build(&model).unwrap();
    let r = run(&model, &graph, &RunConfig::default()).unwrap();
    let h = r.elements["X"].time_history.as_ref().unwrap();
    let xs = &h.mean;
    let n = xs.len();
    let mean: f64 = xs.iter().sum::<f64>() / n as f64;
    let var: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    let cov: f64 = (0..n - 1).map(|i| (xs[i] - mean) * (xs[i + 1] - mean)).sum::<f64>() / (n - 1) as f64;
    let rho_hat = cov / var;
    eprintln!("AR(1) test: n={n}, mean={mean:.3}, var={var:.3}, ρ̂={rho_hat:.3}");
    assert!((rho_hat - 0.7).abs() < 0.05, "expected ρ̂ ≈ 0.7, got {rho_hat:.3}");
}

