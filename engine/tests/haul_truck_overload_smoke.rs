//! Stage-1 RAM demo smoke test: schema_examples_manual/haul_truck_overload_reliability.json.
//!
//! Verifies the single-unit haul-truck overload model runs through the v2 engine and that the
//! RAM insight holds numerically:
//!   * both trucks fail via the `condition`-basis failure state machine (damage stock >= 1.0),
//!   * a 25% overload roughly HALVES time-to-first-failure at beta ~ 3 (power-law life),
//!   * life *uncertainty* concentrates in the overload policy (nominal TTF is ~deterministic
//!     because ratio = 1 makes it beta-independent; overload TTF spreads with beta),
//!   * the run is bit-identical on repeat (the reproducibility guarantee).
//!
//! This is also the regression that exercises a `condition` failure basis driven by a real
//! accumulating damage stock (previously only unit-tested with a clock trigger).

use std::fs;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run(seed: u64) -> SimulationResults {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../schema_examples_manual/haul_truck_overload_reliability.json"
    ))
    .expect("read model");
    let m = parse_v2(&json).expect("parse v2 model");
    let g = ModelGraphV2::build(&m).expect("build graph");
    let cfg = RunConfig { seed: Some(seed), ..RunConfig::default() };
    run_v2(&m, &g, &cfg).expect("run_v2")
}

fn finals(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements[id].final_values.clone()
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

fn std(v: &[f64]) -> f64 {
    let m = mean(v);
    (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
}

fn pct(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((p / 100.0) * (s.len() as f64 - 1.0)).round() as usize;
    s[idx]
}

#[test]
fn haul_truck_overload_stage1_runs_and_shows_halved_life() {
    let r = run(20260728);

    let up_nom = finals(&r, "uptime_nom");
    let up_ovl = finals(&r, "uptime_ovl");
    let fail_nom = finals(&r, "fsm_nom");
    let fail_ovl = finals(&r, "fsm_ovl");

    let (m_nom, m_ovl) = (mean(&up_nom), mean(&up_ovl));
    let (s_nom, s_ovl) = (std(&up_nom), std(&up_ovl));
    let ratio = m_ovl / m_nom;

    println!("nominal  TTF: mean={m_nom:.3} yr  std={s_nom:.4}  p10={:.3} p50={:.3} p90={:.3}",
        pct(&up_nom, 10.0), pct(&up_nom, 50.0), pct(&up_nom, 90.0));
    println!("overload TTF: mean={m_ovl:.3} yr  std={s_ovl:.4}  p10={:.3} p50={:.3} p90={:.3}",
        pct(&up_ovl, 10.0), pct(&up_ovl, 50.0), pct(&up_ovl, 90.0));
    println!("overload/nominal life ratio = {ratio:.3}");

    // (1) Both trucks fail within the 10 yr horizon in every realization (condition-basis FSM
    //     fires; final output 1.0 = failed).
    assert!((mean(&fail_nom) - 1.0).abs() < 1e-9, "nominal truck should always fail: {}", mean(&fail_nom));
    assert!((mean(&fail_ovl) - 1.0).abs() < 1e-9, "overload truck should always fail: {}", mean(&fail_ovl));

    // (2) Nominal life is ~5 yr (base_rate 0.2/yr -> damage 1.0 at t=5) and beta-independent, so
    //     it is essentially deterministic across realizations.
    assert!((4.9..=5.1).contains(&m_nom), "nominal mean TTF should be ~5 yr, got {m_nom}");
    assert!(s_nom < 0.02, "nominal TTF should be ~deterministic (beta cancels), std={s_nom}");

    // (3) THE HEADLINE: a 25% overload roughly halves component life at beta ~ 3.
    assert!((2.2..=2.85).contains(&m_ovl), "overload mean TTF should be ~2.5 yr, got {m_ovl}");
    assert!((0.44..=0.58).contains(&ratio),
        "overload should roughly halve life (ratio ~0.5), got {ratio}");

    // (4) Life uncertainty concentrates in the overload policy: overload TTF spreads with beta
    //     while nominal does not.
    assert!(s_ovl > 0.10, "overload TTF should have real spread from beta uncertainty, std={s_ovl}");
    assert!(s_ovl > 5.0 * s_nom.max(1e-6),
        "uncertainty should concentrate in the overload policy (s_ovl={s_ovl}, s_nom={s_nom})");
    let iqr_ovl = pct(&up_ovl, 90.0) - pct(&up_ovl, 10.0);
    assert!(iqr_ovl > 0.2, "overload P10..P90 life band should be wide, got {iqr_ovl}");

    // (5) Net NPV to first failure is finite and defined for both policies (economics wired).
    for id in ["net_npv_nom", "net_npv_ovl"] {
        let v = finals(&r, id);
        assert!(v.iter().all(|x| x.is_finite()), "{id} must be finite");
        println!("{id}: mean={:.1}", mean(&v));
    }
}

#[test]
fn haul_truck_overload_is_bit_identical() {
    // Same seed -> byte-identical results (the reproducibility guarantee).
    let a = run(20260728);
    let b = run(20260728);
    for id in ["uptime_nom", "uptime_ovl", "damage_ovl", "net_npv_ovl", "beta"] {
        assert_eq!(finals(&a, id), finals(&b, id), "{id} not bit-identical across runs");
    }
}
