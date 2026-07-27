// Validates schema_examples_manual/basket_option.json through the v2 engine (normalize_v1 -> run_v2).
// A European arithmetic-basket call on N=3 equicorrelated assets, priced by exact-terminal Monte
// Carlo. The shocks are correlated via the NATIVE `correlations` field — build_corr_groups assembles
// the NxN matrix and Cholesky-factors it, and the Gaussian copula on normal marginals reproduces the
// linear shock correlation exactly. Generalizes the two-asset spread example to N assets and uses the
// geometric-basket closed form as a control variate.
//
// Checks:
//  (1) Geometric-basket MC == the live closed form (validates the correlated draws + the formula).
//  (2) AM-GM: the arithmetic basket is worth more than the geometric one.
//  (3) The geometric control variate sharply reduces the arithmetic estimator's variance, unbiased.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, std: f64, ci: f64 }

#[test]
fn basket_call_matches_and_reduces_variance() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/basket_option.json")).unwrap();
    let v1: WasimModel = serde_json::from_str(&json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    let ids = ["basket_arith", "basket_geo", "basket_cv", "geo_price"];
    spec.elements = ids.iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { n_realizations: Some(60_000), seed: Some(31313), results_spec: Some(spec),
        ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    let fs = |id: &str| {
        let a = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        FS { mean: a.mean, std: a.std, ci: a.ci_half_width }
    };
    let (arith, geo, cv, gp) = (fs("basket_arith"), fs("basket_geo"), fs("basket_cv"), fs("geo_price"));
    println!("geo_price (closed form) = {:.4}", gp.mean);
    println!("basket_geo   MC: mean={:.4} ci={:.4}", geo.mean, geo.ci);
    println!("basket_arith MC: mean={:.4} std={:.4} ci={:.4}", arith.mean, arith.std, arith.ci);
    println!("basket_cv      : mean={:.4} std={:.4} ci={:.4}", cv.mean, cv.std, cv.ci);

    // (1) Geometric-basket MC matches the live closed form — a joint check of the correlated shocks
    // (native `correlations` field / Cholesky) and the geometric-basket formula.
    assert!((geo.mean - gp.mean).abs() < 4.0 * geo.ci,
        "geometric-basket MC {} should match the closed form {} (ci {})", geo.mean, gp.mean, geo.ci);
    assert!(gp.mean > 3.0, "geometric-basket price should be a sensible positive value, got {}", gp.mean);

    // (2) AM-GM: the arithmetic average dominates the geometric, so the arithmetic basket is worth more.
    assert!(cv.mean > geo.mean, "arithmetic basket ({}) should exceed geometric ({})", cv.mean, geo.mean);

    // (3) The geometric control variate (~0.997 correlated) sharply cuts variance, unbiased.
    println!("variance-reduction factor (std_arith/std_cv) = {:.2}", arith.std / cv.std);
    assert!(cv.std < arith.std / 4.0, "CV should cut std >4x: arith {} vs cv {}", arith.std, cv.std);
    assert!((cv.mean - arith.mean).abs() < 4.0 * (arith.ci + cv.ci),
        "CV mean {} should agree with the plain arithmetic mean {}", cv.mean, arith.mean);
}
