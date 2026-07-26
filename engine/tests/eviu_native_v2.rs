//! Rot guard for the hand-authored WASiM-native EVIU example
//! (`tools/examples/eviu_plane_catching_native.wasim.json`). It is the native
//! counterpart to the mechanically-converted `EVIU_plane_catching.wasim.json`:
//! the airport "when to leave" decision solved with the engine's own
//! sweep-composition machinery — a `Trip` submodel reduced across realizations
//! by `submodel_stat` (mean / percentile / exceedance) and swept over the
//! `Depart` decision axis by `vector_map`, then `argmin_array` / `get_element`
//! to pull out the optimum and the EVIU.
//!
//! Assertions are RNG-robust (shape / ordering / bounds, not exact draws) but
//! tight enough to catch a broken reducer, a collapsed sweep, or a sign flip in
//! the EVIU composition.

use std::path::PathBuf;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/examples/eviu_plane_catching_native.wasim.json")
}

#[test]
fn native_eviu_produces_ushaped_cost_curve_and_positive_eviu() {
    let json = std::fs::read_to_string(example_path()).expect("read example");
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    let scalar = |id: &str| -> f64 {
        r.elements
            .get(id)
            .and_then(|e| e.final_values.first().copied())
            .unwrap_or_else(|| panic!("missing scalar {id}"))
    };
    let curve = |id: &str, n: usize| -> Vec<f64> {
        (1..=n)
            .map(|k| {
                r.elements
                    .get(&format!("{id}#{k}"))
                    .and_then(|e| e.final_values.first().copied())
                    .unwrap_or_else(|| panic!("missing {id}#{k}"))
            })
            .collect()
    };

    let n = 25; // grid size (60..=180 step 5)

    // Mean travel time ≈ 40·e^… + 30·e^… + 30 ≈ 102 min.
    let mean_travel = scalar("Model/mean_travel");
    assert!((90.0..115.0).contains(&mean_travel), "mean_travel {mean_travel}");

    // P(miss) is a probability, monotone non-increasing, ~1 at the shortest lead
    // time and ~0 at the longest.
    let pmiss = curve("Model/prob_miss", n);
    assert!(pmiss[0] > 0.9, "P(miss) at shortest lead time should be ~1, got {}", pmiss[0]);
    assert!(*pmiss.last().unwrap() < 0.05, "P(miss) at longest lead time ~0, got {}", pmiss.last().unwrap());
    for k in 1..n {
        assert!(pmiss[k] <= pmiss[k - 1] + 1e-9, "P(miss) must be non-increasing at {k}");
        assert!((0.0..=1.0).contains(&pmiss[k]), "P(miss) in [0,1] at {k}");
    }

    // Expected cost is U-shaped: the minimum is interior (miss-penalty-dominated
    // on the left, waiting-dominated on the right), not at either endpoint.
    let exp_cost = curve("Model/exp_cost", n);
    let argmin = (0..n).min_by(|&a, &b| exp_cost[a].total_cmp(&exp_cost[b])).unwrap();
    assert!(argmin > 0 && argmin < n - 1, "expected-cost minimum should be interior, got idx {argmin}");
    assert!(exp_cost[0] > exp_cost[argmin] && *exp_cost.last().unwrap() > exp_cost[argmin],
        "curve must rise on both sides of the optimum");

    // Optima: the stochastic decision leaves EARLIER than the naive one that
    // ignores uncertainty (the whole point of the example).
    let best = scalar("Model/best_depart");
    let det_best = scalar("Model/det_best_depart");
    assert!(best > det_best, "stochastic optimum ({best}) must exceed the naive optimum ({det_best})");
    assert!((115.0..=155.0).contains(&best), "stochastic best lead time {best}");

    // EVIU = E[cost | naive decision] − min E[cost] > 0, and equals the curve gap.
    let eviu = scalar("Model/eviu");
    let min_exp = scalar("Model/min_exp_cost");
    let at_det = scalar("Model/exp_cost_at_det");
    assert!(eviu > 0.0, "EVIU must be positive, got {eviu}");
    assert!((eviu - (at_det - min_exp)).abs() < 1e-6, "EVIU must equal exp_cost_at_det − min_exp_cost");
    assert!((60.0..=220.0).contains(&eviu), "EVIU in a plausible band, got {eviu}");
    assert!((min_exp - exp_cost[argmin]).abs() < 1e-6, "min_exp_cost must equal the curve minimum");
}
