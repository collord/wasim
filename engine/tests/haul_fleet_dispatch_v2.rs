//! Stage-3b: dispatch-policy comparison on the fleet model (spec 1.3). One model, run three
//! times with dispatch_policy = 0 (naive round-robin), 1 (wear-levelling), 2 (cohorting).
//! Wear-levelling keeps the fleet balanced (low peak damage spread); cohorting drives a large
//! imbalance and sacrifices one truck early. Proves the wear-levelling diagnostic numerically.

use std::fs;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run_policy(policy: f64) -> SimulationResults {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../parameters_examples/haul_fleet_dispatch.json"
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

/// Peak over time of the mean-across-realizations damage spread (max fleet imbalance reached).
fn peak_spread(r: &SimulationResults) -> f64 {
    r.elements["damage_spread"].time_history.as_ref().unwrap().mean.iter().cloned().fold(0.0, f64::max)
}
fn final_mean(r: &SimulationResults, id: &str) -> f64 {
    let v = &r.elements[id].final_values;
    v.iter().sum::<f64>() / v.len() as f64
}

#[test]
fn dispatch_policies_compare_as_expected() {
    let naive = run_policy(0.0);
    let wl = run_policy(1.0);
    let cohort = run_policy(2.0);

    let (ps_n, ps_w, ps_c) = (peak_spread(&naive), peak_spread(&wl), peak_spread(&cohort));
    let avail = |r| final_mean(r, "available_trucks");
    let (av_n, av_w, av_c) = (avail(&naive), avail(&wl), avail(&cohort));
    let failed = |r| final_mean(r, "failed_count");

    println!("peak damage spread   : naive={ps_n:.3}  wear-level={ps_w:.3}  cohort={ps_c:.3}");
    println!("available @ horizon  : naive={av_n:.3}  wear-level={av_w:.3}  cohort={av_c:.3}  (of 6)");
    println!("failed @ horizon     : naive={:.3}  wear-level={:.3}  cohort={:.3}", failed(&naive), failed(&wl), failed(&cohort));

    // (1) Cohorting drives a large fleet imbalance; wear-levelling and naive stay balanced.
    //     This is the wear-levelling diagnostic (spec 2.4): 0 = balanced, large = a few trucks
    //     racing ahead. Cohort concentrates all overload on one truck; the other two policies
    //     spread it, so their fleets stay even.
    assert!(ps_c > 0.35, "cohorting should reach a large peak spread, got {ps_c}");
    assert!(ps_w < 0.05, "wear-levelling should stay balanced, got {ps_w}");
    assert!(ps_n < 0.20, "naive round-robin should stay fairly balanced, got {ps_n}");

    // (2) Cohort's peak imbalance dwarfs the balancing policies'.
    assert!(ps_c > 5.0 * ps_w, "cohort peak spread should dwarf wear-levelling ({ps_c} vs {ps_w})");
    assert!(ps_c > 3.0 * ps_n, "cohort peak spread should dwarf naive ({ps_c} vs {ps_n})");

    // (3) Wear-levelling is the most balanced of the three (≤ naive).
    assert!(ps_w <= ps_n + 0.02, "wear-levelling should be at least as balanced as naive ({ps_w} vs {ps_n})");

    // (4) The counterintuitive tradeoff (spec 1.3): wear-levelling equalizes damage, which
    //     SYNCHRONIZES lifetimes — the fleet ages together and fails in a cluster near
    //     end-of-life, so MORE trucks have failed by the horizon snapshot than under cohorting,
    //     which deliberately staggers failures (sacrifice T01, then T02, …). Balanced ≠ safer.
    assert!(
        failed(&wl) > failed(&cohort) + 0.1,
        "wear-levelling should cluster failures (more failed by horizon: {:.2}) vs staggered cohorting ({:.2})",
        failed(&wl), failed(&cohort),
    );
    let _ = (av_n, av_w, av_c);
}

#[test]
fn dispatch_run_is_bit_identical() {
    let a = run_policy(1.0);
    let b = run_policy(1.0);
    assert_eq!(
        a.elements["damage_spread"].time_history.as_ref().unwrap().mean,
        b.elements["damage_spread"].time_history.as_ref().unwrap().mean,
        "wear-levelling run not bit-identical",
    );
}
