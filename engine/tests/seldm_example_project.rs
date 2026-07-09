//! End-to-end SELDM example analysis, assembled as a WASiM model from real reftable
//! data and run in the engine's native single-timestep Monte-Carlo mode.
//!
//! Project: SELDM "Example Project Z01" — I-81 highway runoff near Harrisburg PA
//! discharging into a Conodoguinet Creek tributary (Central Appalachian Ridges and
//! Valleys, ecoregion 67). All constants below are the project's actual values,
//! extracted from reftable_export/ (see the SELDM→WASiM port notes):
//!
//!   - Highway site (tblHighwaySite id 1): drainage 18.5 ac, impervious 0.27
//!   - Upstream basin (tblUpstreamBasin id 1): 0.5 mi², impervious 0.007
//!   - Precip: nearest station by Haversine distance = HUNTSDALE PA (HPStation 6166,
//!     ~21 km from the site), storm volume mean 0.64 in / COV 1.13
//!   - RV equations: highway eq 4 "SELDM Highway Sites", upstream eq 2; evaluated at
//!     each site's imperviousness via CalculateRvStats
//!   - RV rank correlation (fndPairedSiteRvRho, default C0/C1/F0/F1) = 0.4918
//!   - 15 highway QW constituents (tblQWHighway, method=1 random)
//!   - pH additionally has a matched upstream/receiving-water constituent for
//!     ecoregion 67 (tblQWUpstream, method=1): mean 7.457, sd 0.328, skew -0.049.
//!     The other highway constituents have no matched method=1 upstream QW in the
//!     reference data for this ecoregion, so they get the full highway-runoff
//!     population but not downstream dilution (this mirrors what the SELDM data
//!     supports, not a WASiM limitation).
//!
//! The model is built in-code (loop over the constituent table) to avoid ~75 hand-
//! written JSON blocks; it is NOT a general transpiler. Parity is checked per
//! constituent at the ECDF level against a transcription of SELDM's chain
//! (GenerateTotalRunoff → GenerateRandomQW → GenerateDownstreamQW), using the shared
//! SELDM statistical functions in seldm_reference.rs.

#[path = "seldm_reference.rs"]
mod seldm;

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

const N: u32 = 20_000;

// ── Real project constants ─────────────────────────────────────────────────────
const HWY_AREA: f64 = 18.5; // acres
const US_AREA: f64 = 0.5 * 640.0; // 0.5 mi² → acres (consistent units for the mix ratio)

// RV (runoff coefficient) Pearson-III stats, from CalculateRvStats at each imperviousness.
const HWY_RV_AVG: f64 = 0.2339;
const HWY_RV_SD: f64 = 0.2189;
const HWY_RV_SKEW: f64 = 1.2336;
const US_RV_AVG: f64 = 0.1306;
const US_RV_SD: f64 = 0.0991;
const US_RV_SKEW: f64 = 1.0761;
const RV_RANK_CORR: f64 = 0.4918;

// Precipitation storm volume (HUNTSDALE PA): lognormal from mean + COV.
const PV_MEAN: f64 = 0.64; // inches
const PV_COV: f64 = 1.13;

/// A highway QW constituent: (id, name, transform, log-space mean/sd/skew).
/// transform: 1 = none (pH special), 2 = 10^, 3 = exp. Constituent 4 (SSC-from-TSS)
/// is method=3 (transport curve) with null random stats — excluded from the random set.
struct Qw {
    id: u32,
    name: &'static str,
    transform: u8,
    mean: f64,
    sd: f64,
    skew: f64,
}

const HWY_QW: &[Qw] = &[
    Qw { id: 1,  name: "FHWA NonUrban TSS",   transform: 2, mean: 1.617445, sd: 0.535814,  skew: 0.1539543 },
    Qw { id: 2,  name: "FHWA Urban TSS",      transform: 2, mean: 2.138599, sd: 0.4427058, skew: -0.1392125 },
    Qw { id: 3,  name: "FHWA UltraUrban TSS", transform: 2, mean: 2.12826,  sd: 0.3829497, skew: 0.2243391 },
    // id 4 (SSC from TSS) omitted: method=3 transport curve, null random stats.
    Qw { id: 5,  name: "MA2009 Total P p00",  transform: 2, mean: -1.05,    sd: 0.423,     skew: -0.679 },
    Qw { id: 6,  name: "MA Total Hardness",   transform: 2, mean: 1.12,     sd: 0.622,     skew: 0.772 },
    Qw { id: 7,  name: "MA pH",               transform: 1, mean: 7.22,     sd: 0.498,     skew: 0.00581 },
    Qw { id: 8,  name: "MA Total Nitrogen",   transform: 2, mean: 0.036,    sd: 0.237,     skew: 0.0106 },
    Qw { id: 9,  name: "MA Total Phosphorus", transform: 2, mean: -1.02,    sd: 0.388,     skew: -0.436 },
    Qw { id: 10, name: "MA Total Cadmium",    transform: 2, mean: -0.615,   sd: 0.476,     skew: 0.762 },
    Qw { id: 11, name: "MA Total Chromium",   transform: 2, mean: 1.08,     sd: 0.373,     skew: 0.228 },
    Qw { id: 12, name: "MA Total Copper",     transform: 2, mean: 1.43,     sd: 0.378,     skew: 0.0232 },
    Qw { id: 13, name: "MA Total Lead",       transform: 2, mean: 0.941,    sd: 0.503,     skew: 0.205 },
    Qw { id: 14, name: "MA Total Zinc",       transform: 2, mean: 2.09,     sd: 0.406,     skew: 0.358 },
    Qw { id: 15, name: "MA Suspended Sed.",   transform: 2, mean: 1.88,     sd: 0.585,     skew: 0.206 },
];

// pH is the one constituent with a matched upstream (receiving-water) QW for eco 67.
const PH_US_MEAN: f64 = 7.457;
const PH_US_SD: f64 = 0.328;
const PH_US_SKEW: f64 = -0.049;
const PH_HYDROGEN_MW: f64 = 19.02331; // SELDM's g/mol factor for the pH load path

fn log10_of(v: f64) -> f64 {
    v.log10()
}

/// lognormal real-space (mean, cov) → log-space (μ, σ) for lognormal_moments.
fn logspace(mean: f64, cov: f64) -> (f64, f64) {
    let sigma = (1.0 + cov * cov).ln().sqrt();
    let mu = mean.ln() - 0.5 * (1.0 + cov * cov).ln();
    (mu, sigma)
}

// ── Build the WASiM model (shared flow subgraph + per-constituent blocks) ───────

fn transform_ast(raw_ref: &str, transform: u8) -> String {
    match transform {
        2 => format!(r#"{{"op": "power", "left": {{"op": "literal", "value": 10}}, "right": {{"op": "ref", "element_id": "{raw_ref}"}}}}"#),
        3 => format!(r#"{{"op": "call", "fn": "exp", "args": [{{"op": "ref", "element_id": "{raw_ref}"}}]}}"#),
        _ => format!(r#"{{"op": "ref", "element_id": "{raw_ref}"}}"#), // transform 1 (none)
    }
}

fn build_model() -> String {
    let (pv_mu, pv_sig) = logspace(PV_MEAN, PV_COV);
    let mut els = String::new();

    // Shared: precip volume + correlated RV coefficients + clamped flows.
    els.push_str(&format!(
        r#"{{"id": "pv", "name": "precip volume", "primitive": "node", "value_rule": "sample",
          "distribution": {{"family": "lognormal", "parameters": {{"mean": {{"value": {pv_mu}, "unit": "1"}}, "stddev": {{"value": {pv_sig}, "unit": "1"}}}}}}}},
        {{"id": "rv_hwy", "name": "hwy Rv", "primitive": "node", "value_rule": "sample",
          "distribution": {{"family": "pearson_iii", "parameters": {{"mean": {{"value": {HWY_RV_AVG}, "unit": "1"}}, "stddev": {{"value": {HWY_RV_SD}, "unit": "1"}}, "skewness": {{"value": {HWY_RV_SKEW}, "unit": "1"}}}}}},
          "correlations": [{{"partner": "rv_us", "coefficient": {RV_RANK_CORR}}}]}},
        {{"id": "rv_us", "name": "us Rv", "primitive": "node", "value_rule": "sample",
          "distribution": {{"family": "pearson_iii", "parameters": {{"mean": {{"value": {US_RV_AVG}, "unit": "1"}}, "stddev": {{"value": {US_RV_SD}, "unit": "1"}}, "skewness": {{"value": {US_RV_SKEW}, "unit": "1"}}}}}}}},
        {{"id": "q_hwy", "name": "hwy runoff", "primitive": "node", "value_rule": "expression", "inputs": ["rv_hwy", "pv"],
          "expression": {{"ast": {{"op": "multiply",
            "left": {{"op": "call", "fn": "max", "args": [{{"op": "literal", "value": 0}}, {{"op": "call", "fn": "min", "args": [{{"op": "literal", "value": 1}}, {{"op": "ref", "element_id": "rv_hwy"}}]}}]}},
            "right": {{"op": "multiply", "left": {{"op": "ref", "element_id": "pv"}}, "right": {{"op": "literal", "value": {HWY_AREA}}}}}}}}},
          "save_results": {{"final_value": true}}}},
        {{"id": "q_us", "name": "us runoff", "primitive": "node", "value_rule": "expression", "inputs": ["rv_us", "pv"],
          "expression": {{"ast": {{"op": "multiply",
            "left": {{"op": "call", "fn": "max", "args": [{{"op": "literal", "value": 0}}, {{"op": "call", "fn": "min", "args": [{{"op": "literal", "value": 1}}, {{"op": "ref", "element_id": "rv_us"}}]}}]}},
            "right": {{"op": "multiply", "left": {{"op": "ref", "element_id": "pv"}}, "right": {{"op": "literal", "value": {US_AREA}}}}}}}}},
          "save_results": {{"final_value": true}}}}"#
    ));

    // Per-constituent: highway concentration + load. Downstream mixing only for pH
    // (the one with a matched upstream partner).
    for c in HWY_QW {
        let raw = format!("c{}_raw", c.id);
        let conc = format!("c{}", c.id);
        els.push_str(&format!(
            r#",
        {{"id": "{raw}", "name": "{name} raw", "primitive": "node", "value_rule": "sample",
          "distribution": {{"family": "pearson_iii", "parameters": {{"mean": {{"value": {mean}, "unit": "1"}}, "stddev": {{"value": {sd}, "unit": "1"}}, "skewness": {{"value": {skew}, "unit": "1"}}}}}}}},
        {{"id": "{conc}", "name": "{name}", "primitive": "node", "value_rule": "expression", "inputs": ["{raw}"],
          "expression": {{"ast": {transform}}},
          "save_results": {{"final_value": true}}}}"#,
            name = c.name, mean = c.mean, sd = c.sd, skew = c.skew,
            transform = transform_ast(&raw, c.transform)
        ));
    }

    // pH downstream mixing via the hydrogen-ion path:
    //   H+ mass = Q · 10^(-pH) · MW;  C_ds = -log10( Σ H+mass / (Q_ds · MW) )
    let (ph_us_raw, ph_us) = ("ph_us_raw", "ph_us");
    els.push_str(&format!(
        r#",
        {{"id": "{ph_us_raw}", "name": "us pH raw", "primitive": "node", "value_rule": "sample",
          "distribution": {{"family": "pearson_iii", "parameters": {{"mean": {{"value": {PH_US_MEAN}, "unit": "1"}}, "stddev": {{"value": {PH_US_SD}, "unit": "1"}}, "skewness": {{"value": {PH_US_SKEW}, "unit": "1"}}}}}}}},
        {{"id": "{ph_us}", "name": "us pH", "primitive": "node", "value_rule": "expression", "inputs": ["{ph_us_raw}"],
          "expression": {{"ast": {{"op": "ref", "element_id": "{ph_us_raw}"}}}}}},
        {{"id": "ph_hplus_hwy", "name": "hwy H+ mass", "primitive": "node", "value_rule": "expression", "inputs": ["q_hwy", "c7"],
          "expression": {{"ast": {{"op": "multiply", "left": {{"op": "ref", "element_id": "q_hwy"}},
            "right": {{"op": "multiply", "left": {{"op": "power", "left": {{"op": "literal", "value": 10}}, "right": {{"op": "neg", "operand": {{"op": "ref", "element_id": "c7"}}}}}}, "right": {{"op": "literal", "value": {PH_HYDROGEN_MW}}}}}}}}}}},
        {{"id": "ph_hplus_us", "name": "us H+ mass", "primitive": "node", "value_rule": "expression", "inputs": ["q_us", "{ph_us}"],
          "expression": {{"ast": {{"op": "multiply", "left": {{"op": "ref", "element_id": "q_us"}},
            "right": {{"op": "multiply", "left": {{"op": "power", "left": {{"op": "literal", "value": 10}}, "right": {{"op": "neg", "operand": {{"op": "ref", "element_id": "{ph_us}"}}}}}}, "right": {{"op": "literal", "value": {PH_HYDROGEN_MW}}}}}}}}}}},
        {{"id": "ph_ds", "name": "downstream pH", "primitive": "node", "value_rule": "expression", "inputs": ["ph_hplus_hwy", "ph_hplus_us", "q_hwy", "q_us"],
          "expression": {{"ast": {{"op": "neg", "operand": {{"op": "call", "fn": "log", "args": [
            {{"op": "divide",
              "left": {{"op": "divide", "left": {{"op": "add", "left": {{"op": "ref", "element_id": "ph_hplus_hwy"}}, "right": {{"op": "ref", "element_id": "ph_hplus_us"}}}},
                        "right": {{"op": "add", "left": {{"op": "ref", "element_id": "q_hwy"}}, "right": {{"op": "ref", "element_id": "q_us"}}}}}},
              "right": {{"op": "literal", "value": {PH_HYDROGEN_MW}}}}}
          ]}}}}}},
          "save_results": {{"final_value": true}}}}"#
    ));

    format!(
        r#"{{"wasim_version": "0.8.0",
        "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": {N}, "seed": 7}},
        "elements": [{els}]}}"#
    )
}

// ── SELDM reference chain ───────────────────────────────────────────────────────

struct SeldmOut {
    hwy_conc: Vec<Vec<f64>>, // per constituent (indexed like HWY_QW)
    ph_ds: Vec<f64>,
}

fn seldm_reference(n: usize) -> SeldmOut {
    // Correlated RV draws (Iman-Conover-equivalent target: rank correlation RV_RANK_CORR).
    // For the reference we generate the two RV series with GetRankCorrelation-style
    // coupling via a shared uniform + the target rho, matching SELDM's paired-site method.
    let mut r_pv = seldm::Mrg32k3a::new(1_111.0, 2_222.0);
    let mut r_rv = seldm::Mrg32k3a::new(3_333.0, 4_444.0);
    let mut r_rv2 = seldm::Mrg32k3a::new(5_555.0, 6_666.0);
    let mut qw_rng: Vec<seldm::Mrg32k3a> =
        HWY_QW.iter().enumerate().map(|(i, _)| seldm::Mrg32k3a::new(10_000.0 + i as f64 * 137.0, 20_000.0 + i as f64 * 251.0)).collect();
    let mut r_ph_us = seldm::Mrg32k3a::new(90_001.0, 90_002.0);
    for r in [&mut r_pv, &mut r_rv, &mut r_rv2, &mut r_ph_us] {
        for _ in 0..3 { r.next_u01(); }
    }
    for r in qw_rng.iter_mut() {
        for _ in 0..3 { r.next_u01(); }
    }

    let (pv_mu, pv_sig) = logspace(PV_MEAN, PV_COV);
    let mut hwy_conc: Vec<Vec<f64>> = HWY_QW.iter().map(|_| Vec::with_capacity(n)).collect();
    let mut ph_ds = Vec::with_capacity(n);

    for _ in 0..n {
        let pv = (pv_mu + pv_sig * seldm::as241_normal(r_pv.next_u01())).exp();

        // Correlated RVs: primary uniform u1, correlated u2 via SELDM's rank-correlation.
        let u1 = r_rv.next_u01();
        let mut u2_seed10 = r_rv2.next_u01() * 1e9;
        let mut u2_seed20 = r_rv2.next_u01() * 7e6;
        let u2 = seldm_rank_corr(RV_RANK_CORR, u1, &mut u2_seed10, &mut u2_seed20);
        let rv_h = clamp01(HWY_RV_AVG + HWY_RV_SD * seldm::wilson_hilferty_kirby(HWY_RV_SKEW, seldm::as241_normal(u1)));
        let rv_u = clamp01(US_RV_AVG + US_RV_SD * seldm::wilson_hilferty_kirby(US_RV_SKEW, seldm::as241_normal(u2)));
        let q_hwy = rv_h * pv * HWY_AREA;
        let q_us = rv_u * pv * US_AREA;

        for (i, c) in HWY_QW.iter().enumerate() {
            let raw = c.mean + c.sd * seldm::wilson_hilferty_kirby(c.skew, seldm::as241_normal(qw_rng[i].next_u01()));
            let conc = match c.transform {
                2 => 10f64.powf(raw),
                3 => raw.exp(),
                _ => raw, // pH: no retransform
            };
            hwy_conc[i].push(conc);
        }

        // pH downstream mixing (hydrogen-ion mass balance).
        let ph_hwy = HWY_QW.iter().position(|c| c.id == 7).map(|i| *hwy_conc[i].last().unwrap()).unwrap();
        let ph_us = PH_US_MEAN + PH_US_SD * seldm::wilson_hilferty_kirby(PH_US_SKEW, seldm::as241_normal(r_ph_us.next_u01()));
        let h_hwy = q_hwy * 10f64.powf(-ph_hwy) * PH_HYDROGEN_MW;
        let h_us = q_us * 10f64.powf(-ph_us) * PH_HYDROGEN_MW;
        let c_ds = (h_hwy + h_us) / (q_hwy + q_us) / PH_HYDROGEN_MW;
        ph_ds.push(-log10_of(c_ds));
    }
    SeldmOut { hwy_conc, ph_ds }
}

fn clamp01(v: f64) -> f64 {
    v.max(0.0).min(1.0)
}

/// SELDM's rank-correlation (GetRankCorrelation/MykytkaRho), condensed: returns a
/// uniform correlated to `u1` at approximately `rho`. Sufficient for a reference-side
/// correlated RV draw; the engine side uses Iman-Conover. We only compare per-
/// constituent QW marginals (independent of the RV correlation) and the pH mix, so this
/// is used only to make the reference's flows plausibly correlated.
fn seldm_rank_corr(rho: f64, u1: f64, seed10: &mut f64, seed20: &mut f64) -> f64 {
    let a = rho.abs();
    let b = (1.0 - a * a).sqrt();
    let mut g = seldm::Mrg32k3a::new(*seed10, *seed20);
    let u3 = g.next_u01();
    // Simple bounded blend approximating the target rank correlation.
    let y = a * u1 + b * u3;
    y.clamp(0.0, 1.0)
}

// ── Statistics helpers ──────────────────────────────────────────────────────────

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

// ── Tests ───────────────────────────────────────────────────────────────────────

/// Writes the generated model to schema_examples/ as a pretty-printed .json so it can
/// be opened and inspected. Re-serializing the parsed JSON guarantees the file is
/// exactly what the engine runs, just formatted (with a `source` block added to match
/// the other examples). Run with: `cargo test --test seldm_example_project emit_model`.
#[test]
fn emit_model_json() {
    let raw = build_model();
    let mut v: serde_json::Value = serde_json::from_str(&raw).expect("model is valid json");
    v.as_object_mut().unwrap().insert(
        "source".to_string(),
        serde_json::json!({
            "generator": "seldm-wasim port",
            "generator_version": null,
            "created": null,
            "notes": "SELDM Example Project Z01 — I-81 highway runoff near Harrisburg PA into a Conodoguinet Creek tributary. 15 highway QW constituents (method=1 random) with real reftable statistics; pH downstream-mixed with its ecoregion-67 receiving-water constituent. Generated by engine/tests/seldm_example_project.rs::build_model()."
        }),
    );
    let pretty = serde_json::to_string_pretty(&v).unwrap();
    // Written into the seldm-wasim port repo rather than wasim/schema_examples/: that
    // directory is a v1-schema corpus validated by integration.rs glob tests, and this
    // is a v2 model — dropping it there breaks those tests. This is the natural home for
    // the SELDM port artifacts anyway.
    let path = "/Users/collord/seldm-wasim/seldm_example_project.wasim.json";
    std::fs::write(path, pretty).expect("write model json");
    eprintln!("wrote {path}");
}

#[test]
fn model_parses_and_runs() {
    let m = parse_v2(&build_model()).expect("model should parse");
    let g = ModelGraphV2::build(&m).expect("graph should build");
    let res = run_v2(&m, &g, &RunConfig::default()).expect("run should succeed");
    // All 15 highway concentrations + flows + downstream pH present.
    assert!(res.elements.contains_key("q_hwy"));
    assert!(res.elements.contains_key("ph_ds"));
    for c in HWY_QW {
        assert!(res.elements.contains_key(&format!("c{}", c.id)), "missing constituent {}", c.id);
    }
}

#[test]
fn highway_concentration_ecdfs_match_seldm() {
    let m = parse_v2(&build_model()).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let res = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let reference = seldm_reference(N as usize);

    eprintln!("\n=== Highway-runoff concentration ECDF parity (15 constituents) ===");
    for (i, c) in HWY_QW.iter().enumerate() {
        let engine = sorted(res.elements[&format!("c{}", c.id)].final_values.clone());
        let seldm_c = sorted(reference.hwy_conc[i].clone());
        // Relative error at each quantile, normalized by *that quantile's own value*
        // (the correct scale for a heavy-tailed distribution — normalizing tail
        // deviations by the median would spuriously inflate them). The residual tail
        // gap (p ≥ 0.9) is the known Pearson-III path divergence (engine 3-param gamma
        // vs. SELDM AS241 + Wilson-Hilferty-Kirby), amplified by the 10^ retransform.
        let rel_at = |p: f64| {
            let qs = quantile(&seldm_c, p).abs().max(1e-9);
            (quantile(&engine, p) - quantile(&seldm_c, p)).abs() / qs
        };
        let worst_body = [0.05, 0.25, 0.5, 0.75].iter().map(|&p| rel_at(p)).fold(0.0, f64::max);
        let worst_tail = [0.9, 0.95].iter().map(|&p| rel_at(p)).fold(0.0, f64::max);
        eprintln!("  [{:>2}] {:<24} median eng={:>9.4} seldm={:>9.4}  rel body(≤p75)={:.3} tail(p90-95)={:.3}",
                  c.id, c.name, quantile(&engine, 0.5), quantile(&seldm_c, 0.5), worst_body, worst_tail);
        // Body ≤4% and tail ≤8% across all 15 constituents at N=20k (the residual is
        // Monte-Carlo noise plus the documented Pearson-III tail-path divergence).
        assert!(worst_body < 0.04, "constituent {} ({}) body ECDF rel {worst_body}", c.id, c.name);
        assert!(worst_tail < 0.08, "constituent {} ({}) tail ECDF rel {worst_tail}", c.id, c.name);
    }
}

#[test]
fn downstream_ph_matches_seldm() {
    let m = parse_v2(&build_model()).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let res = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let reference = seldm_reference(N as usize);

    // A small fraction of storms have ~zero runoff on both sides (Rv clamps to 0),
    // giving 0/0 in the mix → NaN. This is a real SELDM zero-runoff edge case and
    // occurs identically on both sides; drop those storms before comparing the pH
    // population, and report how many were dropped.
    let raw_engine = res.elements["ph_ds"].final_values.clone();
    let n_nan = raw_engine.iter().filter(|v| v.is_nan()).count();
    let engine = sorted(raw_engine.into_iter().filter(|v| v.is_finite()).collect());
    let seldm_p = sorted(reference.ph_ds.into_iter().filter(|v| v.is_finite()).collect());
    eprintln!("\n=== Downstream pH (hydrogen-ion mixing) ===");
    eprintln!("  finite storms: engine {} / {N} ({} zero-runoff NaN dropped)", engine.len(), n_nan);
    eprintln!("  engine mean={:.4}  SELDM mean={:.4}", mean(&engine), mean(&seldm_p));
    // pH is on a log scale already; compare absolute pH units (0.15 pH is tight).
    for &p in &[0.05, 0.25, 0.5, 0.75, 0.95] {
        let (qe, qs) = (quantile(&engine, p), quantile(&seldm_p, p));
        eprintln!("  p={p:.2}  engine={qe:.4}  SELDM={qs:.4}  Δ={:.4}", (qe - qs).abs());
        assert!((qe - qs).abs() < 0.15, "downstream pH mismatch at p={p}: {qe} vs {qs}");
    }
}

#[test]
fn seldm_style_plotting_positions() {
    // Emit the SELDM-style ranked result (plotting positions) for one constituent —
    // this is SELDM's actual analysis product: a ranked population with plotting
    // positions indicating exceedance risk. Cunnane (formula 1), ascending.
    let m = parse_v2(&build_model()).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let res = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let tss = sorted(res.elements["c1"].final_values.clone()); // NonUrban TSS
    let n = tss.len();
    eprintln!("\n=== SELDM-style ranked result: FHWA NonUrban TSS (Cunnane plotting positions) ===");
    for &p in &[0.5, 0.8, 0.9, 0.95, 0.99] {
        let rank = seldm::fnlng_rank_from_pp(p, n, 1, true);
        let value = tss[(rank - 1).min(n - 1)];
        let pp = seldm::plotting_position(rank, n, 1, true);
        eprintln!("  exceedance {:>4.0}%  →  TSS ≈ {:>8.2} mg/L  (rank {}, pp {:.4})",
                  (1.0 - p) * 100.0, value, rank, pp);
    }
    assert!(tss[n - 1] > tss[0], "ranked population should span a range");
}

/// Ground-truth validation: compare the engine's highway-runoff concentration ECDFs
/// against the REAL SELDM Access-application output for this scenario (Run001/).
///
/// This is engine-vs-application, not engine-vs-our-transcription: the Run001 files are
/// the output of the actual USGS SELDM v1.1.1 desktop app for the I-81 example. The real
/// run selected the FHWA TSS trio (blocks 0–2 of the HQ file) as its highway constituents
/// — the same three we model as c1/c2/c3 — plus a transport-curve SSC (block 3) we don't.
///
/// Highway *concentration* is flow-independent in SELDM's random-QW method (C = mean +
/// SD·Ks(U), no flow term), so this comparison is valid regardless of the real run's
/// precipitation-selection method (Rain Zone 3 average) differing from our model inputs;
/// only loads and downstream mixing depend on flow.
///
/// The Run001 files live in the seldm-wasim repo, outside this engine repo. The test
/// skips (does not fail) when they are absent, so it stays portable.
#[test]
fn validate_against_real_seldm_output() {
    let hq_path = "/Users/collord/seldm-wasim/Run001/ExampleAnalysisI81-HQ.txt";
    let Ok(bytes) = std::fs::read(hq_path) else {
        eprintln!("SKIP: real SELDM output not found at {hq_path}");
        return;
    };
    // The SELDM output is latin-1 (contains en-dashes etc.), not UTF-8; convert lossily.
    let contents: String = bytes.iter().map(|&b| b as char).collect();

    // Parse the HQ file into per-constituent blocks. A new block starts at each
    // "StormNumber" header; column 4 (0-based index 3) is RunoffConcentration.
    let mut blocks: Vec<Vec<f64>> = Vec::new();
    for line in contents.lines() {
        if line.starts_with("StormNumber") {
            blocks.push(Vec::new());
            continue;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(cur) = blocks.last_mut() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() >= 4 {
                if let Ok(v) = cols[3].parse::<f64>() {
                    cur.push(v);
                }
            }
        }
    }
    assert!(blocks.len() >= 3, "expected ≥3 HQ constituent blocks, got {}", blocks.len());

    let m = parse_v2(&build_model()).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let res = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // The real run emits the TSS trio in the order documented in the HQ header block:
    // block 0 = NonUrban, block 1 = UltraUrban, block 2 = Urban (block 3 = SSC, excluded).
    // Map each real block to the matching engine constituent (c1/c2/c3) accordingly.
    let pairs = [("c1", 0, "FHWA NonUrban TSS"), ("c3", 1, "FHWA UltraUrban TSS"), ("c2", 2, "FHWA Urban TSS")];
    eprintln!("\n=== Validation vs REAL SELDM Access app (Run001/) — highway TSS concentration ===");
    for (elem, block, name) in pairs {
        let engine = sorted(res.elements[elem].final_values.clone());
        let real = sorted(blocks[block].clone());
        eprintln!("  {name} (real n={}):", real.len());
        eprintln!("    {:>10} {:>12} {:>12} {:>8}", "exceedance", "Access", "WASiM", "rel");
        // Body of the distribution: median through 90th percentile. The far tail (99th)
        // is noisy at the real run's n≈1586 and carries the Pearson-III path divergence;
        // reported below but not asserted.
        for &pp in &[0.5, 0.75, 0.9] {
            let (qr, qe) = (quantile(&real, pp), quantile(&engine, pp));
            let rel = (qe - qr).abs() / qr.abs().max(1e-9);
            eprintln!("    {:>9.0}% {:>12.2} {:>12.2} {:>7.1}%", (1.0 - pp) * 100.0, qr, qe, rel * 100.0);
            assert!(rel < 0.08, "{name} at {}% exceedance: Access {qr} vs WASiM {qe} (rel {rel})", (1.0 - pp) * 100.0);
        }
        for &pp in &[0.95, 0.99] {
            let (qr, qe) = (quantile(&real, pp), quantile(&engine, pp));
            eprintln!("    {:>9.0}% {:>12.2} {:>12.2} {:>7.1}%  (tail, reported)",
                      (1.0 - pp) * 100.0, qr, qe, (qe - qr).abs() / qr.abs().max(1e-9) * 100.0);
        }
    }
}
