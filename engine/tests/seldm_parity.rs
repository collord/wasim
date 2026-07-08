//! Phase-1 parity tests for the SELDM → WASiM port.
//!
//! These compare the WASiM engine's sampling against SELDM's original statistical
//! pipeline (transcribed in `seldm_reference.rs`). Parity is checked at the
//! *distribution* level — quantiles of large samples — not per-draw, because the two
//! use different RNGs (ChaCha vs. MRG32k3a) and different sampling paths (3-parameter
//! gamma vs. AS241 + Wilson-Hilferty-Kirby for Pearson-III).
//!
//! What these answer for the port:
//!   1. Does the engine's `pearson_iii` reproduce SELDM's skewed hydrology marginals?
//!   2. Does the engine's Iman-Conover induce SELDM-usable rank correlation?
//!   3. Is the new `trapezoidal` family consistent between the engine and SELDM's ICDF?

#[path = "seldm_reference.rs"]
mod seldm;

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const N: u32 = 20_000;

fn engine_draws(distribution: &str) -> Vec<f64> {
    let json = format!(
        r#"{{"wasim_version": "0.8.0",
        "simulation_settings": {{"duration": {{"value": 1, "unit": "yr"}}, "timestep": {{"value": 1, "unit": "yr"}}, "n_realizations": {N}, "seed": 7}},
        "elements": [{{"id": "x", "name": "X", "primitive": "node", "value_rule": "sample",
          "distribution": {distribution}, "save_results": {{"final_value": true}}}}]}}"#
    );
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run").elements["x"].final_values.clone()
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(f64::total_cmp);
    v
}

/// Empirical quantile at probability p (0..1) from a sorted sample.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let idx = (p * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn stddev(v: &[f64]) -> f64 {
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() as f64 - 1.0)).sqrt()
}

fn skew(v: &[f64]) -> f64 {
    let m = mean(v);
    let s = stddev(v);
    let n = v.len() as f64;
    v.iter().map(|x| ((x - m) / s).powi(3)).sum::<f64>() * n / ((n - 1.0) * (n - 2.0))
}

/// Draw a SELDM Pearson-III sample of size `n` using MRG32k3a → AS241 → WHK.
fn seldm_pearson3_sample(n: usize, mean: f64, sd: f64, sk: f64) -> Vec<f64> {
    let mut rng = seldm::Mrg32k3a::new(987_654_321.0, 123_456_789.0);
    // Warm up, as SELDM does.
    for _ in 0..3 {
        rng.next_u01();
    }
    (0..n).map(|_| seldm::seldm_pearson3(rng.next_u01(), mean, sd, sk)).collect()
}

#[test]
fn pearson3_moments_match_seldm() {
    // Moderate positive skew — the interesting Pearson-III regime for hydrology.
    let (mu, sd, sk) = (10.0, 3.0, 0.8);
    let seldm = seldm_pearson3_sample(N as usize, mu, sd, sk);
    let engine = engine_draws(&format!(
        r#"{{"family": "pearson_iii", "parameters": {{"mean": {{"value": {mu}, "unit": "1"}}, "stddev": {{"value": {sd}, "unit": "1"}}, "skewness": {{"value": {sk}, "unit": "1"}}}}}}"#
    ));

    // Both should reproduce the target moments (they parameterize by mean/SD/skew).
    // Compare the two samples' moments to each other and to the target.
    let (sm, ss, sk_s) = (mean(&seldm), stddev(&seldm), skew(&seldm));
    let (em, es, ek) = (mean(&engine), stddev(&engine), skew(&engine));

    eprintln!("SELDM  mean={sm:.3} sd={ss:.3} skew={sk_s:.3}");
    eprintln!("engine mean={em:.3} sd={es:.3} skew={ek:.3}");

    assert!((sm - mu).abs() < 0.15, "SELDM mean off: {sm}");
    assert!((em - mu).abs() < 0.15, "engine mean off: {em}");
    assert!((ss - sd).abs() < 0.15, "SELDM sd off: {ss}");
    assert!((es - sd).abs() < 0.15, "engine sd off: {es}");
    // Sample skew is noisy; allow a wider band but confirm both are positive & near target.
    assert!((sk_s - sk).abs() < 0.25, "SELDM skew off: {sk_s}");
    assert!((ek - sk).abs() < 0.25, "engine skew off: {ek}");
}

#[test]
fn pearson3_quantiles_track_seldm() {
    // The stronger test: do the empirical CDFs line up across the body of the
    // distribution? Compare quantiles at several probabilities.
    let (mu, sd, sk) = (10.0, 3.0, 0.8);
    let seldm = sorted(seldm_pearson3_sample(N as usize, mu, sd, sk));
    let engine = sorted(engine_draws(&format!(
        r#"{{"family": "pearson_iii", "parameters": {{"mean": {{"value": {mu}, "unit": "1"}}, "stddev": {{"value": {sd}, "unit": "1"}}, "skewness": {{"value": {sk}, "unit": "1"}}}}}}"#
    )));

    // Tolerance scaled to the distribution's spread (~0.1σ across the central body).
    for &p in &[0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95] {
        let qs = quantile(&seldm, p);
        let qe = quantile(&engine, p);
        eprintln!("p={p:.2}  SELDM={qs:.3}  engine={qe:.3}  Δ={:.3}", (qs - qe).abs());
        assert!(
            (qs - qe).abs() < 0.3,
            "Pearson-III quantile mismatch at p={p}: SELDM={qs} engine={qe}"
        );
    }
}

/// Spearman rank correlation between two equal-length samples.
fn spearman(a: &[f64], b: &[f64]) -> f64 {
    let rank = |v: &[f64]| -> Vec<f64> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|&i, &j| v[i].total_cmp(&v[j]));
        let mut r = vec![0.0; v.len()];
        for (rank, &i) in idx.iter().enumerate() {
            r[i] = rank as f64;
        }
        r
    };
    let ra = rank(a);
    let rb = rank(b);
    // Pearson correlation of the ranks.
    let ma = mean(&ra);
    let mb = mean(&rb);
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for i in 0..ra.len() {
        num += (ra[i] - ma) * (rb[i] - mb);
        da += (ra[i] - ma).powi(2);
        db += (rb[i] - mb).powi(2);
    }
    num / (da.sqrt() * db.sqrt())
}

#[test]
fn iman_conover_induces_target_rank_correlation() {
    // Two correlated lognormal marginals (SELDM's water-quality shape) with a target
    // rank correlation, as SELDM's paired RV variables use. Confirm the engine's
    // Iman-Conover achieves the target ρ — the property SELDM needs, regardless of the
    // fact that the algorithm (Iman-Conover) differs from SELDM's Mykytka method.
    let target = 0.7;
    let json = format!(
        r#"{{"wasim_version": "0.8.0",
        "simulation_settings": {{"duration": {{"value": 1, "unit": "yr"}}, "timestep": {{"value": 1, "unit": "yr"}}, "n_realizations": {N}, "seed": 7}},
        "elements": [
          {{"id": "a", "name": "A", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "lognormal_moments", "parameters": {{"mean": {{"value": 5, "unit": "1"}}, "stddev": {{"value": 2, "unit": "1"}}}}}},
            "correlations": [{{"partner": "b", "coefficient": {target}}}],
            "save_results": {{"final_value": true}}}},
          {{"id": "b", "name": "B", "primitive": "node", "value_rule": "sample",
            "distribution": {{"family": "lognormal_moments", "parameters": {{"mean": {{"value": 8, "unit": "1"}}, "stddev": {{"value": 3, "unit": "1"}}}}}},
            "save_results": {{"final_value": true}}}}
        ]}}"#
    );
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let res = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let a = &res.elements["a"].final_values;
    let b = &res.elements["b"].final_values;

    let rho = spearman(a, b);
    eprintln!("target ρ={target}  achieved Spearman ρ={rho:.3}");
    assert!((rho - target).abs() < 0.05, "rank correlation off: target {target}, got {rho}");
}

#[test]
fn trapezoid_quantiles_match_seldm_icdf() {
    // Compare the engine's trapezoidal draws against SELDM's fndUniform01ToTrapezoid,
    // fed the same uniform stream — quantile-level agreement.
    let (min, lower, upper, max) = (1.0, 3.0, 7.0, 12.0);
    let engine = sorted(engine_draws(&format!(
        r#"{{"family": "trapezoidal", "parameters": {{"min": {{"value": {min}, "unit": "1"}}, "lower": {{"value": {lower}, "unit": "1"}}, "upper": {{"value": {upper}, "unit": "1"}}, "max": {{"value": {max}, "unit": "1"}}}}}}"#
    )));

    // SELDM reference sample from its own uniform stream + the transcribed trapezoid ICDF.
    let mut rng = seldm::Mrg32k3a::new(555_555.0, 444_444.0);
    for _ in 0..3 {
        rng.next_u01();
    }
    let seldm = sorted(
        (0..N as usize)
            .map(|_| {
                let u = rng.next_u01();
                seldm::seldm_trapezoid_public(u, min, lower, upper, max)
            })
            .collect(),
    );

    for &p in &[0.05, 0.25, 0.5, 0.75, 0.95] {
        let qs = quantile(&seldm, p);
        let qe = quantile(&engine, p);
        assert!(
            (qs - qe).abs() < 0.15,
            "trapezoid quantile mismatch at p={p}: SELDM={qs} engine={qe}"
        );
    }
}
