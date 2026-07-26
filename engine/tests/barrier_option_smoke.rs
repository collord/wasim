// Validates schema_examples_manual/barrier_option_down_and_out.json through the v2 engine
// (normalize_v1 -> run_v2). A discretely monitored down-and-out call priced by path Monte Carlo
// (Glasserman Sec 3.2.2), exercising FOUR engine features together and maturing at the TRUE T:
//   gap #1: the GBM process drift/vol reference r/sigma directly (expression-valued process params);
//   gap #2: the running minimum is an expanding-window `filter` (native running statistic), with
//           `include_terminal` folding the terminal fixing S(T) into the monitor natively;
//   gap #3: `terminal_expression` reads the true terminal S(T) — so the vanilla control matures at
//           the true T (no T_eff = T - dt workaround);
//   plus the vanilla call as a control variate whose known mean is a live Black-Scholes price
//           (run_stat2 beta) — with barrier_cv itself a terminal_expression reading that run_stat2.
//
// Checks:
//  (1) Path validation: the vanilla call on the TRUE terminal S(T) matches the live Black-Scholes
//      price at the true maturity T (not T_eff) — validates the whole path and the terminal accessor.
//  (2) Structural: the down-and-out is worth strictly less than the vanilla (knock-out destroys value).
//  (3) Sec 3.2.2 discrete-monitoring bias: discrete monitoring knocks out less often than continuous,
//      so the MC price sits ABOVE the Reiner-Rubinstein continuous-monitoring closed form.
//  (4) Broadie-Glasserman-Kou continuity correction: shifting the barrier by exp(-0.5826 sigma sqrt(dt))
//      yields a corrected reference the discrete MC matches within Monte Carlo error.
//  (5) The vanilla control variate reduces the down-and-out estimator's variance while staying unbiased.
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
    // dt = 0.02 (50 monitoring steps) matching the constant T = 1.0. The BGK barrier shift reads the
    // timestep (time_ref) so it tracks the grid; T is a constant, so duration must stay 1.0.
    let cfg = RunConfig { n_realizations: Some(n), seed: Some(90909), results_spec: Some(spec),
        duration_override: Some(1.0), timestep_override: Some(0.02), ..Default::default() };
    let res = run_v2(&model, &graph, &cfg).expect("run");
    ids().into_iter().map(|id| {
        let fs = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        (id.to_string(), FS { mean: fs.mean, std: fs.std, ci: fs.ci_half_width })
    }).collect()
}
fn ids() -> Vec<&'static str> {
    vec!["vanilla", "barrier", "barrier_cv", "bs_price", "cdo_cont", "cdo_bgk"]
}

#[test]
fn barrier_down_and_out_prices_and_reduces_variance() {
    let json = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../schema_examples_manual/barrier_option_down_and_out.json")).unwrap();
    let s = stats(&json, 30_000);
    let (van, bar, cv, bs, cont, bgk) =
        (&s["vanilla"], &s["barrier"], &s["barrier_cv"], &s["bs_price"], &s["cdo_cont"], &s["cdo_bgk"]);
    println!("bs_price  (vanilla, true T) = {:.4}", bs.mean);
    println!("cdo_cont  (continuous ref)  = {:.4}", cont.mean);
    println!("cdo_bgk   (BGK-corrected)   = {:.4}", bgk.mean);
    println!("vanilla MC : mean={:.4} std={:.4} ci={:.4}", van.mean, van.std, van.ci);
    println!("barrier MC : mean={:.4} std={:.4} ci={:.4}", bar.mean, bar.std, bar.ci);
    println!("barrier_cv : mean={:.4} std={:.4} ci={:.4}", cv.mean, cv.std, cv.ci);

    // (1) Path validation: the vanilla call on the TRUE terminal S(T) (read via terminal_expression)
    // matches the live Black-Scholes price at the true maturity T — no effective-maturity fudge.
    assert!((van.mean - bs.mean).abs() < 4.0 * van.ci,
        "vanilla MC {} should match BS(T) {} (ci {})", van.mean, bs.mean, van.ci);
    assert!(bs.mean > 10.0 && bs.mean < 11.0, "BS(T) sanity: {}", bs.mean);

    // (2) Structural: the down-and-out is worth strictly less than the vanilla.
    assert!(bar.mean < van.mean - 0.5, "barrier {} should be well below vanilla {}", bar.mean, van.mean);

    // (3) Sec 3.2.2 discrete-monitoring bias: discrete monitoring survives more often than continuous,
    // so the MC down-and-out price sits above the continuous-monitoring Reiner-Rubinstein formula.
    assert!(bar.mean > cont.mean + bar.ci,
        "discrete MC {} should exceed continuous ref {} (Sec 3.2.2 bias)", bar.mean, cont.mean);

    // (4) BGK continuity correction: the corrected reference matches the discrete MC within MC error.
    assert!((bar.mean - bgk.mean).abs() < 5.0 * bar.ci,
        "discrete MC {} should match the BGK-corrected reference {} (ci {})", bar.mean, bgk.mean, bar.ci);

    // (5) The vanilla control variate cuts variance while staying unbiased (vanilla and barrier
    // coincide on every surviving path). barrier_cv is a terminal_expression reading run_stat2 beta.
    println!("variance-reduction factor (std_barrier/std_cv) = {:.2}", bar.std / cv.std);
    assert!(cv.std < bar.std, "CV should reduce std: barrier {} vs cv {}", bar.std, cv.std);
    assert!((cv.mean - bar.mean).abs() < 4.0 * (bar.ci + cv.ci),
        "CV mean {} should agree with plain barrier mean {}", cv.mean, bar.mean);
}
