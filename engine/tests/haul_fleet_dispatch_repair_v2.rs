//! Stage-3c: dispatch policies on a fleet with OVERHAUL + heterogeneous ages (young vs old).
//! Trucks start at a damage ramp and are instantly overhauled (reset to young) on failure, so
//! the fleet keeps a mix of ages. In Stage-3b's symmetric run-to-failure fleet naive and
//! wear-levelling were identical; here the age asymmetry gives argmin something to correct, so
//! wear-levelling SEPARATES from naive. This test pins that separation.

use std::fs;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run_policy(policy: f64) -> SimulationResults {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../parameters_examples/haul_fleet_dispatch_repair.json"
    ))
    .expect("read model");
    let mut v: serde_json::Value = serde_json::from_str(&json).expect("json");
    for el in v["elements"].as_array_mut().unwrap() {
        if el["id"] == "dispatch_policy" {
            el["value"]["value"] = serde_json::json!(policy);
        }
    }
    let m = parse_v2(&v.to_string()).expect("parse v2");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { seed: Some(20260729), ..RunConfig::default() };
    run_v2(&m, &g, &cfg).expect("run_v2")
}

/// Mean (over the second half of the horizon = steady state) of the mean-across-realizations
/// damage spread. Avoids the initial-ramp transient.
fn steady_spread(r: &SimulationResults) -> f64 {
    let h = &r.elements["damage_spread"].time_history.as_ref().unwrap().mean;
    let half = h.len() / 2;
    h[half..].iter().sum::<f64>() / (h.len() - half) as f64
}
fn final_mean(r: &SimulationResults, id: &str) -> f64 {
    let v = &r.elements[id].final_values;
    v.iter().sum::<f64>() / v.len() as f64
}

#[test]
fn young_vs_old_separates_wear_levelling_from_naive() {
    let naive = run_policy(0.0);
    let wl = run_policy(1.0);
    let cohort = run_policy(2.0);

    let (ss_n, ss_w, ss_c) = (steady_spread(&naive), steady_spread(&wl), steady_spread(&cohort));
    let cf = |r| final_mean(r, "cum_failures");
    println!("steady-state damage spread : naive={ss_n:.3}  wear-level={ss_w:.3}  cohort={ss_c:.3}");
    println!("overhauls over horizon     : naive={:.2}  wear-level={:.2}  cohort={:.2}", cf(&naive), cf(&wl), cf(&cohort));

    // The headline: with young-vs-old asymmetry, wear-levelling is no longer identical to naive
    // (in Stage-3b both were 0.006). They now differ materially.
    assert!((ss_n - ss_w).abs() > 0.05, "wear-levelling should now SEPARATE from naive ({ss_n} vs {ss_w})");

    // Wear-levelling actively equalizes the mixed ages, so it produces the MOST balanced fleet
    // of the three — below both naive round-robin and cohorting.
    assert!(ss_w < ss_n, "wear-levelling should be more balanced than naive ({ss_w} vs {ss_n})");
    assert!(ss_w < ss_c, "wear-levelling should be the most balanced of the three ({ss_w} vs {ss_c})");

    // (Note: with overhaul, cohorting is no longer the least balanced — it rapidly resets its
    // sacrificed truck (fail → overhaul → fail), so a freshly-young member keeps its spread in
    // the middle. Naive round-robin, phase-distributing trucks across the full sawtooth, is now
    // the least balanced.)
    assert!(ss_n > 0.5 && ss_c > 0.5, "a mixed-age overhauling fleet carries real spread ({ss_n}, {ss_c})");

    // Total overhaul work is roughly policy-independent (same total damage inflicted): the
    // policies differ in HOW wear is distributed, not how much — so no policy is a free lunch.
    let (fn_n, fn_w) = (cf(&naive), cf(&wl));
    assert!((fn_n - fn_w).abs() < 0.35 * fn_n.max(1.0), "overhaul counts should be comparable ({fn_n} vs {fn_w})");
}

#[test]
fn repair_run_is_bit_identical() {
    let a = run_policy(1.0);
    let b = run_policy(1.0);
    assert_eq!(
        a.elements["damage_spread"].time_history.as_ref().unwrap().mean,
        b.elements["damage_spread"].time_history.as_ref().unwrap().mean,
    );
}
