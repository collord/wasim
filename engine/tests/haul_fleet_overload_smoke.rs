//! Stage-3 RAM demo smoke test: parameters_examples/haul_fleet_overload.json.
//!
//! The N-truck fleet model — per-truck cumulative damage over a `Fleet` dimension (the array
//! lane: vector_map + power-law rate + lag/index recurrence), per-truck failure latch, and
//! wear-levelling dispatch (assign the overload to the least-damaged truck via the penalty
//! idiom `argmin_array(damage + BIG*failed)`). This test pins that the full fleet model parses,
//! runs, surfaces per-truck `#k` results, and stays internally consistent, and that it is
//! bit-identical on repeat.

use std::fs;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run() -> SimulationResults {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../parameters_examples/haul_fleet_overload.json"
    ))
    .expect("read model");
    let m = parse_v2(&json).expect("parse v2 fleet model");
    let g = ModelGraphV2::build(&m).expect("build graph");
    let cfg = RunConfig { seed: Some(42), ..RunConfig::default() };
    run_v2(&m, &g, &cfg).expect("run_v2")
}

fn finals(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements[id].final_values.clone()
}
fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

#[test]
fn fleet_model_runs_and_surfaces_per_truck_state() {
    let r = run();

    // Per-truck damage surfaces as `damage#1..#N` (dimensioned output expansion).
    let mut members: Vec<String> = r
        .elements
        .keys()
        .filter(|k| k.starts_with("damage#"))
        .cloned()
        .collect();
    members.sort();
    println!("per-truck damage series: {members:?}");
    assert_eq!(members.len(), 5, "fleet of 5 should surface 5 per-truck damage series");
    for k in &members {
        let v = finals(&r, k);
        assert!(v.iter().all(|x| x.is_finite()), "{k} must be finite");
        println!("  {k}: mean final damage = {:.4}", mean(&v));
    }

    // Fleet aggregates are consistent and the fleet actually accumulates wear.
    let dmean = mean(&finals(&r, "damage_mean"));
    let dspread = mean(&finals(&r, "damage_spread"));
    let avail = mean(&finals(&r, "available_trucks"));
    let npv = mean(&finals(&r, "npv"));
    let production = mean(&finals(&r, "production"));
    println!("fleet: damage_mean={dmean:.4}  damage_spread={dspread:.4}  available={avail:.3}/5  npv={npv:.1}  production={production:.1}");

    assert!(dmean > 0.0, "fleet should accumulate damage, got {dmean}");
    assert!(dspread >= 0.0 && dspread.is_finite(), "spread must be a valid non-negative number: {dspread}");
    // max-min spread cannot exceed the number of trucks times any single damage; a loose sanity cap.
    assert!(dspread <= 5.0 * (dmean + 1.0), "spread implausibly large vs mean: {dspread} vs {dmean}");
    assert!((0.0..=5.0).contains(&avail), "available trucks within [0,5], got {avail}");
    assert!(npv.is_finite() && production >= 0.0, "npv finite ({npv}), production non-negative ({production})");
}

#[test]
fn fleet_model_is_bit_identical() {
    let a = run();
    let b = run();
    for id in ["damage#1", "damage#3", "damage_spread", "npv", "available_trucks"] {
        assert_eq!(finals(&a, id), finals(&b, id), "{id} not bit-identical across runs");
    }
}
