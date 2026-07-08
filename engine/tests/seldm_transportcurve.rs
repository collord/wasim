//! Phase-3 parity: SELDM's dependent transport-curve water quality, authored as a WASiM
//! model using a `lookup` node (invoked via `lookup_call`) plus a `sample` scatter term.
//!
//! SELDM's transport curve (GenerateDependentCurveQW, modMainSELDM.bas):
//!   X       = explanatory variable (here: concurrent flow, cfs)
//!   X_t     = log10(X)                                   [transform ID 2]
//!   C       = intercept + slope·X_t + MADRatio·MAD·KN,   KN = Φ⁻¹(U)  (regression + scatter)
//!   C       = 10^C                                       [retransform ID 2]
//! (single-segment case; multi-segment is a piecewise version of the same line.)
//!
//! In WASiM: a `lookup` table holds the deterministic regression line y = intercept +
//! slope·log10(x) sampled across the flow range; `lookup_call` interpolates it at the
//! storm's flow (exercising the last untested engine path). A `sample` node supplies the
//! MAD·KN scatter, and an `expression` node adds them and applies the 10^· retransform.
//!
//! This proves the `lookup`/`lookup_call` machinery reproduces SELDM's transport-curve
//! concentration population. loadMultiplier = 1.

#[path = "seldm_reference.rs"]
mod seldm;

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const N: u32 = 20_000;

// Regression: log10(C) = INTERCEPT + SLOPE·log10(flow) + MAD·KN.
const INTERCEPT: f64 = -0.5;
const SLOPE: f64 = 0.8;
const MAD: f64 = 0.25; // median absolute deviation of the residuals (scatter scale)

// Flow (explanatory variable X): lognormal real-space moments.
const Q_MEAN: f64 = 30.0;
const Q_SD: f64 = 18.0;

/// Build the lookup table for y = INTERCEPT + SLOPE·log10(x) over a flow grid that
/// spans the sampled flow range. Linear interpolation in x reproduces the (slightly
/// curved in x, exactly linear in log10 x) regression closely; we grid densely enough
/// that interpolation error is far below the parity tolerance.
fn lookup_table_json() -> (String, String) {
    // Grid the flow axis geometrically from 0.5 to 500 cfs (well beyond ±3σ of the
    // lognormal), dense enough that piecewise-linear interp tracks the log curve.
    let n = 200usize;
    let (lo, hi) = (0.5_f64, 500.0_f64);
    let mut xs = Vec::with_capacity(n);
    let mut ys = Vec::with_capacity(n);
    for i in 0..n {
        let f = (i as f64) / (n as f64 - 1.0);
        let x = lo * (hi / lo).powf(f); // geometric spacing
        xs.push(x);
        ys.push(INTERCEPT + SLOPE * x.log10());
    }
    let xj = xs.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(", ");
    let yj = ys.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(", ");
    (xj, yj)
}

fn transport_curve_model() -> String {
    let (xs, ys) = lookup_table_json();
    // scatter node: MAD·KN, KN ~ N(0,1) → Normal(0, MAD).
    format!(
        r#"{{"wasim_version": "0.8.0",
        "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": {N}, "seed": 7}},
        "elements": [
          {{"id": "q", "name": "flow", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "lognormal_moments", "parameters": {{"mean": {{"value": {Q_MEAN}, "unit": "1"}}, "stddev": {{"value": {Q_SD}, "unit": "1"}}}}}},
            "save_results": {{"final_value": true}}}},
          {{"id": "curve", "name": "transport curve", "primitive": "node", "value_rule": "lookup",
            "table": {{"x": [{xs}], "y": [{ys}], "x_unit": "1", "y_unit": "1"}}}},
          {{"id": "scatter", "name": "MAD scatter", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "normal", "parameters": {{"mean": {{"value": 0, "unit": "1"}}, "stddev": {{"value": {MAD}, "unit": "1"}}}}}}}},
          {{"id": "logc", "name": "log10 conc", "primitive": "node", "value_rule": "expression", "inputs": ["curve", "q", "scatter"],
            "expression": {{"ast": {{"op": "add",
              "left": {{"op": "lookup_call", "element_id": "curve", "input": {{"op": "ref", "element_id": "q"}}}},
              "right": {{"op": "ref", "element_id": "scatter"}}}}}}}},
          {{"id": "conc", "name": "concentration", "primitive": "node", "value_rule": "expression", "inputs": ["logc"],
            "expression": {{"ast": {{"op": "power", "left": {{"op": "literal", "value": 10}}, "right": {{"op": "ref", "element_id": "logc"}}}}}},
            "save_results": {{"final_value": true}}}}
        ]}}"#
    )
}

fn engine_run() -> wasim_engine::SimulationResults {
    let m = parse_v2(&transport_curve_model()).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run")
}

/// SELDM transport-curve reference: same flow marginal, same regression + scatter, driven
/// by MRG32k3a + AS241 (independent streams for flow and the KN scatter).
fn seldm_run(n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut rq = seldm::Mrg32k3a::new(101_010.0, 202_020.0);
    let mut rk = seldm::Mrg32k3a::new(303_030.0, 404_040.0);
    for r in [&mut rq, &mut rk] {
        for _ in 0..3 {
            r.next_u01();
        }
    }
    let cov2 = (Q_SD / Q_MEAN).powi(2);
    let sigma = (1.0 + cov2).ln().sqrt();
    let mu = Q_MEAN.ln() - 0.5 * (1.0 + cov2).ln();

    let mut conc = Vec::with_capacity(n);
    let mut flow = Vec::with_capacity(n);
    for _ in 0..n {
        let mut q = (mu + sigma * seldm::as241_normal(rq.next_u01())).exp();
        if q <= 0.0 {
            q = 2f64.powi(-10); // SELDM's zero-flow guard
        }
        let x_t = q.log10();
        let kn = seldm::as241_normal(rk.next_u01());
        let logc = INTERCEPT + SLOPE * x_t + MAD * kn; // MADRatio = 1
        conc.push(10f64.powf(logc));
        flow.push(q);
    }
    (conc, flow)
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v
}
fn quantile(s: &[f64], p: f64) -> f64 {
    s[((p * (s.len() as f64 - 1.0)).round() as usize).min(s.len() - 1)]
}
fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

#[test]
fn transport_curve_concentration_ecdf_matches_seldm() {
    let res = engine_run();
    let engine_c = sorted(res.elements["conc"].final_values.clone());
    let (seldm_c, _) = seldm_run(N as usize);
    let seldm_c = sorted(seldm_c);

    eprintln!("transport-curve conc  engine mean={:.4}  SELDM mean={:.4}", mean(&engine_c), mean(&seldm_c));
    let scale = quantile(&seldm_c, 0.5).abs().max(1e-6);
    for &p in &[0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95] {
        let qe = quantile(&engine_c, p);
        let qs = quantile(&seldm_c, p);
        let rel = (qe - qs).abs() / scale;
        eprintln!("p={p:.2}  engine={qe:.4}  SELDM={qs:.4}  rel={rel:.3}");
        // Slightly looser than the pure-algebra chain: the lookup grid introduces a small
        // piecewise-linear-vs-log interpolation error on top of the RNG/path differences.
        assert!(rel < 0.06, "transport-curve ECDF mismatch at p={p}: engine={qe} SELDM={qs} (rel {rel})");
    }
}

#[test]
fn lookup_call_reproduces_regression_line() {
    // Sanity: with zero scatter the lookup_call must reproduce 10^(intercept+slope·log10 q)
    // at the sampled flows — i.e. the lookup path itself is faithful, independent of RNG.
    let res = engine_run();
    let flows = &res.elements["q"].final_values;
    let concs = &res.elements["conc"].final_values;
    // Reconstruct the deterministic part and check the *median* ratio is ~1 (scatter is
    // symmetric in log space, so 10^(±MAD·KN) has median 1).
    let mut ratios: Vec<f64> = flows
        .iter()
        .zip(concs.iter())
        .map(|(&q, &c)| {
            let det = 10f64.powf(INTERCEPT + SLOPE * q.log10());
            c / det
        })
        .collect();
    ratios.sort_by(f64::total_cmp);
    let median = quantile(&ratios, 0.5);
    eprintln!("median(conc / deterministic-curve) = {median:.4} (expect ~1.0)");
    assert!((median - 1.0).abs() < 0.05, "lookup_call regression line off: median ratio {median}");
}
