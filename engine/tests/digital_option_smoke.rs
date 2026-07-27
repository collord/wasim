// Validates schema_examples_manual/digital_option.json through the v2 engine (normalize_v1 ->
// run_v2). Digital (binary) options priced by exact-terminal Monte Carlo — one N(0,1) draw per
// realization, no discretization bias. The payoff is intentionally DISCONTINUOUS (an indicator); the
// point is that the *price* is a smooth integral and MC converges fine, even though the Greeks would
// not (see DIGITAL_OPTION_SCOPE.md). No new engine feature is exercised — it is a plain expression on
// existing constructs.
//
// Checks:
//  (1) Cash-or-nothing call MC == disc*N(d2) within Monte Carlo error.
//  (2) Asset-or-nothing call MC == S0*N(d1) within Monte Carlo error.
//  (3) Cash parity: cash_call + cash_put == disc on EVERY realization (a digital pays 1 either above
//      or below the strike) — an exact per-path identity.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, ci: f64 }

#[test]
fn digital_options_match_closed_form() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/digital_option.json")).unwrap();
    let v1: WasimModel = serde_json::from_str(&json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    let ids = ["cash_call", "asset_call", "cash_bs", "asset_bs", "parity"];
    spec.elements = ids.iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { n_realizations: Some(200_000), seed: Some(24680), results_spec: Some(spec),
        ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    let fs = |id: &str| {
        let a = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        FS { mean: a.mean, ci: a.ci_half_width }
    };
    let (cash, asset, cash_bs, asset_bs, parity) =
        (fs("cash_call"), fs("asset_call"), fs("cash_bs"), fs("asset_bs"), fs("parity"));
    println!("cash_call MC = {:.4} +/- {:.4}   disc*N(d2) = {:.4}", cash.mean, cash.ci, cash_bs.mean);
    println!("asset_call MC = {:.4} +/- {:.4}   S0*N(d1)  = {:.4}", asset.mean, asset.ci, asset_bs.mean);

    // (1) Cash-or-nothing call == disc*N(d2).
    assert!((cash.mean - cash_bs.mean).abs() < 4.0 * cash.ci,
        "cash-or-nothing MC {} should match disc*N(d2) {} (ci {})", cash.mean, cash_bs.mean, cash.ci);
    assert!(cash_bs.mean > 0.4 && cash_bs.mean < 0.7, "cash digital sanity: {}", cash_bs.mean);

    // (2) Asset-or-nothing call == S0*N(d1).
    assert!((asset.mean - asset_bs.mean).abs() < 4.0 * asset.ci,
        "asset-or-nothing MC {} should match S0*N(d1) {} (ci {})", asset.mean, asset_bs.mean, asset.ci);

    // (3) Exact per-path parity: cash_call + cash_put == disc every realization, so the mean equals
    // exp(-rT) with zero Monte Carlo error.
    let disc = (-0.05_f64).exp();
    assert!((parity.mean - disc).abs() < 1e-9 && parity.ci < 1e-9,
        "cash parity {} should equal disc {} exactly (ci {})", parity.mean, disc, parity.ci);
    println!("cash parity == disc = {:.6} exactly (ci {:.2e})", parity.mean, parity.ci);
}
