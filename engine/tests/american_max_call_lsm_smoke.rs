// Validates schema_examples_manual/american_max_call_lsm.json — an American MAX-CALL on two
// correlated dividend-paying assets, priced by MULTI-ASSET Longstaff-Schwartz least-squares Monte
// Carlo. It is the first multi-asset early-exercise option in the corpus: the `lsm` node's new
// `states` list turns the continuation regression into a MULTIVARIATE monomial basis of (S1, S2) up
// to total degree `basis` over in-the-money paths. The two assets follow correlated per-step GBM
// shocks (rho = 0.30, basket scope Phase 3) and pay a continuous dividend q = 0.10 — the dividend is
// what makes early exercise optimal for a max-call (with q = 0 an American max-call equals the
// European one).
//
// Checks:
//  (1) The OOS multi-asset LSM price matches a Boyle-Evnine-Gibbs 2-asset binomial lattice (9.56 at
//      the LSM effective maturity T_eff = T - dt = 0.98) within tolerance.
//  (2) Early-exercise premium: the American max-call strictly exceeds the European max-call on the
//      same paths (the premium the dividend creates).
//  (3) In-sample bias direction: the in-sample estimate sits at or below the OOS estimate.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, ci: f64 }

#[test]
fn american_max_call_multi_asset_lsm_matches_lattice() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/american_max_call_lsm.json")).unwrap();
    let v1: WasimModel = serde_json::from_str(&json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    let ids = ["american_maxcall", "american_maxcall_is", "euro_maxcall"];
    spec.elements = ids.iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { n_realizations: Some(20_000), seed: Some(20_250_727), results_spec: Some(spec),
        duration_override: Some(1.0), timestep_override: Some(0.02), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    let fs = |id: &str| {
        let a = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        FS { mean: a.mean, ci: a.ci_half_width }
    };
    let (oos, is_, euro) = (fs("american_maxcall"), fs("american_maxcall_is"), fs("euro_maxcall"));
    // Exercise dates are the start-of-step grid, so the option effectively matures at T_eff = T - dt
    // = 0.98 (same one-step-stale convention as the single-asset put). BEG 2-asset lattice at T_eff.
    const LATTICE: f64 = 9.56; // Boyle-Evnine-Gibbs American max-call at T_eff = 0.98 (250 steps)
    println!("American max-call (LSM, OOS) = {:.4} +/- {:.4}   (BEG lattice@T_eff {:.4})", oos.mean, oos.ci, LATTICE);
    println!("American max-call (in-sample)= {:.4} +/- {:.4}", is_.mean, is_.ci);
    println!("European max-call (MC)       = {:.4} +/- {:.4}", euro.mean, euro.ci);

    // (1) The multi-asset OOS price is a LOWER bound that tracks the binomial lattice. LSM prices under
    // a finite-basis (cubic in S1,S2) exercise policy, which is sub-optimal, so it sits at or just below
    // the lattice — never materially above it. Allow a modest basis-suboptimality gap below, but a tight
    // band above (the option cannot be worth more than the true American value).
    assert!(oos.mean <= LATTICE + 4.0 * oos.ci,
        "OOS multi-asset LSM max-call {} must not exceed the true American value {}", oos.mean, LATTICE);
    assert!(oos.mean >= LATTICE - 0.4,
        "OOS multi-asset LSM max-call {} should track the BEG lattice {} (basis gap too large)", oos.mean, LATTICE);

    // (2) Early-exercise premium: American strictly above European on the same paths.
    assert!(oos.mean > euro.mean + 0.2,
        "American max-call {} should exceed European {} by the early-exercise premium", oos.mean, euro.mean);

    // (3) In-sample sits at or below the OOS estimate (the in-sample low bias the cross-fit removes).
    assert!(is_.mean <= oos.mean + 4.0 * (is_.ci + oos.ci),
        "in-sample {} should not exceed the OOS estimate {} (bias direction)", is_.mean, oos.mean);
    println!("multi-asset LSM: OOS matches lattice; early exercise priced on correlated paths.");
}
