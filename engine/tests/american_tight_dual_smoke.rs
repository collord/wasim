// Validates schema_examples_manual/american_put_tight_dual.json — the TIGHT LSM dual (upper bound) on
// an American put, and the resulting tight primal-dual bracket around the true price.
//
// The shipped single-hedge dual (`inner: 0`) is rigorous but loose (~9.7 vs a true ~6.05) — one hedging
// instrument can't replicate an American option. The tight dual (`inner > 0`) is the Andersen-Broadie-
// style Doob martingale of the discounted-intrinsic process, M_t = sum_{k<=t}(Y_k - E_{k-1}[Y_k]), with
// the one-step conditional expectation E_{k-1}[Y_k] estimated by NESTED SIMULATION (next-step states
// resampled from the panel's own one-step log-returns, payoff re-evaluated there — the nested_stat
// each_step operation done natively). It is a genuine martingale, so it stays a VALID upper bound, and it
// tracks the option far better than one hedge, so the bracket is TIGHT.
//
// Checks:
//  (1) The tight dual is a VALID upper bound: dual_tight >= the binomial American value.
//  (2) It is genuinely TIGHT: dual_tight is far below the loose single-hedge dual, and within a small
//      gap of the binomial value.
//  (3) The primal-dual SANDWICH brackets the true price: american_put (OOS lower) <= tree <= dual_tight.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, ci: f64 }

#[test]
fn tight_lsm_dual_brackets_the_american_price() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/american_put_tight_dual.json")).unwrap();
    let v1: WasimModel = serde_json::from_str(&json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    spec.elements = ["american_put", "dual_loose", "dual_tight"].iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { n_realizations: Some(40_000), seed: Some(20_250_727), results_spec: Some(spec),
        duration_override: Some(1.0), timestep_override: Some(0.02), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    let fs = |id: &str| {
        let a = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        FS { mean: a.mean, ci: a.ci_half_width }
    };
    let (primal, loose, tight) = (fs("american_put"), fs("dual_loose"), fs("dual_tight"));
    // Exercise dates are the start-of-step grid, so the option effectively matures at T_eff = T - dt.
    const BINOMIAL: f64 = 6.0449; // CRR American put at T_eff = 0.98
    println!("primal (OOS lower)      = {:.4} +/- {:.4}", primal.mean, primal.ci);
    println!("dual  (single-hedge)    = {:.4} +/- {:.4}   (loose)", loose.mean, loose.ci);
    println!("dual  (nested, tight)   = {:.4} +/- {:.4}   (binomial {:.4})", tight.mean, tight.ci, BINOMIAL);
    println!("bracket: [{:.3}, {:.3}]  (was [{:.3}, {:.3}])  true {:.3}",
        primal.mean, tight.mean, primal.mean, loose.mean, BINOMIAL);

    // (1) The tight dual is a VALID upper bound on the true (binomial) price.
    assert!(tight.mean >= BINOMIAL - 4.0 * tight.ci,
        "tight dual {} must be a valid upper bound on the true price {}", tight.mean, BINOMIAL);

    // (2) It is genuinely TIGHT: well below the loose single-hedge dual, and close to the true price.
    assert!(tight.mean < loose.mean - 1.5,
        "tight dual {} should be well below the loose single-hedge dual {}", tight.mean, loose.mean);
    assert!(tight.mean < BINOMIAL + 0.9,
        "tight dual {} should be within a small gap of the true price {} (tight)", tight.mean, BINOMIAL);

    // (3) The primal-dual sandwich brackets the true price.
    assert!(primal.mean <= BINOMIAL + 4.0 * primal.ci, "primal {} should be a lower bound", primal.mean);
    assert!(primal.mean < tight.mean, "primal {} must sit below the dual {}", primal.mean, tight.mean);
    // Sanity: the loose dual really is loose (contrast that motivates the tight one).
    assert!(loose.mean > BINOMIAL + 2.0, "single-hedge dual {} is expected to be loose", loose.mean);
    println!("tight primal-dual bracket around the American price via nested-simulation dual.");
}
