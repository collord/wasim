// Throwaway smoke test: verify schema_examples_manual/options_pricing_efficiency.json runs through
// the v2 engine (the one the wasm/frontend uses via normalize_v1 -> run_v2), that the exact-simulation
// MC price matches closed-form Black-Scholes within Monte Carlo error, that antithetic variates reduce
// the estimator variance, and that the CI half-width scales ~1/sqrt(N).
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

fn stats(json: &str, n: u32, id: &str) -> (f64, f64, f64) {
    let v1: WasimModel = serde_json::from_str(json).expect("parse v1 model");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    spec.elements =
        vec!["est_plain".into(), "est_antithetic".into(), "cv_est".into(), "bs_price".into()];
    let config = RunConfig {
        n_realizations: Some(n),
        seed: Some(12345),
        results_spec: Some(spec),
        ..Default::default()
    };
    let results = run_v2(&model, &graph, &config).expect("run_v2");
    let fs = results.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
    (fs.mean, fs.std, fs.ci_half_width)
}

#[test]
fn options_pricing_efficiency_runs_and_matches_black_scholes() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../schema_examples_manual/options_pricing_efficiency.json"
    ))
    .unwrap();

    // Closed-form reference (deterministic element -> mean is the exact price).
    let (bs, _, _) = stats(&json, 20_000, "bs_price");
    println!("Black-Scholes price = {bs:.6}");
    assert!((bs - 10.4506).abs() < 1e-2, "BS price off: {bs}");

    // Plain vs antithetic at N = 20k (override the model's default 100k to keep the test quick).
    let (m_plain, s_plain, ci_plain) = stats(&json, 20_000, "est_plain");
    let (m_anti, s_anti, ci_anti) = stats(&json, 20_000, "est_antithetic");
    println!("plain      : mean={m_plain:.6} std={s_plain:.6} ci_half={ci_plain:.6}");
    println!("antithetic : mean={m_anti:.6} std={s_anti:.6} ci_half={ci_anti:.6}");

    // Both estimators unbiased: mean within a few CI half-widths of BS.
    assert!((m_plain - bs).abs() < 4.0 * ci_plain, "plain biased vs BS: {m_plain} vs {bs}");
    assert!((m_anti - bs).abs() < 4.0 * ci_anti, "antithetic biased vs BS: {m_anti} vs {bs}");

    // Antithetic reduces variance (per-realization std) for this monotone payoff.
    println!("variance-reduction factor (std_plain/std_anti) = {:.3}", s_plain / s_anti);
    assert!(s_anti < s_plain, "antithetic should have smaller std");

    // Control variate (run_stat2 beta): variance reduction from a correlated control with
    // known mean. The discounted terminal price is a strong control for a call payoff.
    let (m_cv, s_cv, ci_cv) = stats(&json, 20_000, "cv_est");
    println!("control-var : mean={m_cv:.6} std={s_cv:.6} ci_half={ci_cv:.6}");
    assert!((m_cv - bs).abs() < 4.0 * ci_cv, "CV estimator biased vs BS: {m_cv} vs {bs}");
    assert!(s_cv < s_plain, "control variate should reduce std ({s_cv} !< {s_plain})");
    println!("CV variance-reduction factor (std_plain/std_cv) = {:.3}", s_plain / s_cv);

    // CI half-width scales ~ 1/sqrt(N): 4x fewer realizations -> ~2x wider.
    let (_, _, ci_5k) = stats(&json, 5_000, "est_plain");
    let ratio = ci_5k / ci_plain; // expect ~2.0
    println!("ci(5k)/ci(20k) = {ratio:.3} (expect ~2.0 for 1/sqrt(N))");
    assert!((ratio - 2.0).abs() < 0.35, "sqrt(N) scaling off: {ratio}");
}
