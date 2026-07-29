//! Stage-3d: dispatch policies against a CAPACITY-CONSTRAINED repair shop. The dispatch/overhaul
//! layer sets the failure timing (clustered under wear-levelling, which synchronises ages;
//! staggered under naive/cohort); a fluid repair queue with capped throughput then prices it.
//! When the shop is the binding constraint, wear-levelling's clustered failures back up the
//! queue and cost the most downtime — the case where "balanced" dispatch loses.

use std::fs;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run_policy(policy: f64) -> SimulationResults {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../parameters_examples/haul_fleet_dispatch_shop.json"
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

fn final_mean(r: &SimulationResults, id: &str) -> f64 {
    let v = &r.elements[id].final_values;
    v.iter().sum::<f64>() / v.len() as f64
}
fn peak(r: &SimulationResults, id: &str) -> f64 {
    r.elements[id].time_history.as_ref().unwrap().mean.iter().cloned().fold(0.0, f64::max)
}

#[test]
fn clustered_failures_cost_the_most_downtime_when_the_shop_is_the_constraint() {
    let naive = run_policy(0.0);
    let wl = run_policy(1.0);
    let cohort = run_policy(2.0);

    let dt = |r| final_mean(r, "cum_downtime");
    let (d_n, d_w, d_c) = (dt(&naive), dt(&wl), dt(&cohort));
    let (pb_n, pb_w, pb_c) = (peak(&naive, "backlog"), peak(&wl, "backlog"), peak(&cohort, "backlog"));

    println!("cumulative truck-downtime : naive={d_n:.1}  wear-level={d_w:.1}  cohort={d_c:.1}");
    println!("peak repair backlog       : naive={pb_n:.3}  wear-level={pb_w:.3}  cohort={pb_c:.3}");

    // The payoff: with the shop as the binding constraint, wear-levelling's clustered failures
    // overwhelm it, so it incurs the MOST downtime — more than either staggering policy.
    assert!(d_w > d_n, "wear-levelling should cost more downtime than naive when the shop is the constraint ({d_w:.1} vs {d_n:.1})");
    assert!(d_w > d_c, "wear-levelling should cost more downtime than cohorting ({d_w:.1} vs {d_c:.1})");

    // And it is driven by a higher peak backlog (clustered arrivals queue up).
    assert!(pb_w > pb_n, "wear-levelling should build a higher repair backlog ({pb_w:.3} vs {pb_n:.3})");
}

#[test]
fn shop_run_is_bit_identical() {
    let a = run_policy(1.0);
    let b = run_policy(1.0);
    assert_eq!(
        a.elements["cum_downtime"].time_history.as_ref().unwrap().mean,
        b.elements["cum_downtime"].time_history.as_ref().unwrap().mean,
    );
}
