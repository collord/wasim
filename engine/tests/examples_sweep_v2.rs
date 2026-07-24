//! Rot guard for the hand-authored sweep-composition examples in schema_examples/. The corpus
//! parse test (integration::parse_all_schema_examples) already checks these parse; this asserts
//! they RUN and produce the structurally-correct risk quantities they exist to demonstrate, so a
//! future engine change that quietly breaks the exceedance/reducer path fails loudly here.
//!
//! Assertions are RNG-robust (shape/ordering/bounds, not exact draws) but tight enough to catch
//! a broken reducer or a collapsed curve.

use std::path::PathBuf;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn examples_dir() -> PathBuf {
    // Same convention as integration.rs / timebase_*.rs.
    PathBuf::from(std::env::var("HOME").unwrap()).join("openvsim/wasim/schema_examples")
}

fn final0(r: &wasim_engine::SimulationResults, id: &str) -> f64 {
    r.elements
        .get(id)
        .and_then(|e| e.final_values.first().copied())
        .unwrap_or_else(|| panic!("missing final value for {id}"))
}

fn run(name: &str) -> Option<wasim_engine::SimulationResults> {
    let path = examples_dir().join(name);
    if !path.exists() {
        eprintln!("skipping {name}: corpus not present");
        return None;
    }
    let json = std::fs::read_to_string(&path).unwrap();
    let m = parse_v2(&json).unwrap_or_else(|e| panic!("{name} parse: {e:?}"));
    let g = ModelGraphV2::build(&m).unwrap_or_else(|e| panic!("{name} graph: {e:?}"));
    Some(run_v2(&m, &g, &RunConfig::default()).unwrap_or_else(|e| panic!("{name} run: {e:?}")))
}

/// loss_exceedance_curve.json: vector_map over the exceedance reducer must yield a genuine
/// loss-exceedance curve — one value per threshold, monotone non-increasing as the threshold
/// rises, in [0,1] — and the CTE tail_risk must feed the downstream accept/mitigate decision.
#[test]
fn loss_exceedance_curve_produces_monotone_curve_and_decision() {
    let Some(r) = run("loss_exceedance_curve.json") else { return };

    let curve: Vec<f64> = (1..=5).map(|k| final0(&r, &format!("Model/exceedance_curve#{k}"))).collect();
    for (k, &p) in curve.iter().enumerate() {
        assert!((0.0..=1.0).contains(&p), "curve[{k}] = {p} out of [0,1]");
    }
    for k in 1..5 {
        assert!(curve[k] <= curve[k - 1] + 1e-12,
            "exceedance curve must be non-increasing across ascending thresholds: {curve:?}");
    }
    // Lognormal(2,1) losses: P(loss > 5) is a healthy fraction, P(loss > 80) is a thin tail.
    assert!(curve[0] > 0.3, "P(loss>5) should be substantial, got {}", curve[0]);
    assert!(curve[4] < 0.1, "P(loss>80) should be a thin tail, got {}", curve[4]);

    // The reduced tail risk feeds a downstream decision (sample-as-axis feeding onward).
    let tail = final0(&r, "Model/tail_risk");
    let decision = final0(&r, "Model/decision");
    assert!(tail > 0.0, "CTE tail_risk should be positive, got {tail}");
    let expected = if tail > 50.0 { 1.0 } else { 0.0 };
    assert_eq!(decision, expected, "decision must gate on tail_risk vs the 50-unit budget (tail={tail})");
}

/// submodel_tail_stats.json: each extended reducer produces a sane scalar and they order
/// correctly for a Gamma(2,8) loss (min ≤ mean ≤ CTE_95 ≤ max; prob in [0,1]; capital ≥ 0).
#[test]
fn submodel_tail_stats_reducers_order_correctly() {
    let Some(r) = run("submodel_tail_stats.json") else { return };

    let best = final0(&r, "Model/best_case");        // min
    let mean = final0(&r, "Model/expected_loss");    // mean
    let cte = final0(&r, "Model/cte_95");            // conditional tail expectation
    let worst = final0(&r, "Model/worst_case");      // max
    let prob = final0(&r, "Model/prob_over_25");     // exceedance
    let capital = final0(&r, "Model/capital_need");  // cte - mean

    assert!(best <= mean, "min ({best}) <= mean ({mean})");
    assert!(mean <= cte, "mean ({mean}) <= CTE_95 ({cte}) — the tail mean is above the overall mean");
    assert!(cte <= worst + 1e-9, "CTE_95 ({cte}) <= max ({worst})");
    assert!((0.0..=1.0).contains(&prob), "exceedance prob {prob} out of [0,1]");
    assert!(capital >= 0.0, "capital need (CTE - mean) must be non-negative, got {capital}");
    // Gamma(2,8) has mean 16 analytically; a 4000-sample estimate should be in the ballpark.
    assert!((8.0..24.0).contains(&mean), "expected_loss ~16 for Gamma(2,8), got {mean}");
}
