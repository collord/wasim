// Validates schema_examples_manual/lookback_option_floating_strike.json through the v2 engine
// (normalize_v1 -> run_v2). A floating-strike lookback call — pays disc*(S(T) - min S), i.e. buy at
// the realized minimum, sell at maturity — priced by path Monte Carlo. It rounds out the
// running-extreme family alongside the barrier, exercising the same gap-#1/#2/#3 features on a fresh
// payoff: the floating strike is a filter(min) with include_terminal folding the terminal fixing
// S(T) (gaps #2/#3), the terminal is a terminal_expression (gap #3), and disc*S(T) is a control
// variate (run_stat2). Distinctively, the price satisfies an EXACT identity, checked in-model via a
// run_stat, and the discrete MC sits BELOW the continuous closed form (the mirror of the barrier's
// bias direction).
//
// Checks:
//  (1) In-model identity: MC lookback == S0 - disc*E[min S] (run_stat), to Monte Carlo error. Since
//      disc*E[S(T)] = S0, disc*E[S(T)-min] = S0 - disc*E[min] exactly — validates the payoff wiring,
//      the terminal read, and the running-min monitor together.
//  (2) The control mean disc*E[S(T)] == S0 (the control variate's known mean).
//  (3) Sec 3.2.2 discretization bias: discrete monitoring gives a higher (less extreme) minimum, so
//      the discrete MC lookback sits BELOW the Goldman-Sosin-Gatto continuous closed form.
//  (4) The MC lookback is within striking distance of the continuous form (the gap is the O(sqrt(dt))
//      monitoring bias, ~1.3 here), and the lookback is a substantial positive value.
//  (5) The disc*S(T) control variate reduces the estimator's variance while staying unbiased.
use std::fs;
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

struct FS { mean: f64, std: f64, ci: f64 }

fn stats(json: &str, n: u32) -> std::collections::HashMap<String, FS> {
    let v1: WasimModel = serde_json::from_str(json).expect("parse v1");
    let model = normalize_v1(&v1);
    let graph = ModelGraphV2::build(&model).expect("graph");
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    spec.elements = ids().iter().map(|s| s.to_string()).collect();
    let cfg = RunConfig { n_realizations: Some(n), seed: Some(70707), results_spec: Some(spec),
        duration_override: Some(1.0), timestep_override: Some(0.02), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    ids().into_iter().map(|id| {
        let fs = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        (id.to_string(), FS { mean: fs.mean, std: fs.std, ci: fs.ci_half_width })
    }).collect()
}
fn ids() -> Vec<&'static str> {
    vec!["lookback", "lookback_cv", "lb_cont", "lb_check", "ctrl"]
}

#[test]
fn lookback_floating_strike_prices_and_reduces_variance() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/lookback_option_floating_strike.json")).unwrap();
    let s = stats(&json, 20_000);
    let (lb, cv, cont, check, ctrl) =
        (&s["lookback"], &s["lookback_cv"], &s["lb_cont"], &s["lb_check"], &s["ctrl"]);
    println!("lb_cont  (GSG continuous)  = {:.4}", cont.mean);
    println!("lb_check (S0 - disc*E[min])= {:.4}", check.mean);
    println!("lookback MC : mean={:.4} std={:.4} ci={:.4}", lb.mean, lb.std, lb.ci);
    println!("lookback_cv : mean={:.4} std={:.4} ci={:.4}", cv.mean, cv.std, cv.ci);
    println!("ctrl mean (disc*E[S(T)])   = {:.4}", ctrl.mean);

    // (1) In-model identity: the MC lookback equals S0 - disc*E[min S] within MC error.
    assert!((lb.mean - check.mean).abs() < 4.0 * lb.ci,
        "lookback MC {} should equal the identity S0 - disc*E[min] {} (ci {})", lb.mean, check.mean, lb.ci);

    // (2) The control's known mean disc*E[S(T)] == S0 = 100.
    assert!((ctrl.mean - 100.0).abs() < 4.0 * ctrl.ci, "disc*E[S(T)] should be S0=100, got {}", ctrl.mean);

    // (3) Sec 3.2.2 bias: discrete monitoring => higher minimum => the discrete lookback sits BELOW
    // the Goldman-Sosin-Gatto continuous closed form.
    assert!(lb.mean < cont.mean - lb.ci,
        "discrete MC {} should sit below the continuous GSG price {} (Sec 3.2.2 bias)", lb.mean, cont.mean);

    // (4) It is still close to the continuous form (the gap is the O(sqrt(dt)) monitoring bias) and a
    // substantial positive value.
    assert!(cont.mean - lb.mean < 3.0, "discrete-monitoring gap should be modest, got {}", cont.mean - lb.mean);
    assert!(cont.mean > 15.0 && lb.mean > 13.0, "lookback should be a sizeable positive value");

    // (5) The disc*S(T) control variate cuts variance while staying unbiased.
    println!("variance-reduction factor (std_plain/std_cv) = {:.2}", lb.std / cv.std);
    assert!(cv.std < lb.std, "CV should reduce std: plain {} vs cv {}", lb.std, cv.std);
    assert!((cv.mean - lb.mean).abs() < 4.0 * (lb.ci + cv.ci),
        "CV mean {} should agree with the plain lookback mean {}", cv.mean, lb.mean);
}
