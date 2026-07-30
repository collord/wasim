// Validates schema_examples_manual/asian_option_control_variate.json through the v2 engine
// (normalize_v1 -> run_v2). Checks: (1) the geometric-Asian MC price matches the live
// Kemna-Vorst closed form within Monte Carlo error — validating the GBM path, the t_1..t_n
// averaging-window correction, and the closed form together; (2) the geometric control variate
// sharply reduces the arithmetic estimator's variance; (3) the CV estimator stays unbiased;
// (4) arithmetic >= geometric price (AM-GM).
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, std: f64, ci: f64 }

fn stats(json: &str, n: u32) -> std::collections::HashMap<String, FS> {
    let v1: WasimModel = serde_json::from_str(json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    spec.elements = vec!["payoff_arith".into(), "payoff_geo".into(), "cv_arith".into(), "geo_price".into()];
    // Coarser grid than the model default (6 monitoring steps) to keep the test quick; the live
    // closed form uses cnt/dt so it self-adjusts to whatever grid the run uses.
    let cfg = RunConfig { n_realizations: Some(n), seed: Some(424242), results_spec: Some(spec),
        duration_override: Some(1.0), timestep_override: Some(1.0 / 6.0), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    spec_ids().into_iter().map(|id| {
        let fs = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        (id.to_string(), FS { mean: fs.mean, std: fs.std, ci: fs.ci_half_width })
    }).collect()
}
fn spec_ids() -> Vec<&'static str> { vec!["payoff_arith", "payoff_geo", "cv_arith", "geo_price"] }

#[test]
fn asian_option_control_variate_matches_and_reduces_variance() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/asian_option_control_variate.json")).unwrap();
    let s = stats(&json, 8_000);
    let (arith, geo, cv, gcf) = (&s["payoff_arith"], &s["payoff_geo"], &s["cv_arith"], &s["geo_price"]);
    println!("geo_closed_form = {:.6}", gcf.mean);
    println!("payoff_geo MC   : mean={:.6} std={:.6} ci={:.6}", geo.mean, geo.std, geo.ci);
    println!("payoff_arith    : mean={:.6} std={:.6} ci={:.6}", arith.mean, arith.std, arith.ci);
    println!("cv_arith        : mean={:.6} std={:.6} ci={:.6}", cv.mean, cv.std, cv.ci);

    // (1) Geometric MC price matches the live Kemna-Vorst closed form within a few CI half-widths.
    assert!((geo.mean - gcf.mean).abs() < 4.0 * geo.ci,
        "geometric MC {} vs closed form {} (ci {})", geo.mean, gcf.mean, geo.ci);
    assert!(gcf.mean > 1.0, "geometric closed-form price should be a sensible positive value, got {}", gcf.mean);

    // (2) The control variate sharply reduces variance (arith/geo averages are ~0.99 correlated).
    println!("variance-reduction factor (std_arith/std_cv) = {:.2}", arith.std / cv.std);
    assert!(cv.std < arith.std / 3.0, "CV should cut std by >3x: arith {} vs cv {}", arith.std, cv.std);

    // (3) CV estimator is unbiased for the arithmetic price: agrees with plain within combined error.
    assert!((cv.mean - arith.mean).abs() < 4.0 * (arith.ci + cv.ci),
        "CV mean {} should agree with plain arithmetic mean {}", cv.mean, arith.mean);

    // (4) AM-GM: arithmetic average >= geometric, so the arithmetic Asian call is worth more.
    assert!(cv.mean > geo.mean, "arithmetic Asian ({}) should exceed geometric ({})", cv.mean, geo.mean);
}
