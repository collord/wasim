// Validates schema_examples_manual/digital_option_greeks.json — Monte Carlo GREEKS of a
// cash-or-nothing digital call, comparing the likelihood-ratio method (LRM) with bump-and-revalue.
// The digital is the textbook case where MC Greeks are hard: the *price* is a smooth integral but
// the *delta* differentiates through a discontinuity. LRM (delta = E[payoff * score], score =
// Z/(S0 sigma sqrt(T))) leaves the payoff untouched, so its variance is low and independent of any
// bump size; central bump-and-revalue with common random numbers has the same mean but higher
// variance. Both match the analytic delta disc*n(d2)/(S0 sigma sqrt(T)).
//
// This needs NO new engine feature — the driving normal Z is an explicit random_variable, so the
// LRM score is a plain expression. (Path-dependent Greeks, where the driver is inside a process,
// would need the driver exposed — see DIGITAL_OPTION_SCOPE.md.)
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, std: f64, ci: f64 }

#[test]
fn digital_delta_lrm_beats_bump_and_matches_analytic() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/digital_option_greeks.json")).unwrap();
    let v1: WasimModel = serde_json::from_str(&json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    let ids = ["cash_call", "delta_lrm", "delta_bump", "delta_analytic"];
    spec.elements = ids.iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { n_realizations: Some(200_000), seed: Some(13131), results_spec: Some(spec),
        ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    let fs = |id: &str| {
        let a = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        FS { mean: a.mean, std: a.std, ci: a.ci_half_width }
    };
    let (price, lrm, bump, an) = (fs("cash_call"), fs("delta_lrm"), fs("delta_bump"), fs("delta_analytic"));
    println!("cash digital price = {:.4}", price.mean);
    println!("analytic delta     = {:.6}", an.mean);
    println!("LRM  delta = {:.6} +/- {:.6}  std={:.4}", lrm.mean, lrm.ci, lrm.std);
    println!("bump delta = {:.6} +/- {:.6}  std={:.4}", bump.mean, bump.ci, bump.std);
    println!("std ratio bump/LRM = {:.2}x", bump.std / lrm.std);

    // (1) LRM delta matches the analytic digital delta within Monte Carlo error.
    assert!((lrm.mean - an.mean).abs() < 4.0 * lrm.ci,
        "LRM delta {} should match analytic {} (ci {})", lrm.mean, an.mean, lrm.ci);
    assert!(an.mean > 0.015 && an.mean < 0.023, "analytic digital delta sanity: {}", an.mean);

    // (2) Bump delta has the same expectation (unbiased at this eps) within its own error.
    assert!((bump.mean - an.mean).abs() < 4.0 * bump.ci,
        "bump delta {} should match analytic {} (ci {})", bump.mean, an.mean, bump.ci);

    // (3) LRM is the lower-variance estimator — the whole point on a discontinuous payoff.
    assert!(lrm.std < bump.std,
        "LRM std {} should be below bump std {}", lrm.std, bump.std);
    println!("LRM is the lower-variance delta estimator for the discontinuous digital payoff.");
}
