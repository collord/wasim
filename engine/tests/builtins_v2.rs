//! Gap 1a function-vocabulary builtins (§1a): erf/erfc, date extraction, finance factors, and
//! table introspection now evaluate (rather than falling back to opaque extern_call).

use wasim_engine::{parse_v2, ModelGraphV2, RunConfig, engine_v2};

/// Evaluate a single-element expression model and return the element's final value.
fn eval_expr(ast: &str) -> f64 {
    let json = format!(r#"{{
      "wasim_version": "0.9.0",
      "simulation_settings": {{"duration": {{"value": 1, "unit": "day"}}, "timestep": {{"value": 1, "unit": "day"}}, "seed": 1}},
      "elements": [
        {{"id": "y", "name": "Y", "primitive": "node", "value_rule": "expression",
         "expression": {{"ast": {ast}}}, "save_results": {{"final_value": true}}}}
      ]
    }}"#);
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = engine_v2::run(&m, &g, &RunConfig { seed: Some(1), ..RunConfig::default() }).expect("run");
    *r.elements.get("y").unwrap().final_values.first().unwrap()
}

fn call(fnname: &str, args: &[f64]) -> String {
    let a: Vec<String> = args.iter().map(|v| format!(r#"{{"op":"literal","value":{v}}}"#)).collect();
    format!(r#"{{"op":"call","fn":"{fnname}","args":[{}]}}"#, a.join(","))
}

#[test]
fn erf_and_erfc() {
    assert!((eval_expr(&call("erf", &[0.0])) - 0.0).abs() < 1e-6, "erf(0)=0");
    assert!((eval_expr(&call("erf", &[1.0])) - 0.842_700_79).abs() < 1e-5, "erf(1)≈0.8427");
    assert!((eval_expr(&call("erf", &[-1.0])) + 0.842_700_79).abs() < 1e-5, "erf(-1)≈-0.8427 (odd)");
    // erfc(x) = 1 - erf(x)
    assert!((eval_expr(&call("erfc", &[1.0])) - (1.0 - 0.842_700_79)).abs() < 1e-5, "erfc(1)");
}

#[test]
fn date_extraction() {
    // 2021-03-15 00:00:00 UTC = 1_615_766_400 s since 1970.
    let d = 1_615_766_400.0;
    assert_eq!(eval_expr(&call("get_year", &[d])), 2021.0);
    assert_eq!(eval_expr(&call("get_month", &[d])), 3.0);
    assert_eq!(eval_expr(&call("get_day", &[d])), 15.0);
    // + 13h 37m 42s
    let t = d + 13.0 * 3600.0 + 37.0 * 60.0 + 42.0;
    assert_eq!(eval_expr(&call("get_hour", &[t])), 13.0);
    assert_eq!(eval_expr(&call("get_minute", &[t])), 37.0);
    assert_eq!(eval_expr(&call("get_second", &[t])), 42.0);
}

#[test]
fn finance_factors() {
    // pv_factor(rate, n) = (1+rate)^n
    assert!((eval_expr(&call("pv_factor", &[0.05, 10.0])) - 1.05_f64.powi(10)).abs() < 1e-9);
    // annuity_factor(rate, n) = (1 - (1+rate)^-n) / rate
    let (r, np) = (0.05_f64, 10.0_f64);
    let expected = (1.0 - (1.0 + r).powf(-np)) / r;
    assert!((eval_expr(&call("annuity_factor", &[0.05, 10.0])) - expected).abs() < 1e-9);
    // rate 0 → annuity factor = n
    assert!((eval_expr(&call("annuity_factor", &[0.0, 7.0])) - 7.0).abs() < 1e-9);
}

#[test]
fn table_introspection_over_array() {
    // table_min/max/column_count over an inline array literal.
    let arr = r#"{"op":"array","elements":[
        {"op":"literal","value":3},{"op":"literal","value":1},{"op":"literal","value":4},{"op":"literal","value":1},{"op":"literal","value":5}]}"#;
    assert_eq!(eval_expr(&format!(r#"{{"op":"call","fn":"table_min","args":[{arr}]}}"#)), 1.0);
    assert_eq!(eval_expr(&format!(r#"{{"op":"call","fn":"table_max","args":[{arr}]}}"#)), 5.0);
    assert_eq!(eval_expr(&format!(r#"{{"op":"call","fn":"column_count","args":[{arr}]}}"#)), 5.0);
}
