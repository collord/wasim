// Validates schema_examples_manual/correlated_assets_spread_option.json (normalize_v1 -> run_v2).
// Two correlated GBM assets (2x2 Cholesky construction, Glasserman Sec 2.3), a spread option, and
// the exchange option (Margrabe) as a control variate. Checks: (1) the realized correlation of the
// two drivers reproduces the input rho; (2) the exchange-option MC price matches the live Margrabe
// closed form within MC error — a stringent check that the correlated simulation is right (the
// Margrabe price depends on rho); (3) the exchange control variate sharply reduces the spread
// estimator's variance; (4) the CV estimator stays unbiased.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, std: f64, ci: f64 }
fn run(json: &str, n: u32) -> std::collections::HashMap<String, FS> {
    let model = normalize_v1(&serde_json::from_str::<WasimModel>(json).expect("parse"));
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    let ids = ["spread_payoff", "exchange_payoff", "cv_spread", "exchange_price", "realized_rho"];
    spec.elements = ids.iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { n_realizations: Some(n), seed: Some(20240707), results_spec: Some(spec), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    ids.iter().map(|id| {
        let fs = res.elements[*id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        (id.to_string(), FS { mean: fs.mean, std: fs.std, ci: fs.ci_half_width })
    }).collect()
}

#[test]
fn correlated_spread_option_matches_margrabe_and_reduces_variance() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/correlated_assets_spread_option.json")).unwrap();
    let s = run(&json, 40_000);
    let (spread, exch, cv, mg, rho) = (&s["spread_payoff"], &s["exchange_payoff"], &s["cv_spread"], &s["exchange_price"], &s["realized_rho"]);
    println!("realized_rho     = {:.4} (target 0.5)", rho.mean);
    println!("margrabe (exact) = {:.6}", mg.mean);
    println!("exchange MC      : mean={:.6} std={:.6} ci={:.6}", exch.mean, exch.std, exch.ci);
    println!("spread plain     : mean={:.6} std={:.6} ci={:.6}", spread.mean, spread.std, spread.ci);
    println!("spread CV        : mean={:.6} std={:.6} ci={:.6}", cv.mean, cv.std, cv.ci);

    // (1) The Cholesky construction reproduces the input correlation rho=0.5.
    assert!((rho.mean - 0.5).abs() < 0.02, "realized correlation {} should be ~0.5", rho.mean);

    // (2) The exchange-option MC price matches the live Margrabe closed form within MC error —
    // validating the correlated simulation (Margrabe depends on rho).
    assert!((exch.mean - mg.mean).abs() < 4.0 * exch.ci,
        "exchange MC {} vs Margrabe {} (ci {})", exch.mean, mg.mean, exch.ci);
    assert!(mg.mean > 1.0, "Margrabe price should be a sensible positive value, got {}", mg.mean);

    // (3) The exchange control variate sharply reduces the spread estimator's variance.
    println!("variance-reduction factor (std_plain/std_cv) = {:.2}", spread.std / cv.std);
    assert!(cv.std < spread.std / 3.0, "CV should cut std >3x: plain {} vs cv {}", spread.std, cv.std);

    // (4) CV estimator is unbiased for the spread price.
    assert!((cv.mean - spread.mean).abs() < 4.0 * (spread.ci + cv.ci),
        "CV mean {} should agree with plain spread mean {}", cv.mean, spread.mean);
}
