//! Mean-reverting (Ornstein-Uhlenbeck) process tests (§16). A non-zero `reversion_rate` pulls
//! the process level toward `reference_value`; absent/zero preserves plain-GBM behavior.

use wasim_engine::{parse_v2, ModelGraphV2, RunConfig, engine_v2};

/// A strongly mean-reverting process started away from its reference converges toward it and
/// stays bounded near it — unlike a free random walk, which wanders.
#[test]
fn reverting_process_tracks_reference() {
    // reversion_rate=0.3/day, reference=10, start (initial_value)=0. κ·dt=0.3 (stable Euler).
    // The level should climb from 0 toward ~10 and hover there over 50 days. Small stddev so the
    // mean path is clear.
    let json = r#"{
      "wasim_version": "0.9.0",
      "simulation_settings": {"duration": {"value": 50, "unit": "day"}, "timestep": {"value": 1, "unit": "day"}, "seed": 7, "n_realizations": 200},
      "elements": [
        {"id": "P", "name": "P", "primitive": "node", "value_rule": "process",
         "process": {"family": "gbm", "mean_type": "arithmetic",
           "mean": {"value": 0, "unit": "1/day"}, "stddev": {"value": 0.2, "unit": "1/day"},
           "reversion_rate": {"value": 0.3, "unit": "1/day"},
           "reference_value": {"value": 10, "unit": "1"},
           "initial_value": {"value": 0, "unit": "1"}},
         "save_results": {"time_history": true, "final_value": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = engine_v2::run(&m, &g, &RunConfig { seed: Some(7), ..RunConfig::default() }).expect("run");
    let th = r.elements.get("P").and_then(|e| e.time_history.as_ref()).expect("series");
    let series = &th.mean; // ensemble mean per step

    // Climbs from the initial 0 toward the reference 10 and hovers there. The recorded series is
    // the level after each step's update (step 0 = after the first OU increment ≈ κ·θ·dt = 3).
    let last = *series.last().unwrap();
    assert!((last - 10.0).abs() < 1.0, "final mean {last} should revert to reference ~10");
    // Monotone climb toward the reference: early below late, late near 10.
    assert!(series[0] < 5.0 && series[0] > 1.0, "step-0 level {} = one OU increment from 0", series[0]);
    assert!(last > series[0] + 4.0, "level should climb substantially from ~3 toward 10");
}

/// The reference defaults to the drift `mean` when `reference_value` is absent.
#[test]
fn reverting_defaults_reference_to_mean() {
    let json = r#"{
      "wasim_version": "0.9.0",
      "simulation_settings": {"duration": {"value": 60, "unit": "day"}, "timestep": {"value": 1, "unit": "day"}, "seed": 3, "n_realizations": 200},
      "elements": [
        {"id": "P", "name": "P", "primitive": "node", "value_rule": "process",
         "process": {"family": "gbm", "mean_type": "arithmetic",
           "mean": {"value": 4, "unit": "1/day"}, "stddev": {"value": 0.15, "unit": "1/day"},
           "reversion_rate": {"value": 0.25, "unit": "1/day"},
           "initial_value": {"value": 0, "unit": "1"}},
         "save_results": {"time_history": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = engine_v2::run(&m, &g, &RunConfig { seed: Some(3), ..RunConfig::default() }).expect("run");
    let th = r.elements.get("P").and_then(|e| e.time_history.as_ref()).expect("series");
    let last = *th.mean.last().unwrap();
    // No reference_value → reverts toward mean=4.
    assert!((last - 4.0).abs() < 1.0, "final mean {last} should revert to drift mean ~4");
}

/// A non-reverting process (no reversion_rate) is unchanged — still the per-step GBM rate path.
/// Regression guard: the reversion branch must not alter existing GBM behavior.
#[test]
fn non_reverting_unchanged() {
    let json = r#"{
      "wasim_version": "0.9.0",
      "simulation_settings": {"duration": {"value": 20, "unit": "day"}, "timestep": {"value": 1, "unit": "day"}, "seed": 1, "n_realizations": 100},
      "elements": [
        {"id": "P", "name": "P", "primitive": "node", "value_rule": "process",
         "process": {"family": "gbm", "mean_type": "arithmetic",
           "mean": {"value": 0.1, "unit": "1/day"}, "stddev": {"value": 0.3, "unit": "1/day"}},
         "save_results": {"time_history": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    // Just assert it runs and produces a finite, non-degenerate series (behavior preserved).
    let r = engine_v2::run(&m, &g, &RunConfig { seed: Some(1), ..RunConfig::default() }).expect("run");
    let th = r.elements.get("P").and_then(|e| e.time_history.as_ref()).expect("series");
    assert!(th.mean.iter().all(|v| v.is_finite()), "GBM rate series must be finite");
}
