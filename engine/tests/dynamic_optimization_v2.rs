//! Dynamic (per-timestep) optimization tests (§13a). A submodel-scoped optimization is
//! re-solved each outer timestep, so the optimized variable becomes a time series.

use wasim_engine::{parse_v2, ModelGraphV2, RunConfig};
use wasim_engine::engine_v2;

/// The GoldSim "Dynamic Optimization" corpus model (support.goldsim.com 360047679353): a
/// submodel-scoped optimization minimizes `(Parameter − √Driver)²` where Driver oscillates over
/// the run. The expected optimum is `Parameter = √Driver(t)` at every step — a time series, not
/// the single static scalar the top-level (study) optimization would produce. This is the
/// end-to-end acceptance test for §13a on the re-emitted 0.9.0 corpus file.
#[test]
fn corpus_dynamic_optimization_tracks_sqrt_driver() {
    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
        .join("openvsim/wasim/schema_examples");
    let p = dir.join("dynamicoptimization.json");
    if !p.exists() {
        eprintln!("skipping: corpus not present");
        return;
    }
    let m = parse_v2(&std::fs::read_to_string(&p).unwrap()).expect("parse");
    // The optimization must be submodel-scoped (dynamic), not top-level (static study).
    assert!(m.optimization.is_none(), "top-level optimization should be empty (it moved under the submodel)");
    assert!(m.containers.iter().any(|c| c.optimization.is_some()), "a submodel should carry the optimization");

    let graph = ModelGraphV2::build(&m).expect("graph");
    let r = engine_v2::run(&m, &graph, &RunConfig { seed: Some(42), ..RunConfig::default() }).expect("run");

    let param = r.elements.get("Model/SubModel1/Parameter")
        .and_then(|e| e.time_history.as_ref()).expect("Parameter series");
    let driver = r.elements.get("Model/Driver")
        .and_then(|e| e.time_history.as_ref()).expect("Driver series");
    let (ps, ds) = (&param.p50, &driver.p50);
    assert!(ps.len() > 50, "expected a per-step series");

    // Every step: Parameter ≈ √Driver(t).
    let maxerr = ps.iter().zip(ds).map(|(p, d)| (p - d.sqrt()).abs()).fold(0.0_f64, f64::max);
    assert!(maxerr < 0.05, "Parameter should track √Driver each step; maxerr={maxerr}");
    // And it must actually vary (be a series), not sit at a constant.
    let (pmin, pmax) = (ps.iter().cloned().fold(f64::INFINITY, f64::min),
                        ps.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    assert!(pmax - pmin > 1.0, "Parameter should vary over the run (tracking √Driver), not be constant");
}

/// Minimal dynamic-optimization model: a submodel minimizes `(x − target)²`, where `target`
/// is a time-varying interface input `2·ETime_days`. The optimum x at step t is `target(t)`,
/// so the recovered `x` series must track `2·t` per step — NOT a single static scalar.
///
/// We run the SUBMODEL directly (the level the per-step solve operates at) by extracting it via
/// the public run path: build the parent, then assert the submodel's `x` output series.
#[test]
fn recovers_time_varying_optimum_series() {
    // Parent: Driver = 2 * ETime(days). Submodel: x (fixed, the opt variable) drives
    // ObjFunc = (x - Driver)^2, minimized. Submodel is single-step (duration 0) but re-run
    // by the parent each outer step with the current Driver — the classic dynamic-opt shape.
    let json = r#"{
      "wasim_version": "0.9.0",
      "simulation_settings": {"duration": {"value": 5, "unit": "day"}, "timestep": {"value": 1, "unit": "day"}, "seed": 1},
      "elements": [
        {"id": "Model/Driver", "name": "Driver", "container": "Model", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "multiply", "left": {"op": "literal", "value": 2},
           "right": {"op": "time_ref", "property": "elapsed"}}},
         "save_results": {"time_history": true}},
        {"id": "Model/Sub/DriverIn", "name": "DriverIn", "container": "Model/Sub", "primitive": "node", "value_rule": "fixed",
         "value": {"value": 0, "unit": "1"}},
        {"id": "Model/Sub/x", "name": "x", "container": "Model/Sub", "primitive": "node", "value_rule": "fixed",
         "value": {"value": 0, "unit": "1"}, "editable": true},
        {"id": "Model/Sub/ObjFunc", "name": "ObjFunc", "container": "Model/Sub", "primitive": "node", "value_rule": "expression",
         "inputs": ["Model/Sub/x", "Model/Sub/DriverIn"],
         "expression": {"ast": {"op": "power",
           "left": {"op": "subtract", "left": {"op": "ref", "element_id": "Model/Sub/x"}, "right": {"op": "ref", "element_id": "Model/Sub/DriverIn"}},
           "right": {"op": "literal", "value": 2}}},
         "save_results": {"final_value": true}}
      ],
      "containers": [
        {"id": "Model", "name": "Model", "children": ["Model/Sub"], "elements": ["Model/Driver"]},
        {"id": "Model/Sub", "name": "Sub", "parent": "Model", "kind": "submodel",
         "elements": ["Model/Sub/DriverIn", "Model/Sub/x", "Model/Sub/ObjFunc"],
         "simulation_settings": {"duration": {"value": 0, "unit": "day"}, "timestep": {"value": 1, "unit": "day"}, "n_realizations": 1},
         "interface": {"inputs": [{"input": "Model/Sub/DriverIn", "from": "Model/Driver"}], "outputs": ["Model/Sub/x"]},
         "optimization": {
           "objective": {"element_id": "Model/Sub/ObjFunc", "direction": "minimize"},
           "variables": [{"element_id": "Model/Sub/x", "lower": {"value": -50, "unit": "1"}, "upper": {"value": 50, "unit": "1"}, "initial": {"value": 0, "unit": "1"}}]
         }}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    // Directly run the extracted submodel to observe its per-step x series. extract_submodel is
    // private, so drive through the whole-model engine and read the submodel's element results.
    let graph = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { seed: Some(1), ..RunConfig::default() };
    let results = engine_v2::run(&m, &graph, &cfg).expect("run");

    // The submodel's x series should equal Driver(t) = 2·t at each step: [0, 2, 4, 6, 8].
    // The parent surfaces the submodel output under its interface-output id.
    let x = results.elements.get("Model/Sub/x")
        .and_then(|e| e.time_history.as_ref())
        .expect("x series present");
    let got = &x.p50; // deterministic → all percentiles equal the single value
    let expected = [0.0, 2.0, 4.0, 6.0, 8.0];
    assert_eq!(got.len(), expected.len(), "series length");
    for (i, (&g, e)) in got.iter().zip(expected).enumerate() {
        assert!((g - e).abs() < 0.05, "step {i}: x={g} expected {e} (should track 2·t)");
    }
}
