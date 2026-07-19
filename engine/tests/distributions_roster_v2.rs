//! A4 distribution roster additions (GoldSim parity): moment/shape checks per new family at
//! large n, plus truncation, LHS interaction, and the external-distribution error policy.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}
fn stddev(v: &[f64]) -> f64 {
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

/// Build a one-sample-node model with the given distribution JSON fragment.
fn model(dist: &str, n: u32, seed: u64) -> String {
    format!(
        r#"{{"wasim_version": "0.9.2",
          "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}},
            "n_realizations": {n}, "seed": {seed}}},
          "elements": [
            {{"id": "x", "name": "X", "primitive": "node", "value_rule": "sample",
             "distribution": {dist}, "save_results": {{"final_value": true}}}}
          ]}}"#
    )
}

fn draws(dist: &str, n: u32, seed: u64) -> Vec<f64> {
    let json = model(dist, n, seed);
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    r.elements["x"].final_values.clone()
}

#[test]
fn log_uniform_moments_and_support() {
    // ln(X) ~ U(ln 1, ln 100). Mean = (100-1)/ln(100) ≈ 21.49.
    let d = draws(r#"{"family": "log_uniform", "parameters": {"min": {"value": 1, "unit": "1"}, "max": {"value": 100, "unit": "1"}}}"#, 50000, 1);
    assert!(d.iter().all(|&x| x >= 1.0 && x <= 100.0), "log_uniform out of [1,100]");
    let expected = (100.0 - 1.0) / (100.0_f64).ln();
    assert!((mean(&d) - expected).abs() < 1.0, "log_uniform mean {} not ≈{expected}", mean(&d));
}

#[test]
fn log_triangular_support() {
    let d = draws(r#"{"family": "log_triangular", "parameters": {"min": {"value": 1, "unit": "1"}, "mode": {"value": 4, "unit": "1"}, "max": {"value": 100, "unit": "1"}}}"#, 20000, 2);
    assert!(d.iter().all(|&x| x >= 1.0 && x <= 100.0), "log_triangular out of support");
    // Mode of the log-triangular (in real space) exceeds 1 and the distribution is right-skewed.
    assert!(mean(&d) > 4.0, "log_triangular mean {} should exceed the log-mode", mean(&d));
}

#[test]
fn triangular_1090_matches_percentiles() {
    // p10 = 2, mode = 5, p90 = 12. Check the empirical 10th/90th percentiles land near target.
    let mut d = draws(r#"{"family": "triangular1090", "parameters": {"p10": {"value": 2, "unit": "1"}, "mode": {"value": 5, "unit": "1"}, "p90": {"value": 12, "unit": "1"}}}"#, 100000, 3);
    d.sort_by(f64::total_cmp);
    let p10 = d[(0.10 * d.len() as f64) as usize];
    let p90 = d[(0.90 * d.len() as f64) as usize];
    assert!((p10 - 2.0).abs() < 0.3, "empirical p10 {p10} not ≈2");
    assert!((p90 - 12.0).abs() < 0.4, "empirical p90 {p90} not ≈12");
}

#[test]
fn log_triangular_1090_matches_percentiles() {
    let mut d = draws(r#"{"family": "log_triangular1090", "parameters": {"p10": {"value": 2, "unit": "1"}, "mode": {"value": 5, "unit": "1"}, "p90": {"value": 20, "unit": "1"}}}"#, 100000, 4);
    d.sort_by(f64::total_cmp);
    let p10 = d[(0.10 * d.len() as f64) as usize];
    let p90 = d[(0.90 * d.len() as f64) as usize];
    assert!((p10 - 2.0).abs() < 0.3, "empirical p10 {p10} not ≈2");
    assert!((p90 - 20.0).abs() < 1.0, "empirical p90 {p90} not ≈20");
    assert!(d.iter().all(|&x| x > 0.0), "log_triangular1090 must be positive");
}

#[test]
fn log_cumulative_interpolates_in_log_space() {
    // Breakpoints at (x=1, p=0), (x=1000, p=1): the median (p=0.5) is the geometric mean ≈ 31.6.
    let d = draws(r#"{"family": "log_cumulative", "parameters": {"points": [
        {"x": 1, "cumulative_probability": 0.0}, {"x": 1000, "cumulative_probability": 1.0}]}}"#, 60000, 5);
    let mut s = d.clone();
    s.sort_by(f64::total_cmp);
    let median = s[s.len() / 2];
    assert!((median - 31.6).abs() < 3.0, "log_cumulative median {median} not ≈31.6 (geo mean)");
    assert!(d.iter().all(|&x| x >= 1.0 && x <= 1000.0), "log_cumulative out of support");
}

#[test]
fn binomial_moments() {
    // Binomial(20, 0.3): mean = 6, var = n p (1-p) = 4.2, sd ≈ 2.049.
    let d = draws(r#"{"family": "binomial", "parameters": {"n": {"value": 20, "unit": "1"}, "prob": {"value": 0.3, "unit": "1"}}}"#, 50000, 6);
    assert!((mean(&d) - 6.0).abs() < 0.1, "binomial mean {} not ≈6", mean(&d));
    assert!((stddev(&d) - 2.049).abs() < 0.1, "binomial sd {} not ≈2.05", stddev(&d));
    assert!(d.iter().all(|&x| x >= 0.0 && x <= 20.0 && x == x.round()), "binomial support/integrality");
}

#[test]
fn negative_binomial_moments() {
    // NegBinom(r=5, p=0.4) as #failures before 5th success: mean = r(1-p)/p = 7.5, var = r(1-p)/p^2 = 18.75.
    let d = draws(r#"{"family": "negative_binomial", "parameters": {"r": {"value": 5, "unit": "1"}, "prob": {"value": 0.4, "unit": "1"}}}"#, 80000, 7);
    assert!((mean(&d) - 7.5).abs() < 0.3, "neg-binom mean {} not ≈7.5", mean(&d));
    assert!((stddev(&d) - 18.75_f64.sqrt()).abs() < 0.4, "neg-binom sd {} not ≈4.33", stddev(&d));
    assert!(d.iter().all(|&x| x >= 0.0 && x == x.round()), "neg-binom non-negative integers");
}

#[test]
fn poisson_moments() {
    // Poisson(4): mean = var = 4, sd = 2.
    let d = draws(r#"{"family": "poisson", "parameters": {"lambda": {"value": 4, "unit": "1"}}}"#, 50000, 8);
    assert!((mean(&d) - 4.0).abs() < 0.1, "poisson mean {} not ≈4", mean(&d));
    assert!((stddev(&d) - 2.0).abs() < 0.1, "poisson sd {} not ≈2", stddev(&d));
    assert!(d.iter().all(|&x| x >= 0.0 && x == x.round()), "poisson non-negative integers");
}

#[test]
fn extreme_probability_max_shifts_right() {
    // Max of 10 draws from Uniform(0,1): mean = N/(N+1) = 10/11 ≈ 0.909.
    let d = draws(r#"{"family": "extreme_probability", "parameters": {
        "base": {"family": "uniform", "parameters": {"min": {"value": 0, "unit": "1"}, "max": {"value": 1, "unit": "1"}}},
        "n": {"value": 10, "unit": "1"}, "extreme": "max"}}"#, 50000, 9);
    assert!((mean(&d) - 10.0 / 11.0).abs() < 0.01, "extreme max mean {} not ≈0.909", mean(&d));
    assert!(d.iter().all(|&x| x >= 0.0 && x <= 1.0), "extreme max out of base support");
}

#[test]
fn extreme_probability_min_shifts_left() {
    // Min of 10 draws from Uniform(0,1): mean = 1/(N+1) ≈ 0.0909.
    let d = draws(r#"{"family": "extreme_probability", "parameters": {
        "base": {"family": "uniform", "parameters": {"min": {"value": 0, "unit": "1"}, "max": {"value": 1, "unit": "1"}}},
        "n": {"value": 10, "unit": "1"}, "extreme": "min"}}"#, 50000, 10);
    assert!((mean(&d) - 1.0 / 11.0).abs() < 0.01, "extreme min mean {} not ≈0.091", mean(&d));
}

#[test]
fn beta_success_failure_reparameterizes() {
    // Beta(successes=7, failures=3) => Beta(8, 4): mean = 8/12 = 0.6667.
    let d = draws(r#"{"family": "beta_success_failure", "parameters": {"successes": {"value": 7, "unit": "1"}, "failures": {"value": 3, "unit": "1"}}}"#, 50000, 11);
    assert!((mean(&d) - 8.0 / 12.0).abs() < 0.01, "beta(succ/fail) mean {} not ≈0.667", mean(&d));
    assert!(d.iter().all(|&x| x >= 0.0 && x <= 1.0), "beta out of [0,1]");
}

#[test]
fn beta_success_failure_scaled() {
    // Scaled onto [10, 20]: mean = 10 + 10 * 0.6667 = 16.667.
    let d = draws(r#"{"family": "beta_success_failure", "parameters": {"successes": {"value": 7, "unit": "1"}, "failures": {"value": 3, "unit": "1"},
        "min": {"value": 10, "unit": "1"}, "max": {"value": 20, "unit": "1"}}}"#, 50000, 12);
    assert!((mean(&d) - 16.667).abs() < 0.1, "scaled beta mean {} not ≈16.67", mean(&d));
    assert!(d.iter().all(|&x| x >= 10.0 && x <= 20.0), "scaled beta out of [10,20]");
}

#[test]
fn log_uniform_truncation_respected() {
    let d = draws(r#"{"family": "log_uniform", "parameters": {"min": {"value": 1, "unit": "1"}, "max": {"value": 100, "unit": "1"}},
        "truncation": {"min": 10.0, "max": 50.0}}"#, 20000, 13);
    assert!(d.iter().all(|&x| x >= 10.0 - 1e-9 && x <= 50.0 + 1e-9), "log_uniform truncation not respected");
}

#[test]
fn log_uniform_stratifies_under_lhs() {
    // LHS on log_uniform (which has a closed-form ICDF): exactly one per bin at n = 100.
    let json = format!(
        r#"{{"wasim_version": "0.9.2",
          "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}},
            "n_realizations": 100, "seed": 21, "sampling_method": "lhs"}},
          "elements": [
            {{"id": "x", "name": "X", "primitive": "node", "value_rule": "sample",
             "distribution": {{"family": "log_uniform", "parameters": {{"min": {{"value": 1, "unit": "1"}}, "max": {{"value": 1000, "unit": "1"}}}}}},
             "save_results": {{"final_value": true}}}}
          ]}}"#
    );
    let m = parse_v2(&json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    let d = &r.elements["x"].final_values;
    // Map each draw back to its log-uniform quantile; check one per decile-of-decile bin.
    let (lmin, lmax) = (1.0_f64.ln(), 1000.0_f64.ln());
    let mut bins = [0u32; 100];
    for &x in d {
        let q = (x.ln() - lmin) / (lmax - lmin);
        bins[((q * 100.0).floor() as usize).min(99)] += 1;
    }
    assert!(bins.iter().all(|&c| c == 1), "log_uniform not stratified under LHS: {bins:?}");
}

#[test]
fn external_without_fallback_degrades_with_warning() {
    // With no inline fallback the engine cannot sample an external (DLL/spreadsheet)
    // distribution; it degrades to 0.0 with a warning rather than erroring, so corpus models
    // still emitting `external` as a placeholder keep running (see EMIT_ISSUES doc). The hard
    // error is deferred until emit re-emits these as concrete families.
    let json = model(r#"{"family": "external", "parameters": {"definition": "some.dll"}}"#, 10, 1);
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("external should degrade, not error");
    assert!(r.elements["x"].final_values.iter().all(|&v| v == 0.0), "external degrade should be 0.0");
}

#[test]
fn external_with_fallback_samples_table() {
    // With an inline empirical fallback, external samples that table instead of erroring.
    let json = model(r#"{"family": "external", "parameters": {"definition": "some.dll",
        "fallback": {"samples": [10.0, 20.0, 30.0]}}}"#, 30000, 2);
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let d = &r.elements["x"].final_values;
    assert!(d.iter().all(|&x| x == 10.0 || x == 20.0 || x == 30.0), "fallback sampled off-table");
    assert!((mean(d) - 20.0).abs() < 1.0, "fallback mean {} not ≈20", mean(d));
}
