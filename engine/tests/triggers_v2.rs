//! Resampling-trigger tests: a sample node redraws when its trigger fires and holds
//! its value between firings. n_realizations = 1, so history = the single path.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn hist(json: &str) -> Vec<f64> {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    r.elements["r"].time_history.as_ref().unwrap().mean.clone()
}

#[test]
fn periodic_resampling_holds_between_firings() {
    // period 2 (dt 1): redraw at steps 2 and 4 only → [d0,d0,d1,d1,d2,d2].
    let h = hist(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 6, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 3},
        "elements": [{"id": "r", "name": "R", "primitive": "node", "value_rule": "sample",
          "distribution": {"family": "uniform", "parameters": {"min": {"value": 0, "unit": "1"}, "max": {"value": 1, "unit": "1"}}},
          "resampling": {"mode": "periodic", "period": {"value": 2, "unit": "d"}},
          "save_results": {"time_history": true}}]}"#,
    );
    assert_eq!(h.len(), 6);
    assert_eq!(h[0], h[1], "held over step 1");
    assert_eq!(h[2], h[3], "held over step 3");
    assert_eq!(h[4], h[5], "held over step 5");
    assert_ne!(h[0], h[2], "resampled at step 2");
    assert_ne!(h[2], h[4], "resampled at step 4");
}

#[test]
fn scheduled_resampling_fires_once() {
    // schedule [3]: redraw at step 3 only → [d0,d0,d0,d1,d1,d1].
    let h = hist(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 6, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 5},
        "elements": [{"id": "r", "name": "R", "primitive": "node", "value_rule": "sample",
          "distribution": {"family": "uniform", "parameters": {"min": {"value": 0, "unit": "1"}, "max": {"value": 1, "unit": "1"}}},
          "resampling": {"mode": "on_schedule", "schedule": [{"value": 3, "unit": "d"}]},
          "save_results": {"time_history": true}}]}"#,
    );
    assert_eq!(h[0], h[2], "held before the firing");
    assert_eq!(h[3], h[5], "held after the firing");
    assert_ne!(h[2], h[3], "resampled at step 3");
}
