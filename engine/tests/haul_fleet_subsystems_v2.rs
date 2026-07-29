//! Stage-3e: does multi-subsystem (modal) failure dissolve the wear-levelling burstiness that
//! Stage-3d's single composite failure produced? Each truck fails by three independent
//! subsystems with different load exponents (tires 4, drivetrain 3, structure 1.5), so the
//! failure stream is a superposition of different-rate processes. Hypothesis: less bursty ⇒
//! wear-levelling's shop-downtime penalty vs naive shrinks below Stage-3d's 1.67x.

use std::fs;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run_policy(policy: f64) -> SimulationResults {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../parameters_examples/haul_fleet_subsystems.json"
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

fn fin(r: &SimulationResults, id: &str) -> f64 {
    let v = &r.elements[id].final_values;
    v.iter().sum::<f64>() / v.len() as f64
}
fn peak(r: &SimulationResults, id: &str) -> f64 {
    r.elements[id].time_history.as_ref().unwrap().mean.iter().cloned().fold(0.0, f64::max)
}

const STAGE_3D_RATIO: f64 = 78.7 / 47.2; // 1.667 — composite-failure wear-levelling penalty

#[test]
fn modal_failure_shrinks_the_wear_levelling_shop_penalty() {
    let naive = run_policy(0.0);
    let wl = run_policy(1.0);
    let cohort = run_policy(2.0);

    let (d_n, d_w, d_c) = (fin(&naive, "cum_downtime"), fin(&wl, "cum_downtime"), fin(&cohort, "cum_downtime"));
    let ratio = d_w / d_n;
    println!("cumulative downtime : naive={d_n:.1}  wear-level={d_w:.1}  cohort={d_c:.1}");
    println!("peak backlog        : naive={:.3}  wear-level={:.3}  cohort={:.3}", peak(&naive, "backlog"), peak(&wl, "backlog"), peak(&cohort, "backlog"));
    println!("wear-level/naive downtime ratio = {ratio:.3}  (composite Stage-3d was {STAGE_3D_RATIO:.3})");
    println!("per-mode failures (wear-level): tires={:.1} drive={:.1} struct={:.1}",
        fin(&wl, "cumfail_tires"), fin(&wl, "cumfail_drive"), fin(&wl, "cumfail_struct"));

    // The hypothesis: splitting the composite failure into independent subsystems with different
    // exponents desynchronizes the failure stream, so wear-levelling's shop penalty is smaller
    // than in the single-composite Stage-3d model.
    assert!(ratio < STAGE_3D_RATIO,
        "modal failure should shrink the wear-levelling penalty below the composite {STAGE_3D_RATIO:.3}, got {ratio:.3}");

    // Sanity: the multi-mode structure works — tires (highest exponent + rate) are the most
    // critical subsystem, driving the most failures (standard RAM criticality ranking).
    let (ct, cd, cs) = (fin(&wl, "cumfail_tires"), fin(&wl, "cumfail_drive"), fin(&wl, "cumfail_struct"));
    assert!(ct > cd && cd > cs, "criticality order tires > drive > struct expected, got {ct:.1} {cd:.1} {cs:.1}");
}

#[test]
fn subsystems_run_is_bit_identical() {
    let a = run_policy(1.0);
    let b = run_policy(1.0);
    assert_eq!(
        a.elements["cum_downtime"].time_history.as_ref().unwrap().mean,
        b.elements["cum_downtime"].time_history.as_ref().unwrap().mean,
    );
}
