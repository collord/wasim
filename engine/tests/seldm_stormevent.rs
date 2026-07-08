//! Phase-2 parity: a single-constituent storm-event slice of SELDM, authored as a WASiM
//! model and run in the engine's native single-timestep mode (n_steps = 1), compared
//! against a direct transcription of SELDM's per-storm chain.
//!
//! The chain (highway + upstream → downstream mixing), from modMainSELDM.bas:
//!   1. concentration:  C = mean + SD·Ks,  Ks = WilsonHilfertyKirby(skew, Φ⁻¹(U))
//!      then transform: exp(C)                       [GenerateRandomQW, transform ID 3]
//!   2. load:           L = flow · loadMultiplier · C
//!   3. downstream mix: C_ds = (L_hwy + L_us) / (Q_hwy + Q_us) / loadMultiplier
//!                                                    [GenerateDownstreamQW]
//!
//! In WASiM this is: `sample` nodes (pearson_iii for the untransformed concentration,
//! lognormal_moments for the flows) + `expression` nodes (exp transform, load, mix). The
//! engine draws each sample once per realization and evaluates the expression graph once
//! (single step) — one realization = one SELDM storm.
//!
//! Parity is at the ECDF level (quantiles), not per-draw: the two use different RNGs and
//! different Pearson-III sampling paths. loadMultiplier is 1 here (unit constituent).

#[path = "seldm_reference.rs"]
mod seldm;

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const N: u32 = 20_000;

// Constituent + flow statistics (representative planning-level values).
const C_MEAN: f64 = 1.2; // untransformed (log-space) concentration mean
const C_SD: f64 = 0.5;
const C_SKEW: f64 = 0.6;
const QHWY_MEAN: f64 = 3.0; // highway runoff volume (real-space moments)
const QHWY_SD: f64 = 1.5;
const QUS_MEAN: f64 = 40.0; // upstream concurrent flow (dilution source)
const QUS_SD: f64 = 20.0;
// Upstream concentration is lower (cleaner receiving water).
const CUS_MEAN: f64 = 0.3;
const CUS_SD: f64 = 0.2;
const CUS_SKEW: f64 = 0.4;

fn storm_event_model() -> String {
    // pearson_iii nodes produce the *untransformed* C = mean + SD·Ks; the exp() transform
    // and the load/mix algebra live in expression nodes.
    format!(
        r#"{{"wasim_version": "0.8.0",
        "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": {N}, "seed": 7}},
        "elements": [
          {{"id": "c_hwy_raw", "name": "hwy conc raw", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "pearson_iii", "parameters": {{"mean": {{"value": {C_MEAN}, "unit": "1"}}, "stddev": {{"value": {C_SD}, "unit": "1"}}, "skewness": {{"value": {C_SKEW}, "unit": "1"}}}}}}}},
          {{"id": "c_us_raw", "name": "us conc raw", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "pearson_iii", "parameters": {{"mean": {{"value": {CUS_MEAN}, "unit": "1"}}, "stddev": {{"value": {CUS_SD}, "unit": "1"}}, "skewness": {{"value": {CUS_SKEW}, "unit": "1"}}}}}}}},
          {{"id": "q_hwy", "name": "hwy flow", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "lognormal_moments", "parameters": {{"mean": {{"value": {QHWY_MEAN}, "unit": "1"}}, "stddev": {{"value": {QHWY_SD}, "unit": "1"}}}}}}}},
          {{"id": "q_us", "name": "us flow", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "lognormal_moments", "parameters": {{"mean": {{"value": {QUS_MEAN}, "unit": "1"}}, "stddev": {{"value": {QUS_SD}, "unit": "1"}}}}}}}},

          {{"id": "c_hwy", "name": "hwy conc", "primitive": "node", "value_rule": "expression", "inputs": ["c_hwy_raw"],
            "expression": {{"ast": {{"op": "call", "fn": "exp", "args": [{{"op": "ref", "element_id": "c_hwy_raw"}}]}}}},
            "save_results": {{"final_value": true}}}},
          {{"id": "c_us", "name": "us conc", "primitive": "node", "value_rule": "expression", "inputs": ["c_us_raw"],
            "expression": {{"ast": {{"op": "call", "fn": "exp", "args": [{{"op": "ref", "element_id": "c_us_raw"}}]}}}}}},

          {{"id": "l_hwy", "name": "hwy load", "primitive": "node", "value_rule": "expression", "inputs": ["q_hwy", "c_hwy"],
            "expression": {{"ast": {{"op": "multiply", "left": {{"op": "ref", "element_id": "q_hwy"}}, "right": {{"op": "ref", "element_id": "c_hwy"}}}}}},
            "save_results": {{"final_value": true}}}},
          {{"id": "l_us", "name": "us load", "primitive": "node", "value_rule": "expression", "inputs": ["q_us", "c_us"],
            "expression": {{"ast": {{"op": "multiply", "left": {{"op": "ref", "element_id": "q_us"}}, "right": {{"op": "ref", "element_id": "c_us"}}}}}}}},

          {{"id": "c_ds", "name": "downstream conc", "primitive": "node", "value_rule": "expression", "inputs": ["l_hwy", "l_us", "q_hwy", "q_us"],
            "expression": {{"ast": {{"op": "divide",
              "left": {{"op": "add", "left": {{"op": "ref", "element_id": "l_hwy"}}, "right": {{"op": "ref", "element_id": "l_us"}}}},
              "right": {{"op": "add", "left": {{"op": "ref", "element_id": "q_hwy"}}, "right": {{"op": "ref", "element_id": "q_us"}}}}}}}},
            "save_results": {{"final_value": true}}}}
        ]}}"#
    )
}

fn engine_run() -> wasim_engine::SimulationResults {
    let m = parse_v2(&storm_event_model()).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run")
}

/// SELDM's per-storm chain, driven by MRG32k3a + AS241 + Wilson-Hilferty-Kirby, exactly
/// as GenerateRandomQW / GenerateDownstreamQW do it. Returns (c_ds, l_hwy) samples.
fn seldm_run(n: usize) -> (Vec<f64>, Vec<f64>) {
    // Independent seed streams per variable, as SELDM allocates per sequence number.
    let mut rc_hwy = seldm::Mrg32k3a::new(111_111.0, 222_222.0);
    let mut rc_us = seldm::Mrg32k3a::new(333_333.0, 444_444.0);
    let mut rq_hwy = seldm::Mrg32k3a::new(555_555.0, 666_666.0);
    let mut rq_us = seldm::Mrg32k3a::new(777_777.0, 888_888.0);
    for r in [&mut rc_hwy, &mut rc_us, &mut rq_hwy, &mut rq_us] {
        for _ in 0..3 {
            r.next_u01(); // warm-up, as SELDM does
        }
    }

    // lognormal real-space moments → log-space (μ, σ), matching the engine's
    // lognormal_moments and SELDM's dW/dU adjustment.
    let logspace = |mean: f64, sd: f64| -> (f64, f64) {
        let cov2 = (sd / mean).powi(2);
        let sigma = (1.0 + cov2).ln().sqrt();
        let mu = mean.ln() - 0.5 * (1.0 + cov2).ln();
        (mu, sigma)
    };
    let (qh_mu, qh_sig) = logspace(QHWY_MEAN, QHWY_SD);
    let (qu_mu, qu_sig) = logspace(QUS_MEAN, QUS_SD);

    let mut c_ds = Vec::with_capacity(n);
    let mut l_hwy = Vec::with_capacity(n);
    for _ in 0..n {
        // Concentrations: C = mean + SD·Ks, then exp() transform (transform ID 3).
        let c_hwy = (C_MEAN + C_SD * seldm::wilson_hilferty_kirby(C_SKEW, seldm::as241_normal(rc_hwy.next_u01()))).exp();
        let c_us = (CUS_MEAN + CUS_SD * seldm::wilson_hilferty_kirby(CUS_SKEW, seldm::as241_normal(rc_us.next_u01()))).exp();
        // Flows: lognormal via Φ⁻¹.
        let q_hwy = (qh_mu + qh_sig * seldm::as241_normal(rq_hwy.next_u01())).exp();
        let q_us = (qu_mu + qu_sig * seldm::as241_normal(rq_us.next_u01())).exp();
        // Loads (loadMultiplier = 1).
        let lh = q_hwy * c_hwy;
        let lu = q_us * c_us;
        // Downstream mixing.
        c_ds.push((lh + lu) / (q_hwy + q_us));
        l_hwy.push(lh);
    }
    (c_ds, l_hwy)
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
fn downstream_concentration_ecdf_matches_seldm() {
    let res = engine_run();
    let engine_cds = sorted(res.elements["c_ds"].final_values.clone());
    let (seldm_cds, _) = seldm_run(N as usize);
    let seldm_cds = sorted(seldm_cds);

    eprintln!("downstream concentration  engine mean={:.4}  SELDM mean={:.4}", mean(&engine_cds), mean(&seldm_cds));
    // Tolerance is a fraction of the median (dilution compresses the spread).
    let scale = quantile(&seldm_cds, 0.5).abs().max(1e-6);
    for &p in &[0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95] {
        let qe = quantile(&engine_cds, p);
        let qs = quantile(&seldm_cds, p);
        let rel = (qe - qs).abs() / scale;
        eprintln!("p={p:.2}  engine={qe:.4}  SELDM={qs:.4}  rel={rel:.3}");
        assert!(rel < 0.05, "downstream conc ECDF mismatch at p={p}: engine={qe} SELDM={qs} (rel {rel})");
    }
}

#[test]
fn highway_load_ecdf_matches_seldm() {
    let res = engine_run();
    let engine_lhwy = sorted(res.elements["l_hwy"].final_values.clone());
    let (_, seldm_lhwy) = seldm_run(N as usize);
    let seldm_lhwy = sorted(seldm_lhwy);

    eprintln!("highway load  engine mean={:.3}  SELDM mean={:.3}", mean(&engine_lhwy), mean(&seldm_lhwy));
    let scale = quantile(&seldm_lhwy, 0.5).abs().max(1e-6);
    for &p in &[0.05, 0.25, 0.5, 0.75, 0.9, 0.95] {
        let qe = quantile(&engine_lhwy, p);
        let qs = quantile(&seldm_lhwy, p);
        let rel = (qe - qs).abs() / scale;
        eprintln!("p={p:.2}  engine={qe:.3}  SELDM={qs:.3}  rel={rel:.3}");
        assert!(rel < 0.08, "highway load ECDF mismatch at p={p}: engine={qe} SELDM={qs} (rel {rel})");
    }
}
