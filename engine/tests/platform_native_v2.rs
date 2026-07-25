//! Rot guard for the hand-authored WASiM-native platform-decommissioning example
//! (`tools/examples/platform_decommissioning_native.wasim.json`) — the native
//! counterpart to the mechanically-converted `Platform_2017.wasim.json`.
//!
//! It distills the real model's purpose — rank decommissioning options by
//! multi-attribute value under cost/benefit uncertainty — into the engine's own
//! machinery: a `Decision_MC` submodel scores the options per realization
//! (`vector_map` over the Option axis + `argmax_array`) and exposes SCALAR
//! outputs, which the parent reduces across realizations natively (`mean` for
//! expected scores/cost, `cumulative_prob`/`exceedance` differences for the
//! probability each option is preferred, `argmax_array` over expected scores for
//! the recommendation). This is the across-the-Run-sample reduction the mechanical
//! 0.1.0 conversion cannot express (gap analysis §2), done in-graph.
//!
//! Assertions are RNG-robust: they pin down the decision structure (preference
//! probabilities form a proper distribution, the recommendation is the
//! highest-expected-score option, and the choice is genuinely uncertain — not a
//! foregone conclusion) rather than exact Monte-Carlo values.

use std::path::PathBuf;
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/examples/platform_decommissioning_native.wasim.json")
}

#[test]
fn native_platform_mada_ranks_options_under_uncertainty() {
    let json = std::fs::read_to_string(example_path()).expect("read example");
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    let s = |id: &str| -> f64 {
        r.elements
            .get(id)
            .and_then(|e| e.final_values.first().copied())
            .unwrap_or_else(|| panic!("missing {id}"))
    };

    let (ef, er, el) = (s("Model/exp_score_full"), s("Model/exp_score_reef"), s("Model/exp_score_leave"));
    let (pf, pr, pl) = (s("Model/pref_full"), s("Model/pref_reef"), s("Model/pref_leave"));

    // Preference probabilities form a proper distribution over the three options.
    for (name, p) in [("full", pf), ("reef", pr), ("leave", pl)] {
        assert!((0.0..=1.0).contains(&p), "P({name} preferred) out of range: {p}");
    }
    assert!((pf + pr + pl - 1.0).abs() < 1e-6, "preference probabilities must sum to 1, got {}", pf + pr + pl);

    // Recommendation = the highest-expected-score option (Leave, index 3 here).
    let rec = s("Model/recommended_idx");
    let exp = [ef, er, el];
    let argmax = (0..3).max_by(|&a, &b| exp[a].total_cmp(&exp[b])).unwrap() + 1;
    assert_eq!(rec as usize, argmax, "recommended_idx must be the argmax of expected scores");
    assert!(el > er && er > ef, "expected scores ordered Leave > Reef > Full, got {ef}/{er}/{el}");

    // The point of the probabilistic model: the choice is NOT a foregone
    // conclusion. The runner-up (Reef) is preferred a meaningful fraction of the
    // time, and the dominated option (Full removal) almost never wins.
    assert!(pr > 0.1, "runner-up should win a non-trivial share of futures, got P(reef)={pr}");
    assert!(pl < 0.95, "recommendation should not be near-certain, got P(leave)={pl}");
    assert!(pf < 0.1, "dominated option should rarely be preferred, got P(full)={pf}");

    // Cost outlook of the preferred option is a sensible positive dollar figure.
    let ecost = s("Model/exp_chosen_cost");
    assert!((40.0..=200.0).contains(&ecost), "expected chosen cost out of band: {ecost}");
    assert!((0.0..=1.0).contains(&s("Model/prob_cost_over_budget")));
}
