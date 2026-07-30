// Validates schema_examples_manual/exposure_profile_each_step.json — the `each_step` mode of the
// conditional nested-submodel statistic `nested_stat`. The outer model simulates a factor forward; at
// EVERY timestep the inner submodel reprices a rolling position CONDITIONED on that step's factor
// level. The each_step nested element's time_history is therefore an EXPOSURE PROFILE over time: its
// per-step mean is the Expected-Exposure EE(t) and its p95 is the Potential-Future-Exposure PFE(t) —
// the core objects of counterparty-exposure / CVA analytics, and the per-(path,step) nested simulation
// the American tight dual needs.
//
// Checks (the inner conditional value is Black-Scholes, so the whole profile is checkable):
//  (1) EE(t) — the each_step nested mean — reproduces the closed-form BS profile at EVERY step.
//  (2) PFE(t) — the each_step nested p95 — reproduces the closed-form p95 profile at every step.
//  (3) The exposure profile genuinely GROWS over time (the factor's dispersion widens) — a real,
//      time-varying profile, not a constant.
//  (4) Per-path: the terminal exposure tracks the closed-form BS(S_H) value path-by-path (corr ~ 1).
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

fn mean(v: &[f64]) -> f64 { v.iter().sum::<f64>() / v.len() as f64 }
fn corr(x: &[f64], y: &[f64]) -> f64 {
    let (mx, my) = (mean(x), mean(y));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..x.len() {
        let (dx, dy) = (x[i] - mx, y[i] - my);
        sxy += dx * dy; sxx += dx * dx; syy += dy * dy;
    }
    sxy / (sxx.sqrt() * syy.sqrt())
}

// Heavy per-step nested Monte-Carlo (~67s) with fixed per-step tolerances that need
// the full sample count. Gated out of the default run for speed; run in CI /
// explicitly with `cargo test -- --ignored`.
#[ignore = "slow (~67s) per-step nested-MC accuracy smoke; run with --ignored / in CI"]
#[test]
fn each_step_nested_stat_builds_exposure_profile_matching_black_scholes() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/exposure_profile_each_step.json")).unwrap();
    let v1: WasimModel = serde_json::from_str(&json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    spec.elements = ["exposure_t", "bs_t"].iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { results_spec: Some(spec), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");

    let ee = res.elements["exposure_t"].time_history.as_ref().expect("exposure_t history");
    let bs = res.elements["bs_t"].time_history.as_ref().expect("bs_t history");
    let n = ee.mean.len().min(bs.mean.len());
    println!("step |   EE(t) nested |  EE(t) closed |  PFE(t) nested | PFE(t) closed");
    for s in 0..n {
        println!("  {s:>2} | {:>13.4} | {:>13.4} | {:>13.4} | {:>13.4}",
            ee.mean[s], bs.mean[s], ee.p95[s], bs.p95[s]);
    }

    // (1) EE(t) profile — the each_step nested mean — matches the closed-form BS profile at each step.
    for s in 0..n {
        assert!((ee.mean[s] - bs.mean[s]).abs() < 0.15,
            "EE(t) step {s}: nested {} vs closed-form {}", ee.mean[s], bs.mean[s]);
    }
    // (2) PFE(t) profile — the each_step nested p95 across scenarios — matches the closed-form p95.
    for s in 0..n {
        assert!((ee.p95[s] - bs.p95[s]).abs() < 0.6,
            "PFE(t) step {s}: nested {} vs closed-form {}", ee.p95[s], bs.p95[s]);
    }
    // (3) The exposure profile is a real, time-varying object: it grows as the factor disperses.
    assert!(ee.mean[n - 1] > ee.mean[0] + 0.5, "EE(t) should grow over the horizon: {:?}", ee.mean);
    assert!(ee.p95[n - 1] > ee.p95[0] + 2.0, "PFE(t) should grow over the horizon: {:?}", ee.p95);

    // (4) Per-path: the terminal exposure (final value) tracks the closed-form BS value path-by-path.
    let (pv, bv) = (res.elements["exposure_t"].final_values.clone(), res.elements["bs_t"].final_values.clone());
    let c = corr(&pv, &bv);
    println!("terminal-step per-path corr(exposure, BS) = {c:.5}");
    assert!(c > 0.99, "terminal exposure must track BS(S_H) per path (corr {c})");
    println!("each_step nested_stat: exposure profile EE(t)/PFE(t) reproduces Black-Scholes at every step.");
}
