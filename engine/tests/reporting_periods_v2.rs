//! B4 reporting-period aggregation: accumulated / average / change / rate-of-change over
//! fixed-length periods, exposed through A3's `results_spec`.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, ReportingReduction, ResultsSpec, RunConfig};

/// A deterministic ramp stock: level = t (rate 1/d, dt=1d) over 6 days, history saved.
fn ramp_results(period: f64, reductions: Vec<ReportingReduction>) -> wasim_engine::SimulationResults {
    let json = r#"{"wasim_version": "0.9.4",
      "simulation_settings": {"duration": {"value": 6, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "s", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"},
         "rate": {"value": 1, "unit": "1/d"}, "save_results": {"time_history": true}}
      ]}"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let spec = ResultsSpec { reporting_period: period, reporting_reductions: reductions, ..Default::default() };
    let cfg = RunConfig { seed: Some(1), results_spec: Some(spec), ..RunConfig::default() };
    run_v2(&m, &g, &cfg).unwrap()
}

#[test]
fn no_reporting_period_no_block() {
    // Default results_spec (period 0) → no reporting_periods block.
    let json = r#"{"wasim_version": "0.9.4",
      "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "s", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"},
         "rate": {"value": 1, "unit": "1/d"}, "save_results": {"time_history": true}}
      ]}"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    assert!(r.elements["s"].analysis.is_none(), "no spec → no analysis");
}

#[test]
fn period_grouping_and_change() {
    // dt=1, period=3 → 2 periods over 6 steps: steps [0,1,2] and [3,4,5].
    // The ramp stock records level at end of each step: step 0 level=1, step 1=2, ... step 5=6.
    let r = ramp_results(3.0, vec![ReportingReduction::Change, ReportingReduction::Average]);
    let rp = r.elements["s"].analysis.as_ref().unwrap().reporting_periods.as_ref().unwrap();
    assert_eq!(rp.period, 3.0);
    assert_eq!(rp.periods.len(), 2, "6 steps / 3-step periods = 2 periods");
    assert_eq!(rp.periods[0].start, 0.0);
    assert_eq!(rp.periods[1].start, 3.0);
    // Period 0 covers step means [1,2,3]: change = 3-1 = 2; average = 2.
    assert!((rp.periods[0].change.unwrap() - 2.0).abs() < 1e-9, "p0 change {:?}", rp.periods[0].change);
    assert!((rp.periods[0].average.unwrap() - 2.0).abs() < 1e-9, "p0 average {:?}", rp.periods[0].average);
    // Period 1 covers [4,5,6]: change = 6-4 = 2; average = 5.
    assert!((rp.periods[1].change.unwrap() - 2.0).abs() < 1e-9);
    assert!((rp.periods[1].average.unwrap() - 5.0).abs() < 1e-9);
}

#[test]
fn accumulated_is_time_integral() {
    // Accumulated over a period = Σ(value·dt). Period 0 step means [1,2,3], dt=1 → 1+2+3 = 6.
    let r = ramp_results(3.0, vec![ReportingReduction::Accumulated]);
    let rp = r.elements["s"].analysis.as_ref().unwrap().reporting_periods.as_ref().unwrap();
    assert!((rp.periods[0].accumulated.unwrap() - 6.0).abs() < 1e-9, "p0 acc {:?}", rp.periods[0].accumulated);
    // Period 1: [4,5,6] → 15.
    assert!((rp.periods[1].accumulated.unwrap() - 15.0).abs() < 1e-9);
    // Only accumulated requested → others absent.
    assert!(rp.periods[0].average.is_none());
    assert!(rp.periods[0].change.is_none());
}

#[test]
fn rate_of_change_normalizes_by_period_length() {
    // Period 0 change = 2 over a 3-day period → rate = 2/3.
    let r = ramp_results(3.0, vec![ReportingReduction::RateOfChange]);
    let rp = r.elements["s"].analysis.as_ref().unwrap().reporting_periods.as_ref().unwrap();
    assert!((rp.periods[0].rate_of_change.unwrap() - (2.0 / 3.0)).abs() < 1e-9, "p0 rate {:?}", rp.periods[0].rate_of_change);
}

#[test]
fn empty_reductions_emit_all() {
    // Empty reductions list → all four reductions present.
    let r = ramp_results(2.0, vec![]);
    let rp = r.elements["s"].analysis.as_ref().unwrap().reporting_periods.as_ref().unwrap();
    let p0 = &rp.periods[0];
    assert!(p0.accumulated.is_some() && p0.average.is_some() && p0.change.is_some() && p0.rate_of_change.is_some(),
        "empty reductions should emit all four");
}

#[test]
fn partial_final_period() {
    // dt=1, period=4, 6 steps → periods [0..4) and [4..6) (the last is partial: 2 steps).
    let r = ramp_results(4.0, vec![ReportingReduction::Average]);
    let rp = r.elements["s"].analysis.as_ref().unwrap().reporting_periods.as_ref().unwrap();
    assert_eq!(rp.periods.len(), 2, "6 steps / 4-step periods = 2 (last partial)");
    // Period 1 covers step means [5,6] → average 5.5.
    assert!((rp.periods[1].average.unwrap() - 5.5).abs() < 1e-9, "partial-period avg {:?}", rp.periods[1].average);
}
