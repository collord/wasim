// Validates schema_examples_manual/american_put_lsm.json — an American put priced by
// Longstaff-Schwartz least-squares Monte Carlo (the `lsm` AST node). This is the first early-exercise
// option in the corpus and the first use of the post-run BACKWARD execution mode: after the forward
// run, the engine walks the stored [dates x paths] time_history panel backward, regressing the
// discounted future value on a cubic basis of S over in-the-money paths at each date and exercising
// where intrinsic value beats the fitted continuation. Each path's discounted-to-t0 cashflow is
// injected per realization, so the LSM element's mean is the option price.
//
// Checks:
//  (1) The LSM American put matches the binomial-tree value (6.090) within tolerance. LSM is an
//      in-sample estimator (slightly low-biased), so a small negative bias is expected and allowed.
//  (2) Early-exercise premium: the American price strictly exceeds the European put on the same path.
//  (3) Path validation: the European put (MC, terminal) matches the Black-Scholes closed form.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, ci: f64 }

#[test]
fn american_put_lsm_matches_tree_and_beats_european() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/american_put_lsm.json")).unwrap();
    let v1: WasimModel = serde_json::from_str(&json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    let ids = ["american_put", "euro_put", "bs_euro"];
    spec.elements = ids.iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { n_realizations: Some(40_000), seed: Some(20_250_727), results_spec: Some(spec),
        duration_override: Some(1.0), timestep_override: Some(0.02), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    let fs = |id: &str| {
        let a = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        FS { mean: a.mean, ci: a.ci_half_width }
    };
    let (amer, euro, bs) = (fs("american_put"), fs("euro_put"), fs("bs_euro"));
    // Exercise dates are the start-of-step grid t_1..t_{m-1}, so the option effectively matures at
    // T_eff = T - dt = 0.98 (a stock is read one step stale — same convention as the barrier example).
    const BINOMIAL: f64 = 6.0449; // CRR American put at T_eff = 0.98 (2000 steps)
    println!("American put (LSM)      = {:.4} +/- {:.4}   (binomial@T_eff {:.4})", amer.mean, amer.ci, BINOMIAL);
    println!("European put (MC)       = {:.4} +/- {:.4}", euro.mean, euro.ci);
    println!("European put (BS)       = {:.4}", bs.mean);
    println!("early-exercise premium  = {:.4}", amer.mean - euro.mean);

    // (1) LSM matches the binomial American value at T_eff, allowing the known in-sample low bias.
    assert!(amer.mean > BINOMIAL - 0.25 && amer.mean < BINOMIAL + 0.10,
        "LSM American put {} should be near the binomial value {}", amer.mean, BINOMIAL);
    // The bias is downward (in-sample sub-optimal policy): LSM should not exceed the tree by much.
    assert!(amer.mean <= BINOMIAL + 4.0 * amer.ci, "LSM should not materially exceed the true price");

    // (2) Early-exercise premium: American strictly above European (premium ~0.5 here).
    assert!(amer.mean > euro.mean + 0.2,
        "American {} should exceed European {} by the early-exercise premium", amer.mean, euro.mean);

    // (3) Path validation: European MC matches the Black-Scholes closed form.
    assert!((euro.mean - bs.mean).abs() < 4.0 * euro.ci,
        "European MC {} should match Black-Scholes {} (ci {})", euro.mean, bs.mean, euro.ci);
    assert!((bs.mean - 5.5735).abs() < 0.01, "BS European put reference sanity: {}", bs.mean);
    println!("LSM prices early exercise: matches the tree, beats European, path validated.");
}
