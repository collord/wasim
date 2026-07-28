//! Masked / filtered array reductions: argmin_where / argmax_where / masked_{sum,mean,min,max}
//! / masked_count. These let a wear-levelling dispatch select "the least-damaged AVAILABLE
//! truck" declaratively instead of the penalty idiom `argmin_array(damage + BIG*failed)`.
//!
//! A member is selected when its mask value is non-zero (and not NaN). Ties in argmin/argmax
//! resolve to the LOWEST index (the bit-identity requirement); none-selected → 0.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

/// Build a model whose every element is `expr` evaluated once; return the final value of `id`.
fn eval_exprs(exprs: &[(&str, &str)]) -> SimulationResults {
    let els: Vec<String> = exprs
        .iter()
        .map(|(id, ast)| format!(
            r#"{{"id": "{id}", "name": "{id}", "primitive": "node", "value_rule": "expression",
                "expression": {{"ast": {ast}}}, "save_results": {{"final_value": true}}}}"#
        ))
        .collect();
    let json = format!(
        r#"{{"wasim_version": "0.1.0",
        "simulation_settings": {{"duration": {{"value": 1, "unit": "yr"}}, "timestep": {{"value": 1, "unit": "yr"}}, "n_realizations": 1, "seed": 1}},
        "elements": [{}]}}"#,
        els.join(",")
    );
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run")
}

fn val(r: &SimulationResults, id: &str) -> f64 {
    r.elements[id].final_values[0]
}

// damage = [30, 10, 20]; available = [1, 0, 1]  (truck 2 is failed / unavailable)
const DAMAGE: &str = r#"{"op":"array","elements":[{"op":"literal","value":30},{"op":"literal","value":10},{"op":"literal","value":20}]}"#;
const AVAIL: &str = r#"{"op":"array","elements":[{"op":"literal","value":1},{"op":"literal","value":0},{"op":"literal","value":1}]}"#;

fn call2(f: &str, a: &str, b: &str) -> String {
    format!(r#"{{"op":"call","fn":"{f}","args":[{a},{b}]}}"#)
}

#[test]
fn masked_reductions_select_only_available_members() {
    let r = eval_exprs(&[
        // Among available trucks {1,3} with damage {30,20}: least-damaged is truck 3, most is truck 1.
        ("argmin", &call2("argmin_where", DAMAGE, AVAIL)),
        ("argmax", &call2("argmax_where", DAMAGE, AVAIL)),
        ("mmin", &call2("masked_min", DAMAGE, AVAIL)),
        ("mmax", &call2("masked_max", DAMAGE, AVAIL)),
        ("msum", &call2("masked_sum", DAMAGE, AVAIL)),
        ("mmean", &call2("masked_mean", DAMAGE, AVAIL)),
        ("mcount", &format!(r#"{{"op":"call","fn":"masked_count","args":[{AVAIL}]}}"#)),
    ]);
    assert_eq!(val(&r, "argmin"), 3.0, "least-damaged available truck is #3 (skips the failed #2)");
    assert_eq!(val(&r, "argmax"), 1.0, "most-damaged available truck is #1");
    assert_eq!(val(&r, "mmin"), 20.0, "min damage among available");
    assert_eq!(val(&r, "mmax"), 30.0, "max damage among available");
    assert_eq!(val(&r, "msum"), 50.0, "30 + 20");
    assert_eq!(val(&r, "mmean"), 25.0, "(30 + 20)/2");
    assert_eq!(val(&r, "mcount"), 2.0, "two trucks available");
}

#[test]
fn argmin_where_ties_resolve_to_lowest_index() {
    // damage = [10, 10, 20], all available -> the tie between #1 and #2 goes to #1.
    let dmg = r#"{"op":"array","elements":[{"op":"literal","value":10},{"op":"literal","value":10},{"op":"literal","value":20}]}"#;
    let all = r#"{"op":"array","elements":[{"op":"literal","value":1},{"op":"literal","value":1},{"op":"literal","value":1}]}"#;
    let r = eval_exprs(&[("m", &call2("argmin_where", dmg, all))]);
    assert_eq!(val(&r, "m"), 1.0, "tie -> lowest index (deterministic)");
}

#[test]
fn none_selected_returns_zero() {
    let none = r#"{"op":"array","elements":[{"op":"literal","value":0},{"op":"literal","value":0},{"op":"literal","value":0}]}"#;
    let r = eval_exprs(&[
        ("argmin", &call2("argmin_where", DAMAGE, none)),
        ("argmax", &call2("argmax_where", DAMAGE, none)),
        ("mmin", &call2("masked_min", DAMAGE, none)),
        ("mmean", &call2("masked_mean", DAMAGE, none)),
        ("msum", &call2("masked_sum", DAMAGE, none)),
        ("mcount", &format!(r#"{{"op":"call","fn":"masked_count","args":[{none}]}}"#)),
    ]);
    for id in ["argmin", "argmax", "mmin", "mmean", "msum", "mcount"] {
        assert_eq!(val(&r, id), 0.0, "{id}: none-selected must be 0.0");
    }
}

#[test]
fn masked_argmin_equals_penalty_idiom() {
    // The whole point: argmin_where(damage, available) replaces argmin_array(damage + BIG*failed).
    // failed = 1 - available = [0,1,0]; BIG = 1e6.
    let penalty = r#"{"op":"call","fn":"argmin_array","args":[
        {"op":"add",
         "left":{"op":"array","elements":[{"op":"literal","value":30},{"op":"literal","value":10},{"op":"literal","value":20}]},
         "right":{"op":"array","elements":[{"op":"literal","value":0},{"op":"literal","value":1000000},{"op":"literal","value":0}]}}]}"#;
    let r = eval_exprs(&[
        ("declarative", &call2("argmin_where", DAMAGE, AVAIL)),
        ("penalty", penalty),
    ]);
    assert_eq!(val(&r, "declarative"), val(&r, "penalty"),
        "argmin_where must match the penalty idiom it replaces");
    assert_eq!(val(&r, "declarative"), 3.0);
}
