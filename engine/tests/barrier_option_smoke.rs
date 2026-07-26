// Validates schema_examples_manual/barrier_option_down_and_out.json through the v2 engine
// (normalize_v1 -> run_v2). A discretely monitored down-and-out call priced by path Monte Carlo
// (Glasserman Sec 3.2.2), exercising three engine features together: the GBM process drift/vol
// reference r/sigma directly (gap #1: expression-valued process params); the running minimum is an
// expanding-window `filter` (gap #2: native running statistic); and the plain call on the same path
// is a control variate whose known mean is a live Black-Scholes price (run_stat2 beta).
//
// Checks:
//  (1) Path validation: the vanilla call on the monitored terminal matches the live BS price at the
//      effective maturity T_eff = T - dt (a stock is read one step stale) — validates the whole path.
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
    // dt = 0.02 (50 monitoring steps); the live closed forms read the timestep (time_ref) so they
    // self-adjust to whatever grid the run uses. The BGK correction needs a reasonably fine grid.
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
    println!("bs_price  (vanilla, T_eff) = {:.4}", bs.mean);
    println!("cdo_cont  (continuous ref) = {:.4}", cont.mean);
    println!("cdo_bgk   (BGK-corrected)  = {:.4}", bgk.mean);
    println!("vanilla MC : mean={:.4} std={:.4} ci={:.4}", van.mean, van.std, van.ci);
    println!("barrier MC : mean={:.4} std={:.4} ci={:.4}", bar.mean, bar.std, bar.ci);
    println!("barrier_cv : mean={:.4} std={:.4} ci={:.4}", cv.mean, cv.std, cv.ci);

    // (1) Path validation: the vanilla call on the monitored terminal matches the live BS price at
    // the effective maturity — a stringent joint check of the GBM path and the terminal-read timing.
    assert!((van.mean - bs.mean).abs() < 4.0 * van.ci,
        "vanilla MC {} should match BS(T_eff) {} (ci {})", van.mean, bs.mean, van.ci);
    assert!(bs.mean > 9.0 && bs.mean < 11.0, "BS(T_eff) sanity: {}", bs.mean);

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
    // coincide on every surviving path, so they are strongly correlated).
    println!("variance-reduction factor (std_barrier/std_cv) = {:.2}", bar.std / cv.std);
    assert!(cv.std < bar.std, "CV should reduce std: barrier {} vs cv {}", bar.std, cv.std);
    assert!((cv.mean - bar.mean).abs() < 4.0 * (bar.ci + cv.ci),
        "CV mean {} should agree with plain barrier mean {}", cv.mean, bar.mean);
}
