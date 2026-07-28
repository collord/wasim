// Validates schema_examples_manual/nested_var_horizon.json — the CONDITIONAL nested-submodel
// statistic `nested_stat`. The outer model simulates a market factor to a risk horizon H; for each
// outer scenario the inner submodel `innerBS` is re-run with its start price BOUND to that scenario's
// horizon price S_H, and the mean inner discounted payoff is the portfolio value in that scenario.
// The distribution of `port_value` across outer paths is the horizon P&L distribution — the exact
// nested-simulation structure of VaR / CVA.
//
// Checks:
//  (1) The nested MC reproduces the closed-form conditional expectation: mean(port_value) matches
//      mean(bs_ref) (Black-Scholes at S_H), and the two track PER PATH (Pearson corr ~ 1) — the
//      defining property of a CONDITIONAL nested statistic.
//  (2) The tail risk measure (5th percentile of the horizon value) matches between the nested MC and
//      the closed form — a VaR-style read only the nested construct can produce.
//  (3) The MARGINAL contrast: submodel_stat (marginal_bs) is a single constant BS(100) ~ 8.77 shared
//      by every outer path, NOT conditioned on S_H — showing why nested_stat is a distinct construct.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

fn mean(v: &[f64]) -> f64 { v.iter().sum::<f64>() / v.len() as f64 }
fn pctile(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((p / 100.0) * (s.len() as f64 - 1.0)).round() as usize;
    s[idx.min(s.len() - 1)]
}
fn corr(x: &[f64], y: &[f64]) -> f64 {
    let (mx, my) = (mean(x), mean(y));
    let mut sxy = 0.0; let mut sxx = 0.0; let mut syy = 0.0;
    for i in 0..x.len() {
        let (dx, dy) = (x[i] - mx, y[i] - my);
        sxy += dx * dy; sxx += dx * dx; syy += dy * dy;
    }
    sxy / (sxx.sqrt() * syy.sqrt())
}

#[test]
fn nested_stat_reproduces_conditional_bs_and_horizon_var() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/nested_var_horizon.json")).unwrap();
    let v1: WasimModel = serde_json::from_str(&json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    spec.elements = ["port_value", "bs_ref", "marginal_bs", "S_H"].iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { results_spec: Some(spec), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    let finals = |id: &str| -> Vec<f64> {
        res.elements[id].final_values.clone()
    };
    let (pv, bs, mg, sh) = (finals("port_value"), finals("bs_ref"), finals("marginal_bs"), finals("S_H"));
    let (pv_m, bs_m) = (mean(&pv), mean(&bs));
    let c = corr(&pv, &bs);
    println!("mean port_value (nested MC) = {pv_m:.4}   mean bs_ref (closed form) = {bs_m:.4}");
    println!("per-path corr(port_value, bs_ref) = {c:.5}");
    println!("horizon value 5th pctile: nested {:.4}  closed-form {:.4}  (mean {:.4})",
        pctile(&pv, 5.0), pctile(&bs, 5.0), pv_m);
    println!("marginal_bs (unconditioned, constant) = {:.4}   S_H range [{:.1}, {:.1}]",
        mg[0], sh.iter().cloned().fold(f64::INFINITY, f64::min), sh.iter().cloned().fold(f64::NEG_INFINITY, f64::max));

    // (1) Conditional expectation reproduced: means agree, and — the key property — the nested value
    // tracks the closed-form conditional value PER OUTER PATH (not just on average).
    assert!((pv_m - bs_m).abs() < 0.15,
        "nested MC mean {pv_m} should match closed-form conditional mean {bs_m}");
    assert!(c > 0.995,
        "nested value must track the closed-form conditional value per path (corr {c} should be ~1)");

    // (2) Tail risk (5th-percentile horizon value) matches the closed form — a VaR-style read.
    assert!((pctile(&pv, 5.0) - pctile(&bs, 5.0)).abs() < 0.4,
        "nested 5th-pctile {} should match closed-form {}", pctile(&pv, 5.0), pctile(&bs, 5.0));
    // The horizon value genuinely varies across scenarios (it's a distribution, not a point).
    assert!(pctile(&pv, 95.0) - pctile(&pv, 5.0) > 5.0, "horizon value should span a real distribution");

    // (3) Marginal contrast: submodel_stat is a single CONSTANT (~BS(100)=8.77) across all paths, and
    // it is NOT the conditional value (it does not track S_H).
    assert!(mg.iter().all(|&x| (x - mg[0]).abs() < 1e-9), "marginal_bs must be constant across outer paths");
    // marginal_bs is ONE inner MC run of N_inner=4000 (SE ~ 0.24 on the call payoff), so allow ~2 SE
    // around the closed-form BS(100)=8.77. (The pooled nested mean above is far tighter because it
    // averages N_outer × N_inner samples.)
    assert!((mg[0] - 8.77).abs() < 0.5, "marginal_bs should be BS at spot ~ 8.77 (single-run MC), got {}", mg[0]);
    // The conditional mean (undiscounted horizon value, ~10.6) is materially different from the single
    // marginal constant — the marginal-vs-conditional distinction the construct exists for.
    assert!(pv_m > mg[0] + 1.0, "conditional horizon mean {pv_m} should differ from the marginal constant {}", mg[0]);
    println!("nested_stat: conditional value reproduced per path; horizon VaR read; marginal contrast shown.");
}
