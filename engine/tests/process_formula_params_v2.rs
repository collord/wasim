// Gap #1: expression-valued process drift/vol. A GBM whose process `mean`/`stddev` are formulas
// referencing editable constants must (a) resolve to the same values as literal parameters
// (bit-identical under the same seed — backward compatibility) and (b) produce the correct GBM
// statistics: E[S_T] = S0*exp(r*T), Var(ln S_T) = sigma^2*T.
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, RunConfig, WasimModel};

fn model(mean_p: &str, stddev_p: &str) -> String {
    format!(r#"{{
      "wasim_version":"0.1.0",
      "simulation_settings":{{"duration":{{"value":1.0,"unit":"yr"}},"timestep":{{"value":0.02,"unit":"yr"}},"n_realizations":8000,"seed":9}},
      "elements":[
        {{"id":"r","name":"r","type":"constant","value":{{"value":0.10,"unit":"1/yr"}},"editable":true,"bounds":{{"min":null,"max":null}}}},
        {{"id":"sig","name":"sig","type":"constant","value":{{"value":0.30,"unit":"1/yr"}},"editable":true,"bounds":{{"min":null,"max":null}}}},
        {{"id":"P","name":"P","type":"stochastic_process",
         "process":{{"family":"gbm","mean_type":"arithmetic","mean":{mean_p},"stddev":{stddev_p}}}}},
        {{"id":"S","name":"S","type":"accumulator","initial_value":{{"value":100.0,"unit":"USD"}},
         "inputs":["S","P"],
         "rate":{{"ast":{{"op":"multiply","left":{{"op":"ref","element_id":"S"}},"right":{{"op":"ref","element_id":"P"}}}},"display":"S*P"}},
         "min_value":null,"capacity":null,"save_results":{{"final_value":true}}}}
      ]
    }}"#)
}

fn s_final(json: &str) -> Vec<f64> {
    let m = normalize_v1(&serde_json::from_str::<WasimModel>(json).expect("parse"));
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run").elements["S"].final_values.clone()
}

#[test]
fn formula_process_params_match_literals_and_gbm_moments() {
    let literal = model(r#"{"value":0.10,"unit":"1/yr"}"#, r#"{"value":0.30,"unit":"1/yr"}"#);
    let formula = model(r#"{"ast":{"op":"ref","element_id":"r"}}"#, r#"{"ast":{"op":"ref","element_id":"sig"}}"#);
    let (lit, frm) = (s_final(&literal), s_final(&formula));

    // (a) Formula params resolve to the same scalars as literals => bit-identical paths (same seed).
    assert_eq!(lit.len(), frm.len());
    for (a, b) in lit.iter().zip(&frm) {
        assert_eq!(a.to_bits(), b.to_bits(), "formula-driven path must equal the literal-driven path");
    }

    // (b) Correct GBM moments: E[S_T] = 100*exp(0.10) = 110.517; Var(ln S_T) = sigma^2*T = 0.09.
    let n = frm.len() as f64;
    let mean = frm.iter().sum::<f64>() / n;
    let logs: Vec<f64> = frm.iter().map(|x| x.ln()).collect();
    let lm = logs.iter().sum::<f64>() / n;
    let lv = logs.iter().map(|x| (x - lm).powi(2)).sum::<f64>() / n;
    println!("E[S_T]={mean:.4} (want ~110.52)  Var(ln S_T)={lv:.4} (want ~0.09)");
    assert!((mean - 110.517).abs() < 1.5, "E[S_T] ~ 100*exp(0.10), got {mean}");
    assert!((lv - 0.09).abs() < 0.01, "Var(ln S_T) ~ sigma^2*T = 0.09, got {lv}");
}
