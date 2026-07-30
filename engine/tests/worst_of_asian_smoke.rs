// Validates schema_examples_manual/worst_of_asian_correlated.json — a worst-of Asian call on two
// correlated assets. It is the first PATH-DEPENDENT MULTI-ASSET example, so it needs correlated
// per-step PROCESS shocks (basket scope Phase 3): the payoff is on running averages, which a single
// terminal draw cannot express, so the whole paths must co-move. No closed form; validated
// structurally and by the sign of the correlation effect.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

fn price(json: &str, id: &str, n: u32) -> (f64, f64) {
    let m = normalize_v1(&serde_json::from_str::<WasimModel>(json).unwrap());
    let g = ModelGraphV2::build(&m).unwrap();
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    spec.elements = vec!["worstof".into(), "asian1".into(), "asian2".into()];
    let cfg = RunConfig { n_realizations: Some(n), seed: Some(808080), results_spec: Some(spec),
        duration_override: Some(1.0), timestep_override: Some(0.02), ..Default::default() };
    let r = run_v2(&m, &g, &cfg).unwrap();
    let a = r.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
    (a.mean, a.ci_half_width)
}

#[test]
fn worst_of_asian_on_correlated_paths_is_structurally_sound() {
    let base = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/worst_of_asian_correlated.json")).unwrap();
    let (wo, wo_ci) = price(&base, "worstof", 15_000);
    let (a1, _) = price(&base, "asian1", 15_000);
    let (a2, _) = price(&base, "asian2", 15_000);
    println!("worst-of Asian = {wo:.4} +/- {wo_ci:.4}   single Asians = {a1:.4}, {a2:.4}");

    // (1) Structural: the worst-of is worth no more than either single-asset Asian (min(a,b) <= a,b),
    // and is a positive value.
    assert!(wo <= a1 + 4.0 * wo_ci && wo <= a2 + 4.0 * wo_ci,
        "worst-of {wo} must not exceed either single Asian ({a1}, {a2})");
    assert!(wo > 1.0, "worst-of Asian should be a sensible positive value, got {wo}");

    // (2) Correlation MONOTONICITY — the defining effect of the correlated process shocks. A worst-of
    // is worth MORE when the assets are more correlated (they fall together less often, so the minimum
    // is higher). Re-price at rho = 0.95 vs rho = -0.5 and check the price rises with correlation.
    let hi = base.replace("\"coefficient\": 0.5", "\"coefficient\": 0.95");
    let lo = base.replace("\"coefficient\": 0.5", "\"coefficient\": -0.5");
    assert!(hi.contains("0.95") && lo.contains("-0.5"), "rho substitution must actually apply");
    let (wo_hi, _) = price(&hi, "worstof", 30_000);
    let (wo_lo, _) = price(&lo, "worstof", 30_000);
    println!("worst-of at rho=-0.5: {wo_lo:.4}   rho=0.5: {wo:.4}   rho=0.95: {wo_hi:.4}");
    assert!(wo_hi > wo && wo > wo_lo,
        "worst-of Asian should increase with correlation: {wo_lo} < {wo} < {wo_hi}");
    println!("correlated process shocks drive a path-dependent multi-asset payoff (monotone in rho).");
}
