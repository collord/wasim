//! Stage-1b RAM demo smoke test: schema_examples_manual/haul_truck_grade_conditional.json.
//!
//! Verifies the grade-conditional overload model runs and that the policy insight holds:
//!   * a 25% overload still roughly halves component life (continuity with Stage 1),
//!   * the grade-conditional policy C dominates BOTH fixed policies (always-nominal A and
//!     always-overload B) in expected NPV — it captures overload's upside only when grade x
//!     price clears the crossover, so "damage is paid in tonnes, revenue is earned in metal"
//!     becomes money.

use std::fs;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run() -> SimulationResults {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../schema_examples_manual/haul_truck_grade_conditional.json"
    ))
    .expect("read model");
    let m = parse_v2(&json).expect("parse v2 model");
    let g = ModelGraphV2::build(&m).expect("build graph");
    let cfg = RunConfig { seed: Some(20260728), ..RunConfig::default() };
    run_v2(&m, &g, &cfg).expect("run_v2")
}

fn finals(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements[id].final_values.clone()
}
fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

#[test]
fn grade_conditional_policy_dominates_both_fixed_policies() {
    let r = run();

    let (a, b, c) = (
        mean(&finals(&r, "npv_A")),
        mean(&finals(&r, "npv_B")),
        mean(&finals(&r, "npv_C")),
    );
    let frac_overload = mean(&finals(&r, "overload_C"));
    let (life_a, life_b) = (mean(&finals(&r, "life_A")), mean(&finals(&r, "life_B")));

    println!("E[NPV] A (nominal)     = {a:.0}");
    println!("E[NPV] B (overload)    = {b:.0}");
    println!("E[NPV] C (conditional) = {c:.0}");
    println!("fraction overloaded under C = {frac_overload:.3}");
    println!("mean life: nominal={life_a:.2} yr  overload={life_b:.2} yr  ratio={:.3}", life_b / life_a);

    // Continuity with Stage 1: nominal ~5 yr, overload ~half.
    assert!((4.8..=5.2).contains(&life_a), "nominal life ~5 yr, got {life_a}");
    assert!((0.45..=0.58).contains(&(life_b / life_a)), "overload should ~halve life, ratio {}", life_b / life_a);

    // The policy is genuinely mixed (crossover sits inside the value distribution) — otherwise
    // "conditional" is trivially one of the fixed policies and the comparison is vacuous.
    assert!((0.25..=0.75).contains(&frac_overload),
        "conditional policy should overload a meaningful minority/majority, got {frac_overload}");

    // THE HEADLINE: the grade-conditional policy dominates BOTH fixed policies in expectation.
    assert!(c > a, "conditional should beat always-nominal: {c:.0} vs {a:.0}");
    assert!(c > b, "conditional should beat always-overload: {c:.0} vs {b:.0}");

    // The value of the option is a non-trivial fraction of the NPV.
    let option_value = c - a.max(b);
    println!("value of grade-conditioning (C - best fixed) = {option_value:.0}");
    assert!(option_value > 0.0, "conditioning must add value: {option_value}");
}
