// Basket scope, Phase 3: correlated PROCESS shocks. Correlations previously lived only on
// `random_variable` (single terminal draws); a `stochastic_process` now takes a `correlations` field
// too, so the per-step GBM shocks of a connected group are Cholesky-correlated each step and the
// asset PATHS are correlated — not just terminal draws. This unlocks path-dependent multi-asset
// options (Asian-basket, worst-of, two-asset barrier).
//
// Proven directly: two GBM processes declared with correlation rho produce paths whose terminal
// log-returns are correlated at ~rho; without the field they are independent (~0); and the marginal
// (E[S(T)] = S0 e^{rT}) is unchanged by the correlation.
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, RunConfig, WasimModel};

fn model_json(correlated: bool) -> String {
    let corr = if correlated { r#","correlations":[{"partner":"P2","coefficient":0.6}]"# } else { "" };
    format!(r#"{{
      "wasim_version":"0.1.0",
      "simulation_settings":{{"duration":{{"value":1.0,"unit":"yr"}},"timestep":{{"value":0.05,"unit":"yr"}},"n_realizations":20000,"seed":4242}},
      "elements":[
        {{"id":"P1","name":"P1","type":"stochastic_process","process":{{"family":"gbm","mean_type":"arithmetic","mean":{{"value":0.05,"unit":"1/yr"}},"stddev":{{"value":0.25,"unit":"1/yr"}}}}{corr}}},
        {{"id":"P2","name":"P2","type":"stochastic_process","process":{{"family":"gbm","mean_type":"arithmetic","mean":{{"value":0.05,"unit":"1/yr"}},"stddev":{{"value":0.25,"unit":"1/yr"}}}}}},
        {{"id":"S1","name":"S1","type":"accumulator","initial_value":{{"value":100.0,"unit":"USD"}},"inputs":["S1","P1"],
         "rate":{{"ast":{{"op":"multiply","left":{{"op":"ref","element_id":"S1"}},"right":{{"op":"ref","element_id":"P1"}}}},"display":"S1*P1"}},
         "min_value":null,"capacity":null,"save_results":{{"final_value":true}}}},
        {{"id":"S2","name":"S2","type":"accumulator","initial_value":{{"value":100.0,"unit":"USD"}},"inputs":["S2","P2"],
         "rate":{{"ast":{{"op":"multiply","left":{{"op":"ref","element_id":"S2"}},"right":{{"op":"ref","element_id":"P2"}}}},"display":"S2*P2"}},
         "min_value":null,"capacity":null,"save_results":{{"final_value":true}}}}
      ]
    }}"#)
}

fn terminals(correlated: bool) -> (Vec<f64>, Vec<f64>) {
    let m = normalize_v1(&serde_json::from_str::<WasimModel>(&model_json(correlated)).unwrap());
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    (r.elements["S1"].final_values.clone(), r.elements["S2"].final_values.clone())
}

fn corr(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let mut cov = 0.0; let (mut va, mut vb) = (0.0, 0.0);
    for i in 0..a.len() { cov += (a[i]-ma)*(b[i]-mb); va += (a[i]-ma).powi(2); vb += (b[i]-mb).powi(2); }
    cov / (va.sqrt() * vb.sqrt())
}
fn logret(v: &[f64]) -> Vec<f64> { v.iter().map(|s| (s / 100.0).ln()).collect() }

#[test]
fn correlated_process_shocks_correlate_the_paths() {
    // Correlated: terminal log-returns correlated at ~0.6 (the per-step shock correlation, preserved
    // through the accumulated path).
    let (s1, s2) = terminals(true);
    let rho = corr(&logret(&s1), &logret(&s2));
    println!("correlated:   realized terminal log-return corr = {rho:.3} (target 0.6)");
    assert!((rho - 0.6).abs() < 0.04, "correlated paths should show corr ~0.6, got {rho}");

    // Uncorrelated (no `correlations` field): independent → corr ~ 0.
    let (u1, u2) = terminals(false);
    let rho0 = corr(&logret(&u1), &logret(&u2));
    println!("uncorrelated: realized terminal log-return corr = {rho0:.3} (target 0)");
    assert!(rho0.abs() < 0.04, "uncorrelated paths should be independent, got {rho0}");

    // The marginal is unchanged by correlation: E[S(T)] ~ 100 e^{rT} = 105.13 for both assets, in
    // both the correlated and uncorrelated runs (Cholesky only couples the shocks, not the marginals).
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    for (label, m) in [("corr S1", mean(&s1)), ("corr S2", mean(&s2)), ("indep S1", mean(&u1))] {
        assert!((m - 105.13).abs() < 1.2, "{label} marginal E[S(T)] ~ 105.13, got {m}");
    }
    println!("correlated process shocks: paths correlate at the target, marginals preserved.");
}
