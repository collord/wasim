// Prototype of the terminal-value accessor (Option A from the design discussion): a
// `terminal_expression` element evaluated exactly once, after the run, against each realization's
// TERMINAL state. A `ref` to a stock resolves to its end-of-run level S(T) — not the one-step-stale
// start-of-step value S(T-dt) an ordinary expression reads. This removes the effective-maturity
// (T_eff = T - dt) workaround the path-dependent option examples had to carry.
//
// Two claims, one statistical and one exact:
//   (1) EXACT: a terminal expression `ref(S)` equals the stock's own saved final value S(T), per
//       realization, to floating-point tolerance — while an ordinary expression `ref(S)` lags it
//       by one step (S(T-dt)). This pins the mechanism without any statistics.
//   (2) A European call payoff built on the terminal read matures at the TRUE T: its MC mean
//       converges to Black-Scholes at T (10.4506), not to BS at T_eff = 0.98 (10.3219) — the value
//       the same payoff written as an ordinary expression would give.
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, ResultsSpec, RunConfig, WasimModel};

const JSON: &str = r#"{
  "wasim_version":"0.1.0",
  "simulation_settings":{"duration":{"value":1.0,"unit":"yr"},"timestep":{"value":0.02,"unit":"yr"},"n_realizations":80000,"seed":13579},
  "elements":[
    {"id":"S0","name":"S0","type":"constant","value":{"value":100.0,"unit":"USD"},"editable":false,"bounds":{"min":null,"max":null}},
    {"id":"K","name":"K","type":"constant","value":{"value":100.0,"unit":"USD"},"editable":false,"bounds":{"min":null,"max":null}},
    {"id":"r","name":"r","type":"constant","value":{"value":0.05,"unit":"1"},"editable":false,"bounds":{"min":null,"max":null}},
    {"id":"sigma","name":"sigma","type":"constant","value":{"value":0.20,"unit":"1"},"editable":false,"bounds":{"min":null,"max":null}},
    {"id":"T","name":"T","type":"constant","value":{"value":1.0,"unit":"1"},"editable":false,"bounds":{"min":null,"max":null}},

    {"id":"P","name":"P","type":"stochastic_process","process":{"family":"gbm","mean_type":"arithmetic","mean":{"ast":{"op":"ref","element_id":"r"},"display":"r"},"stddev":{"ast":{"op":"ref","element_id":"sigma"},"display":"sigma"}}},
    {"id":"S","name":"S","type":"accumulator","initial_value":{"value":100.0,"unit":"USD"},"inputs":["S","P"],
     "rate":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"S"},"right":{"op":"ref","element_id":"P"}},"display":"S*P"},
     "min_value":null,"capacity":null,"save_results":{"final_value":true}},

    {"id":"disc","name":"disc","type":"expression","inputs":["r","T"],
     "expression":{"ast":{"op":"call","fn":"exp","args":[{"op":"neg","operand":{"op":"multiply","left":{"op":"ref","element_id":"r"},"right":{"op":"ref","element_id":"T"}}}]},"display":"exp(-r*T)"}},

    {"id":"lag_read","name":"lag_read","type":"expression","inputs":["S"],
     "expression":{"ast":{"op":"ref","element_id":"S"},"display":"S (start-of-step)"},"save_results":{"final_value":true}},

    {"id":"term_read","name":"term_read","type":"terminal_expression","inputs":["S"],
     "expression":{"ast":{"op":"ref","element_id":"S"},"display":"S (terminal)"},"save_results":{"final_value":true}},

    {"id":"payoff_term","name":"payoff_term","type":"terminal_expression","inputs":["disc","S","K"],
     "expression":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"disc"},
        "right":{"op":"call","fn":"max","args":[{"op":"subtract","left":{"op":"ref","element_id":"S"},"right":{"op":"ref","element_id":"K"}},{"op":"literal","value":0.0}]}},"display":"disc*max(S-K,0)"},
     "save_results":{"final_value":true}}
  ]
}"#;

fn bs_call(s: f64, k: f64, r: f64, sig: f64, t: f64) -> f64 {
    let phi = |x: f64| 0.5 * (1.0 + libm_erf(x / 2f64.sqrt()));
    let d1 = ((s / k).ln() + (r + 0.5 * sig * sig) * t) / (sig * t.sqrt());
    let d2 = d1 - sig * t.sqrt();
    s * phi(d1) - k * (-r * t).exp() * phi(d2)
}
// erf via Abramowitz & Stegun 7.1.26 (matches the engine's own erf to ~1e-7 — fine for a benchmark).
fn libm_erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * (-x * x).exp();
    if x < 0.0 { -y } else { y }
}

#[test]
fn terminal_expression_reads_true_terminal() {
    let m = normalize_v1(&serde_json::from_str::<WasimModel>(JSON).unwrap());
    let g = ModelGraphV2::build(&m).unwrap();
    let mut spec = ResultsSpec::default();
    spec.final_stats = true;
    spec.elements = vec!["S".into(), "lag_read".into(), "term_read".into(), "payoff_term".into()];
    let cfg = RunConfig { results_spec: Some(spec), ..Default::default() };
    let res = run_v2(&m, &g, &cfg).unwrap();

    let sfin = &res.elements["S"].final_values;
    let term = &res.elements["term_read"].final_values;
    let fs = |id: &str| {
        let a = res.elements[id].analysis.as_ref().unwrap().final_stats.as_ref().unwrap();
        (a.mean, a.ci_half_width)
    };

    // (1a) EXACT, per realization: the terminal read equals the stock's own end-of-run level S(T).
    let n = sfin.len();
    for i in 0..n {
        assert!((term[i] - sfin[i]).abs() < 1e-9,
            "terminal read must equal S's final at realization {i}: {} vs {}", term[i], sfin[i]);
    }
    // (1b) The ordinary expression lags by one step: it reads S(T-dt) != S(T). Distinct in the mean.
    let (m_lag, _) = fs("lag_read");
    let (m_term, _) = fs("term_read");
    assert!((m_term - m_lag).abs() > 0.02,
        "terminal read {m_term} should differ from the one-step-stale ordinary read {m_lag}");
    // Both are lognormal means: E[S(T)] = 100 e^{rT}, E[S(T-dt)] = 100 e^{r(T-dt)}.
    assert!((m_term - 100.0 * 0.05f64.exp()).abs() < 0.3, "E[S(T)] ~ 105.13, got {m_term}");
    assert!((m_lag - 100.0 * (0.05 * 0.98f64).exp()).abs() < 0.3, "E[S(T-dt)] ~ 105.03, got {m_lag}");

    // (2) A European call on the terminal read matures at the TRUE T: mean -> BS(T) = 10.4506,
    // NOT BS(T_eff=0.98) = 10.3219 (what the same payoff as an ordinary expression would give).
    let bs_t = bs_call(100.0, 100.0, 0.05, 0.20, 1.0);
    let bs_teff = bs_call(100.0, 100.0, 0.05, 0.20, 0.98);
    let (m_pay, ci_pay) = fs("payoff_term");
    println!("payoff_term MC = {m_pay:.4} +/- {ci_pay:.4}   BS(T)={bs_t:.4}  BS(T_eff)={bs_teff:.4}");
    assert!((m_pay - bs_t).abs() < 4.0 * ci_pay,
        "terminal payoff {m_pay} should converge to BS(T) {bs_t} (ci {ci_pay})");
    // And it sits clearly on the BS(T) side of the T_eff value the workaround produced.
    assert!(m_pay > 0.5 * (bs_t + bs_teff),
        "terminal payoff {m_pay} should be above the BS(T)/BS(T_eff) midpoint {}", 0.5 * (bs_t + bs_teff));
    println!("terminal_expression: per-realization S(T) identity holds; payoff matures at true T");
}
