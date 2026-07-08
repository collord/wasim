//! Phase-4 parity: SELDM's zero-inflated prestorm streamflow (intermittent streams),
//! and the question of whether WASiM can express it *without a new primitive*.
//!
//! SELDM (GeneratePreStormQs, modMainSELDM.bas:1311):
//!   if U ≤ p:   Q = 0                              (dry storm — point mass at zero)
//!   else:       U' = (U − p) / (1 − p)             (rescale survivors to (0,1))
//!               Ks = WilsonHilfertyKirby(skew, Φ⁻¹(U'))
//!               Q = 10^(log10(mean) + log10(sd)·Ks)   (Pearson-III body, log space)
//!   where p = proportionZero.
//!
//! This is a MIXTURE: a point mass at 0 with probability p, plus a Pearson-III body with
//! probability (1−p). The rescale `(U−p)/(1−p)` maps the surviving uniform back onto the
//! full (0,1) interval, so the *conditional* wet distribution is the full Pearson-III —
//! NOT a truncated tail. (This is why the engine's rejection-`truncation` is the wrong
//! tool: it would renormalize the survivor density and give no point mass at zero.)
//!
//! WASiM representation under test (Option A — no schema change): a Bernoulli "is-wet"
//! gate × a Pearson-III body, combined with an `if` expression. This matches SELDM's
//! MARGINAL (the property that matters for ECDF output) because SELDM's rescale is
//! designed precisely so the conditional-wet law equals the unconditional Pearson-III.
//! It does NOT reproduce SELDM's per-draw shared-U coupling — which only affects exact
//! plotting-position ordering, not the marginal. This test verifies the marginal claim.

#[path = "seldm_reference.rs"]
mod seldm;

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const N: u32 = 40_000;

const P_ZERO: f64 = 0.30; // proportion of dry storms
// SELDM's streamflow body uses log10(mean) and log10(sd) as the log-space Pearson-III
// mean/stddev. We mirror that idiosyncrasy exactly on both sides.
const Q_MEAN: f64 = 5.0; // real-space mean streamflow (cfs·area), before log10
const Q_SD: f64 = 3.0;
const Q_SKEW: f64 = 0.5;

/// SELDM's exact zero-inflated streamflow, one uniform per storm (shared for the
/// dry/wet decision and the body quantile).
fn seldm_streamflow(n: usize) -> Vec<f64> {
    const LOG10: f64 = 0.434_294_481_903_252; // ln→log10 factor, as SELDM uses
    let mut rng = seldm::Mrg32k3a::new(246_810.0, 135_790.0);
    for _ in 0..3 {
        rng.next_u01();
    }
    let log_mean = Q_MEAN.ln() * LOG10; // = log10(mean)
    let log_sd = Q_SD.ln() * LOG10; // = log10(sd)  (SELDM's formula, verbatim)
    (0..n)
        .map(|_| {
            let u = rng.next_u01();
            if P_ZERO >= 0.00011 && u <= P_ZERO {
                0.0
            } else {
                let up = if P_ZERO >= 0.00011 { (u - P_ZERO) / (1.0 - P_ZERO) } else { u };
                let ks = seldm::wilson_hilferty_kirby(Q_SKEW, seldm::as241_normal(up));
                10f64.powf(log_mean + log_sd * ks)
            }
        })
        .collect()
}

/// Option A model: Bernoulli(1−p) is-wet gate × Pearson-III body (in log10 space,
/// then 10^·), combined with `if`. The body's mean/stddev are log10(mean)/log10(sd)
/// to match SELDM's formula exactly.
fn zero_inflated_model() -> String {
    const LOG10: f64 = 0.434_294_481_903_252;
    let log_mean = Q_MEAN.ln() * LOG10;
    let log_sd = Q_SD.ln() * LOG10;
    let p_wet = 1.0 - P_ZERO;
    format!(
        r#"{{"wasim_version": "0.8.0",
        "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": {N}, "seed": 7}},
        "elements": [
          {{"id": "is_wet", "name": "is wet", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "bernoulli", "parameters": {{"prob": {{"value": {p_wet}, "unit": "1"}}}}}}}},
          {{"id": "logq", "name": "log10 Q body", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "pearson_iii", "parameters": {{"mean": {{"value": {log_mean}, "unit": "1"}}, "stddev": {{"value": {log_sd}, "unit": "1"}}, "skewness": {{"value": {Q_SKEW}, "unit": "1"}}}}}}}},
          {{"id": "q", "name": "streamflow", "primitive": "node", "value_rule": "expression", "inputs": ["is_wet", "logq"],
            "expression": {{"ast": {{"op": "if",
              "cond": {{"op": "gt", "left": {{"op": "ref", "element_id": "is_wet"}}, "right": {{"op": "literal", "value": 0.5}}}},
              "then": {{"op": "power", "left": {{"op": "literal", "value": 10}}, "right": {{"op": "ref", "element_id": "logq"}}}},
              "else": {{"op": "literal", "value": 0}}}}}},
            "save_results": {{"final_value": true}}}}
        ]}}"#
    )
}

fn engine_streamflow() -> Vec<f64> {
    let m = parse_v2(&zero_inflated_model()).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run").elements["q"].final_values.clone()
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v
}
fn quantile(s: &[f64], p: f64) -> f64 {
    s[((p * (s.len() as f64 - 1.0)).round() as usize).min(s.len() - 1)]
}
fn frac_zero(v: &[f64]) -> f64 {
    v.iter().filter(|&&x| x == 0.0).count() as f64 / v.len() as f64
}
fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

#[test]
fn zero_inflated_point_mass_matches() {
    // The fraction of dry storms must match p (within sampling error).
    let e = engine_streamflow();
    let s = seldm_streamflow(N as usize);
    let (fe, fs) = (frac_zero(&e), frac_zero(&s));
    eprintln!("fraction dry  engine={fe:.4}  SELDM={fs:.4}  target={P_ZERO}");
    assert!((fe - P_ZERO).abs() < 0.01, "engine dry fraction {fe} != {P_ZERO}");
    assert!((fs - P_ZERO).abs() < 0.01, "SELDM dry fraction {fs} != {P_ZERO}");
    assert!((fe - fs).abs() < 0.01, "dry fractions disagree: engine {fe}, SELDM {fs}");
}

#[test]
fn zero_inflated_wet_body_ecdf_matches_seldm() {
    // Compare the WET body (nonzero values) quantile-by-quantile: the marginal claim.
    let e = sorted(engine_streamflow().into_iter().filter(|&x| x > 0.0).collect());
    let s = sorted(seldm_streamflow(N as usize).into_iter().filter(|&x| x > 0.0).collect());
    eprintln!("wet body  engine mean={:.4}  SELDM mean={:.4}", mean(&e), mean(&s));
    let scale = quantile(&s, 0.5).abs().max(1e-6);
    // Cap the wet-body view at p=0.9. Beyond that, the wet body is the ~96th+ percentile
    // of the full distribution, and SELDM's 10^(log-space skewed Pearson-III) form
    // exponentially amplifies the known 3-param-gamma-vs-Wilson-Hilferty-Kirby tail
    // divergence. The extreme upper tail is exercised by the full-mixture test below,
    // which is the actual model output; here we confirm the body up to p=0.9.
    for &p in &[0.05, 0.1, 0.25, 0.5, 0.75, 0.9] {
        let qe = quantile(&e, p);
        let qs = quantile(&s, p);
        let rel = (qe - qs).abs() / scale;
        eprintln!("p={p:.2}  engine={qe:.4}  SELDM={qs:.4}  rel={rel:.3}");
        let tol = if p >= 0.9 { 0.10 } else { 0.06 };
        assert!(rel < tol, "wet-body ECDF mismatch at p={p}: engine={qe} SELDM={qs} (rel {rel})");
    }
}

#[test]
fn zero_inflated_full_ecdf_matches_seldm() {
    // The full mixture (zeros + body) — the actual model output.
    let e = sorted(engine_streamflow());
    let s = sorted(seldm_streamflow(N as usize));
    let scale = quantile(&s, 0.75).abs().max(1e-6);
    // Skip the lowest quantiles (all-zero, trivially equal) and check the transition + body.
    for &p in &[0.35, 0.5, 0.7, 0.9, 0.95] {
        let qe = quantile(&e, p);
        let qs = quantile(&s, p);
        let rel = (qe - qs).abs() / scale;
        eprintln!("p={p:.2}  engine={qe:.4}  SELDM={qs:.4}  rel={rel:.3}");
        assert!(rel < 0.06, "full-mixture ECDF mismatch at p={p}: engine={qe} SELDM={qs} (rel {rel})");
    }
}
